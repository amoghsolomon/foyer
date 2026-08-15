use std::{
    env, fs,
    fs::{File, OpenOptions},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use foyer_shell_protocol::{EventEnvelope, PROTOCOL_VERSION, PresentationSlide, WorkEvent};
use serde::{Deserialize, Serialize};

use crate::compile_slide;

pub const PRESENTATION_SCHEMA_VERSION: u16 = 1;
const COMPILER_VERSION: u16 = 1;
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresentationStatus {
    Authoring,
    PreparingAudio,
    #[default]
    Completed,
    Partial,
    Cancelled,
    Failed,
}

impl PresentationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Authoring => "authoring",
            Self::PreparingAudio => "preparing_audio",
            Self::Completed => "completed",
            Self::Partial => "partial",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PresentationManifest {
    pub schema_version: u16,
    pub protocol_version: u16,
    pub compiler_version: u16,
    pub presentation_id: String,
    pub activity_id: String,
    pub run_id: String,
    pub title: String,
    pub request: String,
    pub status: PresentationStatus,
    pub created_at_ms: i64,
    pub completed_at_ms: Option<i64>,
    pub duration_ms: u64,
    pub slide_count: usize,
    pub summary: String,
    pub model: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CompiledPresentation {
    schema_version: u16,
    compiler_version: u16,
    slides: Vec<PresentationSlide>,
}

#[derive(Clone, Debug)]
pub struct PresentationBundle {
    pub path: PathBuf,
    pub manifest: PresentationManifest,
    pub slides: Vec<PresentationSlide>,
}

impl PresentationBundle {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let manifest: PresentationManifest = read_json(&path.join("manifest.json"))?;
        if manifest.schema_version != PRESENTATION_SCHEMA_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "unsupported presentation schema {}",
                    manifest.schema_version
                ),
            ));
        }
        let presentation: CompiledPresentation =
            read_json(&path.join("compiled-presentation.json"))?;
        if presentation.schema_version != PRESENTATION_SCHEMA_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "unsupported compiled presentation schema {}",
                    presentation.schema_version
                ),
            ));
        }
        Ok(Self {
            path,
            manifest,
            slides: presentation.slides,
        })
    }

    pub fn narration_path(&self, beat_id: &str) -> PathBuf {
        self.path
            .join("narration")
            .join(narration_file_name(beat_id))
    }

    pub fn discover() -> io::Result<Vec<Self>> {
        Self::discover_in(presentation_root())
    }

    pub fn discover_in(root: impl AsRef<Path>) -> io::Result<Vec<Self>> {
        let Ok(entries) = fs::read_dir(root) else {
            return Ok(Vec::new());
        };
        let mut bundles = entries
            .filter_map(Result::ok)
            .filter_map(|entry| Self::open(entry.path()).ok())
            .collect::<Vec<_>>();
        bundles.sort_by(|left, right| {
            right
                .manifest
                .created_at_ms
                .cmp(&left.manifest.created_at_ms)
                .then_with(|| {
                    right
                        .manifest
                        .presentation_id
                        .cmp(&left.manifest.presentation_id)
                })
        });
        Ok(bundles)
    }
}

pub struct PresentationRecorder {
    bundle_path: PathBuf,
    narration_path: PathBuf,
    manifest: PresentationManifest,
    events: BufWriter<File>,
    slides: Vec<PresentationSlide>,
    authoring_finished: bool,
}

impl PresentationRecorder {
    pub fn begin(request: &str) -> io::Result<Self> {
        Self::begin_in(&presentation_root(), request)
    }

    pub fn begin_at(root: impl AsRef<Path>, request: &str) -> io::Result<Self> {
        Self::begin_in(root.as_ref(), request)
    }

    fn begin_in(root: &Path, request: &str) -> io::Result<Self> {
        let now = now_ms();
        let sequence = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let id = format!("presentation-{now}-{}-{sequence}", std::process::id());
        let activity_id = format!("activity-{id}");
        let run_id = format!("run-{id}");
        let bundle_path = root.join(&id);
        let narration_path = bundle_path.join("narration");
        fs::create_dir_all(&narration_path)?;
        fs::create_dir_all(bundle_path.join("assets"))?;
        write_json_atomic(
            &bundle_path.join("evidence.json"),
            &Vec::<serde_json::Value>::new(),
        )?;
        write_json_atomic(
            &bundle_path.join("compiled-presentation.json"),
            &CompiledPresentation {
                schema_version: PRESENTATION_SCHEMA_VERSION,
                compiler_version: COMPILER_VERSION,
                slides: Vec::new(),
            },
        )?;
        let events = BufWriter::new(
            OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(bundle_path.join("source-events.jsonl"))?,
        );
        let manifest = PresentationManifest {
            schema_version: PRESENTATION_SCHEMA_VERSION,
            protocol_version: PROTOCOL_VERSION,
            compiler_version: COMPILER_VERSION,
            presentation_id: id,
            activity_id,
            run_id,
            title: bounded_title(request),
            request: request.chars().take(4_096).collect(),
            status: PresentationStatus::Authoring,
            created_at_ms: now,
            completed_at_ms: None,
            duration_ms: 0,
            slide_count: 0,
            summary: String::new(),
            model: "openai-codex/gpt-5.6-luna".into(),
        };
        write_json_atomic(&bundle_path.join("manifest.json"), &manifest)?;
        Ok(Self {
            bundle_path,
            narration_path,
            manifest,
            events,
            slides: Vec::new(),
            authoring_finished: false,
        })
    }

