//! Shared, typed persistence for Foyer Shell-owned durable state.

use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        mpsc::{self, RecvTimeoutError},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_channel::{Receiver, Sender};
use rusqlite::{Connection, params};

const RETRY_INTERVAL: Duration = Duration::from_secs(2);
const HISTORY_LIMIT: usize = 500;
const HISTORY_MAX_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Availability {
    Loading,
    Available,
    Unavailable(String),
}

impl Availability {
    pub fn detail(&self) -> &str {
        match self {
            Self::Loading => "Opening Foyer Shell storage…",
            Self::Available => "Available",
            Self::Unavailable(error) => error,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationUrgency {
    Low,
    Normal,
    Critical,
}

impl NotificationUrgency {
    fn as_i64(self) -> i64 {
        match self {
            Self::Low => 0,
            Self::Normal => 1,
            Self::Critical => 2,
        }
    }

    fn from_i64(value: i64) -> Self {
        match value {
            0 => Self::Low,
            2 => Self::Critical,
            _ => Self::Normal,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationRecord {
    pub id: i64,
    pub source_id: u32,
    pub app_name: String,
    pub summary: String,
    pub body: String,
    pub urgency: NotificationUrgency,
    pub received_at_ms: i64,
    pub is_read: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresentationRecord {
    pub id: String,
    pub activity_id: String,
    pub run_id: String,
    pub title: String,
    pub request: String,
    pub bundle_path: PathBuf,
    pub status: String,
    pub created_at_ms: i64,
    pub duration_ms: u64,
    pub slide_count: usize,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub availability: Availability,
    pub notifications: Arc<Vec<NotificationRecord>>,
    pub unread_count: usize,
    pub do_not_disturb: bool,
    /// EDS source identifiers hidden by the user. Event and task bodies remain owned by EDS.
    pub hidden_agenda_sources: Arc<Vec<String>>,
    pub presentations: Arc<Vec<PresentationRecord>>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            availability: Availability::Loading,
            notifications: Arc::new(Vec::new()),
            unread_count: 0,
            do_not_disturb: false,
            hidden_agenda_sources: Arc::new(Vec::new()),
            presentations: Arc::new(Vec::new()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Controller {
    commands: mpsc::Sender<Command>,
}

impl Controller {
    pub fn upsert_notification(
        &self,
        source_id: u32,
        app_name: String,
        summary: String,
        body: String,
        urgency: NotificationUrgency,
    ) {
        self.send(Command::UpsertNotification {
            source_id,
            app_name,
            summary,
            body,
            urgency,
        });
    }

    pub fn mark_notification_read(&self, source_id: u32) {
        self.send(Command::MarkNotificationRead(source_id));
    }

    pub fn mark_all_notifications_read(&self) {
        self.send(Command::MarkAllNotificationsRead);
    }

    pub fn delete_notification(&self, id: i64) {
        self.send(Command::DeleteNotification(id));
    }

    pub fn clear_notifications(&self) {
        self.send(Command::ClearNotifications);
    }

    pub fn set_do_not_disturb(&self, enabled: bool) {
        self.send(Command::SetDoNotDisturb(enabled));
    }

    pub fn set_agenda_source_visible(&self, source_id: String, visible: bool) {
        self.send(Command::SetAgendaSourceVisible { source_id, visible });
    }

    pub fn refresh_presentations(&self) {
        self.send(Command::RefreshPresentations);
    }

    fn send(&self, command: Command) {
        if self.commands.send(command).is_err() {
            tracing::warn!("storage worker is not running");
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
        .name("foyer-shell-storage".into())
        .spawn(move || run_worker(updates_tx, command_rx))
        .expect("failed to start storage worker");
    Runtime {
        updates,
        controller: Controller { commands },
    }
}

#[derive(Debug)]
enum Command {
    UpsertNotification {
        source_id: u32,
        app_name: String,
        summary: String,
        body: String,
        urgency: NotificationUrgency,
    },
    MarkNotificationRead(u32),
    MarkAllNotificationsRead,
    DeleteNotification(i64),
    ClearNotifications,
    SetDoNotDisturb(bool),
    SetAgendaSourceVisible {
        source_id: String,
        visible: bool,
    },
    RefreshPresentations,
}

fn run_worker(updates: Sender<Snapshot>, commands: mpsc::Receiver<Command>) {
    let path = database_path();
    loop {
        match Database::open(&path) {
            Ok(mut database) => {
                publish(&database, &updates);
                loop {
                    match commands.recv() {
                        Ok(command) => {
                            if let Err(error) = database.apply(command) {
                                tracing::error!(%error, "storage command failed");
                                let _ = updates.send_blocking(Snapshot {
                                    availability: Availability::Unavailable(error.to_string()),
                                    ..Snapshot::default()
                                });
                            } else {
                                publish(&database, &updates);
                            }
                        }
                        Err(_) => return,
                    }
                }
            }
            Err(error) => {
                tracing::error!(path = %path.display(), %error, "failed to open Foyer Shell database");
                if updates
                    .send_blocking(Snapshot {
                        availability: Availability::Unavailable(error.to_string()),
                        ..Snapshot::default()
                    })
                    .is_err()
                {
                    return;
                }
                match commands.recv_timeout(RETRY_INTERVAL) {
                    Ok(_) | Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        }
    }
}

fn publish(database: &Database, updates: &Sender<Snapshot>) {
    match database.snapshot() {
        Ok(snapshot) => {
            let _ = updates.send_blocking(snapshot);
        }
        Err(error) => {
            tracing::error!(%error, "failed to read storage snapshot");
            let _ = updates.send_blocking(Snapshot {
                availability: Availability::Unavailable(error.to_string()),
                ..Snapshot::default()
            });
        }
    }
}

fn database_path() -> PathBuf {
    if let Some(path) = env::var_os("FOYER_SHELL_DATABASE_PATH") {
        return PathBuf::from(path);
    }
    let root = foyer_shell_paths::data_root();
    let legacy = root.join("shell.sqlite3");
    let current = root.join("foyer-shell.sqlite3");
    if !current.exists()
        && legacy.is_file()
        && let Err(error) = fs::rename(&legacy, &current)
    {
        tracing::warn!(
            from = %legacy.display(),
            to = %current.display(),
            %error,
            "could not migrate the Foyer Shell database filename"
        );
    }
    current
}

struct Database {
    connection: Connection,
    session_id: i64,
}

impl Database {
    fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        }
        let connection = Connection::open(path)?;
        Self::initialize(connection)
    }

    #[cfg(test)]
    fn open_in_memory() -> rusqlite::Result<Self> {
        Self::initialize(Connection::open_in_memory()?)
    }

    fn initialize(connection: Connection) -> rusqlite::Result<Self> {
        connection.busy_timeout(Duration::from_secs(2))?;
        connection.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        let schema_version =
            connection.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))?;
        match schema_version {
            0 => connection.execute_batch(
                "BEGIN IMMEDIATE;
             CREATE TABLE storage_sessions (
                 id INTEGER PRIMARY KEY,
                 started_at_ms INTEGER NOT NULL
             );
             CREATE TABLE notifications (
                 id INTEGER PRIMARY KEY,
                 session_id INTEGER NOT NULL REFERENCES storage_sessions(id),
                 source_id INTEGER NOT NULL,
                 app_name TEXT NOT NULL,
                 summary TEXT NOT NULL,
                 body TEXT NOT NULL,
                 urgency INTEGER NOT NULL CHECK (urgency BETWEEN 0 AND 2),
                 received_at_ms INTEGER NOT NULL,
                 is_read INTEGER NOT NULL DEFAULT 0 CHECK (is_read IN (0, 1)),
                 UNIQUE(session_id, source_id)
             );
             CREATE INDEX notifications_received_at
                 ON notifications(received_at_ms DESC, id DESC);
             CREATE TABLE foyer_shell_preferences (
                 key TEXT PRIMARY KEY,
                 boolean_value INTEGER NOT NULL CHECK (boolean_value IN (0, 1))
             );
             CREATE TABLE agenda_source_preferences (
                 source_id TEXT PRIMARY KEY,
                 visible INTEGER NOT NULL CHECK (visible IN (0, 1))
             );
             CREATE TABLE presentations (
                 id TEXT PRIMARY KEY,
                 activity_id TEXT NOT NULL,
                 run_id TEXT NOT NULL,
                 title TEXT NOT NULL,
                 request TEXT NOT NULL,
                 bundle_path TEXT NOT NULL UNIQUE,
                 status TEXT NOT NULL,
                 created_at_ms INTEGER NOT NULL,
                 duration_ms INTEGER NOT NULL,
                 slide_count INTEGER NOT NULL
             );
             CREATE INDEX presentations_created_at
                 ON presentations(created_at_ms DESC, id DESC);
             PRAGMA user_version = 5;
             COMMIT;",
            )?,
            1 => connection.execute_batch(
                "BEGIN IMMEDIATE;
                 CREATE TABLE foyer_shell_preferences (
                     key TEXT PRIMARY KEY,
                     boolean_value INTEGER NOT NULL CHECK (boolean_value IN (0, 1))
                 );
                 CREATE TABLE agenda_source_preferences (
                     source_id TEXT PRIMARY KEY,
                     visible INTEGER NOT NULL CHECK (visible IN (0, 1))
                 );
                 CREATE TABLE presentations (
                     id TEXT PRIMARY KEY, activity_id TEXT NOT NULL, run_id TEXT NOT NULL,
                     title TEXT NOT NULL, request TEXT NOT NULL, bundle_path TEXT NOT NULL UNIQUE,
                     status TEXT NOT NULL, created_at_ms INTEGER NOT NULL,
                     duration_ms INTEGER NOT NULL, slide_count INTEGER NOT NULL
                 );
                 CREATE INDEX presentations_created_at
                     ON presentations(created_at_ms DESC, id DESC);
                 PRAGMA user_version = 5;
                 COMMIT;",
            )?,
            2 => connection.execute_batch(
                "BEGIN IMMEDIATE;
                 CREATE TABLE agenda_source_preferences (
                     source_id TEXT PRIMARY KEY,
                     visible INTEGER NOT NULL CHECK (visible IN (0, 1))
                 );
                 CREATE TABLE presentations (
                     id TEXT PRIMARY KEY, activity_id TEXT NOT NULL, run_id TEXT NOT NULL,
                     title TEXT NOT NULL, request TEXT NOT NULL, bundle_path TEXT NOT NULL UNIQUE,
                     status TEXT NOT NULL, created_at_ms INTEGER NOT NULL,
                     duration_ms INTEGER NOT NULL, slide_count INTEGER NOT NULL
                 );
                 CREATE INDEX presentations_created_at
                     ON presentations(created_at_ms DESC, id DESC);
                 PRAGMA user_version = 5;
                 COMMIT;",
            )?,
            3 => connection.execute_batch(
                "BEGIN IMMEDIATE;
                 CREATE TABLE presentations (
                     id TEXT PRIMARY KEY, activity_id TEXT NOT NULL, run_id TEXT NOT NULL,
                     title TEXT NOT NULL, request TEXT NOT NULL, bundle_path TEXT NOT NULL UNIQUE,
                     status TEXT NOT NULL, created_at_ms INTEGER NOT NULL,
                     duration_ms INTEGER NOT NULL, slide_count INTEGER NOT NULL
                 );
                 CREATE INDEX presentations_created_at
                     ON presentations(created_at_ms DESC, id DESC);
                 PRAGMA user_version = 5;
                 COMMIT;",
            )?,
            4 => {
                let has_legacy_preferences = connection.query_row(
                    "SELECT EXISTS(
                        SELECT 1 FROM sqlite_master
                        WHERE type = 'table' AND name = 'shell_preferences'
                    )",
                    [],
                    |row| row.get::<_, bool>(0),
                )?;
                let has_legacy_catalog = connection.query_row(
                    "SELECT EXISTS(
                        SELECT 1 FROM sqlite_master
                        WHERE type = 'table' AND name = 'explanations'
                    )",
                    [],
                    |row| row.get::<_, bool>(0),
                )?;
                connection.execute_batch("BEGIN IMMEDIATE;")?;
                if has_legacy_preferences {
                    connection.execute_batch(
                        "ALTER TABLE shell_preferences RENAME TO foyer_shell_preferences;",
                    )?;
                }
                if has_legacy_catalog {
                    connection.execute_batch(
                        "DROP INDEX IF EXISTS explanations_created_at;
                         ALTER TABLE explanations RENAME TO presentations;
                         CREATE INDEX presentations_created_at
                             ON presentations(created_at_ms DESC, id DESC);",
                    )?;
                }
                connection.execute_batch("PRAGMA user_version = 5; COMMIT;")?;
            }
            5 => {}
            version => {
                return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("unsupported Foyer Shell database schema version {version}"),
                    ),
                )));
            }
        }
        connection.execute(
            "INSERT INTO storage_sessions(started_at_ms) VALUES (?1)",
            [now_ms()],
        )?;
        let session_id = connection.last_insert_rowid();
        let mut database = Self {
            connection,
            session_id,
        };
        database.sync_presentations()?;
        Ok(database)
    }

    fn apply(&mut self, command: Command) -> rusqlite::Result<()> {
        match command {
            Command::UpsertNotification {
                source_id,
                app_name,
                summary,
                body,
                urgency,
            } => {
                let now = now_ms();
                let transaction = self.connection.transaction()?;
                transaction.execute(
                    "INSERT INTO notifications(
                         session_id, source_id, app_name, summary, body, urgency,
                         received_at_ms, is_read
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)
                     ON CONFLICT(session_id, source_id) DO UPDATE SET
                         app_name = excluded.app_name,
                         summary = excluded.summary,
                         body = excluded.body,
                         urgency = excluded.urgency,
                         received_at_ms = excluded.received_at_ms,
                         is_read = 0",
                    params![
                        self.session_id,
                        source_id,
                        bounded(&app_name, 80),
                        bounded(&summary, 160),
                        bounded(&body, 1_024),
                        urgency.as_i64(),
                        now,
                    ],
                )?;
                prune(&transaction, now)?;
                transaction.commit()?;
            }
            Command::MarkNotificationRead(source_id) => {
                self.connection.execute(
                    "UPDATE notifications SET is_read = 1
                     WHERE session_id = ?1 AND source_id = ?2",
                    params![self.session_id, source_id],
                )?;
            }
            Command::MarkAllNotificationsRead => {
                self.connection
                    .execute("UPDATE notifications SET is_read = 1 WHERE is_read = 0", [])?;
            }
            Command::DeleteNotification(id) => {
                self.connection
                    .execute("DELETE FROM notifications WHERE id = ?1", [id])?;
            }
            Command::ClearNotifications => {
                self.connection.execute("DELETE FROM notifications", [])?;
            }
            Command::SetDoNotDisturb(enabled) => {
                self.connection.execute(
                    "INSERT INTO foyer_shell_preferences(key, boolean_value)
                     VALUES ('do_not_disturb', ?1)
                     ON CONFLICT(key) DO UPDATE SET boolean_value = excluded.boolean_value",
                    [i64::from(enabled)],
                )?;
            }
            Command::SetAgendaSourceVisible { source_id, visible } => {
                let source_id = bounded(source_id.trim(), 256);
                if !source_id.is_empty() {
                    if visible {
                        self.connection.execute(
                            "DELETE FROM agenda_source_preferences WHERE source_id = ?1",
                            [source_id],
                        )?;
                    } else {
                        self.connection.execute(
                            "INSERT INTO agenda_source_preferences(source_id, visible)
                             VALUES (?1, 0)
                             ON CONFLICT(source_id) DO UPDATE SET visible = 0",
                            [source_id],
                        )?;
                    }
                }
            }
            Command::RefreshPresentations => self.sync_presentations()?,
        }
        Ok(())
    }

    fn sync_presentations(&mut self) -> rusqlite::Result<()> {
        self.sync_presentations_from(foyer_shell_presentation::presentation_root())
    }

    fn sync_presentations_from(&mut self, root: impl AsRef<Path>) -> rusqlite::Result<()> {
        let bundles = foyer_shell_presentation::PresentationBundle::discover_in(root)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        let transaction = self.connection.transaction()?;
        for bundle in bundles {
            let manifest = bundle.manifest;
            transaction.execute(
                "INSERT INTO presentations(
                     id, activity_id, run_id, title, request, bundle_path, status,
                     created_at_ms, duration_ms, slide_count
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(id) DO UPDATE SET
                     activity_id = excluded.activity_id,
                     run_id = excluded.run_id,
                     title = excluded.title,
                     request = excluded.request,
                     bundle_path = excluded.bundle_path,
                     status = excluded.status,
                     created_at_ms = excluded.created_at_ms,
                     duration_ms = excluded.duration_ms,
                     slide_count = excluded.slide_count",
                params![
                    manifest.presentation_id,
                    manifest.activity_id,
                    manifest.run_id,
                    manifest.title,
                    manifest.request,
                    bundle.path.to_string_lossy(),
                    manifest.status.as_str(),
                    manifest.created_at_ms,
                    manifest.duration_ms.min(i64::MAX as u64) as i64,
                    manifest.slide_count.min(i64::MAX as usize) as i64,
                ],
            )?;
        }
        transaction.commit()
    }

    fn snapshot(&self) -> rusqlite::Result<Snapshot> {
        let mut statement = self.connection.prepare(
            "SELECT id, source_id, app_name, summary, body, urgency,
                    received_at_ms, is_read
             FROM notifications
             ORDER BY received_at_ms DESC, id DESC
             LIMIT ?1",
        )?;
        let notifications = statement
            .query_map([HISTORY_LIMIT as i64], |row| {
                Ok(NotificationRecord {
                    id: row.get(0)?,
                    source_id: row.get(1)?,
                    app_name: row.get(2)?,
                    summary: row.get(3)?,
                    body: row.get(4)?,
                    urgency: NotificationUrgency::from_i64(row.get(5)?),
                    received_at_ms: row.get(6)?,
                    is_read: row.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let unread_count = self.connection.query_row(
            "SELECT COUNT(*) FROM notifications WHERE is_read = 0",
            [],
            |row| row.get::<_, i64>(0),
        )? as usize;
        let do_not_disturb = self
            .connection
            .query_row(
                "SELECT boolean_value FROM foyer_shell_preferences WHERE key = 'do_not_disturb'",
                [],
                |row| row.get::<_, bool>(0),
            )
            .unwrap_or(false);
        let mut agenda_statement = self.connection.prepare(
            "SELECT source_id FROM agenda_source_preferences
             WHERE visible = 0 ORDER BY source_id LIMIT 128",
        )?;
        let hidden_agenda_sources = agenda_statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut presentation_statement = self.connection.prepare(
            "SELECT id, activity_id, run_id, title, request, bundle_path, status,
                    created_at_ms, duration_ms, slide_count
             FROM presentations
             ORDER BY created_at_ms DESC, id DESC
             LIMIT 256",
        )?;
        let presentations = presentation_statement
            .query_map([], |row| {
                Ok(PresentationRecord {
                    id: row.get(0)?,
                    activity_id: row.get(1)?,
                    run_id: row.get(2)?,
                    title: row.get(3)?,
                    request: row.get(4)?,
                    bundle_path: PathBuf::from(row.get::<_, String>(5)?),
                    status: row.get(6)?,
                    created_at_ms: row.get(7)?,
                    duration_ms: row.get::<_, i64>(8)?.max(0) as u64,
                    slide_count: row.get::<_, i64>(9)?.max(0) as usize,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(Snapshot {
            availability: Availability::Available,
            notifications: Arc::new(notifications),
            unread_count,
            do_not_disturb,
            hidden_agenda_sources: Arc::new(hidden_agenda_sources),
            presentations: Arc::new(presentations),
        })
    }
}

fn prune(transaction: &rusqlite::Transaction<'_>, now: i64) -> rusqlite::Result<()> {
    let cutoff = now - HISTORY_MAX_AGE.as_millis() as i64;
    transaction.execute(
        "DELETE FROM notifications WHERE received_at_ms < ?1",
        [cutoff],
    )?;
    transaction.execute(
        "DELETE FROM notifications
         WHERE id NOT IN (
             SELECT id FROM notifications
             ORDER BY received_at_ms DESC, id DESC
             LIMIT ?1
         )",
        [HISTORY_LIMIT as i64],
    )?;
    Ok(())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn bounded(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upsert(source_id: u32, summary: &str) -> Command {
        Command::UpsertNotification {
            source_id,
            app_name: "Test app".into(),
            summary: summary.into(),
            body: "Body".into(),
            urgency: NotificationUrgency::Normal,
        }
    }

    #[test]
    fn persists_replacements_and_read_state() {
        let mut database = Database::open_in_memory().unwrap();
        database.apply(upsert(7, "First")).unwrap();
        database.apply(upsert(7, "Replacement")).unwrap();
        let snapshot = database.snapshot().unwrap();
        assert_eq!(snapshot.notifications.len(), 1);
        assert_eq!(snapshot.notifications[0].summary, "Replacement");
        assert_eq!(snapshot.unread_count, 1);

        database.apply(Command::MarkNotificationRead(7)).unwrap();
        let snapshot = database.snapshot().unwrap();
        assert!(snapshot.notifications[0].is_read);
        assert_eq!(snapshot.unread_count, 0);
    }

    #[test]
    fn deletes_one_or_all_notifications() {
        let mut database = Database::open_in_memory().unwrap();
        database.apply(upsert(1, "One")).unwrap();
        database.apply(upsert(2, "Two")).unwrap();
        let id = database.snapshot().unwrap().notifications[0].id;
        database.apply(Command::DeleteNotification(id)).unwrap();
        assert_eq!(database.snapshot().unwrap().notifications.len(), 1);

        database.apply(Command::ClearNotifications).unwrap();
        assert!(database.snapshot().unwrap().notifications.is_empty());
    }

    #[test]
    fn persists_do_not_disturb_preference() {
        let mut database = Database::open_in_memory().unwrap();
        assert!(!database.snapshot().unwrap().do_not_disturb);
        database.apply(Command::SetDoNotDisturb(true)).unwrap();
        assert!(database.snapshot().unwrap().do_not_disturb);
    }

    #[test]
    fn persists_agenda_source_visibility() {
        let mut database = Database::open_in_memory().unwrap();
        database
            .apply(Command::SetAgendaSourceVisible {
                source_id: "calendar-1".into(),
                visible: false,
            })
            .unwrap();
        assert_eq!(
            database.snapshot().unwrap().hidden_agenda_sources.as_ref(),
            &["calendar-1".to_string()]
        );
        database
            .apply(Command::SetAgendaSourceVisible {
                source_id: "calendar-1".into(),
                visible: true,
            })
            .unwrap();
        assert!(
            database
                .snapshot()
                .unwrap()
                .hidden_agenda_sources
                .is_empty()
        );
    }

    #[test]
    fn migrates_notification_database_to_foyer_shell_preferences() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE storage_sessions (
                     id INTEGER PRIMARY KEY,
                     started_at_ms INTEGER NOT NULL
                 );
                 CREATE TABLE notifications (
                     id INTEGER PRIMARY KEY,
                     session_id INTEGER NOT NULL REFERENCES storage_sessions(id),
                     source_id INTEGER NOT NULL,
                     app_name TEXT NOT NULL,
                     summary TEXT NOT NULL,
                     body TEXT NOT NULL,
                     urgency INTEGER NOT NULL,
                     received_at_ms INTEGER NOT NULL,
                     is_read INTEGER NOT NULL DEFAULT 0,
                     UNIQUE(session_id, source_id)
                 );
                 CREATE INDEX notifications_received_at
                     ON notifications(received_at_ms DESC, id DESC);
                 PRAGMA user_version = 1;",
            )
            .unwrap();
        let database = Database::initialize(connection).unwrap();
        assert!(!database.snapshot().unwrap().do_not_disturb);
        assert_eq!(
            database
                .connection
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            5
        );
    }

    #[test]
    fn migrates_preferences_database_to_agenda_sources() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE storage_sessions (
                     id INTEGER PRIMARY KEY,
                     started_at_ms INTEGER NOT NULL
                 );
                 CREATE TABLE notifications (
                     id INTEGER PRIMARY KEY,
                     session_id INTEGER NOT NULL REFERENCES storage_sessions(id),
                     source_id INTEGER NOT NULL,
                     app_name TEXT NOT NULL,
                     summary TEXT NOT NULL,
                     body TEXT NOT NULL,
                     urgency INTEGER NOT NULL,
                     received_at_ms INTEGER NOT NULL,
                     is_read INTEGER NOT NULL DEFAULT 0,
                     UNIQUE(session_id, source_id)
                 );
                 CREATE INDEX notifications_received_at
                     ON notifications(received_at_ms DESC, id DESC);
                 CREATE TABLE foyer_shell_preferences (
                     key TEXT PRIMARY KEY,
                     boolean_value INTEGER NOT NULL CHECK (boolean_value IN (0, 1))
                 );
                 PRAGMA user_version = 2;",
            )
            .unwrap();
        let database = Database::initialize(connection).unwrap();
        assert!(
            database
                .snapshot()
                .unwrap()
                .hidden_agenda_sources
                .is_empty()
        );
        assert_eq!(
            database
                .connection
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            5
        );
    }

    #[test]
    fn migrates_schema_four_product_names() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE storage_sessions (
                     id INTEGER PRIMARY KEY,
                     started_at_ms INTEGER NOT NULL
                 );
                 CREATE TABLE notifications (
                     id INTEGER PRIMARY KEY,
                     session_id INTEGER NOT NULL REFERENCES storage_sessions(id),
                     source_id INTEGER NOT NULL,
                     app_name TEXT NOT NULL,
                     summary TEXT NOT NULL,
                     body TEXT NOT NULL,
                     urgency INTEGER NOT NULL,
                     received_at_ms INTEGER NOT NULL,
                     is_read INTEGER NOT NULL DEFAULT 0,
                     UNIQUE(session_id, source_id)
                 );
                 CREATE INDEX notifications_received_at
                     ON notifications(received_at_ms DESC, id DESC);
                 CREATE TABLE shell_preferences (
                     key TEXT PRIMARY KEY,
                     boolean_value INTEGER NOT NULL
                 );
                 INSERT INTO shell_preferences VALUES ('do_not_disturb', 1);
                 CREATE TABLE agenda_source_preferences (
                     source_id TEXT PRIMARY KEY,
                     visible INTEGER NOT NULL
                 );
                 CREATE TABLE explanations (
                     id TEXT PRIMARY KEY,
                     activity_id TEXT NOT NULL,
                     run_id TEXT NOT NULL,
                     title TEXT NOT NULL,
                     request TEXT NOT NULL,
                     bundle_path TEXT NOT NULL UNIQUE,
                     status TEXT NOT NULL,
                     created_at_ms INTEGER NOT NULL,
                     duration_ms INTEGER NOT NULL,
                     slide_count INTEGER NOT NULL
                 );
                 INSERT INTO explanations VALUES (
                     'legacy', 'activity', 'run', 'Legacy', 'Legacy request',
                     '/tmp/legacy-presentation', 'completed', 1, 2, 3
                 );
                 CREATE INDEX explanations_created_at
                     ON explanations(created_at_ms DESC, id DESC);
                 PRAGMA user_version = 4;",
            )
            .unwrap();

        let database = Database::initialize(connection).unwrap();
        let snapshot = database.snapshot().unwrap();
        assert!(snapshot.do_not_disturb);
        assert!(
            snapshot
                .presentations
                .iter()
                .any(|item| item.id == "legacy")
        );
        assert_eq!(
            database
                .connection
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            5
        );
    }

    #[test]
    fn history_survives_reopening_the_database() {
        let path = env::temp_dir().join(format!(
            "foyer-shell-storage-{}-{}.sqlite3",
            std::process::id(),
            now_ms()
        ));
        {
            let mut database = Database::open(&path).unwrap();
            database.apply(upsert(42, "Saved before restart")).unwrap();
        }
        {
            let database = Database::open(&path).unwrap();
            let snapshot = database.snapshot().unwrap();
            assert_eq!(snapshot.notifications.len(), 1);
            assert_eq!(snapshot.notifications[0].summary, "Saved before restart");
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn reused_protocol_ids_do_not_replace_an_older_session() {
        let mut database = Database::open_in_memory().unwrap();
        let older_session = database.session_id + 1;
        database
            .connection
            .execute(
                "INSERT INTO storage_sessions(id, started_at_ms) VALUES (?1, ?2)",
                params![older_session, now_ms() - 1_000],
            )
            .unwrap();
        database
            .connection
            .execute(
                "INSERT INTO notifications(
                     session_id, source_id, app_name, summary, body, urgency,
                     received_at_ms, is_read
                 ) VALUES (?1, 1, 'Old app', 'Older session', '', 1, ?2, 1)",
                params![older_session, now_ms() - 1_000],
            )
            .unwrap();

        database.apply(upsert(1, "Current session")).unwrap();
        let snapshot = database.snapshot().unwrap();
        assert_eq!(snapshot.notifications.len(), 2);
        assert_eq!(snapshot.notifications[0].summary, "Current session");
        assert_eq!(snapshot.notifications[1].summary, "Older session");
    }

    #[test]
    fn presentation_catalog_is_rebuilt_from_bundle_manifests() {
        let root = env::temp_dir().join(format!(
            "foyer-shell-catalog-test-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let mut recorder =
            foyer_shell_presentation::PresentationRecorder::begin_at(&root, "Explain the shell")
                .unwrap();
        recorder.finish_audio().unwrap();

        let mut database = Database::open_in_memory().unwrap();
        // In-memory databases exercise the same startup import as production,
        // which may discover the developer's real presentation bundles. Clear
        // that startup state so this test measures only its explicit fixture.
        database
            .connection
            .execute("DELETE FROM presentations", [])
            .unwrap();
        database.sync_presentations_from(&root).unwrap();
        let snapshot = database.snapshot().unwrap();
        assert_eq!(snapshot.presentations.len(), 1);
        assert_eq!(snapshot.presentations[0].title, "Explain the shell");
        // A recorder with no authored slides finishes as a partial bundle; the
        // catalog must preserve that durable status instead of upgrading it.
        assert_eq!(snapshot.presentations[0].status, "partial");
        assert_eq!(snapshot.presentations[0].bundle_path, recorder.path());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bounds_notification_history() {
        let mut database = Database::open_in_memory().unwrap();
        for id in 0..=HISTORY_LIMIT as u32 {
            database.apply(upsert(id, "Notification")).unwrap();
        }
        assert_eq!(
            database.snapshot().unwrap().notifications.len(),
            HISTORY_LIMIT
        );

        let old_session = database.session_id + 1;
        database
            .connection
            .execute(
                "INSERT INTO storage_sessions(id, started_at_ms) VALUES (?1, 0)",
                [old_session],
            )
            .unwrap();
        database
            .connection
            .execute(
                "INSERT INTO notifications(
                     session_id, source_id, app_name, summary, body, urgency,
                     received_at_ms, is_read
                 ) VALUES (?1, 9, 'Old app', 'Too old', '', 1, 0, 1)",
                [old_session],
            )
            .unwrap();
        database.apply(upsert(u32::MAX, "Fresh")).unwrap();
        assert!(
            database
                .snapshot()
                .unwrap()
                .notifications
                .iter()
                .all(|notification| notification.summary != "Too old")
        );
    }
}
