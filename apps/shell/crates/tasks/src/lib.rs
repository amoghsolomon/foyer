//! Hosted tasks adapter. Reads come from immutable snapshots; I/O never runs in GPUI.

use std::{
    env, fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
};

use async_channel::{Receiver, Sender};
use async_trait::async_trait;
use futures_lite::StreamExt;
use powersync::{
    BackendConnector, ConnectionPool, CrudEntry, PowerSyncCredentials, PowerSyncDatabase,
    SyncOptions,
    env::PowerSyncEnvironment,
    error::PowerSyncError,
    schema::{Column, Schema, Table},
};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use uuid::Uuid;

pub mod api;
pub mod markdown;

const CLIENT_OPERATION: &str = "client_operation";
const OPERATION_ID: &str = "operation_id";
const EXPECTED_REVISION: &str = "expected_revision";
const DELETED_LOCAL: &str = "deleted_local";
const CLIENT_PAYLOAD: &str = "client_payload";
pub const TASK_LISTS_TABLE: &str = "task_lists";
pub const TASKS_TABLE: &str = "tasks";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Availability {
    Loading,
    Available,
    Unavailable(String),
}

impl Availability {
    pub fn detail(&self) -> &str {
        match self {
            Self::Loading => "Loading tasks…",
            Self::Available => "Available",
            Self::Unavailable(error) => error,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskList {
    pub id: String,
    pub name: String,
    pub position: i32,
    pub revision: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Due {
    pub local: String,
    #[serde(rename = "timeZone", skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,
    #[serde(rename = "allDay")]
    pub all_day: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

impl Due {
    pub fn date_only(local: impl Into<String>) -> Self {
        Self {
            local: local.into(),
            time_zone: None,
            all_day: true,
            at: None,
        }
    }

    pub fn display_label(&self) -> String {
        if self.all_day {
            return self.local.clone();
        }
        match self.time_zone.as_deref() {
            Some(zone) if !zone.is_empty() => format!("{} {zone}", self.local.replace('T', " ")),
            _ => self.local.replace('T', " "),
        }
    }

    pub fn parse(local: &str, time_zone: Option<&str>, all_day: bool) -> Option<Self> {
        let local = local.trim();
        if local.is_empty() {
            return None;
        }
        if all_day {
            if local.len() != 10 || local.as_bytes()[4] != b'-' || local.as_bytes()[7] != b'-' {
                return None;
            }
            return Some(Self {
                local: local.to_string(),
                time_zone: time_zone
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string),
                all_day: true,
                at: None,
            });
        }
        if local.len() != 19 || !local.contains('T') {
            return None;
        }
        let time_zone = time_zone
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        if let Some(zone) = time_zone.as_deref()
            && zone != "UTC"
            && !zone.contains('/')
        {
            return None;
        }
        Some(Self {
            local: local.to_string(),
            time_zone,
            all_day: false,
            at: None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    #[serde(rename = "listId")]
    pub list_id: String,
    pub title: String,
    pub description: String,
    pub due: Option<Due>,
    pub priority: i32,
    pub completed: bool,
    pub position: i32,
    pub revision: i64,
    #[serde(rename = "updatedAt", default)]
    pub updated_at: String,
}

impl Task {
    pub fn summary(&self) -> String {
        markdown::summary_of(&self.description)
    }

    pub fn priority_label(&self) -> &'static str {
        match self.priority {
            0 => "None",
            1..=3 => "High",
            4..=6 => "Medium",
            _ => "Low",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub availability: Availability,
    pub development: bool,
    pub using_powersync: bool,
    pub sharing_replica: bool,
    pub offline: bool,
    pub pending_uploads: usize,
    pub last_error: Option<String>,
    pub lists: Arc<Vec<TaskList>>,
    pub tasks: Arc<Vec<Task>>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            availability: Availability::Loading,
            development: true,
            using_powersync: false,
            sharing_replica: false,
            offline: false,
            pending_uploads: 0,
            last_error: None,
            lists: Arc::new(Vec::new()),
            tasks: Arc::new(Vec::new()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncBanner {
    Offline { pending: usize },
    Pending { pending: usize },
    StaleRevision { message: String },
    Error { message: String },
}

impl Snapshot {
    pub fn list(&self, id: &str) -> Option<&TaskList> {
        self.lists.iter().find(|list| list.id == id)
    }

    pub fn task(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().find(|task| task.id == id)
    }

    pub fn tasks_in(&self, list_id: &str) -> Vec<Task> {
        let mut tasks = self
            .tasks
            .iter()
            .filter(|task| task.list_id == list_id)
            .cloned()
            .collect::<Vec<_>>();
        tasks.sort_by(task_order);
        tasks
    }

    pub fn open_tasks(&self) -> Vec<Task> {
        let mut tasks = self
            .tasks
            .iter()
            .filter(|task| !task.completed)
            .cloned()
            .collect::<Vec<_>>();
        tasks.sort_by(task_order);
        tasks
    }

    pub fn open_tasks_in(&self, list_id: &str) -> Vec<Task> {
        self.tasks_in(list_id)
            .into_iter()
            .filter(|task| !task.completed)
            .collect()
    }

    pub fn completed_tasks_in(&self, list_id: &str) -> Vec<Task> {
        self.tasks_in(list_id)
            .into_iter()
            .filter(|task| task.completed)
            .collect()
    }

    pub fn list_is_empty(&self, list_id: &str) -> bool {
        self.tasks_in(list_id).is_empty()
    }

    pub fn validate_list_delete(&self, list_id: &str) -> Result<(), String> {
        if self.list(list_id).is_none() {
            return Err("The list was not found.".into());
        }
        if self.list_is_empty(list_id) {
            Ok(())
        } else {
            Err("List is not empty. Move or delete its tasks first.".into())
        }
    }

    pub fn valid_move_targets(&self) -> Vec<TaskList> {
        let mut lists = self.lists.iter().cloned().collect::<Vec<_>>();
        lists.sort_by(|left, right| {
            left.position
                .cmp(&right.position)
                .then(left.name.cmp(&right.name))
                .then(left.id.cmp(&right.id))
        });
        lists
    }

    pub fn sync_banner(&self) -> Option<SyncBanner> {
        sync_banner(
            self.availability.clone(),
            self.offline,
            self.pending_uploads,
            self.last_error.clone(),
        )
    }
}

fn task_order(left: &Task, right: &Task) -> std::cmp::Ordering {
    left.completed
        .cmp(&right.completed)
        .then(left.position.cmp(&right.position))
        .then(
            left.due
                .as_ref()
                .map(|due| due.local.as_str())
                .unwrap_or("")
                .cmp(
                    right
                        .due
                        .as_ref()
                        .map(|due| due.local.as_str())
                        .unwrap_or(""),
                ),
        )
        .then(left.title.cmp(&right.title))
        .then(left.id.cmp(&right.id))
}

pub fn sync_banner(
    availability: Availability,
    offline: bool,
    pending_uploads: usize,
    last_error: Option<String>,
) -> Option<SyncBanner> {
    if let Availability::Unavailable(error) = availability {
        return Some(SyncBanner::Error { message: error });
    }
    if let Some(error) = last_error.filter(|value| !value.is_empty()) {
        return Some(if is_stale_revision(&error) {
            SyncBanner::StaleRevision { message: error }
        } else {
            SyncBanner::Error { message: error }
        });
    }
    if offline {
        Some(SyncBanner::Offline {
            pending: pending_uploads,
        })
    } else if pending_uploads > 0 {
        Some(SyncBanner::Pending {
            pending: pending_uploads,
        })
    } else {
        None
    }
}

fn is_stale_revision(error: &str) -> bool {
    error.contains("stale_revision") || error.to_ascii_lowercase().contains("stale revision")
}

#[derive(Clone, Debug)]
pub struct Controller {
    commands: Sender<Command>,
}

impl Controller {
    pub fn attach(commands: Sender<Command>) -> Self {
        Self { commands }
    }

    pub fn refresh(&self) {
        self.send(Command::Refresh);
    }

    pub fn create_list(&self, name: String) {
        self.send(Command::CreateList { name });
    }

    pub fn rename_list(&self, id: String, revision: i64, name: String) {
        self.send(Command::RenameList { id, revision, name });
    }

    pub fn delete_list(&self, id: String, revision: i64) {
        self.send(Command::DeleteList { id, revision });
    }

    pub fn create_task(
        &self,
        list_id: String,
        title: String,
        description: String,
        due: Option<Due>,
        priority: i32,
    ) {
        self.send(Command::CreateTask {
            list_id,
            title,
            description,
            due,
            priority,
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_task(
        &self,
        id: String,
        revision: i64,
        title: String,
        description: String,
        due: Option<Due>,
        priority: i32,
        position: i32,
    ) {
        self.send(Command::UpdateTask {
            id,
            revision,
            title,
            description,
            due,
            priority,
            position,
        });
    }

    pub fn move_task(&self, id: String, revision: i64, list_id: String) {
        self.send(Command::MoveTask {
            id,
            revision,
            list_id,
        });
    }

    pub fn complete_task(&self, id: String, revision: i64) {
        self.send(Command::CompleteTask { id, revision });
    }

    pub fn reopen_task(&self, id: String, revision: i64) {
        self.send(Command::ReopenTask { id, revision });
    }

    pub fn delete_task(&self, id: String, revision: i64) {
        self.send(Command::DeleteTask { id, revision });
    }

    fn send(&self, command: Command) {
        if self.commands.try_send(command).is_err() {
            tracing::warn!("tasks worker is not running");
        }
    }
}

pub struct Runtime {
    pub updates: Receiver<Snapshot>,
    pub controller: Controller,
}

pub fn start() -> Runtime {
    start_at(replica_path())
}

pub fn start_at(path: PathBuf) -> Runtime {
    let (updates_tx, updates) = async_channel::unbounded();
    let (commands, command_rx) = async_channel::unbounded();
    thread::Builder::new()
        .name("foyer-shell-tasks".into())
        .spawn(move || run_worker(path, updates_tx, command_rx))
        .expect("failed to start tasks worker");
    Runtime {
        updates,
        controller: Controller { commands },
    }
}

/// Prefer a shared personal replica when the host already opened Notes against one file.
pub fn replica_path() -> PathBuf {
    env::var_os("FOYER_SHELL_TASKS_REPLICA_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(foyer_shell_paths::personal_replica_path)
}

pub fn shared_notes_replica_path() -> PathBuf {
    env::var_os("FOYER_SHELL_NOTES_REPLICA_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(foyer_shell_paths::personal_replica_path)
}

pub fn schema_tables() -> Vec<Table> {
    let client_columns = || {
        vec![
            Column::text(CLIENT_OPERATION),
            Column::text(OPERATION_ID),
            Column::integer(EXPECTED_REVISION),
            Column::integer(DELETED_LOCAL),
            Column::text(CLIENT_PAYLOAD),
        ]
    };
    let mut list_columns = vec![
        Column::text("user_id"),
        Column::text("name"),
        Column::integer("position"),
        Column::text("href"),
        Column::text("etag"),
        Column::integer("revision"),
        Column::text("created_at"),
        Column::text("updated_at"),
    ];
    list_columns.extend(client_columns());
    let mut task_columns = vec![
        Column::text("user_id"),
        Column::text("list_id"),
        Column::text("title"),
        Column::text("description"),
        Column::text("due_at"),
        Column::text("due_local"),
        Column::text("due_time_zone"),
        Column::integer("due_all_day"),
        Column::integer("priority"),
        Column::integer("completed"),
        Column::text("completed_at"),
        Column::integer("position"),
        Column::text("href"),
        Column::text("etag"),
        Column::integer("revision"),
        Column::text("created_at"),
        Column::text("updated_at"),
    ];
    task_columns.extend(client_columns());
    vec![
        Table::create(TASK_LISTS_TABLE, list_columns, |_| {}),
        Table::create(TASKS_TABLE, task_columns, |_| {}),
    ]
}

pub fn tasks_schema() -> Schema {
    Schema {
        tables: schema_tables(),
        ..Schema::default()
    }
}

pub fn merge_tables(mut existing: Vec<Table>) -> Vec<Table> {
    existing.extend(schema_tables());
    existing
}

#[derive(Debug)]
pub enum Command {
    Refresh,
    CreateList {
        name: String,
    },
    RenameList {
        id: String,
        revision: i64,
        name: String,
    },
    DeleteList {
        id: String,
        revision: i64,
    },
    CreateTask {
        list_id: String,
        title: String,
        description: String,
        due: Option<Due>,
        priority: i32,
    },
    UpdateTask {
        id: String,
        revision: i64,
        title: String,
        description: String,
        due: Option<Due>,
        priority: i32,
        position: i32,
    },
    MoveTask {
        id: String,
        revision: i64,
        list_id: String,
    },
    CompleteTask {
        id: String,
        revision: i64,
    },
    ReopenTask {
        id: String,
        revision: i64,
    },
    DeleteTask {
        id: String,
        revision: i64,
    },
}

fn run_worker(path: PathBuf, updates: Sender<Snapshot>, commands: Receiver<Command>) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ =
                updates.send_blocking(unavailable(format!("create tasks async runtime: {error}")));
            return;
        }
    };
    if let Err(error) = runtime.block_on(run_async(path, updates.clone(), commands)) {
        let _ = updates.send_blocking(unavailable(error));
    }
}

async fn run_async(
    path: PathBuf,
    updates: Sender<Snapshot>,
    commands: Receiver<Command>,
) -> Result<(), String> {
    let sharing_replica = path == shared_notes_replica_path()
        || env::var_os("FOYER_SHELL_PERSONAL_REPLICA_PATH").is_some();
    updates
        .send(Snapshot {
            development: true,
            using_powersync: true,
            sharing_replica,
            ..Snapshot::default()
        })
        .await
        .map_err(|_| "tasks UI stopped receiving updates".to_string())?;

    let api = api::Client::from_env().await?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create tasks replica directory: {error}"))?;
    }
    PowerSyncEnvironment::powersync_auto_extension()
        .map_err(|error| format!("initialize PowerSync SQLite extension: {error}"))?;
    let pool = ConnectionPool::open(&path)
        .map_err(|error| format!("open tasks replica {}: {error}", path.display()))?;
    let environment = PowerSyncEnvironment::custom(
        reqwest::Client::new(),
        pool,
        PowerSyncEnvironment::tokio_timer(),
    );
    let db = PowerSyncDatabase::new(environment, tasks_schema());
    let conflict = Arc::new(Mutex::new(None));
    let connector = Connector {
        db: db.clone(),
        api,
        conflict: conflict.clone(),
    };
    let _tasks = db.async_tasks().spawn_with_tokio();
    db.connect(SyncOptions::new(connector)).await;

    let mut table_updates = Box::pin(db.watch_tables(true, [TASK_LISTS_TABLE, TASKS_TABLE]));
    let mut status_updates = Box::pin(db.watch_status());
    loop {
        tokio::select! {
            command = commands.recv() => {
                let Ok(command) = command else {
                    db.disconnect().await;
                    return Ok(());
                };
                let command_error = apply_local(&db, command).await.err();
                publish(&updates, &db, &conflict, sharing_replica, command_error).await?;
            }
            update = table_updates.next() => {
                if update.is_none() {
                    return Err("PowerSync tasks table watcher stopped".into());
                }
                publish(&updates, &db, &conflict, sharing_replica, None).await?;
            }
            status = status_updates.next() => {
                if status.is_none() {
                    return Err("PowerSync tasks status watcher stopped".into());
                }
                publish(&updates, &db, &conflict, sharing_replica, None).await?;
            }
        }
    }
}

async fn publish(
    updates: &Sender<Snapshot>,
    db: &PowerSyncDatabase,
    conflict: &Mutex<Option<String>>,
    sharing_replica: bool,
    command_error: Option<String>,
) -> Result<(), String> {
    let mut snapshot = read_snapshot(db, conflict, sharing_replica).await?;
    if command_error.is_some() {
        snapshot.last_error = command_error;
    }
    updates
        .send(snapshot)
        .await
        .map_err(|_| "tasks UI stopped receiving updates".to_string())
}

pub async fn read_snapshot(
    db: &PowerSyncDatabase,
    conflict: &Mutex<Option<String>>,
    sharing_replica: bool,
) -> Result<Snapshot, String> {
    let reader = db
        .reader()
        .await
        .map_err(|error| format!("read tasks replica: {error}"))?;
    let lists = {
        let mut statement = reader
            .prepare(
                "SELECT id, name, position, revision FROM task_lists \
                 WHERE COALESCE(deleted_local, 0) = 0",
            )
            .map_err(|error| format!("prepare list snapshot: {error}"))?;
        statement
            .query_map([], |row| {
                Ok(TaskList {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    position: row.get(2)?,
                    revision: row.get(3)?,
                })
            })
            .map_err(|error| format!("query list snapshot: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("decode list snapshot: {error}"))?
    };
    let tasks = {
        let mut statement = reader
            .prepare(
                "SELECT id, list_id, title, description, due_local, due_time_zone, due_all_day, \
                        due_at, priority, completed, position, revision, updated_at \
                 FROM tasks WHERE COALESCE(deleted_local, 0) = 0",
            )
            .map_err(|error| format!("prepare task snapshot: {error}"))?;
        statement
            .query_map([], |row| {
                let due_local: Option<String> = row.get(4)?;
                let due_zone: Option<String> = row.get(5)?;
                let due_all_day: Option<i64> = row.get(6)?;
                let due_at: Option<String> = row.get(7)?;
                let due = due_local.as_deref().and_then(|local| {
                    let mut due =
                        Due::parse(local, due_zone.as_deref(), due_all_day.unwrap_or(0) == 1)?;
                    due.at = due_at.clone();
                    Some(due)
                });
                Ok(Task {
                    id: row.get(0)?,
                    list_id: row.get(1)?,
                    title: row.get(2)?,
                    description: row.get(3)?,
                    due,
                    priority: row.get::<_, Option<i32>>(8)?.unwrap_or(0),
                    completed: row.get::<_, Option<i64>>(9)?.unwrap_or(0) != 0,
                    position: row.get::<_, Option<i32>>(10)?.unwrap_or(0),
                    revision: row.get(11)?,
                    updated_at: row.get::<_, Option<String>>(12)?.unwrap_or_default(),
                })
            })
            .map_err(|error| format!("query task snapshot: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("decode task snapshot: {error}"))?
    };
    let pending_uploads = reader
        .query_row("SELECT COUNT(*) FROM ps_crud", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|error| format!("count pending tasks writes: {error}"))?
        .max(0) as usize;
    drop(reader);

    let status = db.status();
    let sync_error = status
        .download_error()
        .or_else(|| status.upload_error())
        .map(ToString::to_string);
    let conflict = conflict
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    Ok(Snapshot {
        availability: Availability::Available,
        development: foyer_shell_auth::development_auth_enabled(),
        using_powersync: true,
        sharing_replica,
        offline: !status.is_connected(),
        pending_uploads,
        last_error: conflict.or(sync_error),
        lists: Arc::new(lists),
        tasks: Arc::new(tasks),
    })
}

pub async fn apply_local(db: &PowerSyncDatabase, command: Command) -> Result<(), String> {
    if let Command::DeleteList { id, .. } = &command {
        let snapshot = read_snapshot(db, &Mutex::new(None), false).await?;
        snapshot.validate_list_delete(id)?;
    }
    let writer = db
        .writer()
        .await
        .map_err(|error| format!("write tasks replica: {error}"))?;
    let now = chrono::Utc::now().to_rfc3339();
    let operation_id = Uuid::new_v4().to_string();
    match command {
        Command::Refresh => return Ok(()),
        Command::CreateList { name } => {
            let name = required_title(name, "List name")?;
            writer.execute(
                "INSERT INTO task_lists \
                 (id, user_id, name, position, href, etag, revision, created_at, updated_at, \
                  client_operation, operation_id, expected_revision, deleted_local) \
                 VALUES (?, '', ?, 0, '', NULL, 1, ?, ?, 'create', ?, NULL, 0)",
                params![Uuid::new_v4().to_string(), name, now, now, operation_id],
            )
        }
        Command::RenameList { id, revision, name } => {
            let name = required_title(name, "List name")?;
            writer.execute(
                "UPDATE task_lists SET name = ?, revision = ?, updated_at = ?, \
                 client_operation = 'rename', operation_id = ?, expected_revision = ? WHERE id = ?",
                params![name, revision + 1, now, operation_id, revision, id],
            )
        }
        Command::DeleteList { id, revision } => writer.execute(
            "UPDATE task_lists SET deleted_local = 1, revision = ?, updated_at = ?, \
             client_operation = 'delete', operation_id = ?, expected_revision = ? WHERE id = ?",
            params![revision + 1, now, operation_id, revision, id],
        ),
        Command::CreateTask {
            list_id,
            title,
            description,
            due,
            priority,
        } => {
            let title = required_title(title, "Task title")?;
            let payload = task_payload(&operation_id, &title, &description, due.as_ref(), priority, 0);
            writer.execute(
                "INSERT INTO tasks \
                 (id, user_id, list_id, title, description, due_at, due_local, due_time_zone, due_all_day, \
                  priority, completed, completed_at, position, href, etag, revision, created_at, updated_at, \
                  client_operation, operation_id, expected_revision, deleted_local, client_payload) \
                 VALUES (?, '', ?, ?, ?, ?, ?, ?, ?, ?, 0, NULL, 0, '', '', 1, ?, ?, 'create', ?, NULL, 0, ?)",
                params![
                    Uuid::new_v4().to_string(),
                    list_id,
                    title,
                    description,
                    due.as_ref().and_then(|due| due.at.clone()),
                    due.as_ref().map(|due| due.local.clone()),
                    due.as_ref().and_then(|due| due.time_zone.clone()),
                    due.as_ref().map(|due| i64::from(due.all_day)).unwrap_or(0),
                    priority,
                    now,
                    now,
                    operation_id,
                    payload
                ],
            )
        }
        Command::UpdateTask {
            id,
            revision,
            title,
            description,
            due,
            priority,
            position,
        } => {
            let title = required_title(title, "Task title")?;
            let payload =
                task_payload(&operation_id, &title, &description, due.as_ref(), priority, position);
            writer.execute(
                "UPDATE tasks SET title = ?, description = ?, due_at = ?, due_local = ?, due_time_zone = ?, \
                 due_all_day = ?, priority = ?, position = ?, revision = ?, updated_at = ?, \
                 client_operation = 'update', operation_id = ?, expected_revision = ?, client_payload = ? \
                 WHERE id = ?",
                params![
                    title,
                    description,
                    due.as_ref().and_then(|due| due.at.clone()),
                    due.as_ref().map(|due| due.local.clone()),
                    due.as_ref().and_then(|due| due.time_zone.clone()),
                    due.as_ref().map(|due| i64::from(due.all_day)).unwrap_or(0),
                    priority,
                    position,
                    revision + 1,
                    now,
                    operation_id,
                    revision,
                    payload,
                    id
                ],
            )
        }
        Command::MoveTask {
            id,
            revision,
            list_id,
        } => writer.execute(
            "UPDATE tasks SET list_id = ?, revision = ?, updated_at = ?, \
             client_operation = 'move', operation_id = ?, expected_revision = ? WHERE id = ?",
            params![list_id, revision + 1, now, operation_id, revision, id],
        ),
        Command::CompleteTask { id, revision } => writer.execute(
            "UPDATE tasks SET completed = 1, completed_at = ?, revision = ?, updated_at = ?, \
             client_operation = 'complete', operation_id = ?, expected_revision = ? WHERE id = ?",
            params![now, revision + 1, now, operation_id, revision, id],
        ),
        Command::ReopenTask { id, revision } => writer.execute(
            "UPDATE tasks SET completed = 0, completed_at = NULL, revision = ?, updated_at = ?, \
             client_operation = 'reopen', operation_id = ?, expected_revision = ? WHERE id = ?",
            params![revision + 1, now, operation_id, revision, id],
        ),
        Command::DeleteTask { id, revision } => writer.execute(
            "UPDATE tasks SET deleted_local = 1, revision = ?, updated_at = ?, \
             client_operation = 'delete', operation_id = ?, expected_revision = ? WHERE id = ?",
            params![revision + 1, now, operation_id, revision, id],
        ),
    }
    .map_err(|error| format!("apply local tasks command: {error}"))?;
    Ok(())
}

fn task_payload(
    operation_id: &str,
    title: &str,
    description: &str,
    due: Option<&Due>,
    priority: i32,
    position: i32,
) -> String {
    json!({
        "operationId": operation_id,
        "title": title,
        "description": description,
        "due": due,
        "priority": priority,
        "position": position,
    })
    .to_string()
}

fn required_title(value: String, label: &str) -> Result<String, String> {
    let value = value.trim().to_string();
    if value.is_empty() {
        Err(format!("{label} is required"))
    } else {
        Ok(value)
    }
}

fn unavailable(error: String) -> Snapshot {
    Snapshot {
        availability: Availability::Unavailable(error.clone()),
        development: true,
        using_powersync: true,
        offline: true,
        last_error: Some(error),
        ..Snapshot::default()
    }
}

#[derive(Clone)]
struct Connector {
    db: PowerSyncDatabase,
    api: api::Client,
    conflict: Arc<Mutex<Option<String>>>,
}

#[async_trait]
impl BackendConnector for Connector {
    async fn fetch_credentials(&self) -> Result<PowerSyncCredentials, PowerSyncError> {
        let credentials = self
            .api
            .sync_credentials()
            .await
            .map_err(PowerSyncError::upload_error)?;
        Ok(PowerSyncCredentials {
            endpoint: credentials.endpoint,
            token: credentials.token,
        })
    }

    async fn upload_data(&self) -> Result<(), PowerSyncError> {
        let Some(transaction) = self.db.next_crud_transaction().await? else {
            return Ok(());
        };
        for entry in &transaction.crud {
            if let Err(error) = self.upload(entry).await {
                if error.is_permanent_command_rejection() {
                    *self
                        .conflict
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                        Some(public_upload_error(&error));
                    transaction.complete().await?;
                    return Ok(());
                }
                return Err(PowerSyncError::upload_error(error));
            }
        }
        *self
            .conflict
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        transaction.complete().await
    }
}

impl Connector {
    async fn upload(&self, entry: &CrudEntry) -> Result<(), api::ApiError> {
        upload_entry(&self.api, entry).await
    }
}

pub async fn upload_entry(api: &api::Client, entry: &CrudEntry) -> Result<(), api::ApiError> {
    let empty = Map::new();
    let data = entry.data.as_ref().unwrap_or(&empty);
    let command = required_text(data, CLIENT_OPERATION)?;
    let operation_id = required_text(data, OPERATION_ID)?;
    match (entry.table.as_str(), command) {
        (TASK_LISTS_TABLE, "create") => {
            api.create_list(
                operation_id,
                &entry.id,
                required_text(data, "name")?,
                optional_i64(data, "position"),
            )
            .await?;
        }
        (TASK_LISTS_TABLE, "rename") => {
            api.rename_list(
                operation_id,
                &entry.id,
                required_i64(data, EXPECTED_REVISION)?,
                required_text(data, "name")?,
            )
            .await?;
        }
        (TASK_LISTS_TABLE, "delete") => {
            api.delete_list(
                operation_id,
                &entry.id,
                required_i64(data, EXPECTED_REVISION)?,
            )
            .await?;
        }
        (TASKS_TABLE, "create") => {
            let payload = decode_payload(data)?;
            api.create_task(
                operation_id,
                &entry.id,
                required_text(data, "list_id")?,
                &payload.title,
                &payload.description,
                payload.due.as_ref(),
                payload.priority,
                optional_i64(data, "position"),
            )
            .await?;
        }
        (TASKS_TABLE, "update") => {
            let payload = decode_payload(data)?;
            api.update_task(
                operation_id,
                &entry.id,
                required_i64(data, EXPECTED_REVISION)?,
                &payload.title,
                &payload.description,
                payload.due.as_ref(),
                payload.priority,
                payload.position,
            )
            .await?;
        }
        (TASKS_TABLE, "move") => {
            api.move_task(
                operation_id,
                &entry.id,
                required_i64(data, EXPECTED_REVISION)?,
                required_text(data, "list_id")?,
                optional_i64(data, "position"),
            )
            .await?;
        }
        (TASKS_TABLE, "complete") => {
            api.complete_task(
                operation_id,
                &entry.id,
                required_i64(data, EXPECTED_REVISION)?,
            )
            .await?;
        }
        (TASKS_TABLE, "reopen") => {
            api.reopen_task(
                operation_id,
                &entry.id,
                required_i64(data, EXPECTED_REVISION)?,
            )
            .await?;
        }
        (TASKS_TABLE, "delete") => {
            api.delete_task(
                operation_id,
                &entry.id,
                required_i64(data, EXPECTED_REVISION)?,
            )
            .await?;
        }
        (table, operation) => {
            return Err(api::ApiError::InvalidCommand(format!(
                "unsupported local tasks command {table}/{operation}"
            )));
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct TaskPayload {
    title: String,
    description: String,
    due: Option<Due>,
    #[serde(default)]
    priority: i32,
    #[serde(default)]
    position: i32,
}

fn decode_payload(data: &Map<String, Value>) -> Result<TaskPayload, api::ApiError> {
    let payload = required_text(data, CLIENT_PAYLOAD)?;
    serde_json::from_str(payload).map_err(|error| {
        api::ApiError::InvalidCommand(format!("invalid durable task payload: {error}"))
    })
}

fn optional_text<'a>(data: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    data.get(key).and_then(Value::as_str)
}

fn required_text<'a>(data: &'a Map<String, Value>, key: &str) -> Result<&'a str, api::ApiError> {
    optional_text(data, key)
        .ok_or_else(|| api::ApiError::InvalidCommand(format!("missing upload field {key}")))
}

fn optional_i64(data: &Map<String, Value>, key: &str) -> Option<i64> {
    data.get(key)
        .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()))
}

fn required_i64(data: &Map<String, Value>, key: &str) -> Result<i64, api::ApiError> {
    optional_i64(data, key)
        .ok_or_else(|| api::ApiError::InvalidCommand(format!("missing upload field {key}")))
}

pub fn public_upload_error(error: &api::ApiError) -> String {
    match error {
        api::ApiError::Response { body, .. } => {
            if let Ok(value) = serde_json::from_str::<Value>(body) {
                let code = value
                    .get("error")
                    .and_then(|error| error.get("code"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let message = value
                    .get("error")
                    .and_then(|error| error.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                return match code {
                    "stale_revision" => {
                        "Stale revision: another device changed this item. The server copy will replace the rejected edit.".into()
                    }
                    "gone" => "This item was deleted on the server and cannot be restored.".into(),
                    "invalid_parent" => "That task list destination is not valid.".into(),
                    _ if !message.is_empty() => message.to_string(),
                    _ => error.to_string(),
                };
            }
            error.to_string()
        }
        _ => error.to_string(),
    }
}

pub fn powersync_status() -> &'static str {
    "Reading the native PowerSync replica. Offline writes are queued in the shared personal replica and uploaded only through foyer-server."
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_database(path: &std::path::Path) -> PowerSyncDatabase {
        PowerSyncEnvironment::powersync_auto_extension().unwrap();
        let pool = ConnectionPool::open(path).unwrap();
        let environment = PowerSyncEnvironment::custom(
            reqwest::Client::new(),
            pool,
            PowerSyncEnvironment::tokio_timer(),
        );
        PowerSyncDatabase::new(environment, tasks_schema())
    }

    fn sample_snapshot() -> Snapshot {
        Snapshot {
            availability: Availability::Available,
            development: true,
            using_powersync: true,
            lists: Arc::new(vec![
                TaskList {
                    id: "inbox".into(),
                    name: "Inbox".into(),
                    position: 0,
                    revision: 1,
                },
                TaskList {
                    id: "later".into(),
                    name: "Later".into(),
                    position: 1,
                    revision: 1,
                },
            ]),
            tasks: Arc::new(vec![
                Task {
                    id: "open".into(),
                    list_id: "inbox".into(),
                    title: "Write ADR".into(),
                    description: "# Heading\n\nBody".into(),
                    due: Some(Due::date_only("2026-08-15")),
                    priority: 1,
                    completed: false,
                    position: 0,
                    revision: 1,
                    updated_at: String::new(),
                },
                Task {
                    id: "done".into(),
                    list_id: "inbox".into(),
                    title: "Done".into(),
                    description: String::new(),
                    due: None,
                    priority: 0,
                    completed: true,
                    position: 1,
                    revision: 1,
                    updated_at: String::new(),
                },
            ]),
            ..Snapshot::default()
        }
    }

    #[test]
    fn catalog_order_and_delete_rules() {
        let snapshot = sample_snapshot();
        assert_eq!(
            snapshot
                .open_tasks_in("inbox")
                .into_iter()
                .map(|task| task.id)
                .collect::<Vec<_>>(),
            vec!["open"]
        );
        assert_eq!(
            snapshot.validate_list_delete("inbox").unwrap_err(),
            "List is not empty. Move or delete its tasks first."
        );
        assert!(snapshot.validate_list_delete("later").is_ok());
        assert_eq!(snapshot.task("open").unwrap().summary(), "Body");
    }

    #[test]
    fn due_parse_and_banner() {
        assert!(Due::parse("2026-08-15", None, true).unwrap().all_day);
        assert!(Due::parse("2026-08-15T18:00:00", Some("UTC"), false).is_some());
        assert!(Due::parse("08/15/2026", None, true).is_none());
        assert_eq!(
            sync_banner(
                Availability::Available,
                true,
                2,
                Some("Stale revision: another device changed this item.".into()),
            ),
            Some(SyncBanner::StaleRevision {
                message: "Stale revision: another device changed this item.".into()
            })
        );
    }

    #[test]
    fn public_upload_error_maps_stale_revision() {
        let error = api::ApiError::Response {
            status: reqwest::StatusCode::CONFLICT,
            body: r#"{"error":{"code":"stale_revision","message":"The expected revision does not match the current revision."}}"#.into(),
        };
        assert!(public_upload_error(&error).starts_with("Stale revision:"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn offline_commands_survive_reopening_the_replica() {
        let path = env::temp_dir().join(format!(
            "foyer-tasks-powersync-test-{}.sqlite3",
            Uuid::new_v4()
        ));
        let conflict = Mutex::new(None);
        let db = test_database(&path);
        apply_local(
            &db,
            Command::CreateList {
                name: "Inbox".into(),
            },
        )
        .await
        .unwrap();
        let list_id = read_snapshot(&db, &conflict, false).await.unwrap().lists[0]
            .id
            .clone();
        apply_local(
            &db,
            Command::CreateTask {
                list_id: list_id.clone(),
                title: "Restart-safe".into(),
                description: "# Native\n\nqueued Markdown".into(),
                due: Some(Due::date_only("2026-08-16")),
                priority: 1,
            },
        )
        .await
        .unwrap();
        drop(db);

        let reopened = test_database(&path);
        let snapshot = read_snapshot(&reopened, &conflict, false).await.unwrap();
        assert!(snapshot.using_powersync);
        assert_eq!(snapshot.pending_uploads, 2);
        assert_eq!(snapshot.lists[0].name, "Inbox");
        assert_eq!(snapshot.tasks[0].title, "Restart-safe");
        assert_eq!(snapshot.tasks[0].description, "# Native\n\nqueued Markdown");
        assert_eq!(
            snapshot.tasks[0].due.as_ref().map(|due| due.local.as_str()),
            Some("2026-08-16")
        );
        drop(reopened);
        for suffix in ["", "-shm", "-wal"] {
            let _ = fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn complete_move_and_nonempty_delete_follow_server_rules() {
        let path = env::temp_dir().join(format!(
            "foyer-tasks-powersync-move-{}.sqlite3",
            Uuid::new_v4()
        ));
        let conflict = Mutex::new(None);
        let db = test_database(&path);
        apply_local(
            &db,
            Command::CreateList {
                name: "Inbox".into(),
            },
        )
        .await
        .unwrap();
        apply_local(
            &db,
            Command::CreateList {
                name: "Later".into(),
            },
        )
        .await
        .unwrap();
        let snapshot = read_snapshot(&db, &conflict, false).await.unwrap();
        let inbox = snapshot
            .lists
            .iter()
            .find(|list| list.name == "Inbox")
            .cloned()
            .unwrap();
        let later = snapshot
            .lists
            .iter()
            .find(|list| list.name == "Later")
            .cloned()
            .unwrap();
        apply_local(
            &db,
            Command::CreateTask {
                list_id: inbox.id.clone(),
                title: "Keep".into(),
                description: "body".into(),
                due: None,
                priority: 0,
            },
        )
        .await
        .unwrap();
        let rejected = apply_local(
            &db,
            Command::DeleteList {
                id: inbox.id.clone(),
                revision: inbox.revision,
            },
        )
        .await
        .unwrap_err();
        assert!(rejected.contains("not empty"));
        let task = read_snapshot(&db, &conflict, false).await.unwrap().tasks[0].clone();
        apply_local(
            &db,
            Command::CompleteTask {
                id: task.id.clone(),
                revision: task.revision,
            },
        )
        .await
        .unwrap();
        apply_local(
            &db,
            Command::MoveTask {
                id: task.id.clone(),
                revision: task.revision + 1,
                list_id: later.id.clone(),
            },
        )
        .await
        .unwrap();
        let moved = read_snapshot(&db, &conflict, false)
            .await
            .unwrap()
            .task(&task.id)
            .cloned()
            .unwrap();
        assert_eq!(moved.list_id, later.id);
        assert!(moved.completed);
        drop(db);
        for suffix in ["", "-shm", "-wal"] {
            let _ = fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }
}