    pub fn path(&self) -> &Path {
        &self.bundle_path
    }

    pub fn narration_dir(&self) -> &Path {
        &self.narration_path
    }

    pub fn presentation_id(&self) -> &str {
        &self.manifest.presentation_id
    }

    pub fn record_events(&mut self, batch: &[EventEnvelope]) -> io::Result<()> {
        for envelope in batch {
            serde_json::to_writer(&mut self.events, envelope)?;
            self.events.write_all(b"\n")?;
            match &envelope.event {
                WorkEvent::SlidePlanned { slide } => self.slides.push(slide.clone()),
                WorkEvent::SessionCompleted {
                    status, summary, ..
                } => {
                    self.manifest.summary = summary.chars().take(1_024).collect();
                    self.manifest.status = match status {
                        foyer_shell_protocol::CompletionStatus::Completed => {
                            PresentationStatus::PreparingAudio
                        }
                        foyer_shell_protocol::CompletionStatus::Partial => {
                            PresentationStatus::Partial
                        }
                        foyer_shell_protocol::CompletionStatus::Cancelled => {
                            PresentationStatus::Cancelled
                        }
                        foyer_shell_protocol::CompletionStatus::Failed => {
                            PresentationStatus::Failed
                        }
                    };
                }
                _ => {}
            }
        }
        self.events.flush()?;
        Ok(())
    }

    pub fn record_evidence(&self, evidence: &str) -> io::Result<()> {
        #[derive(Serialize)]
        struct RetainedEvidence<'a> {
            schema_version: u16,
            briefing: &'a str,
        }
        write_json_atomic(
            &self.bundle_path.join("evidence.json"),
            &RetainedEvidence {
                schema_version: PRESENTATION_SCHEMA_VERSION,
                briefing: evidence,
            },
        )
    }

    pub fn finish_authoring(&mut self) -> io::Result<()> {
        if self.authoring_finished {
            return Ok(());
        }
        for (ordinal, slide) in self.slides.iter_mut().enumerate() {
            compile_slide(slide, ordinal);
        }
        self.manifest.slide_count = self.slides.len();
        write_json_atomic(
            &self.bundle_path.join("compiled-presentation.json"),
            &CompiledPresentation {
                schema_version: PRESENTATION_SCHEMA_VERSION,
                compiler_version: COMPILER_VERSION,
                slides: self.slides.clone(),
            },
        )?;
        if self.manifest.status == PresentationStatus::Authoring {
            self.manifest.status = PresentationStatus::PreparingAudio;
        }
        write_json_atomic(&self.bundle_path.join("manifest.json"), &self.manifest)?;
        self.authoring_finished = true;
        Ok(())
    }

    pub fn finish_audio(&mut self) -> io::Result<()> {
        self.finish_authoring()?;
        let (recorded, duration_ms) = narration_summary(&self.narration_path)?;
        self.manifest.duration_ms = duration_ms;
        if self.manifest.status == PresentationStatus::PreparingAudio {
            self.manifest.status =
                if self.manifest.slide_count > 0 && recorded == self.manifest.slide_count {
                    PresentationStatus::Completed
                } else {
                    PresentationStatus::Partial
                };
        }
        self.manifest.completed_at_ms = Some(now_ms());
        write_json_atomic(&self.bundle_path.join("manifest.json"), &self.manifest)
    }
}

pub fn presentation_root() -> PathBuf {
    if let Some(path) = env::var_os("FOYER_SHELL_PRESENTATIONS_PATH") {
        return PathBuf::from(path);
    }
    let data_root = foyer_shell_paths::data_root();
    let legacy = data_root.join("scenes");
    let current = data_root.join("presentations");
    if !current.exists() && legacy.is_dir() {
        let _ = fs::rename(legacy, &current);
    }
    migrate_legacy_bundles(&current);
    current
}

fn migrate_legacy_bundles(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let bundle = entry.path();
        if !bundle.is_dir() {
            continue;
        }
        let legacy_compiled = bundle.join("compiled-scene.json");
        let compiled = bundle.join("compiled-presentation.json");
        if !compiled.exists() && legacy_compiled.is_file() {
            let _ = fs::rename(legacy_compiled, compiled);
        }

        let manifest_path = bundle.join("manifest.json");
        let Ok(bytes) = fs::read(&manifest_path) else {
            continue;
        };
        let Ok(mut manifest) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue;
        };
        let Some(object) = manifest.as_object_mut() else {
            continue;
        };
        let Some(id) = object.remove("explanation_id") else {
            continue;
        };
        object.insert("presentation_id".into(), id);
        let _ = write_json_atomic(&manifest_path, &manifest);
    }
}

