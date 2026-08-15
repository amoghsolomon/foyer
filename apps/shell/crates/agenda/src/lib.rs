//! Typed agenda state backed by an optional Evolution Data Server provider.

use std::{
    collections::{HashMap, HashSet},
    env,
    io::Read as _,
    process::{Command as ProcessCommand, Stdio},
    sync::{
        Arc,
        mpsc::{self, RecvTimeoutError},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use async_channel::{Receiver, Sender};
use serde::Deserialize;
use uuid::Uuid;

const BRIDGE_SCRIPT: &str = include_str!("eds_bridge.py");
const PROTOCOL_VERSION: u32 = 1;
const REFRESH_INTERVAL: Duration = Duration::from_secs(60);
const FAILURE_RETRY_INITIAL: Duration = Duration::from_secs(5 * 60);
const FAILURE_RETRY_MAX: Duration = Duration::from_secs(30 * 60);
const BRIDGE_TIMEOUT: Duration = Duration::from_secs(32);
const ITEM_NAMESPACE: Uuid = Uuid::from_u128(0x7a284770_e6b7_5f0b_9682_780ee612715c);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Availability {
    Loading,
    Available,
    Unavailable(String),
}

impl Availability {
    pub fn detail(&self) -> &str {
        match self {
            Self::Loading => "Loading calendar and tasks…",
            Self::Available => "Available",
            Self::Unavailable(error) => error,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Calendar,
    TaskList,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgendaSource {
    pub id: String,
    pub name: String,
    pub kind: SourceKind,
    pub writable: bool,
    pub visible: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    Event,
    Task,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgendaItem {
    /// Stable opaque identity suitable for later broker and tool references.
    pub id: String,
    pub source_id: String,
    pub kind: ItemKind,
    pub title: String,
    pub description: String,
    pub location: String,
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
    pub due_ms: Option<i64>,
    pub all_day: bool,
    pub completed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub availability: Availability,
    pub sources: Arc<Vec<AgendaSource>>,
    /// Items from visible sources, sorted by actionable time and bounded by the provider.
    pub items: Arc<Vec<AgendaItem>>,
    pub last_updated_ms: Option<i64>,
    pub last_error: Option<String>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            availability: Availability::Loading,
            sources: Arc::new(Vec::new()),
            items: Arc::new(Vec::new()),
            last_updated_ms: None,
            last_error: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Controller {
    commands: mpsc::Sender<Command>,
}

impl Controller {
    pub fn refresh(&self) {
        self.send(Command::Refresh);
    }

    /// Applies Foyer Shell-owned source preferences. This is deliberately semantic and contains no EDS
    /// query or mutation escape hatch, so a future broker can safely share the same boundary.
    pub fn set_hidden_sources(&self, source_ids: Vec<String>) {
        self.send(Command::SetHiddenSources(source_ids));
    }

    fn send(&self, command: Command) {
        if self.commands.send(command).is_err() {
            tracing::warn!("agenda worker is not running");
        }
    }
}

pub struct Runtime {
    pub updates: Receiver<Snapshot>,
    pub controller: Controller,
}

pub fn start() -> Runtime {
    let (updates_tx, updates) = async_channel::unbounded();
    let (commands, command_rx) = mpsc::channel();
    thread::Builder::new()
        .name("foyer-shell-agenda".into())
        .spawn(move || run_worker(EdsProvider, updates_tx, command_rx))
        .expect("failed to start agenda worker");
    Runtime {
        updates,
        controller: Controller { commands },
    }
}

#[derive(Debug)]
enum Command {
    Refresh,
    SetHiddenSources(Vec<String>),
}

trait Provider {
    fn load(&mut self) -> Result<ProviderSnapshot, String>;
}

#[derive(Default)]
struct EdsProvider;

impl Provider for EdsProvider {
    fn load(&mut self) -> Result<ProviderSnapshot, String> {
        load_eds()
    }
}

fn run_worker(
    mut provider: impl Provider,
    updates: Sender<Snapshot>,
    commands: mpsc::Receiver<Command>,
) {
    let mut hidden_sources = HashSet::new();
    let mut provider_snapshot = None;
    let mut refresh = true;
    let mut failure_retry = FAILURE_RETRY_INITIAL;
    let mut next_refresh = Instant::now();

    loop {
        if refresh {
            provider_snapshot = Some(match provider.load() {
                Ok(snapshot) => snapshot,
                Err(error) => ProviderSnapshot::unavailable(error),
            });
            refresh = false;
            let delay = refresh_delay(
                provider_snapshot.as_ref().expect("provider snapshot"),
                &mut failure_retry,
            );
            next_refresh = Instant::now() + delay;
        }
        let snapshot = public_snapshot(
            provider_snapshot.as_ref().expect("provider snapshot"),
            &hidden_sources,
        );
        if updates.send_blocking(snapshot).is_err() {
            return;
        }

        let remaining = next_refresh.saturating_duration_since(Instant::now());
        match commands.recv_timeout(remaining) {
            Ok(Command::Refresh) | Err(RecvTimeoutError::Timeout) => refresh = true,
            Ok(Command::SetHiddenSources(source_ids)) => {
                hidden_sources = source_ids.into_iter().take(128).collect();
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn refresh_delay(snapshot: &ProviderSnapshot, failure_retry: &mut Duration) -> Duration {
    if matches!(snapshot.availability, Availability::Available) {
        *failure_retry = FAILURE_RETRY_INITIAL;
        REFRESH_INTERVAL
    } else {
        let delay = *failure_retry;
        *failure_retry = failure_retry.saturating_mul(2).min(FAILURE_RETRY_MAX);
        delay
    }
}

#[derive(Clone, Debug)]
struct ProviderSnapshot {
    availability: Availability,
    sources: Vec<AgendaSource>,
    items: Vec<AgendaItem>,
    last_updated_ms: Option<i64>,
    last_error: Option<String>,
}

impl ProviderSnapshot {
    fn unavailable(error: String) -> Self {
        Self {
            availability: Availability::Unavailable(error.clone()),
            sources: Vec::new(),
            items: Vec::new(),
            last_updated_ms: None,
            last_error: Some(error),
        }
    }
}

/// Builds a normalized agenda snapshot from hosted calendar and task records.
/// Hidden source ids remain Foyer Shell-owned visibility preferences.
pub fn compose(
    availability: Availability,
    sources: Vec<AgendaSource>,
    items: Vec<AgendaItem>,
    hidden: impl IntoIterator<Item = impl AsRef<str>>,
    last_error: Option<String>,
    last_updated_ms: Option<i64>,
) -> Snapshot {
    let hidden = hidden
        .into_iter()
        .map(|id| id.as_ref().to_string())
        .collect::<HashSet<_>>();
    public_snapshot(
        &ProviderSnapshot {
            availability,
            sources,
            items,
            last_updated_ms,
            last_error,
        },
        &hidden,
    )
}

fn public_snapshot(provider: &ProviderSnapshot, hidden: &HashSet<String>) -> Snapshot {
    let mut sources = provider.sources.clone();
    for source in &mut sources {
        source.visible = !hidden.contains(&source.id);
    }
    let visible = sources
        .iter()
        .filter(|source| source.visible)
        .map(|source| source.id.as_str())
        .collect::<HashSet<_>>();
    let items = provider
        .items
        .iter()
        .filter(|item| visible.contains(item.source_id.as_str()))
        .cloned()
        .collect();
    Snapshot {
        availability: provider.availability.clone(),
        sources: Arc::new(sources),
        items: Arc::new(items),
        last_updated_ms: provider.last_updated_ms,
        last_error: provider.last_error.clone(),
    }
}

fn load_eds() -> Result<ProviderSnapshot, String> {
    let python = env::var_os("FOYER_SHELL_AGENDA_PYTHON").unwrap_or_else(|| "python3".into());
    let now = now_ms();
    let start_seconds = now.saturating_sub(24 * 60 * 60 * 1_000) / 1_000;
    let end_seconds = now.saturating_add(45 * 24 * 60 * 60 * 1_000) / 1_000;
    let mut child = ProcessCommand::new(python)
        .args(["-c", BRIDGE_SCRIPT])
        .env(
            "FOYER_SHELL_AGENDA_START_SECONDS",
            start_seconds.to_string(),
        )
        .env("FOYER_SHELL_AGENDA_END_SECONDS", end_seconds.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("EDS agenda bridge is unavailable: {error}"))?;
    let stdout_reader = child.stdout.take().map(|mut pipe| {
        thread::spawn(move || {
            let mut output = String::new();
            let _ = pipe.read_to_string(&mut output);
            output
        })
    });
    let stderr_reader = child.stderr.take().map(|mut pipe| {
        thread::spawn(move || {
            let mut output = String::new();
            let _ = pipe.read_to_string(&mut output);
            output
        })
    });

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = join_output(stdout_reader);
                let stderr = join_output(stderr_reader);
                let snapshot = normalize_bridge(&stdout);
                if status.success() {
                    return snapshot;
                }
                if let Ok(snapshot) = snapshot {
                    tracing::warn!(
                        ?status,
                        "EDS agenda bridge exited uncleanly after returning a valid snapshot"
                    );
                    return Ok(snapshot);
                }
                return Err(format!(
                    "EDS agenda bridge failed: {}",
                    first_detail(&stderr).unwrap_or("unknown bridge error")
                ));
            }
            Ok(None) if started.elapsed() < BRIDGE_TIMEOUT => {
                thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("EDS agenda bridge timed out".into());
            }
            Err(error) => return Err(format!("failed to monitor EDS agenda bridge: {error}")),
        }
    }
}

fn join_output(reader: Option<thread::JoinHandle<String>>) -> String {
    reader
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default()
}

fn first_detail(stderr: &str) -> Option<&str> {
    stderr
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.contains("dconf-CRITICAL"))
}

#[derive(Debug, Deserialize)]
struct BridgeOutput {
    protocol_version: u32,
    #[serde(default)]
    sources: Vec<BridgeSource>,
    #[serde(default)]
    items: Vec<BridgeItem>,
    #[serde(default)]
    errors: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct BridgeSource {
    id: String,
    name: String,
    kind: String,
    writable: bool,
}

#[derive(Debug, Deserialize)]
struct BridgeItem {
    source_id: String,
    component_uid: String,
    recurrence_id: Option<String>,
    kind: String,
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    location: String,
    start_ms: Option<i64>,
    end_ms: Option<i64>,
    due_ms: Option<i64>,
    #[serde(default)]
    all_day: bool,
    #[serde(default)]
    completed: bool,
}

fn normalize_bridge(json: &str) -> Result<ProviderSnapshot, String> {
    let bridge: BridgeOutput = serde_json::from_str(json.trim())
        .map_err(|error| format!("EDS agenda bridge returned invalid data: {error}"))?;
    if bridge.protocol_version != PROTOCOL_VERSION {
        return Err(format!(
            "unsupported EDS agenda bridge protocol {}",
            bridge.protocol_version
        ));
    }

    let mut sources = bridge
        .sources
        .into_iter()
        .take(64)
        .filter_map(|source| {
            let id = bounded(&source.id, 256);
            if id.is_empty() {
                return None;
            }
            let kind = match source.kind.as_str() {
                "calendar" => SourceKind::Calendar,
                "task_list" => SourceKind::TaskList,
                _ => return None,
            };
            Some(AgendaSource {
                id,
                name: non_empty_bounded(&source.name, 160, "Unnamed source"),
                kind,
                writable: source.writable,
                visible: true,
            })
        })
        .collect::<Vec<_>>();
    sources.sort_by_key(|source| source.name.to_lowercase());

    let source_kinds = sources
        .iter()
        .map(|source| (source.id.as_str(), source.kind))
        .collect::<HashMap<_, _>>();
    let mut items = bridge
        .items
        .into_iter()
        .take(1_024)
        .filter_map(|item| {
            let source_kind = *source_kinds.get(item.source_id.as_str())?;
            let kind = match (item.kind.as_str(), source_kind) {
                ("event", SourceKind::Calendar) => ItemKind::Event,
                ("task", SourceKind::TaskList) => ItemKind::Task,
                _ => return None,
            };
            let source_id = bounded(&item.source_id, 256);
            let component_uid = bounded(&item.component_uid, 256);
            if component_uid.is_empty() {
                return None;
            }
            let recurrence_id = item.recurrence_id.as_deref().unwrap_or_default();
            let identity = format!("{source_id}\u{1f}{component_uid}\u{1f}{recurrence_id}");
            Some(AgendaItem {
                id: Uuid::new_v5(&ITEM_NAMESPACE, identity.as_bytes()).to_string(),
                source_id,
                kind,
                title: non_empty_bounded(
                    &item.title,
                    512,
                    match kind {
                        ItemKind::Event => "Untitled event",
                        ItemKind::Task => "Untitled task",
                    },
                ),
                description: bounded(&item.description, 4_096),
                location: bounded(&item.location, 256),
                start_ms: item.start_ms,
                end_ms: item.end_ms,
                due_ms: item.due_ms,
                all_day: item.all_day,
                completed: item.completed,
            })
        })
        .collect::<Vec<_>>();
    items.sort_by_key(|item| {
        (
            item.completed,
            item.start_ms.or(item.due_ms).unwrap_or(i64::MAX),
            item.title.to_lowercase(),
        )
    });

    let errors = bridge
        .errors
        .into_iter()
        .map(|error| bounded(&error, 240))
        .filter(|error| !error.is_empty())
        .take(4)
        .collect::<Vec<_>>();
    let last_error = (!errors.is_empty()).then(|| errors.join(" · "));
    let availability = if sources.is_empty() && last_error.is_some() {
        Availability::Unavailable(last_error.clone().expect("checked"))
    } else {
        Availability::Available
    };
    Ok(ProviderSnapshot {
        availability,
        sources,
        items,
        last_updated_ms: Some(now_ms()),
        last_error,
    })
}

fn bounded(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn non_empty_bounded(value: &str, max_chars: usize, fallback: &str) -> String {
    let value = bounded(value.trim(), max_chars);
    if value.is_empty() {
        fallback.into()
    } else {
        value
    }
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

    fn fixture() -> ProviderSnapshot {
        normalize_bridge(
            r#"{
                "protocol_version": 1,
                "sources": [
                    {"id":"cal-1","name":"Personal","kind":"calendar","writable":true},
                    {"id":"tasks-1","name":"Tasks","kind":"task_list","writable":true}
                ],
                "items": [
                    {"source_id":"cal-1","component_uid":"event-1","recurrence_id":null,
                     "kind":"event","title":"Standup","start_ms":2000,"end_ms":3000,
                     "all_day":false,"completed":false},
                    {"source_id":"tasks-1","component_uid":"task-1","recurrence_id":null,
                     "kind":"task","title":"Ship it","due_ms":1000,"completed":false}
                ]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn normalizes_and_orders_bridge_items() {
        let snapshot = fixture();
        assert_eq!(snapshot.sources.len(), 2);
        assert_eq!(snapshot.items.len(), 2);
        assert_eq!(snapshot.items[0].title, "Ship it");
        assert_eq!(snapshot.items[1].title, "Standup");
        assert_eq!(snapshot.items[1].id.len(), 36);
    }

    #[test]
    fn compose_applies_hosted_visibility() {
        let snapshot = compose(
            Availability::Available,
            fixture().sources,
            fixture().items,
            ["cal-1"],
            None,
            Some(1),
        );
        assert_eq!(snapshot.sources.len(), 2);
        assert!(
            !snapshot
                .sources
                .iter()
                .find(|source| source.id == "cal-1")
                .unwrap()
                .visible
        );
        assert_eq!(snapshot.items.len(), 1);
        assert_eq!(snapshot.items[0].kind, ItemKind::Task);
    }

    #[test]
    fn source_preferences_filter_items_without_losing_sources() {
        let provider = fixture();
        let hidden = HashSet::from(["cal-1".to_string()]);
        let snapshot = public_snapshot(&provider, &hidden);
        assert_eq!(snapshot.sources.len(), 2);
        assert!(
            !snapshot
                .sources
                .iter()
                .find(|source| source.id == "cal-1")
                .unwrap()
                .visible
        );
        assert_eq!(snapshot.items.len(), 1);
        assert_eq!(snapshot.items[0].kind, ItemKind::Task);
    }

    #[test]
    fn rejects_unknown_protocol_versions() {
        let error = normalize_bridge(r#"{"protocol_version":99}"#).unwrap_err();
        assert!(error.contains("protocol 99"));
    }

    #[test]
    fn global_bridge_error_is_unavailable() {
        let snapshot = normalize_bridge(
            r#"{"protocol_version":1,"sources":[],"items":[],"errors":["No user bus"]}"#,
        )
        .unwrap();
        assert_eq!(
            snapshot.availability,
            Availability::Unavailable("No user bus".into())
        );
    }

    #[test]
    fn failed_providers_back_off_and_success_resets_the_delay() {
        let failed = ProviderSnapshot::unavailable("offline".into());
        let mut retry = FAILURE_RETRY_INITIAL;
        assert_eq!(refresh_delay(&failed, &mut retry), FAILURE_RETRY_INITIAL);
        assert_eq!(
            refresh_delay(&failed, &mut retry),
            FAILURE_RETRY_INITIAL * 2
        );

        let available = fixture();
        assert_eq!(refresh_delay(&available, &mut retry), REFRESH_INTERVAL);
        assert_eq!(retry, FAILURE_RETRY_INITIAL);
    }
}
