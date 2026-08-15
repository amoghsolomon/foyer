//! One PowerSync replica, worker, and connector for hosted personal data.
//!
//! Domain crates keep normalized models, controllers, validation, and CRUD
//! application. This crate owns the replica file, assembled schema, and the
//! single Tokio worker that talks to SQLite and Foyer Server. GPUI never opens
//! the replica.

use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
};

use async_channel::{Receiver, Sender};
use async_trait::async_trait;
use futures_lite::StreamExt;
use powersync::{
    BackendConnector, ConnectionPool, CrudEntry, PowerSyncCredentials, PowerSyncDatabase,
    SyncOptions, env::PowerSyncEnvironment, error::PowerSyncError, schema::Schema,
};

const WATCHED_TABLES: [&str; 10] = [
    foyer_shell_notes::FOLDERS_TABLE,
    foyer_shell_notes::NOTES_TABLE,
    foyer_shell_tasks::TASK_LISTS_TABLE,
    foyer_shell_tasks::TASKS_TABLE,
    foyer_shell_contacts::ADDRESS_BOOKS_TABLE,
    foyer_shell_contacts::CONTACTS_TABLE,
    foyer_shell_calendar::CALENDARS_TABLE,
    foyer_shell_calendar::EVENTS_TABLE,
    foyer_shell_bookmarks::FOLDERS_TABLE,
    foyer_shell_bookmarks::BOOKMARKS_TABLE,
];

pub fn replica_path() -> PathBuf {
    foyer_shell_paths::personal_replica_path()
}

pub struct Runtime {
    pub notes_updates: Receiver<foyer_shell_notes::Snapshot>,
    pub notes: foyer_shell_notes::Controller,
    pub tasks_updates: Receiver<foyer_shell_tasks::Snapshot>,
    pub tasks: foyer_shell_tasks::Controller,
    pub contacts_updates: Receiver<foyer_shell_contacts::Snapshot>,
    pub contacts: foyer_shell_contacts::Controller,
    pub calendar_updates: Receiver<foyer_shell_calendar::Snapshot>,
    pub calendar: foyer_shell_calendar::Controller,
    pub bookmarks_updates: Receiver<foyer_shell_bookmarks::Snapshot>,
    pub bookmarks: foyer_shell_bookmarks::Controller,
}

pub fn start() -> Runtime {
    let (notes_tx, notes_updates) = async_channel::unbounded();
    let (notes_commands, notes_rx) = async_channel::unbounded();
    let (tasks_tx, tasks_updates) = async_channel::unbounded();
    let (tasks_commands, tasks_rx) = async_channel::unbounded();
    let (contacts_tx, contacts_updates) = async_channel::unbounded();
    let (contacts_commands, contacts_rx) = async_channel::unbounded();
    let (calendar_tx, calendar_updates) = async_channel::unbounded();
    let (calendar_commands, calendar_rx) = async_channel::unbounded();
    let (bookmarks_tx, bookmarks_updates) = async_channel::unbounded();
    let (bookmarks_commands, bookmarks_rx) = async_channel::unbounded();

    thread::Builder::new()
        .name("foyer-shell-personal".into())
        .spawn(move || {
            run_worker(
                Channels {
                    notes: notes_tx,
                    tasks: tasks_tx,
                    contacts: contacts_tx,
                    calendar: calendar_tx,
                    bookmarks: bookmarks_tx,
                },
                CommandReceivers {
                    notes: notes_rx,
                    tasks: tasks_rx,
                    contacts: contacts_rx,
                    calendar: calendar_rx,
                    bookmarks: bookmarks_rx,
                },
            )
        })
        .expect("failed to start personal-data worker");

    Runtime {
        notes_updates,
        notes: foyer_shell_notes::Controller::attach(notes_commands),
        tasks_updates,
        tasks: foyer_shell_tasks::Controller::attach(tasks_commands),
        contacts_updates,
        contacts: foyer_shell_contacts::Controller::attach(contacts_commands),
        calendar_updates,
        calendar: foyer_shell_calendar::Controller::attach(calendar_commands),
        bookmarks_updates,
        bookmarks: foyer_shell_bookmarks::Controller::attach(bookmarks_commands),
    }
}