pub fn narration_file_name(beat_id: &str) -> String {
    let bounded = beat_id
        .chars()
        .take(96)
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    format!("{}.wav", if bounded.is_empty() { "beat" } else { &bounded })
}

fn bounded_title(request: &str) -> String {
    let title = request.split_whitespace().collect::<Vec<_>>().join(" ");
    let title = title.chars().take(96).collect::<String>();
    if title.is_empty() {
        "Untitled presentation".into()
    } else {
        title
    }
}

fn narration_summary(path: &Path) -> io::Result<(usize, u64)> {
    let Ok(entries) = fs::read_dir(path) else {
        return Ok((0, 0));
    };
    let mut count = 0;
    let mut duration = 0;
    for entry in entries.filter_map(Result::ok) {
        let bytes = fs::read(entry.path())?;
        if bytes.len() >= 44 && &bytes[..4] == b"RIFF" {
            let rate = u32::from_le_bytes(bytes[24..28].try_into().unwrap_or_default());
            let data = u32::from_le_bytes(bytes[40..44].try_into().unwrap_or_default());
            if rate > 0 {
                duration += u64::from(data) * 1_000 / (u64::from(rate) * 2);
            }
            count += 1;
        }
    }
    Ok((count, duration))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> io::Result<T> {
    serde_json::from_slice(&fs::read(path)?)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let temporary = path.with_extension("tmp");
    let mut file = File::create(&temporary)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(temporary, path)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use foyer_shell_protocol::{CompletionStatus, EventEnvelope, WorkEvent};

    #[test]
    fn narration_names_cannot_escape_the_bundle() {
        assert_eq!(
            narration_file_name("../../slide one"),
            "------slide-one.wav"
        );
    }

    #[test]
    fn status_strings_are_stable() {
        assert_eq!(
            PresentationStatus::PreparingAudio.as_str(),
            "preparing_audio"
        );
    }

    #[test]
    fn completed_bundle_reopens_without_runtime_state() {
        let root = env::temp_dir().join(format!(
            "foyer-shell-presentation-test-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let mut recorder =
            PresentationRecorder::begin_in(&root, "Explain durable presentations").unwrap();
        let session_id = recorder.presentation_id().to_string();
        recorder
            .record_events(&[
                EventEnvelope::new(
                    0,
                    0,
                    WorkEvent::SessionStarted {
                        session_id,
                        goal: "Explain durable presentations".into(),
                    },
                ),
                EventEnvelope::new(
                    1,
                    10,
                    WorkEvent::SessionCompleted {
                        status: CompletionStatus::Completed,
                        summary: "Done".into(),
                        answer_markdown: "Done".into(),
                        artifact_ids: Vec::new(),
                    },
                ),
            ])
            .unwrap();
        recorder.finish_audio().unwrap();
        let bundle = PresentationBundle::open(recorder.path()).unwrap();
        assert_eq!(bundle.manifest.request, "Explain durable presentations");
        assert_eq!(bundle.manifest.status, PresentationStatus::Partial);
        assert!(bundle.path.join("source-events.jsonl").is_file());
        assert!(bundle.path.join("compiled-presentation.json").is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn authoring_bundle_is_immediately_discoverable() {
        let root = env::temp_dir().join(format!(
            "foyer-shell-presentation-authoring-test-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let recorder = PresentationRecorder::begin_at(&root, "Explain persistence").unwrap();
        let bundle = PresentationBundle::open(recorder.path()).unwrap();

        assert_eq!(bundle.manifest.status, PresentationStatus::Authoring);
        assert!(bundle.slides.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn migrates_legacy_bundle_names_once() {
        let root = env::temp_dir().join(format!(
            "foyer-shell-presentation-migration-test-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let bundle = root.join("legacy");
        fs::create_dir_all(&bundle).unwrap();
        write_json_atomic(
            &bundle.join("manifest.json"),
            &serde_json::json!({
                "schema_version": PRESENTATION_SCHEMA_VERSION,
                "protocol_version": PROTOCOL_VERSION,
                "compiler_version": COMPILER_VERSION,
                "explanation_id": "legacy",
                "activity_id": "activity-legacy",
                "run_id": "run-legacy",
                "title": "Legacy presentation",
                "request": "Replay it",
                "status": "completed",
                "created_at_ms": 1,
                "completed_at_ms": 2,
                "duration_ms": 3,
                "slide_count": 0,
                "summary": "",
                "model": "test"
            }),
        )
        .unwrap();
        write_json_atomic(
            &bundle.join("compiled-scene.json"),
            &serde_json::json!({
                "schema_version": PRESENTATION_SCHEMA_VERSION,
                "compiler_version": COMPILER_VERSION,
                "slides": []
            }),
        )
        .unwrap();

        migrate_legacy_bundles(&root);
        let migrated = PresentationBundle::open(&bundle).unwrap();
        assert_eq!(migrated.manifest.presentation_id, "legacy");
        assert!(bundle.join("compiled-presentation.json").is_file());
        assert!(!bundle.join("compiled-scene.json").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
