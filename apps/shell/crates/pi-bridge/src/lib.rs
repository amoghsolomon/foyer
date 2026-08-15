//! Supervised Foyer Shell reasoning/directing sidecar integration.
//!
//! Rust owns the process boundary and stable JSONL protocol. A dedicated Pi SDK process runs the
//! reasoner, disposable status narrator, and semantic director without loading Pi's CLI or TUI.
//! The native compiler remains authoritative for presentation structure and timing.

use std::{
    env,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    thread,
    time::Duration,
};

use foyer_shell_protocol::EventEnvelope;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const SIDECAR_PROTOCOL_VERSION: u16 = 9;

#[derive(Debug, Error)]
pub enum BridgeError {
    #[error("failed to start the Pi presentation sidecar: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("the presentation sidecar did not expose {0}")]
    MissingPipe(&'static str),
    #[error("failed to write a sidecar command: {0}")]
    Write(#[source] std::io::Error),
    #[error("failed to encode a sidecar command: {0}")]
    Encode(#[source] serde_json::Error),
    #[error("the Pi presentation sidecar entrypoint does not exist: {0}")]
    MissingEntrypoint(String),
    #[error("the presentation sidecar did not become ready within 20 seconds")]
    StartupTimeout,
    #[error("the presentation sidecar stopped during startup")]
    StartupClosed,
    #[error("the presentation sidecar rejected startup: {0}")]
    StartupRejected(String),
}

#[derive(Clone, Debug)]
pub struct PiConfig {
    pub node: String,
    pub entrypoint: PathBuf,
    pub model: String,
}

impl Default for PiConfig {
    fn default() -> Self {
        Self {
            node: "node".into(),
            entrypoint: Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sidecar/src/main.mjs"),
            model: "openai-codex/gpt-5.6-luna".into(),
        }
    }
}

impl PiConfig {
    pub fn for_workspace(root: &Path) -> Self {
        let mut config = Self {
            entrypoint: resolve_sidecar_entrypoint(root),
            ..Self::default()
        };
        if let Ok(model) = env::var("FOYER_SHELL_WORKER_MODEL") {
            config.model = model;
        }
        if let Ok(model) = env::var("FOYER_SHELL_MODEL") {
            config.model = model;
        }
        config
    }
}

fn resolve_sidecar_entrypoint(root: &Path) -> PathBuf {
    if let Some(path) = env::var_os("FOYER_SHELL_SIDECAR_ENTRYPOINT") {
        return PathBuf::from(path);
    }

    let workspace = root.join("sidecar/src/main.mjs");
    if workspace.is_file() {
        return workspace;
    }

    let installed = foyer_shell_paths::data_root().join("sidecar/src/main.mjs");
    if installed.is_file() {
        return installed;
    }

    PiConfig::default().entrypoint
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SidecarMessage {
    Ready {
        protocol_version: u16,
        model: String,
    },
    Events {
        protocol_version: u16,
        request_id: String,
        events: Vec<EventEnvelope>,
    },
    PresentationMaterials {
        protocol_version: u16,
        request_id: String,
        evidence: String,
    },
    Settled {
        protocol_version: u16,
        request_id: String,
        status: String,
        duration_ms: u64,
    },
    Error {
        protocol_version: u16,
        #[serde(default)]
        request_id: Option<String>,
        #[serde(default)]
        component: Option<String>,
        message: String,
        #[serde(default)]
        fatal: bool,
    },
}

impl SidecarMessage {
    pub fn protocol_version(&self) -> u16 {
        match self {
            Self::Ready {
                protocol_version, ..
            }
            | Self::Events {
                protocol_version, ..
            }
            | Self::PresentationMaterials {
                protocol_version, ..
            }
            | Self::Settled {
                protocol_version, ..
            }
            | Self::Error {
                protocol_version, ..
            } => *protocol_version,
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SidecarCommand<'a> {
    Start {
        protocol_version: u16,
        id: &'a str,
        goal: &'a str,
    },
    Steer {
        protocol_version: u16,
        message: &'a str,
    },
    Abort {
        protocol_version: u16,
    },
    Shutdown {
        protocol_version: u16,
    },
}

pub struct PiHarness {
    child: Child,
    stdin: ChildStdin,
    events: Receiver<Result<SidecarMessage, String>>,
}

impl PiHarness {
    pub fn spawn(
        config: &PiConfig,
        working_dir: &Path,
        state_dir: &Path,
    ) -> Result<Self, BridgeError> {
        if !config.entrypoint.is_file() {
            return Err(BridgeError::MissingEntrypoint(
                config.entrypoint.display().to_string(),
            ));
        }
        let mut child = Command::new(&config.node)
            .arg(&config.entrypoint)
            .arg("--cwd")
            .arg(working_dir)
            .arg("--state-dir")
            .arg(state_dir)
            .arg("--model")
            .arg(&config.model)
            .current_dir(working_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(BridgeError::Spawn)?;
        let stdin = child
            .stdin
            .take()
            .ok_or(BridgeError::MissingPipe("stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or(BridgeError::MissingPipe("stdout"))?;
        let (sender, events) = mpsc::channel();

        thread::Builder::new()
            .name("foyer-shell-pi-sidecar-reader".into())
            .spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines() {
                    let result = match line {
                        Ok(line) => serde_json::from_str::<SidecarMessage>(&line)
                            .map_err(|error| format!("{error}: {line}")),
                        Err(error) => Err(error.to_string()),
                    };
                    if sender.send(result).is_err() {
                        break;
                    }
                }
            })
            .map_err(BridgeError::Spawn)?;

        let ready = match events.recv_timeout(Duration::from_secs(20)) {
            Ok(Ok(message)) => message,
            Ok(Err(error)) => return Err(BridgeError::StartupRejected(error)),
            Err(RecvTimeoutError::Timeout) => return Err(BridgeError::StartupTimeout),
            Err(RecvTimeoutError::Disconnected) => return Err(BridgeError::StartupClosed),
        };
        match ready {
            SidecarMessage::Ready {
                protocol_version, ..
            } if protocol_version == SIDECAR_PROTOCOL_VERSION => {}
            SidecarMessage::Error { message, .. } => {
                return Err(BridgeError::StartupRejected(message));
            }
            message => {
                return Err(BridgeError::StartupRejected(format!(
                    "expected ready message, received {message:?}"
                )));
            }
        }

        Ok(Self {
            child,
            stdin,
            events,
        })
    }

    pub fn prompt(&mut self, request_id: &str, prompt: &str) -> Result<(), BridgeError> {
        self.send(&SidecarCommand::Start {
            protocol_version: SIDECAR_PROTOCOL_VERSION,
            id: request_id,
            goal: prompt,
        })
    }

    pub fn steer(&mut self, message: &str) -> Result<(), BridgeError> {
        self.send(&SidecarCommand::Steer {
            protocol_version: SIDECAR_PROTOCOL_VERSION,
            message,
        })
    }

    pub fn abort(&mut self) -> Result<(), BridgeError> {
        self.send(&SidecarCommand::Abort {
            protocol_version: SIDECAR_PROTOCOL_VERSION,
        })
    }

    pub fn events(&self) -> &Receiver<Result<SidecarMessage, String>> {
        &self.events
    }

    fn send(&mut self, value: &SidecarCommand<'_>) -> Result<(), BridgeError> {
        serde_json::to_writer(&mut self.stdin, value).map_err(BridgeError::Encode)?;
        self.stdin.write_all(b"\n").map_err(BridgeError::Write)?;
        self.stdin.flush().map_err(BridgeError::Write)
    }
}

impl Drop for PiHarness {
    fn drop(&mut self) {
        let _ = serde_json::to_writer(
            &mut self.stdin,
            &SidecarCommand::Shutdown {
                protocol_version: SIDECAR_PROTOCOL_VERSION,
            },
        );
        let _ = self.stdin.write_all(b"\n");
        let _ = self.stdin.flush();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use foyer_shell_protocol::{CompletionStatus, WorkEvent};

    use super::*;

    #[test]
    fn parses_a_sidecar_event_batch() {
        let message: SidecarMessage = serde_json::from_str(
            r#"{
                "protocol_version": 4,
                "type": "events",
                "request_id": "test",
                "events": [{
                    "version": 3,
                    "sequence": 2,
                    "elapsed_ms": 10,
                    "type": "session_completed",
                    "status": "completed",
                    "summary": "Done",
                    "answer_markdown": "Done",
                    "artifact_ids": []
                }]
            }"#,
        )
        .unwrap();
        let SidecarMessage::Events { events, .. } = message else {
            panic!("expected events")
        };
        assert!(matches!(
            events[0].event,
            WorkEvent::SessionCompleted {
                status: CompletionStatus::Completed,
                ..
            }
        ));
    }

    #[test]
    fn sidecar_commands_use_the_versioned_protocol() {
        let command = serde_json::to_value(SidecarCommand::Start {
            protocol_version: SIDECAR_PROTOCOL_VERSION,
            id: "request",
            goal: "Explain this",
        })
        .unwrap();
        assert_eq!(command["type"], "start");
        assert_eq!(command["protocol_version"], SIDECAR_PROTOCOL_VERSION);
    }

    #[test]
    fn parses_presentation_materials_for_durable_evidence() {
        let message: SidecarMessage = serde_json::from_value(serde_json::json!({
            "protocol_version": SIDECAR_PROTOCOL_VERSION,
            "type": "presentation_materials",
            "request_id": "presentation-1",
            "evidence": "A bounded evidence briefing"
        }))
        .unwrap();

        assert!(matches!(
            message,
            SidecarMessage::PresentationMaterials {
                request_id,
                evidence,
                ..
            } if request_id == "presentation-1" && evidence == "A bounded evidence briefing"
        ));
    }

    #[test]
    fn luna_runtime_is_pinned() {
        let config = PiConfig::default();
        assert_eq!(config.model, "openai-codex/gpt-5.6-luna");
    }

    #[test]
    fn workspace_config_resolves_the_repository_sidecar() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        assert!(PiConfig::for_workspace(&root).entrypoint.is_file());
    }
}
