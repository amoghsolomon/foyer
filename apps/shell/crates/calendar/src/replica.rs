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
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::{
    Availability, CALENDARS_TABLE, Calendar, EVENTS_TABLE, Event, EventDraft, Snapshot, api,
};

const CLIENT_OPERATION: &str = "client_operation";
const OPERATION_ID: &str = "operation_id";
const EXPECTED_REVISION: &str = "expected_revision";
const EXPECTED_ETAG: &str = "expected_etag";
const DELETED_LOCAL: &str = "deleted_local";
const CLIENT_PAYLOAD: &str = "client_payload";

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

    pub fn select_calendar(&self, id: Option<String>) {
        self.send(Command::SelectCalendar { id });
    }

    pub fn create_calendar(
        &self,
        display_name: String,
        description: String,
        color: Option<String>,
    ) {
        self.send(Command::CreateCalendar {
            display_name,
            description,
            color,
        });
    }

    pub fn rename_calendar(&self, id: String, revision: i64, etag: String, display_name: String) {
        self.send(Command::RenameCalendar {
            id,
            revision,
            etag,
            display_name,
        });
    }

    pub fn delete_calendar(&self, id: String, revision: i64, etag: String) {
        self.send(Command::DeleteCalendar { id, revision, etag });
    }

    pub fn create_event(&self, draft: EventDraft) {
        self.send(Command::CreateEvent { draft });
    }

    pub fn update_event(&self, id: String, revision: i64, etag: String, draft: EventDraft) {
        self.send(Command::UpdateEvent {
            id,
            revision,
            etag,
            draft,
        });
    }

    pub fn delete_event(&self, id: String, revision: i64, etag: String) {
        self.send(Command::DeleteEvent { id, revision, etag });
    }

    fn send(&self, command: Command) {
        if self.commands.try_send(command).is_err() {
            tracing::warn!("calendar worker is not running");
        }
    }
}

pub struct Runtime {
    pub updates: Receiver<Snapshot>,
    pub controller: Controller,
}

pub fn start() -> Runtime {
    let (updates_tx, updates) = async_channel::unbounded();
    let (commands, command_rx) = async_channel::unbounded();
    thread::Builder::new()
        .name("foyer-shell-calendar".into())
        .spawn(move || run_worker(updates_tx, command_rx))
        .expect("failed to start calendar worker");
    Runtime {
        updates,
        controller: Controller { commands },
    }
}

pub fn replica_path() -> PathBuf {
    env::var_os("FOYER_SHELL_CALENDAR_REPLICA_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(foyer_shell_paths::personal_replica_path)
}

#[derive(Debug)]
pub enum Command {
    Refresh,
    SelectCalendar {
        id: Option<String>,
    },
    CreateCalendar {
        display_name: String,
        description: String,
        color: Option<String>,
    },
    RenameCalendar {
        id: String,
        revision: i64,
        etag: String,
        display_name: String,
    },
    DeleteCalendar {
        id: String,
        revision: i64,
        etag: String,
    },
    CreateEvent {
        draft: EventDraft,
    },
    UpdateEvent {
        id: String,
        revision: i64,
        etag: String,
        draft: EventDraft,
    },
    DeleteEvent {
        id: String,
        revision: i64,
        etag: String,
    },
}

fn run_worker(updates: Sender<Snapshot>, commands: Receiver<Command>) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = updates.send_blocking(unavailable(format!(
                "create calendar async runtime: {error}"
            )));
            return;
        }
    };
    if let Err(error) = runtime.block_on(run_async(updates.clone(), commands)) {
        let _ = updates.send_blocking(unavailable(error));
    }
}