pub fn schema() -> Schema {
    let mut tables = foyer_shell_notes::schema_tables();
    tables.extend(foyer_shell_tasks::schema_tables());
    tables.extend(foyer_shell_contacts::schema_tables());
    tables.extend(foyer_shell_calendar::schema_tables());
    tables.extend(foyer_shell_bookmarks::schema_tables());
    Schema {
        tables,
        ..Schema::default()
    }
}

/// Projects hosted calendar occurrences and tasks into the existing Agenda snapshot.
pub fn agenda_snapshot(
    calendar: &foyer_shell_calendar::Snapshot,
    tasks: &foyer_shell_tasks::Snapshot,
    hidden: &[String],
) -> foyer_shell_agenda::Snapshot {
    let availability = match (&calendar.availability, &tasks.availability) {
        (
            foyer_shell_calendar::Availability::Unavailable(error),
            foyer_shell_tasks::Availability::Unavailable(_),
        ) => foyer_shell_agenda::Availability::Unavailable(error.clone()),
        (foyer_shell_calendar::Availability::Unavailable(error), _) => {
            foyer_shell_agenda::Availability::Unavailable(error.clone())
        }
        (_, foyer_shell_tasks::Availability::Unavailable(error)) => {
            foyer_shell_agenda::Availability::Unavailable(error.clone())
        }
        (foyer_shell_calendar::Availability::Loading, foyer_shell_tasks::Availability::Loading) => {
            foyer_shell_agenda::Availability::Loading
        }
        _ => foyer_shell_agenda::Availability::Available,
    };

    let mut sources = calendar
        .calendars
        .iter()
        .map(|calendar| foyer_shell_agenda::AgendaSource {
            id: calendar.id.clone(),
            name: calendar.display_name.clone(),
            kind: foyer_shell_agenda::SourceKind::Calendar,
            writable: true,
            visible: true,
        })
        .collect::<Vec<_>>();
    sources.extend(
        tasks
            .lists
            .iter()
            .map(|list| foyer_shell_agenda::AgendaSource {
                id: list.id.clone(),
                name: list.name.clone(),
                kind: foyer_shell_agenda::SourceKind::TaskList,
                writable: true,
                visible: true,
            }),
    );

    let today = chrono::Local::now().date_naive();
    let window_start = today - chrono::Duration::days(1);
    let window_end = today + chrono::Duration::days(45);
    let mut items = Vec::new();
    for event in calendar.events.iter() {
        let occurrences =
            match foyer_shell_calendar::expand_event(event, window_start, window_end, 64) {
                Ok(occurrences) => occurrences,
                Err(_) => continue,
            };
        for occurrence in occurrences {
            items.push(foyer_shell_agenda::AgendaItem {
                id: format!("{}:{}", occurrence.event_id, occurrence.recurrence_id),
                source_id: occurrence.calendar_id,
                kind: foyer_shell_agenda::ItemKind::Event,
                title: occurrence.summary,
                description: occurrence.description,
                location: occurrence.location,
                start_ms: occurrence.start_ms,
                end_ms: occurrence.end_ms,
                due_ms: None,
                all_day: occurrence.all_day,
                completed: false,
            });
        }
    }
    for task in tasks.tasks.iter() {
        items.push(foyer_shell_agenda::AgendaItem {
            id: task.id.clone(),
            source_id: task.list_id.clone(),
            kind: foyer_shell_agenda::ItemKind::Task,
            title: task.title.clone(),
            description: task.description.clone(),
            location: String::new(),
            start_ms: None,
            end_ms: None,
            due_ms: task.due.as_ref().and_then(due_ms),
            all_day: task.due.as_ref().is_some_and(|due| due.all_day),
            completed: task.completed,
        });
    }
    items.sort_by_key(|item| {
        (
            item.completed,
            item.start_ms.or(item.due_ms).unwrap_or(i64::MAX),
            item.title.to_ascii_lowercase(),
        )
    });

    let last_error = calendar
        .last_error
        .clone()
        .or_else(|| tasks.last_error.clone());
    foyer_shell_agenda::compose(
        availability,
        sources,
        items,
        hidden,
        last_error,
        Some(chrono::Utc::now().timestamp_millis()),
    )
}

fn due_ms(due: &foyer_shell_tasks::Due) -> Option<i64> {
    if due.all_day {
        return chrono::NaiveDate::parse_from_str(&due.local, "%Y-%m-%d")
            .ok()
            .and_then(|date| date.and_hms_opt(0, 0, 0))
            .map(|datetime| datetime.and_utc().timestamp_millis());
    }
    chrono::NaiveDateTime::parse_from_str(&due.local, "%Y-%m-%dT%H:%M:%S")
        .ok()
        .map(|datetime| datetime.and_utc().timestamp_millis())
}

