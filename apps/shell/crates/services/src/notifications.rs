use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use async_channel::{Receiver, Sender};
use zbus::{
    blocking::{Connection, connection::Builder},
    fdo::{RequestNameFlags, RequestNameReply},
    zvariant::OwnedValue,
};

use crate::Availability;

const BUS_NAME: &str = "org.freedesktop.Notifications";
const OBJECT_PATH: &str = "/org/freedesktop/Notifications";
const INTERFACE: &str = "org.freedesktop.Notifications";
const RETRY_INTERVAL: Duration = Duration::from_secs(2);
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Urgency {
    Low,
    Normal,
    Critical,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationAction {
    pub key: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notification {
    pub id: u32,
    pub app_name: String,
    pub summary: String,
    pub body: String,
    pub urgency: Urgency,
    pub actions: Vec<NotificationAction>,
    pub desktop_entry: Option<String>,
    pub resident: bool,
    /// `None` is a persistent notification, as requested by a zero timeout.
    pub timeout: Option<Duration>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum CloseReason {
    Expired = 1,
    DismissedByUser = 2,
    ClosedByCall = 3,
    Undefined = 4,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Status(Availability),
    Show(Notification),
    Close(u32),
}

#[derive(Clone, Debug)]
pub struct Controller {
    commands: mpsc::Sender<Command>,
}

impl Controller {
    pub fn closed(&self, id: u32, reason: CloseReason) {
        if self.commands.send(Command::Closed(id, reason)).is_err() {
            tracing::warn!(id, "notification daemon is not running");
        }
    }

    pub fn invoke(&self, id: u32, action_key: String) {
        if self
            .commands
            .send(Command::Invoked(id, action_key))
            .is_err()
        {
            tracing::warn!(id, "notification daemon is not running");
        }
    }
}

pub struct Runtime {
    pub events: Receiver<Event>,
    pub controller: Controller,
}

pub fn start() -> Runtime {
    let (events_tx, events) = async_channel::unbounded();
    let (commands, command_rx) = mpsc::channel();
    thread::Builder::new()
        .name("foyer-shell-notifications".into())
        .spawn(move || run(events_tx, command_rx))
        .expect("failed to start notification daemon worker");
    Runtime {
        events,
        controller: Controller { commands },
    }
}

#[derive(Clone, Debug)]
enum Command {
    Closed(u32, CloseReason),
    Invoked(u32, String),
}

struct NotificationInterface {
    events: Sender<Event>,
    active_ids: Arc<Mutex<HashSet<u32>>>,
    next_id: AtomicU32,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl NotificationInterface {
    fn get_capabilities(&self) -> Vec<String> {
        vec!["actions".into(), "body".into(), "persistence".into()]
    }

    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        _app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let id = self.notification_id(replaces_id);
        let urgency = hints
            .get("urgency")
            .and_then(|value| u8::try_from(value).ok())
            .map(|value| match value {
                0 => Urgency::Low,
                2 => Urgency::Critical,
                _ => Urgency::Normal,
            })
            .unwrap_or(Urgency::Normal);
        let notification = Notification {
            id,
            app_name: plain_text(&app_name, 80),
            summary: plain_text(&summary, 160),
            body: plain_text(&body, 1_024),
            urgency,
            actions: notification_actions(actions),
            desktop_entry: hint_string(&hints, "desktop-entry", 160),
            resident: hint_bool(&hints, "resident"),
            timeout: timeout(expire_timeout, urgency),
        };
        let _ = self.events.send_blocking(Event::Show(notification));
        id
    }

    fn close_notification(&self, id: u32) {
        let is_active = self
            .active_ids
            .lock()
            .is_ok_and(|active_ids| active_ids.contains(&id));
        if is_active {
            let _ = self.events.send_blocking(Event::Close(id));
        }
    }

    fn get_server_information(&self) -> (String, String, String, String) {
        (
            "Foyer Shell".into(),
            "Amazity".into(),
            env!("CARGO_PKG_VERSION").into(),
            "1.2".into(),
        )
    }
}

impl NotificationInterface {
    fn notification_id(&self, replaces_id: u32) -> u32 {
        let mut active_ids = self
            .active_ids
            .lock()
            .expect("notification ID set poisoned");
        if replaces_id != 0 && active_ids.contains(&replaces_id) {
            return replaces_id;
        }

        loop {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed).max(1);
            if active_ids.insert(id) {
                return id;
            }
        }
    }
}

fn run(events: Sender<Event>, commands: mpsc::Receiver<Command>) {
    let active_ids = Arc::new(Mutex::new(HashSet::new()));
    loop {
        match connect(events.clone(), active_ids.clone()) {
            Ok(connection) => {
                if events
                    .send_blocking(Event::Status(Availability::Available))
                    .is_err()
                {
                    return;
                }
                if serve_commands(&connection, &events, &commands, &active_ids) {
                    return;
                }
            }
            Err(error) => {
                if events
                    .send_blocking(Event::Status(Availability::Unavailable(error)))
                    .is_err()
                {
                    return;
                }
                match commands.recv_timeout(RETRY_INTERVAL) {
                    Ok(Command::Closed(id, _)) => remove_id(&active_ids, id),
                    Ok(Command::Invoked(_, _)) => {}
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
        }
    }
}

fn connect(
    events: Sender<Event>,
    active_ids: Arc<Mutex<HashSet<u32>>>,
) -> Result<Connection, String> {
    let interface = NotificationInterface {
        events,
        active_ids,
        next_id: AtomicU32::new(1),
    };
    let connection = Builder::session()
        .map_err(|error| error.to_string())?
        .serve_at(OBJECT_PATH, interface)
        .map_err(|error| error.to_string())?
        .build()
        .map_err(|error| error.to_string())?;
    let reply = connection
        .request_name_with_flags(BUS_NAME, RequestNameFlags::DoNotQueue.into())
        .map_err(|error| error.to_string())?;
    match reply {
        RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner => Ok(connection),
        RequestNameReply::Exists | RequestNameReply::InQueue => {
            Err("Another notification daemon owns the session-bus name".into())
        }
    }
}

/// Returns true when every controller has been dropped and the worker should stop.
fn serve_commands(
    connection: &Connection,
    events: &Sender<Event>,
    commands: &mpsc::Receiver<Command>,
    active_ids: &Arc<Mutex<HashSet<u32>>>,
) -> bool {
    loop {
        match commands.recv_timeout(RETRY_INTERVAL) {
            Ok(Command::Closed(id, reason)) => {
                remove_id(active_ids, id);
                if let Err(error) = connection.emit_signal(
                    Option::<&str>::None,
                    OBJECT_PATH,
                    INTERFACE,
                    "NotificationClosed",
                    &(id, reason as u32),
                ) {
                    tracing::warn!(%error, "notification bus connection was lost");
                    let _ = events
                        .send_blocking(Event::Status(Availability::Unavailable(error.to_string())));
                    return false;
                }
            }
            Ok(Command::Invoked(id, action_key)) => {
                if let Err(error) = connection.emit_signal(
                    Option::<&str>::None,
                    OBJECT_PATH,
                    INTERFACE,
                    "ActionInvoked",
                    &(id, action_key),
                ) {
                    tracing::warn!(%error, "notification bus connection was lost");
                    let _ = events
                        .send_blocking(Event::Status(Availability::Unavailable(error.to_string())));
                    return false;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if connection.is_closed() {
                    let _ = events.send_blocking(Event::Status(Availability::Unavailable(
                        "Session bus connection closed".into(),
                    )));
                    return false;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return true,
        }
    }
}

fn remove_id(active_ids: &Arc<Mutex<HashSet<u32>>>, id: u32) {
    if let Ok(mut active_ids) = active_ids.lock() {
        active_ids.remove(&id);
    }
}

fn timeout(requested_ms: i32, urgency: Urgency) -> Option<Duration> {
    match requested_ms {
        0 => None,
        value if value > 0 => Some(Duration::from_millis(value.clamp(1_000, 30_000) as u64)),
        _ if urgency == Urgency::Critical => None,
        _ => Some(DEFAULT_TIMEOUT),
    }
}

fn notification_actions(actions: Vec<String>) -> Vec<NotificationAction> {
    actions
        .chunks_exact(2)
        .take(6)
        .filter_map(|pair| {
            let key = pair[0].chars().take(80).collect::<String>();
            let label = plain_text(&pair[1], 80);
            (!key.is_empty()).then_some(NotificationAction { key, label })
        })
        .collect()
}

fn hint_bool(hints: &HashMap<String, OwnedValue>, name: &str) -> bool {
    hints
        .get(name)
        .and_then(|value| bool::try_from(value).ok())
        .unwrap_or(false)
}

fn hint_string(
    hints: &HashMap<String, OwnedValue>,
    name: &str,
    max_chars: usize,
) -> Option<String> {
    hints
        .get(name)
        .and_then(|value| <&str>::try_from(value).ok())
        .map(|value| value.chars().take(max_chars).collect::<String>())
        .filter(|value| !value.is_empty())
}

pub fn plain_text(input: &str, max_chars: usize) -> String {
    let mut output = String::new();
    let mut in_tag = false;
    for character in input.chars() {
        match character {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => output.push(character),
            _ => {}
        }
        if output.chars().count() >= max_chars {
            break;
        }
    }
    output
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_markup_decodes_entities_and_bounds_payloads() {
        assert_eq!(
            plain_text("<b>Hello</b> &amp; <i>goodbye</i>", 128),
            "Hello & goodbye"
        );
        assert_eq!(plain_text("abcdef", 4), "abcd");
    }

    #[test]
    fn maps_notification_timeouts() {
        assert_eq!(timeout(0, Urgency::Normal), None);
        assert_eq!(timeout(-1, Urgency::Normal), Some(DEFAULT_TIMEOUT));
        assert_eq!(timeout(-1, Urgency::Critical), None);
        assert_eq!(timeout(200, Urgency::Normal), Some(Duration::from_secs(1)));
        assert_eq!(
            timeout(90_000, Urgency::Normal),
            Some(Duration::from_secs(30))
        );
    }

    #[test]
    fn parses_bounded_action_pairs() {
        let actions = notification_actions(vec![
            "default".into(),
            "Open".into(),
            "reply".into(),
            "Reply".into(),
            "dangling".into(),
        ]);
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].key, "default");
        assert_eq!(actions[1].label, "Reply");
    }

    #[test]
    fn controller_preserves_the_exact_action_key() {
        let (commands, receiver) = mpsc::channel();
        let controller = Controller { commands };
        controller.invoke(42, "archive-message".into());
        assert!(matches!(
            receiver.recv().unwrap(),
            Command::Invoked(42, key) if key == "archive-message"
        ));
    }
}