async fn run_async(updates: Sender<Snapshot>, commands: Receiver<Command>) -> Result<(), String> {
    updates
        .send(Snapshot {
            development: true,
            using_powersync: true,
            ..Snapshot::default()
        })
        .await
        .map_err(|_| "calendar UI stopped receiving updates".to_string())?;

    let api = api::Client::from_env().await?;
    let path = replica_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create calendar replica directory: {error}"))?;
    }
    PowerSyncEnvironment::powersync_auto_extension()
        .map_err(|error| format!("initialize PowerSync SQLite extension: {error}"))?;
    let pool = ConnectionPool::open(&path)
        .map_err(|error| format!("open calendar replica {}: {error}", path.display()))?;
    let environment = PowerSyncEnvironment::custom(
        reqwest::Client::new(),
        pool,
        PowerSyncEnvironment::tokio_timer(),
    );
    let db = PowerSyncDatabase::new(environment, calendar_schema());
    let conflict = Arc::new(Mutex::new(None));
    let selected = Arc::new(Mutex::new(None::<String>));
    let connector = Connector {
        db: db.clone(),
        api,
        conflict: conflict.clone(),
    };
    let _tasks = db.async_tasks().spawn_with_tokio();
    db.connect(SyncOptions::new(connector)).await;

    let mut table_updates = Box::pin(db.watch_tables(true, [CALENDARS_TABLE, EVENTS_TABLE]));
    let mut status_updates = Box::pin(db.watch_status());
    loop {
        tokio::select! {
            command = commands.recv() => {
                let Ok(command) = command else {
                    db.disconnect().await;
                    return Ok(());
                };
                if let Command::SelectCalendar { id } = &command {
                    *selected.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = id.clone();
                    publish(&updates, &db, &conflict, &selected, None).await?;
                    continue;
                }
                let command_error = apply_local(&db, command).await.err();
                publish(&updates, &db, &conflict, &selected, command_error).await?;
            }
            update = table_updates.next() => {
                if update.is_none() {
                    return Err("PowerSync calendar table watcher stopped".into());
                }
                publish(&updates, &db, &conflict, &selected, None).await?;
            }
            status = status_updates.next() => {
                if status.is_none() {
                    return Err("PowerSync calendar status watcher stopped".into());
                }
                publish(&updates, &db, &conflict, &selected, None).await?;
            }
        }
    }
}

pub fn schema_tables() -> Vec<Table> {
    let client_columns = || {
        vec![
            Column::text(CLIENT_OPERATION),
            Column::text(OPERATION_ID),
            Column::integer(EXPECTED_REVISION),
            Column::text(EXPECTED_ETAG),
            Column::integer(DELETED_LOCAL),
            Column::text(CLIENT_PAYLOAD),
        ]
    };
    let mut calendar_columns = vec![
        Column::text("user_id"),
        Column::text("uid"),
        Column::text("href"),
        Column::text("etag"),
        Column::text("display_name"),
        Column::text("description"),
        Column::text("color"),
        Column::text("ctag"),
        Column::text("sync_token"),
        Column::integer("revision"),
        Column::text("created_at"),
        Column::text("updated_at"),
    ];
    calendar_columns.extend(client_columns());
    let mut event_columns = vec![
        Column::text("user_id"),
        Column::text("calendar_id"),
        Column::text("uid"),
        Column::text("href"),
        Column::text("etag"),
        Column::text("summary"),
        Column::text("description"),
        Column::text("location"),
        Column::integer("all_day"),
        Column::text("dtstart"),
        Column::text("dtend"),
        Column::text("tzid"),
        Column::text("rrule"),
        Column::text("exdates"),
        Column::integer("revision"),
        Column::text("created_at"),
        Column::text("updated_at"),
    ];
    event_columns.extend(client_columns());
    vec![
        Table::create(CALENDARS_TABLE, calendar_columns, |_| {}),
        Table::create(EVENTS_TABLE, event_columns, |_| {}),
    ]
}

pub fn calendar_schema() -> Schema {
    Schema {
        tables: schema_tables(),
        ..Schema::default()
    }
}

async fn publish(
    updates: &Sender<Snapshot>,
    db: &PowerSyncDatabase,
    conflict: &Mutex<Option<String>>,
    selected: &Mutex<Option<String>>,
    command_error: Option<String>,
) -> Result<(), String> {
    let mut snapshot = read_snapshot(db, conflict, selected).await?;
    if command_error.is_some() {
        snapshot.last_error = command_error;
    }
    updates
        .send(snapshot)
        .await
        .map_err(|_| "calendar UI stopped receiving updates".to_string())
}