struct Channels {
    notes: Sender<foyer_shell_notes::Snapshot>,
    tasks: Sender<foyer_shell_tasks::Snapshot>,
    contacts: Sender<foyer_shell_contacts::Snapshot>,
    calendar: Sender<foyer_shell_calendar::Snapshot>,
    bookmarks: Sender<foyer_shell_bookmarks::Snapshot>,
}

struct CommandReceivers {
    notes: Receiver<foyer_shell_notes::Command>,
    tasks: Receiver<foyer_shell_tasks::Command>,
    contacts: Receiver<foyer_shell_contacts::Command>,
    calendar: Receiver<foyer_shell_calendar::Command>,
    bookmarks: Receiver<foyer_shell_bookmarks::Command>,
}

struct Conflicts {
    notes: Mutex<Option<String>>,
    tasks: Mutex<Option<String>>,
    contacts: Mutex<Option<String>>,
    calendar: Mutex<Option<String>>,
    bookmarks: Mutex<Option<String>>,
}

impl Conflicts {
    fn new() -> Self {
        Self {
            notes: Mutex::new(None),
            tasks: Mutex::new(None),
            contacts: Mutex::new(None),
            calendar: Mutex::new(None),
            bookmarks: Mutex::new(None),
        }
    }

    fn slot(&self, table: &str) -> &Mutex<Option<String>> {
        if owns_notes(table) {
            &self.notes
        } else if owns_tasks(table) {
            &self.tasks
        } else if owns_contacts(table) {
            &self.contacts
        } else if owns_calendar(table) {
            &self.calendar
        } else {
            &self.bookmarks
        }
    }
}

fn run_worker(updates: Channels, commands: CommandReceivers) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            publish_unavailable(
                &updates,
                format!("create personal-data async runtime: {error}"),
                false,
            );
            return;
        }
    };
    if let Err(error) = runtime.block_on(run_async(updates, commands)) {
        // The async loop already published a terminal snapshot when it can.
        tracing::error!(error = %redact_auth_log(&error), "personal-data worker stopped");
    }
}

async fn connect_session() -> Result<foyer_shell_auth::ApiSession, String> {
    foyer_shell_auth::ApiSession::from_env()
        .await
        .map_err(|error| error.public_message())
}

async fn wait_for_retry(commands: &CommandReceivers) -> Result<(), String> {
    tokio::select! {
        command = commands.notes.recv() => { command.map(|_| ()).map_err(|_| "notes UI stopped receiving updates".to_string()) }
        command = commands.tasks.recv() => { command.map(|_| ()).map_err(|_| "tasks UI stopped receiving updates".to_string()) }
        command = commands.contacts.recv() => { command.map(|_| ()).map_err(|_| "contacts UI stopped receiving updates".to_string()) }
        command = commands.calendar.recv() => { command.map(|_| ()).map_err(|_| "calendar UI stopped receiving updates".to_string()) }
        command = commands.bookmarks.recv() => { command.map(|_| ()).map_err(|_| "bookmarks UI stopped receiving updates".to_string()) }
    }
}

fn redact_auth_log(error: &str) -> &str {
    if error.contains("Bearer ")
        || error.contains("signingPayload")
        || error.contains("accessToken")
    {
        "personal-data authentication failed"
    } else {
        error
    }
}

