//! Hosted contacts adapter. Reads come from immutable snapshots; I/O never runs in GPUI.

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

const CLIENT_OPERATION: &str = "client_operation";
const OPERATION_ID: &str = "operation_id";
const EXPECTED_ETAG: &str = "expected_etag";
const EXPECTED_REVISION: &str = "expected_revision";
const DELETED_LOCAL: &str = "deleted_local";
const CLIENT_PAYLOAD: &str = "client_payload";

pub const ADDRESS_BOOKS_TABLE: &str = "contacts_address_books";
pub const CONTACTS_TABLE: &str = "contacts";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Availability {
    Loading,
    Available,
    Unavailable(String),
}

impl Availability {
    pub fn detail(&self) -> &str {
        match self {
            Self::Loading => "Loading contacts…",
            Self::Available => "Available",
            Self::Unavailable(error) => error,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredName {
    #[serde(default, rename = "familyName")]
    pub family_name: String,
    #[serde(default, rename = "givenName")]
    pub given_name: String,
    #[serde(default, rename = "additionalNames")]
    pub additional_names: String,
    #[serde(default, rename = "honorificPrefix")]
    pub honorific_prefix: String,
    #[serde(default, rename = "honorificSuffix")]
    pub honorific_suffix: String,
}

impl StructuredName {
    pub fn is_empty(&self) -> bool {
        self.family_name.is_empty()
            && self.given_name.is_empty()
            && self.additional_names.is_empty()
            && self.honorific_prefix.is_empty()
            && self.honorific_suffix.is_empty()
    }

    pub fn formatted(&self) -> String {
        [
            self.honorific_prefix.as_str(),
            self.given_name.as_str(),
            self.additional_names.as_str(),
            self.family_name.as_str(),
            self.honorific_suffix.as_str(),
        ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypedValue {
    pub value: String,
    #[serde(default = "default_type")]
    pub r#type: String,
    #[serde(default)]
    pub pref: bool,
}

fn default_type() -> String {
    "other".into()
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostalAddress {
    #[serde(default, rename = "poBox")]
    pub po_box: String,
    #[serde(default)]
    pub extended: String,
    #[serde(default)]
    pub street: String,
    #[serde(default)]
    pub locality: String,
    #[serde(default)]
    pub region: String,
    #[serde(default, rename = "postalCode")]
    pub postal_code: String,
    #[serde(default)]
    pub country: String,
    #[serde(default = "default_type")]
    pub r#type: String,
    #[serde(default)]
    pub pref: bool,
}

impl PostalAddress {
    pub fn one_line(&self) -> String {
        [
            self.street.as_str(),
            self.locality.as_str(),
            self.region.as_str(),
            self.postal_code.as_str(),
            self.country.as_str(),
        ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddressBook {
    pub id: String,
    #[serde(default)]
    pub uid: String,
    #[serde(default)]
    pub href: String,
    pub etag: Option<String>,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    pub revision: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contact {
    pub id: String,
    #[serde(rename = "addressBookId")]
    pub address_book_id: String,
    #[serde(default)]
    pub uid: String,
    #[serde(default)]
    pub href: String,
    #[serde(default)]
    pub etag: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(default)]
    pub name: StructuredName,
    #[serde(default)]
    pub emails: Vec<TypedValue>,
    #[serde(default)]
    pub phones: Vec<TypedValue>,
    #[serde(default)]
    pub organization: String,
    #[serde(default, rename = "jobTitle")]
    pub job_title: String,
    #[serde(default)]
    pub addresses: Vec<PostalAddress>,
    pub birthday: Option<String>,
    #[serde(default)]
    pub notes: String,
    pub revision: i64,
    #[serde(rename = "updatedAt", default)]
    pub updated_at: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContactDraft {
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(default)]
    pub name: StructuredName,
    #[serde(default)]
    pub emails: Vec<TypedValue>,
    #[serde(default)]
    pub phones: Vec<TypedValue>,
    #[serde(default)]
    pub organization: String,
    #[serde(default, rename = "jobTitle")]
    pub job_title: String,
    #[serde(default)]
    pub addresses: Vec<PostalAddress>,
    pub birthday: Option<String>,
    #[serde(default)]
    pub notes: String,
    #[serde(rename = "addressBookId")]
    pub address_book_id: String,
}

impl ContactDraft {
    pub fn from_contact(contact: &Contact) -> Self {
        Self {
            display_name: contact.display_name.clone(),
            name: contact.name.clone(),
            emails: contact.emails.clone(),
            phones: contact.phones.clone(),
            organization: contact.organization.clone(),
            job_title: contact.job_title.clone(),
            addresses: contact.addresses.clone(),
            birthday: contact.birthday.clone(),
            notes: contact.notes.clone(),
            address_book_id: contact.address_book_id.clone(),
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "displayName": self.display_name,
            "name": self.name,
            "emails": self.emails,
            "phones": self.phones,
            "organization": self.organization,
            "jobTitle": self.job_title,
            "addresses": self.addresses,
            "birthday": self.birthday,
            "notes": self.notes,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub availability: Availability,
    pub development: bool,
    pub using_powersync: bool,
    pub offline: bool,
    pub pending_uploads: usize,
    pub last_error: Option<String>,
    pub address_books: Arc<Vec<AddressBook>>,
    pub contacts: Arc<Vec<Contact>>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            availability: Availability::Loading,
            development: true,
            using_powersync: false,
            offline: false,
            pending_uploads: 0,
            last_error: None,
            address_books: Arc::new(Vec::new()),
            contacts: Arc::new(Vec::new()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncBanner {
    Offline { pending: usize },
    Pending { pending: usize },
    StaleEtag { message: String },
    Error { message: String },
}

impl Snapshot {
    pub fn address_book(&self, id: &str) -> Option<&AddressBook> {
        self.address_books.iter().find(|book| book.id == id)
    }

    pub fn contact(&self, id: &str) -> Option<&Contact> {
        self.contacts.iter().find(|contact| contact.id == id)
    }

    pub fn contacts_in(&self, address_book_id: Option<&str>) -> Vec<Contact> {
        let mut contacts = self
            .contacts
            .iter()
            .filter(|contact| address_book_id.is_none_or(|id| contact.address_book_id == id))
            .cloned()
            .collect::<Vec<_>>();
        contacts.sort_by(|left, right| {
            left.display_name
                .to_ascii_lowercase()
                .cmp(&right.display_name.to_ascii_lowercase())
                .then(left.id.cmp(&right.id))
        });
        contacts
    }

    pub fn search(&self, query: &str, address_book_id: Option<&str>) -> Vec<Contact> {
        let query = query.trim().to_ascii_lowercase();
        self.contacts_in(address_book_id)
            .into_iter()
            .filter(|contact| contact_matches(contact, &query))
            .collect()
    }

    pub fn validate_move(&self, contact_id: &str, address_book_id: &str) -> Result<(), String> {
        if self.contact(contact_id).is_none() {
            return Err("The contact was not found.".into());
        }
        if self.address_book(address_book_id).is_none() {
            return Err("The destination address book was not found.".into());
        }
        Ok(())
    }

    pub fn validate_address_book_delete(&self, address_book_id: &str) -> Result<(), String> {
        if self.address_book(address_book_id).is_none() {
            return Err("The address book was not found.".into());
        }
        if self.contacts_in(Some(address_book_id)).is_empty() {
            Ok(())
        } else {
            Err("Address book is not empty. Move or delete its contacts first.".into())
        }
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

pub fn contact_matches(contact: &Contact, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let mut haystacks = vec![
        contact.display_name.as_str(),
        contact.name.given_name.as_str(),
        contact.name.family_name.as_str(),
        contact.organization.as_str(),
        contact.job_title.as_str(),
        contact.notes.as_str(),
    ];
    for email in &contact.emails {
        haystacks.push(email.value.as_str());
    }
    for phone in &contact.phones {
        haystacks.push(phone.value.as_str());
    }
    haystacks
        .into_iter()
        .any(|value| value.to_ascii_lowercase().contains(query))
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
        return Some(if is_stale(&error) {
            SyncBanner::StaleEtag { message: error }
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

fn is_stale(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("stale_etag") || lower.contains("stale etag") || lower.contains("stale revision")
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

    pub fn create_address_book(&self, display_name: String) {
        self.send(Command::CreateAddressBook { display_name });
    }

    pub fn rename_address_book(
        &self,
        id: String,
        revision: i64,
        etag: Option<String>,
        display_name: String,
    ) {
        self.send(Command::RenameAddressBook {
            id,
            revision,
            etag,
            display_name,
        });
    }

    pub fn delete_address_book(&self, id: String, revision: i64, etag: Option<String>) {
        self.send(Command::DeleteAddressBook { id, revision, etag });
    }

    pub fn create_contact(&self, draft: ContactDraft) {
        self.send(Command::CreateContact { draft });
    }

    pub fn update_contact(&self, id: String, revision: i64, etag: String, draft: ContactDraft) {
        self.send(Command::UpdateContact {
            id,
            revision,
            etag,
            draft,
        });
    }

    pub fn move_contact(&self, id: String, revision: i64, etag: String, address_book_id: String) {
        self.send(Command::MoveContact {
            id,
            revision,
            etag,
            address_book_id,
        });
    }

    pub fn delete_contact(&self, id: String, revision: i64, etag: String) {
        self.send(Command::DeleteContact { id, revision, etag });
    }

    fn send(&self, command: Command) {
        if self.commands.try_send(command).is_err() {
            tracing::warn!("contacts worker is not running");
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
        .name("foyer-shell-contacts".into())
        .spawn(move || run_worker(updates_tx, command_rx))
        .expect("failed to start contacts worker");
    Runtime {
        updates,
        controller: Controller { commands },
    }
}

pub fn replica_path() -> PathBuf {
    env::var_os("FOYER_SHELL_CONTACTS_REPLICA_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(foyer_shell_paths::personal_replica_path)
}

#[derive(Debug)]
pub enum Command {
    Refresh,
    CreateAddressBook {
        display_name: String,
    },
    RenameAddressBook {
        id: String,
        revision: i64,
        etag: Option<String>,
        display_name: String,
    },
    DeleteAddressBook {
        id: String,
        revision: i64,
        etag: Option<String>,
    },
    CreateContact {
        draft: ContactDraft,
    },
    UpdateContact {
        id: String,
        revision: i64,
        etag: String,
        draft: ContactDraft,
    },
    MoveContact {
        id: String,
        revision: i64,
        etag: String,
        address_book_id: String,
    },
    DeleteContact {
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
                "create contacts async runtime: {error}"
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
        .map_err(|_| "contacts UI stopped receiving updates".to_string())?;

    let api = api::Client::from_env().await?;
    let path = replica_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create contacts replica directory: {error}"))?;
    }
    PowerSyncEnvironment::powersync_auto_extension()
        .map_err(|error| format!("initialize PowerSync SQLite extension: {error}"))?;
    let pool = ConnectionPool::open(&path)
        .map_err(|error| format!("open contacts replica {}: {error}", path.display()))?;
    let environment = PowerSyncEnvironment::custom(
        reqwest::Client::new(),
        pool,
        PowerSyncEnvironment::tokio_timer(),
    );
    let db = PowerSyncDatabase::new(environment, contacts_schema());
    let conflict = Arc::new(Mutex::new(None));
    let connector = Connector {
        db: db.clone(),
        api,
        conflict: conflict.clone(),
    };
    let _tasks = db.async_tasks().spawn_with_tokio();
    db.connect(SyncOptions::new(connector)).await;

    let mut table_updates = Box::pin(db.watch_tables(true, [ADDRESS_BOOKS_TABLE, CONTACTS_TABLE]));
    let mut status_updates = Box::pin(db.watch_status());
    loop {
        tokio::select! {
            command = commands.recv() => {
                let Ok(command) = command else {
                    db.disconnect().await;
                    return Ok(());
                };
                let command_error = apply_local(&db, command).await.err();
                publish(&updates, &db, &conflict, command_error).await?;
            }
            update = table_updates.next() => {
                if update.is_none() {
                    return Err("PowerSync contacts table watcher stopped".into());
                }
                publish(&updates, &db, &conflict, None).await?;
            }
            status = status_updates.next() => {
                if status.is_none() {
                    return Err("PowerSync contacts status watcher stopped".into());
                }
                publish(&updates, &db, &conflict, None).await?;
            }
        }
    }
}

pub fn schema_tables() -> Vec<Table> {
    let client_columns = || {
        vec![
            Column::text(CLIENT_OPERATION),
            Column::text(OPERATION_ID),
            Column::text(EXPECTED_ETAG),
            Column::integer(EXPECTED_REVISION),
            Column::integer(DELETED_LOCAL),
            Column::text(CLIENT_PAYLOAD),
        ]
    };
    let mut book_columns = vec![
        Column::text("user_id"),
        Column::text("uid"),
        Column::text("href"),
        Column::text("etag"),
        Column::text("display_name"),
        Column::text("description"),
        Column::text("sync_token"),
        Column::text("ctag"),
        Column::integer("revision"),
        Column::text("created_at"),
        Column::text("updated_at"),
    ];
    book_columns.extend(client_columns());
    let mut contact_columns = vec![
        Column::text("user_id"),
        Column::text("address_book_id"),
        Column::text("uid"),
        Column::text("href"),
        Column::text("etag"),
        Column::text("display_name"),
        Column::text("given_name"),
        Column::text("family_name"),
        Column::text("additional_names"),
        Column::text("honorific_prefix"),
        Column::text("honorific_suffix"),
        Column::text("organization"),
        Column::text("job_title"),
        Column::text("birthday"),
        Column::text("notes"),
        Column::text("emails"),
        Column::text("phones"),
        Column::text("addresses"),
        Column::integer("revision"),
        Column::text("created_at"),
        Column::text("updated_at"),
    ];
    contact_columns.extend(client_columns());
    vec![
        Table::create(ADDRESS_BOOKS_TABLE, book_columns, |_| {}),
        Table::create(CONTACTS_TABLE, contact_columns, |_| {}),
    ]
}

pub fn contacts_schema() -> Schema {
    Schema {
        tables: schema_tables(),
        ..Schema::default()
    }
}

async fn publish(
    updates: &Sender<Snapshot>,
    db: &PowerSyncDatabase,
    conflict: &Mutex<Option<String>>,
    command_error: Option<String>,
) -> Result<(), String> {
    let mut snapshot = read_snapshot(db, conflict).await?;
    if command_error.is_some() {
        snapshot.last_error = command_error;
    }
    updates
        .send(snapshot)
        .await
        .map_err(|_| "contacts UI stopped receiving updates".to_string())
}

pub async fn read_snapshot(
    db: &PowerSyncDatabase,
    conflict: &Mutex<Option<String>>,
) -> Result<Snapshot, String> {
    let reader = db
        .reader()
        .await
        .map_err(|error| format!("read contacts replica: {error}"))?;
    let address_books = {
        let mut statement = reader
            .prepare(
                "SELECT id, uid, href, etag, display_name, description, revision \
                 FROM contacts_address_books WHERE COALESCE(deleted_local, 0) = 0",
            )
            .map_err(|error| format!("prepare address book snapshot: {error}"))?;
        statement
            .query_map([], |row| {
                Ok(AddressBook {
                    id: row.get(0)?,
                    uid: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    href: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    etag: row.get(3)?,
                    display_name: row.get(4)?,
                    description: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                    revision: row.get(6)?,
                })
            })
            .map_err(|error| format!("query address book snapshot: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("decode address book snapshot: {error}"))?
    };
    let contacts = {
        let mut statement = reader
            .prepare(
                "SELECT id, address_book_id, uid, href, etag, display_name, given_name, family_name, \
                        additional_names, honorific_prefix, honorific_suffix, organization, job_title, \
                        birthday, notes, emails, phones, addresses, revision, updated_at \
                 FROM contacts WHERE COALESCE(deleted_local, 0) = 0 ORDER BY display_name, id",
            )
            .map_err(|error| format!("prepare contact snapshot: {error}"))?;
        statement
            .query_map([], |row| {
                Ok(Contact {
                    id: row.get(0)?,
                    address_book_id: row.get(1)?,
                    uid: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    href: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    etag: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    display_name: row.get(5)?,
                    name: StructuredName {
                        given_name: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                        family_name: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
                        additional_names: row.get::<_, Option<String>>(8)?.unwrap_or_default(),
                        honorific_prefix: row.get::<_, Option<String>>(9)?.unwrap_or_default(),
                        honorific_suffix: row.get::<_, Option<String>>(10)?.unwrap_or_default(),
                    },
                    organization: row.get::<_, Option<String>>(11)?.unwrap_or_default(),
                    job_title: row.get::<_, Option<String>>(12)?.unwrap_or_default(),
                    birthday: row.get(13)?,
                    notes: row.get::<_, Option<String>>(14)?.unwrap_or_default(),
                    emails: decode_list(row.get(15)?),
                    phones: decode_list(row.get(16)?),
                    addresses: decode_list(row.get(17)?),
                    revision: row.get(18)?,
                    updated_at: row.get::<_, Option<String>>(19)?.unwrap_or_default(),
                })
            })
            .map_err(|error| format!("query contact snapshot: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("decode contact snapshot: {error}"))?
    };
    let pending_uploads = reader
        .query_row("SELECT COUNT(*) FROM ps_crud", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|error| format!("count pending contacts writes: {error}"))?
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
        offline: !status.is_connected(),
        pending_uploads,
        last_error: conflict.or(sync_error),
        address_books: Arc::new(address_books),
        contacts: Arc::new(contacts),
    })
}

fn decode_list<T: for<'de> Deserialize<'de>>(raw: Option<String>) -> Vec<T> {
    raw.and_then(|value| serde_json::from_str(&value).ok())
        .unwrap_or_default()
}

pub async fn apply_local(db: &PowerSyncDatabase, command: Command) -> Result<(), String> {
    if let Command::DeleteAddressBook { id, .. } = &command {
        let snapshot = read_snapshot(db, &Mutex::new(None)).await?;
        snapshot.validate_address_book_delete(id)?;
    }
    if let Command::MoveContact {
        id,
        address_book_id,
        ..
    } = &command
    {
        let snapshot = read_snapshot(db, &Mutex::new(None)).await?;
        snapshot.validate_move(id, address_book_id)?;
    }
    let writer = db
        .writer()
        .await
        .map_err(|error| format!("write contacts replica: {error}"))?;
    let now = chrono::Utc::now().to_rfc3339();
    let operation_id = Uuid::new_v4().to_string();
    match command {
        Command::Refresh => return Ok(()),
        Command::CreateAddressBook { display_name } => {
            let display_name = required_title(display_name, "Address book name")?;
            let id = Uuid::new_v4().to_string();
            writer.execute(
                "INSERT INTO contacts_address_books \
                 (id, user_id, uid, href, etag, display_name, description, revision, created_at, updated_at, \
                  client_operation, operation_id, expected_etag, expected_revision, deleted_local) \
                 VALUES (?, '', ?, '', NULL, ?, '', 1, ?, ?, 'create', ?, NULL, NULL, 0)",
                params![id.clone(), id, display_name, now, now, operation_id],
            )
        }
        Command::RenameAddressBook {
            id,
            revision,
            etag,
            display_name,
        } => {
            let display_name = required_title(display_name, "Address book name")?;
            writer.execute(
                "UPDATE contacts_address_books SET display_name = ?, revision = ?, updated_at = ?, \
                 client_operation = 'update', operation_id = ?, expected_etag = ?, expected_revision = ? \
                 WHERE id = ?",
                params![display_name, revision + 1, now, operation_id, etag, revision, id],
            )
        }
        Command::DeleteAddressBook { id, revision, etag } => writer.execute(
            "UPDATE contacts_address_books SET deleted_local = 1, revision = ?, updated_at = ?, \
             client_operation = 'delete', operation_id = ?, expected_etag = ?, expected_revision = ? \
             WHERE id = ?",
            params![revision + 1, now, operation_id, etag, revision, id],
        ),
        Command::CreateContact { draft } => {
            let display_name = match required_title(draft.display_name.clone(), "Display name") {
                Ok(name) => name,
                Err(_) if !draft.name.formatted().is_empty() => draft.name.formatted(),
                Err(_) => {
                    return Err("A contact needs a name, email, or organization.".into());
                }
            };
            if draft.address_book_id.is_empty() {
                return Err("Choose an address book.".into());
            }
            let id = Uuid::new_v4().to_string();
            let payload = draft.to_json().to_string();
            writer.execute(
                "INSERT INTO contacts \
                 (id, user_id, address_book_id, uid, href, etag, display_name, given_name, family_name, \
                  additional_names, honorific_prefix, honorific_suffix, organization, job_title, \
                  birthday, notes, emails, phones, addresses, revision, created_at, updated_at, \
                  client_operation, operation_id, expected_etag, expected_revision, deleted_local, client_payload) \
                 VALUES (?, '', ?, ?, '', '', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, \
                         'create', ?, NULL, NULL, 0, ?)",
                params![
                    id.clone(),
                    draft.address_book_id,
                    format!("urn:uuid:{id}"),
                    display_name,
                    draft.name.given_name,
                    draft.name.family_name,
                    draft.name.additional_names,
                    draft.name.honorific_prefix,
                    draft.name.honorific_suffix,
                    draft.organization,
                    draft.job_title,
                    draft.birthday,
                    draft.notes,
                    serde_json::to_string(&draft.emails).unwrap_or_else(|_| "[]".into()),
                    serde_json::to_string(&draft.phones).unwrap_or_else(|_| "[]".into()),
                    serde_json::to_string(&draft.addresses).unwrap_or_else(|_| "[]".into()),
                    now,
                    now,
                    operation_id,
                    payload
                ],
            )
        }
        Command::UpdateContact {
            id,
            revision,
            etag,
            draft,
        } => {
            let display_name = required_title(draft.display_name.clone(), "Display name")?;
            let payload = draft.to_json().to_string();
            writer.execute(
                "UPDATE contacts SET display_name = ?, given_name = ?, family_name = ?, additional_names = ?, \
                 honorific_prefix = ?, honorific_suffix = ?, organization = ?, job_title = ?, birthday = ?, \
                 notes = ?, emails = ?, phones = ?, addresses = ?, revision = ?, updated_at = ?, \
                 client_operation = 'update', operation_id = ?, expected_etag = ?, expected_revision = ?, \
                 client_payload = ? WHERE id = ?",
                params![
                    display_name,
                    draft.name.given_name,
                    draft.name.family_name,
                    draft.name.additional_names,
                    draft.name.honorific_prefix,
                    draft.name.honorific_suffix,
                    draft.organization,
                    draft.job_title,
                    draft.birthday,
                    draft.notes,
                    serde_json::to_string(&draft.emails).unwrap_or_else(|_| "[]".into()),
                    serde_json::to_string(&draft.phones).unwrap_or_else(|_| "[]".into()),
                    serde_json::to_string(&draft.addresses).unwrap_or_else(|_| "[]".into()),
                    revision + 1,
                    now,
                    operation_id,
                    etag,
                    revision,
                    payload,
                    id
                ],
            )
        }
        Command::MoveContact {
            id,
            revision,
            etag,
            address_book_id,
        } => writer.execute(
            "UPDATE contacts SET address_book_id = ?, revision = ?, updated_at = ?, \
             client_operation = 'move', operation_id = ?, expected_etag = ?, expected_revision = ? \
             WHERE id = ?",
            params![
                address_book_id,
                revision + 1,
                now,
                operation_id,
                etag,
                revision,
                id
            ],
        ),
        Command::DeleteContact { id, revision, etag } => writer.execute(
            "UPDATE contacts SET deleted_local = 1, revision = ?, updated_at = ?, \
             client_operation = 'delete', operation_id = ?, expected_etag = ?, expected_revision = ? \
             WHERE id = ?",
            params![revision + 1, now, operation_id, etag, revision, id],
        ),
    }
    .map_err(|error| format!("apply local contacts command: {error}"))?;
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
        ("contacts_address_books", "create") => {
            api.create_address_book(
                operation_id,
                &entry.id,
                required_text(data, "display_name")?,
                optional_text(data, "description"),
            )
            .await?;
        }
        ("contacts_address_books", "update") => {
            api.update_address_book(
                operation_id,
                &entry.id,
                optional_text(data, EXPECTED_ETAG),
                required_i64(data, EXPECTED_REVISION)?,
                required_text(data, "display_name")?,
            )
            .await?;
        }
        ("contacts_address_books", "delete") => {
            api.delete_address_book(
                operation_id,
                &entry.id,
                optional_text(data, EXPECTED_ETAG),
                required_i64(data, EXPECTED_REVISION)?,
            )
            .await?;
        }
        ("contacts", "create") => {
            let draft = draft_from_entry(data, &entry.id)?;
            api.create_contact(
                operation_id,
                &entry.id,
                required_text(data, "address_book_id")?,
                &draft,
            )
            .await?;
        }
        ("contacts", "update") => {
            let draft = draft_from_entry(data, &entry.id)?;
            api.update_contact(
                operation_id,
                &entry.id,
                optional_text(data, EXPECTED_ETAG),
                required_i64(data, EXPECTED_REVISION)?,
                &draft,
            )
            .await?;
        }
        ("contacts", "move") => {
            api.move_contact(
                operation_id,
                &entry.id,
                optional_text(data, EXPECTED_ETAG),
                required_i64(data, EXPECTED_REVISION)?,
                required_text(data, "address_book_id")?,
            )
            .await?;
        }
        ("contacts", "delete") => {
            api.delete_contact(
                operation_id,
                &entry.id,
                optional_text(data, EXPECTED_ETAG),
                required_i64(data, EXPECTED_REVISION)?,
            )
            .await?;
        }
        (table, operation) => {
            return Err(api::ApiError::InvalidCommand(format!(
                "unsupported local contacts command {table}/{operation}"
            )));
        }
    }
    Ok(())
}

fn draft_from_entry(data: &Map<String, Value>, _id: &str) -> Result<ContactDraft, api::ApiError> {
    if let Some(payload) = optional_text(data, CLIENT_PAYLOAD) {
        if let Ok(draft) = serde_json::from_str::<ContactDraft>(payload) {
            let mut draft = draft;
            if draft.address_book_id.is_empty() {
                draft.address_book_id = optional_text(data, "address_book_id")
                    .unwrap_or_default()
                    .to_string();
            }
            return Ok(draft);
        }
        if let Ok(mut value) = serde_json::from_str::<Value>(payload) {
            if let Some(object) = value.as_object_mut() {
                object.insert(
                    "addressBookId".into(),
                    json!(optional_text(data, "address_book_id").unwrap_or_default()),
                );
            }
            if let Ok(draft) = serde_json::from_value::<ContactDraft>(value) {
                return Ok(draft);
            }
        }
    }
    Ok(ContactDraft {
        display_name: required_text(data, "display_name")?.to_string(),
        name: StructuredName {
            given_name: optional_text(data, "given_name")
                .unwrap_or_default()
                .to_string(),
            family_name: optional_text(data, "family_name")
                .unwrap_or_default()
                .to_string(),
            additional_names: optional_text(data, "additional_names")
                .unwrap_or_default()
                .to_string(),
            honorific_prefix: optional_text(data, "honorific_prefix")
                .unwrap_or_default()
                .to_string(),
            honorific_suffix: optional_text(data, "honorific_suffix")
                .unwrap_or_default()
                .to_string(),
        },
        emails: optional_text(data, "emails")
            .and_then(|raw| serde_json::from_str(raw).ok())
            .unwrap_or_default(),
        phones: optional_text(data, "phones")
            .and_then(|raw| serde_json::from_str(raw).ok())
            .unwrap_or_default(),
        organization: optional_text(data, "organization")
            .unwrap_or_default()
            .to_string(),
        job_title: optional_text(data, "job_title")
            .unwrap_or_default()
            .to_string(),
        addresses: optional_text(data, "addresses")
            .and_then(|raw| serde_json::from_str(raw).ok())
            .unwrap_or_default(),
        birthday: optional_text(data, "birthday").map(str::to_string),
        notes: optional_text(data, "notes").unwrap_or_default().to_string(),
        address_book_id: required_text(data, "address_book_id")?.to_string(),
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
                    "stale_etag" | "stale_revision" => {
                        "This contact changed on another device. The server copy will replace the rejected edit.".into()
                    }
                    "address_book_not_empty" => {
                        "Address book is not empty. Move or delete its contacts first.".into()
                    }
                    "invalid_parent" => "That address book destination is not valid.".into(),
                    "gone" => "This item was deleted on the server and cannot be restored.".into(),
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
        PowerSyncDatabase::new(environment, contacts_schema())
    }

    fn sample_snapshot() -> Snapshot {
        Snapshot {
            availability: Availability::Available,
            development: true,
            using_powersync: true,
            address_books: Arc::new(vec![
                AddressBook {
                    id: "personal".into(),
                    uid: "personal".into(),
                    href: "/u/personal/".into(),
                    etag: Some("\"e1\"".into()),
                    display_name: "Personal".into(),
                    description: String::new(),
                    revision: 1,
                },
                AddressBook {
                    id: "work".into(),
                    uid: "work".into(),
                    href: "/u/work/".into(),
                    etag: Some("\"e2\"".into()),
                    display_name: "Work".into(),
                    description: String::new(),
                    revision: 1,
                },
            ]),
            contacts: Arc::new(vec![Contact {
                id: "ada".into(),
                address_book_id: "personal".into(),
                uid: "urn:uuid:ada".into(),
                href: "/u/personal/ada.vcf".into(),
                etag: "\"c1\"".into(),
                display_name: "Ada Lovelace".into(),
                name: StructuredName {
                    given_name: "Ada".into(),
                    family_name: "Lovelace".into(),
                    ..StructuredName::default()
                },
                emails: vec![TypedValue {
                    value: "ada@example.com".into(),
                    r#type: "work".into(),
                    pref: true,
                }],
                phones: vec![TypedValue {
                    value: "+1-555-0100".into(),
                    r#type: "cell".into(),
                    pref: false,
                }],
                organization: "Analytical".into(),
                job_title: "Mathematician".into(),
                addresses: Vec::new(),
                birthday: Some("1815-12-10".into()),
                notes: "Notes stay lossless.\n".into(),
                revision: 1,
                updated_at: String::new(),
            }]),
            ..Snapshot::default()
        }
    }

    #[test]
    fn search_and_book_validation() {
        let snapshot = sample_snapshot();
        assert_eq!(snapshot.search("ADA@", None)[0].id, "ada");
        assert!(snapshot.search("nobody", None).is_empty());
        assert!(snapshot.validate_move("ada", "work").is_ok());
        assert_eq!(
            snapshot.validate_move("ada", "missing").unwrap_err(),
            "The destination address book was not found."
        );
        assert_eq!(
            snapshot
                .validate_address_book_delete("personal")
                .unwrap_err(),
            "Address book is not empty. Move or delete its contacts first."
        );
        assert!(snapshot.validate_address_book_delete("work").is_ok());
    }

    #[test]
    fn sync_banner_prefers_stale_etag() {
        assert_eq!(
            sync_banner(Availability::Available, true, 2, Some("stale_etag".into()),),
            Some(SyncBanner::StaleEtag {
                message: "stale_etag".into()
            })
        );
        assert_eq!(
            sync_banner(Availability::Available, true, 3, None),
            Some(SyncBanner::Offline { pending: 3 })
        );
    }

    #[test]
    fn public_upload_error_maps_stale_etag() {
        let error = api::ApiError::Response {
            status: reqwest::StatusCode::CONFLICT,
            body:
                r#"{"error":{"code":"stale_etag","message":"The expected ETag does not match."}}"#
                    .into(),
        };
        assert!(public_upload_error(&error).contains("another device"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn offline_commands_survive_reopening_the_replica() {
        let path = env::temp_dir().join(format!(
            "foyer-contacts-powersync-test-{}.sqlite3",
            Uuid::new_v4()
        ));
        let conflict = Mutex::new(None);
        let db = test_database(&path);
        apply_local(
            &db,
            Command::CreateAddressBook {
                display_name: "Personal".into(),
            },
        )
        .await
        .unwrap();
        let first = read_snapshot(&db, &conflict).await.unwrap();
        let book_id = first.address_books[0].id.clone();
        apply_local(
            &db,
            Command::CreateContact {
                draft: ContactDraft {
                    display_name: "Ada".into(),
                    notes: "Keep  trailing  \n".into(),
                    emails: vec![TypedValue {
                        value: "ada@example.com".into(),
                        r#type: "work".into(),
                        pref: true,
                    }],
                    address_book_id: book_id,
                    ..ContactDraft::default()
                },
            },
        )
        .await
        .unwrap();
        drop(db);

        let reopened = test_database(&path);
        let snapshot = read_snapshot(&reopened, &conflict).await.unwrap();
        assert!(snapshot.using_powersync);
        assert_eq!(snapshot.pending_uploads, 2);
        assert_eq!(snapshot.address_books[0].display_name, "Personal");
        assert_eq!(snapshot.contacts[0].display_name, "Ada");
        assert_eq!(snapshot.contacts[0].notes, "Keep  trailing  \n");
        assert_eq!(snapshot.contacts[0].emails[0].value, "ada@example.com");
        drop(reopened);

        for suffix in ["", "-shm", "-wal"] {
            let _ = fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn nonempty_book_delete_is_rejected_locally() {
        let path = env::temp_dir().join(format!(
            "foyer-contacts-powersync-empty-{}.sqlite3",
            Uuid::new_v4()
        ));
        let conflict = Mutex::new(None);
        let db = test_database(&path);
        apply_local(
            &db,
            Command::CreateAddressBook {
                display_name: "Busy".into(),
            },
        )
        .await
        .unwrap();
        let book = read_snapshot(&db, &conflict).await.unwrap().address_books[0].clone();
        apply_local(
            &db,
            Command::CreateContact {
                draft: ContactDraft {
                    display_name: "Someone".into(),
                    address_book_id: book.id.clone(),
                    ..ContactDraft::default()
                },
            },
        )
        .await
        .unwrap();
        let rejected = apply_local(
            &db,
            Command::DeleteAddressBook {
                id: book.id,
                revision: book.revision,
                etag: book.etag,
            },
        )
        .await
        .unwrap_err();
        assert!(rejected.contains("not empty"));
        drop(db);
        for suffix in ["", "-shm", "-wal"] {
            let _ = fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }
}