pub async fn read_snapshot(
    db: &PowerSyncDatabase,
    conflict: &Mutex<Option<String>>,
    selected: &Mutex<Option<String>>,
) -> Result<Snapshot, String> {
    let reader = db
        .reader()
        .await
        .map_err(|error| format!("read calendar replica: {error}"))?;
    let calendars = {
        let mut statement = reader
            .prepare(&format!(
                "SELECT id, uid, href, etag, display_name, description, color, revision \
                 FROM {CALENDARS_TABLE} WHERE COALESCE(deleted_local, 0) = 0 \
                 ORDER BY display_name, id"
            ))
            .map_err(|error| format!("prepare calendar snapshot: {error}"))?;
        statement
            .query_map([], |row| {
                Ok(Calendar {
                    id: row.get(0)?,
                    uid: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    href: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    etag: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    display_name: row.get(4)?,
                    description: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                    color: row.get(6)?,
                    revision: row.get(7)?,
                })
            })
            .map_err(|error| format!("query calendar snapshot: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("decode calendar snapshot: {error}"))?
    };
    let events = {
        let mut statement = reader
            .prepare(&format!(
                "SELECT id, calendar_id, uid, href, etag, summary, description, location, all_day, \
                        dtstart, dtend, tzid, rrule, exdates, revision \
                 FROM {EVENTS_TABLE} WHERE COALESCE(deleted_local, 0) = 0 \
                 ORDER BY dtstart, id"
            ))
            .map_err(|error| format!("prepare event snapshot: {error}"))?;
        statement
            .query_map([], |row| {
                Ok(Event {
                    id: row.get(0)?,
                    calendar_id: row.get(1)?,
                    uid: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    href: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    etag: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    summary: row.get(5)?,
                    description: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                    location: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
                    all_day: row.get::<_, i64>(8)? != 0,
                    dtstart: row.get(9)?,
                    dtend: row.get(10)?,
                    tzid: row.get(11)?,
                    rrule: row.get(12)?,
                    exdates: row
                        .get::<_, Option<String>>(13)?
                        .unwrap_or_else(|| "[]".into()),
                    revision: row.get(14)?,
                })
            })
            .map_err(|error| format!("query event snapshot: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("decode event snapshot: {error}"))?
    };
    let pending_uploads = reader
        .query_row("SELECT COUNT(*) FROM ps_crud", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|error| format!("count pending calendar writes: {error}"))?
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
    let selected_calendar_id = selected
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    Ok(Snapshot {
        availability: Availability::Available,
        development: foyer_shell_auth::development_auth_enabled(),
        using_powersync: true,
        offline: !status.is_connected(),
        pending_uploads,
        last_error: conflict.or(sync_error),
        calendars: Arc::new(calendars),
        events: Arc::new(events),
        selected_calendar_id,
    })
}

pub async fn apply_local(db: &PowerSyncDatabase, command: Command) -> Result<(), String> {
    if let Command::DeleteCalendar { id, .. } = &command {
        let snapshot = read_snapshot(db, &Mutex::new(None), &Mutex::new(None)).await?;
        snapshot.validate_calendar_delete(id)?;
    }
    if let Command::CreateEvent { draft } | Command::UpdateEvent { draft, .. } = &command {
        let snapshot = read_snapshot(db, &Mutex::new(None), &Mutex::new(None)).await?;
        snapshot.validate_event_draft(draft)?;
    }
    let writer = db
        .writer()
        .await
        .map_err(|error| format!("write calendar replica: {error}"))?;
    let now = chrono::Utc::now().to_rfc3339();
    let operation_id = Uuid::new_v4().to_string();
    match command {
        Command::Refresh | Command::SelectCalendar { .. } => return Ok(()),
        Command::CreateCalendar {
            display_name,
            description,
            color,
        } => {
            let name = required_title(display_name, "Calendar name")?;
            let id = Uuid::new_v4().to_string();
            writer.execute(
                &format!(
                    "INSERT INTO {CALENDARS_TABLE} \
                     (id, user_id, uid, href, etag, display_name, description, color, revision, \
                      created_at, updated_at, client_operation, operation_id, expected_revision, \
                      expected_etag, deleted_local) \
                     VALUES (?, '', ?, '', '', ?, ?, ?, 1, ?, ?, 'create', ?, NULL, NULL, 0)"
                ),
                params![id.clone(), id, name, description, color, now, now, operation_id],
            )
        }
        Command::RenameCalendar {
            id,
            revision,
            etag,
            display_name,
        } => {
            let name = required_title(display_name, "Calendar name")?;
            writer.execute(
                &format!(
                    "UPDATE {CALENDARS_TABLE} SET display_name = ?, revision = ?, updated_at = ?, \
                     client_operation = 'rename', operation_id = ?, expected_revision = ?, \
                     expected_etag = ? WHERE id = ?"
                ),
                params![name, revision + 1, now, operation_id, revision, etag, id],
            )
        }
        Command::DeleteCalendar { id, revision, etag } => writer.execute(
            &format!(
                "UPDATE {CALENDARS_TABLE} SET deleted_local = 1, revision = ?, updated_at = ?, \
                 client_operation = 'delete', operation_id = ?, expected_revision = ?, \
                 expected_etag = ? WHERE id = ?"
            ),
            params![revision + 1, now, operation_id, revision, etag, id],
        ),
        Command::CreateEvent { draft } => {
            let summary = required_title(draft.summary.clone(), "Event title")?;
            let id = Uuid::new_v4().to_string();
            let payload = draft_payload(&operation_id, &draft);
            writer.execute(
                &format!(
                    "INSERT INTO {EVENTS_TABLE} \
                     (id, user_id, calendar_id, uid, href, etag, summary, description, location, \
                      all_day, dtstart, dtend, tzid, rrule, exdates, revision, created_at, updated_at, \
                      client_operation, operation_id, expected_revision, expected_etag, deleted_local, \
                      client_payload) \
                     VALUES (?, '', ?, ?, '', '', ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, 'create', ?, NULL, NULL, 0, ?)"
                ),
                params![
                    id.clone(),
                    draft.calendar_id,
                    id,
                    summary,
                    draft.description,
                    draft.location,
                    if draft.all_day { 1 } else { 0 },
                    draft.dtstart,
                    draft.dtend,
                    draft.tzid,
                    draft.rrule,
                    crate::encode_exdates(&draft.exdates),
                    now,
                    now,
                    operation_id,
                    payload
                ],
            )
        }
        Command::UpdateEvent {
            id,
            revision,
            etag,
            draft,
        } => {
            let summary = required_title(draft.summary.clone(), "Event title")?;
            let payload = draft_payload(&operation_id, &draft);
            writer.execute(
                &format!(
                    "UPDATE {EVENTS_TABLE} SET calendar_id = ?, summary = ?, description = ?, \
                     location = ?, all_day = ?, dtstart = ?, dtend = ?, tzid = ?, rrule = ?, \
                     exdates = ?, revision = ?, updated_at = ?, client_operation = 'update', \
                     operation_id = ?, expected_revision = ?, expected_etag = ?, client_payload = ? \
                     WHERE id = ?"
                ),
                params![
                    draft.calendar_id,
                    summary,
                    draft.description,
                    draft.location,
                    if draft.all_day { 1 } else { 0 },
                    draft.dtstart,
                    draft.dtend,
                    draft.tzid,
                    draft.rrule,
                    crate::encode_exdates(&draft.exdates),
                    revision + 1,
                    now,
                    operation_id,
                    revision,
                    etag,
                    payload,
                    id
                ],
            )
        }
        Command::DeleteEvent { id, revision, etag } => writer.execute(
            &format!(
                "UPDATE {EVENTS_TABLE} SET deleted_local = 1, revision = ?, updated_at = ?, \
                 client_operation = 'delete', operation_id = ?, expected_revision = ?, \
                 expected_etag = ? WHERE id = ?"
            ),
            params![revision + 1, now, operation_id, revision, etag, id],
        ),
    }
    .map_err(|error| format!("apply local calendar command: {error}"))?;
    Ok(())
}

fn required_title(value: String, label: &str) -> Result<String, String> {
    let value = value.trim().to_string();
    if value.is_empty() {
        Err(format!("{label} is required"))
    } else {
        Ok(value)
    }
}

fn draft_payload(operation_id: &str, draft: &EventDraft) -> String {
    json!({
        "operationId": operation_id,
        "calendarId": draft.calendar_id,
        "summary": draft.summary,
        "description": draft.description,
        "location": draft.location,
        "allDay": draft.all_day,
        "dtstart": draft.dtstart,
        "dtend": draft.dtend,
        "tzid": draft.tzid,
        "rrule": draft.rrule,
        "exdates": draft.exdates,
    })
    .to_string()
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
        (CALENDARS_TABLE, "create") => {
            api.create_calendar(
                operation_id,
                &entry.id,
                required_text(data, "display_name")?,
                optional_text(data, "description").unwrap_or(""),
                optional_text(data, "color"),
            )
            .await?;
        }
        (CALENDARS_TABLE, "rename") => {
            api.rename_calendar(
                operation_id,
                &entry.id,
                required_i64(data, EXPECTED_REVISION)?,
                optional_text(data, EXPECTED_ETAG),
                required_text(data, "display_name")?,
            )
            .await?;
        }
        (CALENDARS_TABLE, "delete") => {
            api.delete_calendar(
                operation_id,
                &entry.id,
                required_i64(data, EXPECTED_REVISION)?,
                optional_text(data, EXPECTED_ETAG),
            )
            .await?;
        }
        (EVENTS_TABLE, "create") => {
            api.create_event(
                operation_id,
                &entry.id,
                optional_text(data, "uid"),
                &event_draft(data)?,
            )
            .await?;
        }
        (EVENTS_TABLE, "update") => {
            api.update_event(
                operation_id,
                &entry.id,
                required_i64(data, EXPECTED_REVISION)?,
                optional_text(data, EXPECTED_ETAG),
                &event_draft(data)?,
            )
            .await?;
        }
        (EVENTS_TABLE, "move") => {
            api.move_event(
                operation_id,
                &entry.id,
                required_i64(data, EXPECTED_REVISION)?,
                optional_text(data, EXPECTED_ETAG),
                required_text(data, "calendar_id")?,
            )
            .await?;
        }
        (EVENTS_TABLE, "delete") => {
            api.delete_event(
                operation_id,
                &entry.id,
                required_i64(data, EXPECTED_REVISION)?,
                optional_text(data, EXPECTED_ETAG),
            )
            .await?;
        }
        (table, other) => {
            return Err(api::ApiError::InvalidCommand(format!(
                "unknown calendar command {table}/{other}"
            )));
        }
    }
    Ok(())
}

fn event_draft(data: &Map<String, Value>) -> Result<EventDraft, api::ApiError> {
    if let Some(encoded) = optional_text(data, CLIENT_PAYLOAD)
        && let Ok(payload) = serde_json::from_str::<Value>(encoded)
    {
        return Ok(EventDraft {
            summary: payload
                .get("summary")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            description: payload
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            location: payload
                .get("location")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            all_day: payload
                .get("allDay")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            dtstart: payload
                .get("dtstart")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            dtend: payload
                .get("dtend")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            tzid: payload
                .get("tzid")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            rrule: payload
                .get("rrule")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            exdates: payload
                .get("exdates")
                .and_then(Value::as_array)
                .map(|rows| {
                    rows.iter()
                        .filter_map(Value::as_str)
                        .map(ToString::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            calendar_id: payload
                .get("calendarId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        });
    }
    Ok(EventDraft {
        summary: required_text(data, "summary")?.to_string(),
        description: optional_text(data, "description").unwrap_or("").to_string(),
        location: optional_text(data, "location").unwrap_or("").to_string(),
        all_day: optional_i64(data, "all_day").unwrap_or(0) != 0,
        dtstart: required_text(data, "dtstart")?.to_string(),
        dtend: optional_text(data, "dtend").map(ToString::to_string),
        tzid: optional_text(data, "tzid").map(ToString::to_string),
        rrule: optional_text(data, "rrule").map(ToString::to_string),
        exdates: crate::parse_exdates(optional_text(data, "exdates").unwrap_or("[]")),
        calendar_id: required_text(data, "calendar_id")?.to_string(),
    })
}

fn required_text<'a>(data: &'a Map<String, Value>, key: &str) -> Result<&'a str, api::ApiError> {
    optional_text(data, key).ok_or_else(|| {
        api::ApiError::InvalidCommand(format!("missing calendar upload field {key}"))
    })
}

fn optional_text<'a>(data: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    data.get(key)
        .and_then(Value::as_str)
        .filter(|value| *value != "null")
}

fn required_i64(data: &Map<String, Value>, key: &str) -> Result<i64, api::ApiError> {
    optional_i64(data, key).ok_or_else(|| {
        api::ApiError::InvalidCommand(format!("missing calendar upload field {key}"))
    })
}

fn optional_i64(data: &Map<String, Value>, key: &str) -> Option<i64> {
    data.get(key)
        .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()))
}

pub fn public_upload_error(error: &api::ApiError) -> String {
    let text = error.to_string();
    if is_stale_text(&text) {
        "Someone else changed this item. The server copy will replace the rejected edit.".into()
    } else {
        text
    }
}

fn is_stale_text(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("stale")
}