async fn run_async(updates: Channels, commands: CommandReceivers) -> Result<(), String> {
    let development = foyer_shell_auth::development_auth_enabled();
    send_loading(&updates, development).await?;
    let session = loop {
        match connect_session().await {
            Ok(session) => break session,
            Err(error) => {
                publish_unavailable(&updates, error, development);
                wait_for_retry(&commands).await?;
                send_loading(&updates, development).await?;
            }
        }
    };
    let notes_api = foyer_shell_notes::api::Client::from_session(session.clone());
    let tasks_api = foyer_shell_tasks::api::Client::from_session(session.clone());
    let contacts_api = foyer_shell_contacts::api::Client::from_session(session.clone());
    let calendar_api = foyer_shell_calendar::api::Client::from_session(session.clone());
    let bookmarks_api = foyer_shell_bookmarks::api::Client::from_session(session);

    let path = replica_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create personal replica directory: {error}"))?;
    }
    PowerSyncEnvironment::powersync_auto_extension()
        .map_err(|error| format!("initialize PowerSync SQLite extension: {error}"))?;
    let pool = ConnectionPool::open(&path)
        .map_err(|error| format!("open personal replica {}: {error}", path.display()))?;
    let environment = PowerSyncEnvironment::custom(
        reqwest::Client::new(),
        pool,
        PowerSyncEnvironment::tokio_timer(),
    );
    let db = PowerSyncDatabase::new(environment, schema());
    let conflicts = Arc::new(Conflicts::new());
    let selected_calendar = Arc::new(Mutex::new(None::<String>));
    let connector = Connector {
        db: db.clone(),
        notes: notes_api,
        tasks: tasks_api,
        contacts: contacts_api,
        calendar: calendar_api,
        bookmarks: bookmarks_api,
        conflicts: conflicts.clone(),
    };
    let _tasks = db.async_tasks().spawn_with_tokio();
    db.connect(SyncOptions::new(connector)).await;

    let mut table_updates = Box::pin(db.watch_tables(true, WATCHED_TABLES));
    let mut status_updates = Box::pin(db.watch_status());
    loop {
        tokio::select! {
            command = commands.notes.recv() => {
                let Ok(command) = command else { break; };
                let error = foyer_shell_notes::apply_local(&db, command).await.err();
                publish_notes(&updates.notes, &db, &conflicts.notes, development, error).await?;
            }
            command = commands.tasks.recv() => {
                let Ok(command) = command else { break; };
                let error = foyer_shell_tasks::apply_local(&db, command).await.err();
                publish_tasks(&updates.tasks, &db, &conflicts.tasks, development, error).await?;
            }
            command = commands.contacts.recv() => {
                let Ok(command) = command else { break; };
                let error = foyer_shell_contacts::apply_local(&db, command).await.err();
                publish_contacts(&updates.contacts, &db, &conflicts.contacts, development, error).await?;
            }
            command = commands.calendar.recv() => {
                let Ok(command) = command else { break; };
                if let foyer_shell_calendar::Command::SelectCalendar { id } = &command {
                    *selected_calendar
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) = id.clone();
                    publish_calendar(
                        &updates.calendar,
                        &db,
                        &conflicts.calendar,
                        &selected_calendar,
                        development,
                        None,
                    )
                    .await?;
                    continue;
                }
                let error = foyer_shell_calendar::apply_local(&db, command).await.err();
                publish_calendar(
                    &updates.calendar,
                    &db,
                    &conflicts.calendar,
                    &selected_calendar,
                    development,
                    error,
                )
                .await?;
            }
            command = commands.bookmarks.recv() => {
                let Ok(command) = command else { break; };
                let error = foyer_shell_bookmarks::apply_local(&db, command).await.err();
                publish_bookmarks(&updates.bookmarks, &db, &conflicts.bookmarks, development, error).await?;
            }
            update = table_updates.next() => {
                if update.is_none() {
                    return Err("PowerSync personal table watcher stopped".into());
                }
                publish_all(&updates, &db, &conflicts, &selected_calendar, development).await?;
            }
            status = status_updates.next() => {
                if status.is_none() {
                    return Err("PowerSync personal status watcher stopped".into());
                }
                publish_all(&updates, &db, &conflicts, &selected_calendar, development).await?;
            }
        }
    }
    db.disconnect().await;
    Ok(())
}

async fn send_loading(updates: &Channels, development: bool) -> Result<(), String> {
    updates
        .notes
        .send(foyer_shell_notes::Snapshot {
            development,
            using_powersync: true,
            ..foyer_shell_notes::Snapshot::default()
        })
        .await
        .map_err(|_| "notes UI stopped receiving updates".to_string())?;
    updates
        .tasks
        .send(foyer_shell_tasks::Snapshot {
            development,
            using_powersync: true,
            sharing_replica: true,
            ..foyer_shell_tasks::Snapshot::default()
        })
        .await
        .map_err(|_| "tasks UI stopped receiving updates".to_string())?;
    updates
        .contacts
        .send(foyer_shell_contacts::Snapshot {
            development,
            using_powersync: true,
            ..foyer_shell_contacts::Snapshot::default()
        })
        .await
        .map_err(|_| "contacts UI stopped receiving updates".to_string())?;
    updates
        .calendar
        .send(foyer_shell_calendar::Snapshot {
            development,
            using_powersync: true,
            ..foyer_shell_calendar::Snapshot::default()
        })
        .await
        .map_err(|_| "calendar UI stopped receiving updates".to_string())?;
    updates
        .bookmarks
        .send(foyer_shell_bookmarks::Snapshot {
            development,
            using_powersync: true,
            ..foyer_shell_bookmarks::Snapshot::default()
        })
        .await
        .map_err(|_| "bookmarks UI stopped receiving updates".to_string())?;
    Ok(())
}

fn publish_unavailable(updates: &Channels, error: String, development: bool) {
    let _ = updates.notes.send_blocking(foyer_shell_notes::Snapshot {
        availability: foyer_shell_notes::Availability::Unavailable(error.clone()),
        development,
        using_powersync: true,
        last_error: Some(error.clone()),
        ..foyer_shell_notes::Snapshot::default()
    });
    let _ = updates.tasks.send_blocking(foyer_shell_tasks::Snapshot {
        availability: foyer_shell_tasks::Availability::Unavailable(error.clone()),
        development,
        using_powersync: true,
        sharing_replica: true,
        last_error: Some(error.clone()),
        ..foyer_shell_tasks::Snapshot::default()
    });
    let _ = updates
        .contacts
        .send_blocking(foyer_shell_contacts::Snapshot {
            availability: foyer_shell_contacts::Availability::Unavailable(error.clone()),
            development,
            using_powersync: true,
            last_error: Some(error.clone()),
            ..foyer_shell_contacts::Snapshot::default()
        });
    let _ = updates
        .calendar
        .send_blocking(foyer_shell_calendar::Snapshot {
            availability: foyer_shell_calendar::Availability::Unavailable(error.clone()),
            development,
            using_powersync: true,
            last_error: Some(error.clone()),
            ..foyer_shell_calendar::Snapshot::default()
        });
    let _ = updates
        .bookmarks
        .send_blocking(foyer_shell_bookmarks::Snapshot {
            availability: foyer_shell_bookmarks::Availability::Unavailable(error.clone()),
            development,
            using_powersync: true,
            last_error: Some(error),
            ..foyer_shell_bookmarks::Snapshot::default()
        });
}

async fn publish_all(
    updates: &Channels,
    db: &PowerSyncDatabase,
    conflicts: &Conflicts,
    selected_calendar: &Mutex<Option<String>>,
    development: bool,
) -> Result<(), String> {
    publish_notes(&updates.notes, db, &conflicts.notes, development, None).await?;
    publish_tasks(&updates.tasks, db, &conflicts.tasks, development, None).await?;
    publish_contacts(
        &updates.contacts,
        db,
        &conflicts.contacts,
        development,
        None,
    )
    .await?;
    publish_calendar(
        &updates.calendar,
        db,
        &conflicts.calendar,
        selected_calendar,
        development,
        None,
    )
    .await?;
    publish_bookmarks(
        &updates.bookmarks,
        db,
        &conflicts.bookmarks,
        development,
        None,
    )
    .await
}

async fn publish_notes(
    updates: &Sender<foyer_shell_notes::Snapshot>,
    db: &PowerSyncDatabase,
    conflict: &Mutex<Option<String>>,
    development: bool,
    command_error: Option<String>,
) -> Result<(), String> {
    let mut snapshot = foyer_shell_notes::read_snapshot(db, conflict).await?;
    snapshot.development = development;
    if command_error.is_some() {
        snapshot.last_error = command_error;
    }
    updates
        .send(snapshot)
        .await
        .map_err(|_| "notes UI stopped receiving updates".to_string())
}

async fn publish_tasks(
    updates: &Sender<foyer_shell_tasks::Snapshot>,
    db: &PowerSyncDatabase,
    conflict: &Mutex<Option<String>>,
    development: bool,
    command_error: Option<String>,
) -> Result<(), String> {
    let mut snapshot = foyer_shell_tasks::read_snapshot(db, conflict, true).await?;
    snapshot.development = development;
    if command_error.is_some() {
        snapshot.last_error = command_error;
    }
    updates
        .send(snapshot)
        .await
        .map_err(|_| "tasks UI stopped receiving updates".to_string())
}

async fn publish_contacts(
    updates: &Sender<foyer_shell_contacts::Snapshot>,
    db: &PowerSyncDatabase,
    conflict: &Mutex<Option<String>>,
    development: bool,
    command_error: Option<String>,
) -> Result<(), String> {
    let mut snapshot = foyer_shell_contacts::read_snapshot(db, conflict).await?;
    snapshot.development = development;
    if command_error.is_some() {
        snapshot.last_error = command_error;
    }
    updates
        .send(snapshot)
        .await
        .map_err(|_| "contacts UI stopped receiving updates".to_string())
}

async fn publish_calendar(
    updates: &Sender<foyer_shell_calendar::Snapshot>,
    db: &PowerSyncDatabase,
    conflict: &Mutex<Option<String>>,
    selected: &Mutex<Option<String>>,
    development: bool,
    command_error: Option<String>,
) -> Result<(), String> {
    let mut snapshot = foyer_shell_calendar::read_snapshot(db, conflict, selected).await?;
    snapshot.development = development;
    if command_error.is_some() {
        snapshot.last_error = command_error;
    }
    updates
        .send(snapshot)
        .await
        .map_err(|_| "calendar UI stopped receiving updates".to_string())
}

async fn publish_bookmarks(
    updates: &Sender<foyer_shell_bookmarks::Snapshot>,
    db: &PowerSyncDatabase,
    conflict: &Mutex<Option<String>>,
    development: bool,
    command_error: Option<String>,
) -> Result<(), String> {
    let mut snapshot = foyer_shell_bookmarks::read_snapshot(db, conflict).await?;
    snapshot.development = development;
    if command_error.is_some() {
        snapshot.last_error = command_error;
    }
    updates
        .send(snapshot)
        .await
        .map_err(|_| "bookmarks UI stopped receiving updates".to_string())
}

#[derive(Clone)]
struct Connector {
    db: PowerSyncDatabase,
    notes: foyer_shell_notes::api::Client,
    tasks: foyer_shell_tasks::api::Client,
    contacts: foyer_shell_contacts::api::Client,
    calendar: foyer_shell_calendar::api::Client,
    bookmarks: foyer_shell_bookmarks::api::Client,
    conflicts: Arc<Conflicts>,
}

#[async_trait]
impl BackendConnector for Connector {
    async fn fetch_credentials(&self) -> Result<PowerSyncCredentials, PowerSyncError> {
        let credentials = self
            .notes
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
                if error.permanent {
                    *self
                        .conflicts
                        .slot(&entry.table)
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(error.message);
                    transaction.complete().await?;
                    return Ok(());
                }
                return Err(PowerSyncError::upload_error(UploadMessage(error.message)));
            }
        }
        for slot in [
            &self.conflicts.notes,
            &self.conflicts.tasks,
            &self.conflicts.contacts,
            &self.conflicts.calendar,
            &self.conflicts.bookmarks,
        ] {
            *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        }
        transaction.complete().await
    }
}

impl Connector {
    async fn upload(&self, entry: &CrudEntry) -> Result<(), UploadFailure> {
        if owns_notes(&entry.table) {
            return map_upload(
                foyer_shell_notes::upload_entry(&self.notes, entry).await,
                foyer_shell_notes::public_upload_error,
            );
        }
        if owns_tasks(&entry.table) {
            return map_upload(
                foyer_shell_tasks::upload_entry(&self.tasks, entry).await,
                foyer_shell_tasks::public_upload_error,
            );
        }
        if owns_contacts(&entry.table) {
            return map_upload(
                foyer_shell_contacts::upload_entry(&self.contacts, entry).await,
                foyer_shell_contacts::public_upload_error,
            );
        }
        if owns_calendar(&entry.table) {
            return map_upload(
                foyer_shell_calendar::upload_entry(&self.calendar, entry).await,
                foyer_shell_calendar::public_upload_error,
            );
        }
        if owns_bookmarks(&entry.table) {
            return map_upload(
                foyer_shell_bookmarks::upload_entry(&self.bookmarks, entry).await,
                foyer_shell_bookmarks::public_upload_error,
            );
        }
        Err(UploadFailure {
            permanent: true,
            message: format!("unsupported personal replica table {}", entry.table),
        })
    }
}

struct UploadFailure {
    permanent: bool,
    message: String,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct UploadMessage(String);

fn map_upload<E>(result: Result<(), E>, public_error: fn(&E) -> String) -> Result<(), UploadFailure>
where
    E: PermanentRejection,
{
    result.map_err(|error| UploadFailure {
        permanent: error.is_permanent(),
        message: public_error(&error),
    })
}

trait PermanentRejection {
    fn is_permanent(&self) -> bool;
}

impl PermanentRejection for foyer_shell_notes::api::ApiError {
    fn is_permanent(&self) -> bool {
        self.is_permanent_command_rejection()
    }
}

impl PermanentRejection for foyer_shell_tasks::api::ApiError {
    fn is_permanent(&self) -> bool {
        self.is_permanent_command_rejection()
    }
}

impl PermanentRejection for foyer_shell_contacts::api::ApiError {
    fn is_permanent(&self) -> bool {
        self.is_permanent_command_rejection()
    }
}

impl PermanentRejection for foyer_shell_calendar::api::ApiError {
    fn is_permanent(&self) -> bool {
        self.is_permanent_command_rejection()
    }
}

impl PermanentRejection for foyer_shell_bookmarks::api::ApiError {
    fn is_permanent(&self) -> bool {
        self.is_permanent_command_rejection()
    }
}

fn owns_notes(table: &str) -> bool {
    table == foyer_shell_notes::FOLDERS_TABLE || table == foyer_shell_notes::NOTES_TABLE
}

fn owns_tasks(table: &str) -> bool {
    table == foyer_shell_tasks::TASK_LISTS_TABLE || table == foyer_shell_tasks::TASKS_TABLE
}

fn owns_contacts(table: &str) -> bool {
    table == foyer_shell_contacts::ADDRESS_BOOKS_TABLE
        || table == foyer_shell_contacts::CONTACTS_TABLE
}

fn owns_calendar(table: &str) -> bool {
    table == foyer_shell_calendar::CALENDARS_TABLE || table == foyer_shell_calendar::EVENTS_TABLE
}

fn owns_bookmarks(table: &str) -> bool {
    table == foyer_shell_bookmarks::FOLDERS_TABLE || table == foyer_shell_bookmarks::BOOKMARKS_TABLE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assembled_schema_covers_every_hosted_table() {
        let tables = schema()
            .tables
            .into_iter()
            .map(|table| table.name.to_string())
            .collect::<Vec<_>>();
        for name in WATCHED_TABLES {
            assert!(tables.iter().any(|table| table == name), "missing {name}");
        }
        assert_eq!(tables.len(), WATCHED_TABLES.len());
    }

    #[test]
    fn agenda_projection_filters_hidden_sources() {
        let calendar = foyer_shell_calendar::Snapshot {
            availability: foyer_shell_calendar::Availability::Available,
            calendars: std::sync::Arc::new(vec![foyer_shell_calendar::Calendar {
                id: "cal-1".into(),
                uid: "cal-1".into(),
                href: String::new(),
                etag: String::new(),
                display_name: "Work".into(),
                description: String::new(),
                color: None,
                revision: 1,
            }]),
            events: std::sync::Arc::new(vec![foyer_shell_calendar::Event {
                id: "event-1".into(),
                calendar_id: "cal-1".into(),
                uid: "event-1".into(),
                href: String::new(),
                etag: String::new(),
                summary: "Standup".into(),
                description: String::new(),
                location: String::new(),
                all_day: true,
                dtstart: chrono::Local::now()
                    .date_naive()
                    .format("%Y%m%d")
                    .to_string(),
                dtend: None,
                tzid: None,
                rrule: None,
                exdates: "[]".into(),
                revision: 1,
            }]),
            ..foyer_shell_calendar::Snapshot::default()
        };
        let tasks = foyer_shell_tasks::Snapshot {
            availability: foyer_shell_tasks::Availability::Available,
            lists: std::sync::Arc::new(vec![foyer_shell_tasks::TaskList {
                id: "list-1".into(),
                name: "Inbox".into(),
                position: 0,
                revision: 1,
            }]),
            tasks: std::sync::Arc::new(vec![foyer_shell_tasks::Task {
                id: "task-1".into(),
                list_id: "list-1".into(),
                title: "Ship it".into(),
                description: String::new(),
                due: None,
                priority: 0,
                completed: false,
                position: 0,
                revision: 1,
                updated_at: String::new(),
            }]),
            ..foyer_shell_tasks::Snapshot::default()
        };
        let snapshot = agenda_snapshot(&calendar, &tasks, &["cal-1".into()]);
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
        assert_eq!(snapshot.items[0].kind, foyer_shell_agenda::ItemKind::Task);
    }
}
