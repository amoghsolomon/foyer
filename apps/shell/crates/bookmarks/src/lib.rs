//! Hosted bookmarks adapter. Reads come from immutable snapshots; I/O never runs in GPUI.

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
pub mod validation;

pub const FOLDERS_TABLE: &str = "bookmarks_folders";
pub const BOOKMARKS_TABLE: &str = "bookmarks";

const CLIENT_OPERATION: &str = "client_operation";
const OPERATION_ID: &str = "operation_id";
const EXPECTED_REVISION: &str = "expected_revision";
const DELETED_LOCAL: &str = "deleted_local";
const CLIENT_PAYLOAD: &str = "client_payload";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Availability {
    Loading,
    Available,
    Unavailable(String),
}

impl Availability {
    pub fn detail(&self) -> &str {
        match self {
            Self::Loading => "Loading bookmarks…",
            Self::Available => "Available",
            Self::Unavailable(error) => error,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Folder {
    pub id: String,
    #[serde(rename = "parentId")]
    pub parent_id: Option<String>,
    pub name: String,
    pub position: i32,
    pub revision: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: String,
    #[serde(rename = "folderId")]
    pub folder_id: String,
    pub url: String,
    pub title: String,
    pub description: String,
    pub tags: Vec<String>,
    pub favorite: bool,
    pub archived: bool,
    pub position: i32,
    pub revision: i64,
    #[serde(rename = "updatedAt", default)]
    pub updated_at: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    All,
    Favorites,
    Archived,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub availability: Availability,
    pub development: bool,
    pub using_powersync: bool,
    pub offline: bool,
    pub pending_uploads: usize,
    pub last_error: Option<String>,
    pub folders: Arc<Vec<Folder>>,
    pub bookmarks: Arc<Vec<Bookmark>>,
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
            folders: Arc::new(Vec::new()),
            bookmarks: Arc::new(Vec::new()),
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
    pub fn folder(&self, id: &str) -> Option<&Folder> {
        self.folders.iter().find(|folder| folder.id == id)
    }

    pub fn bookmark(&self, id: &str) -> Option<&Bookmark> {
        self.bookmarks.iter().find(|bookmark| bookmark.id == id)
    }

    pub fn child_folders(&self, parent_id: Option<&str>) -> Vec<Folder> {
        let mut folders = self
            .folders
            .iter()
            .filter(|folder| folder.parent_id.as_deref() == parent_id)
            .cloned()
            .collect::<Vec<_>>();
        folders.sort_by(|left, right| {
            left.position
                .cmp(&right.position)
                .then(left.name.cmp(&right.name))
                .then(left.id.cmp(&right.id))
        });
        folders
    }

    pub fn bookmarks_in(&self, folder_id: &str) -> Vec<Bookmark> {
        let mut items = self
            .bookmarks
            .iter()
            .filter(|bookmark| bookmark.folder_id == folder_id)
            .cloned()
            .collect::<Vec<_>>();
        items.sort_by(|left, right| {
            left.archived
                .cmp(&right.archived)
                .then(right.favorite.cmp(&left.favorite))
                .then(left.position.cmp(&right.position))
                .then(left.title.cmp(&right.title))
                .then(left.id.cmp(&right.id))
        });
        items
    }

    pub fn folder_path(&self, folder_id: &str) -> Vec<Folder> {
        let mut path = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut current = self.folder(folder_id).cloned();
        while let Some(folder) = current {
            if !seen.insert(folder.id.clone()) {
                break;
            }
            let parent_id = folder.parent_id.clone();
            path.push(folder);
            current = parent_id.as_deref().and_then(|id| self.folder(id)).cloned();
        }
        path.reverse();
        path
    }

    pub fn folder_path_label(&self, folder_id: &str) -> String {
        self.folder_path(folder_id)
            .into_iter()
            .map(|folder| folder.name)
            .collect::<Vec<_>>()
            .join(" / ")
    }

    pub fn descendant_folder_ids(&self, folder_id: &str) -> std::collections::HashSet<String> {
        let mut ids = std::collections::HashSet::new();
        let mut stack = vec![folder_id.to_string()];
        while let Some(id) = stack.pop() {
            if !ids.insert(id.clone()) {
                continue;
            }
            stack.extend(
                self.child_folders(Some(&id))
                    .into_iter()
                    .map(|folder| folder.id),
            );
        }
        ids
    }

    pub fn folder_is_empty(&self, folder_id: &str) -> bool {
        self.child_folders(Some(folder_id)).is_empty() && self.bookmarks_in(folder_id).is_empty()
    }

    pub fn valid_folder_move_targets(&self, folder_id: &str) -> Vec<Folder> {
        let blocked = self.descendant_folder_ids(folder_id);
        let mut folders = self
            .folders
            .iter()
            .filter(|folder| !blocked.contains(&folder.id))
            .cloned()
            .collect::<Vec<_>>();
        folders.sort_by(|left, right| {
            self.folder_path_label(&left.id)
                .cmp(&self.folder_path_label(&right.id))
                .then(left.id.cmp(&right.id))
        });
        folders
    }

    pub fn validate_folder_move(
        &self,
        folder_id: &str,
        parent_id: Option<&str>,
    ) -> Result<(), String> {
        if self.folder(folder_id).is_none() {
            return Err("The folder was not found.".into());
        }
        let Some(parent_id) = parent_id else {
            return Ok(());
        };
        if parent_id == folder_id {
            return Err("A folder cannot be moved into itself.".into());
        }
        if self.descendant_folder_ids(folder_id).contains(parent_id) {
            return Err("A folder cannot be moved into its own descendant.".into());
        }
        if self.folder(parent_id).is_none() {
            return Err("The destination folder was not found.".into());
        }
        Ok(())
    }

    pub fn validate_folder_delete(&self, folder_id: &str) -> Result<(), String> {
        if self.folder(folder_id).is_none() {
            return Err("The folder was not found.".into());
        }
        if self.folder_is_empty(folder_id) {
            Ok(())
        } else {
            Err("Folder is not empty. Move or delete its bookmarks and folders first.".into())
        }
    }

    pub fn visible_bookmarks(
        &self,
        query: &str,
        filter: Filter,
        tag: Option<&str>,
        folder_id: Option<&str>,
    ) -> Vec<Bookmark> {
        let needle = query.trim().to_ascii_lowercase();
        let mut items = self
            .bookmarks
            .iter()
            .filter(|bookmark| match filter {
                Filter::All => !bookmark.archived,
                Filter::Favorites => bookmark.favorite && !bookmark.archived,
                Filter::Archived => bookmark.archived,
            })
            .filter(|bookmark| folder_id.is_none_or(|id| bookmark.folder_id == id))
            .filter(|bookmark| {
                tag.is_none_or(|wanted| bookmark.tags.iter().any(|tag| tag == wanted))
            })
            .filter(|bookmark| {
                needle.is_empty()
                    || bookmark.title.to_ascii_lowercase().contains(&needle)
                    || bookmark.url.to_ascii_lowercase().contains(&needle)
                    || bookmark.description.to_ascii_lowercase().contains(&needle)
                    || bookmark
                        .tags
                        .iter()
                        .any(|tag| tag.to_ascii_lowercase().contains(&needle))
            })
            .cloned()
            .collect::<Vec<_>>();
        items.sort_by(|left, right| {
            left.archived
                .cmp(&right.archived)
                .then(right.favorite.cmp(&left.favorite))
                .then(right.updated_at.cmp(&left.updated_at))
                .then(left.title.cmp(&right.title))
                .then(left.id.cmp(&right.id))
        });
        items
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

    pub fn create_folder(&self, name: String, parent_id: Option<String>) {
        self.send(Command::CreateFolder { name, parent_id });
    }

    pub fn rename_folder(&self, id: String, revision: i64, name: String) {
        self.send(Command::RenameFolder { id, revision, name });
    }

    pub fn move_folder(&self, id: String, revision: i64, parent_id: Option<String>) {
        self.send(Command::MoveFolder {
            id,
            revision,
            parent_id,
        });
    }

    pub fn delete_folder(&self, id: String, revision: i64) {
        self.send(Command::DeleteFolder { id, revision });
    }

    pub fn create_bookmark(
        &self,
        folder_id: String,
        url: String,
        title: String,
        description: String,
        tags: Vec<String>,
    ) {
        self.send(Command::CreateBookmark {
            folder_id,
            url,
            title,
            description,
            tags,
        });
    }

    pub fn update_bookmark(
        &self,
        id: String,
        revision: i64,
        url: String,
        title: String,
        description: String,
        tags: Vec<String>,
    ) {
        self.send(Command::UpdateBookmark {
            id,
            revision,
            url,
            title,
            description,
            tags,
        });
    }

    pub fn move_bookmark(&self, id: String, revision: i64, folder_id: String) {
        self.send(Command::MoveBookmark {
            id,
            revision,
            folder_id,
        });
    }

    pub fn favorite_bookmark(&self, id: String, revision: i64, favorite: bool) {
        self.send(Command::FavoriteBookmark {
            id,
            revision,
            favorite,
        });
    }

    pub fn archive_bookmark(&self, id: String, revision: i64, archived: bool) {
        self.send(Command::ArchiveBookmark {
            id,
            revision,
            archived,
        });
    }

    pub fn delete_bookmark(&self, id: String, revision: i64) {
        self.send(Command::DeleteBookmark { id, revision });
    }

    fn send(&self, command: Command) {
        if self.commands.try_send(command).is_err() {
            tracing::warn!("bookmarks worker is not running");
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
        .name("foyer-shell-bookmarks".into())
        .spawn(move || run_worker(updates_tx, command_rx))
        .expect("failed to start bookmarks worker");
    Runtime {
        updates,
        controller: Controller { commands },
    }
}

pub fn replica_path() -> PathBuf {
    env::var_os("FOYER_SHELL_BOOKMARKS_REPLICA_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(foyer_shell_paths::personal_replica_path)
}

#[derive(Debug)]
pub enum Command {
    Refresh,
    CreateFolder {
        name: String,
        parent_id: Option<String>,
    },
    RenameFolder {
        id: String,
        revision: i64,
        name: String,
    },
    MoveFolder {
        id: String,
        revision: i64,
        parent_id: Option<String>,
    },
    DeleteFolder {
        id: String,
        revision: i64,
    },
    CreateBookmark {
        folder_id: String,
        url: String,
        title: String,
        description: String,
        tags: Vec<String>,
    },
    UpdateBookmark {
        id: String,
        revision: i64,
        url: String,
        title: String,
        description: String,
        tags: Vec<String>,
    },
    MoveBookmark {
        id: String,
        revision: i64,
        folder_id: String,
    },
    FavoriteBookmark {
        id: String,
        revision: i64,
        favorite: bool,
    },
    ArchiveBookmark {
        id: String,
        revision: i64,
        archived: bool,
    },
    DeleteBookmark {
        id: String,
        revision: i64,
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
                "create bookmarks async runtime: {error}"
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
        .map_err(|_| "bookmarks UI stopped receiving updates".to_string())?;

    let api = api::Client::from_env().await?;
    let path = replica_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create bookmarks replica directory: {error}"))?;
    }
    PowerSyncEnvironment::powersync_auto_extension()
        .map_err(|error| format!("initialize PowerSync SQLite extension: {error}"))?;
    let pool = ConnectionPool::open(&path)
        .map_err(|error| format!("open bookmarks replica {}: {error}", path.display()))?;
    let environment = PowerSyncEnvironment::custom(
        reqwest::Client::new(),
        pool,
        PowerSyncEnvironment::tokio_timer(),
    );
    let db = PowerSyncDatabase::new(environment, bookmarks_schema());
    let conflict = Arc::new(Mutex::new(None));
    let connector = Connector {
        db: db.clone(),
        api,
        conflict: conflict.clone(),
    };
    let _tasks = db.async_tasks().spawn_with_tokio();
    db.connect(SyncOptions::new(connector)).await;

    let mut table_updates = Box::pin(db.watch_tables(true, ["bookmarks_folders", "bookmarks"]));
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
                    return Err("PowerSync bookmarks table watcher stopped".into());
                }
                publish(&updates, &db, &conflict, None).await?;
            }
            status = status_updates.next() => {
                if status.is_none() {
                    return Err("PowerSync bookmarks status watcher stopped".into());
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
            Column::integer(EXPECTED_REVISION),
            Column::integer(DELETED_LOCAL),
            Column::text(CLIENT_PAYLOAD),
        ]
    };
    let mut folder_columns = vec![
        Column::text("user_id"),
        Column::text("parent_id"),
        Column::text("name"),
        Column::integer("position"),
        Column::integer("revision"),
        Column::text("created_at"),
        Column::text("updated_at"),
    ];
    folder_columns.extend(client_columns());
    let mut bookmark_columns = vec![
        Column::text("user_id"),
        Column::text("folder_id"),
        Column::text("url"),
        Column::text("title"),
        Column::text("description"),
        Column::text("tags"),
        Column::integer("favorite"),
        Column::integer("archived"),
        Column::integer("position"),
        Column::integer("revision"),
        Column::text("created_at"),
        Column::text("updated_at"),
    ];
    bookmark_columns.extend(client_columns());
    vec![
        Table::create(FOLDERS_TABLE, folder_columns, |_| {}),
        Table::create(BOOKMARKS_TABLE, bookmark_columns, |_| {}),
    ]
}

pub fn bookmarks_schema() -> Schema {
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
        .map_err(|_| "bookmarks UI stopped receiving updates".to_string())
}

pub async fn read_snapshot(
    db: &PowerSyncDatabase,
    conflict: &Mutex<Option<String>>,
) -> Result<Snapshot, String> {
    let reader = db
        .reader()
        .await
        .map_err(|error| format!("read bookmarks replica: {error}"))?;
    let folders = {
        let mut statement = reader
            .prepare(
                "SELECT id, parent_id, name, position, revision FROM bookmarks_folders \
                 WHERE COALESCE(deleted_local, 0) = 0",
            )
            .map_err(|error| format!("prepare folder snapshot: {error}"))?;
        statement
            .query_map([], |row| {
                Ok(Folder {
                    id: row.get(0)?,
                    parent_id: row.get(1)?,
                    name: row.get(2)?,
                    position: row.get(3)?,
                    revision: row.get(4)?,
                })
            })
            .map_err(|error| format!("query folder snapshot: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("decode folder snapshot: {error}"))?
    };
    let bookmarks = {
        let mut statement = reader
            .prepare(
                "SELECT id, folder_id, url, title, description, tags, favorite, archived, \
                        position, revision, updated_at FROM bookmarks \
                 WHERE COALESCE(deleted_local, 0) = 0 ORDER BY updated_at DESC, id",
            )
            .map_err(|error| format!("prepare bookmark snapshot: {error}"))?;
        statement
            .query_map([], |row| {
                Ok(Bookmark {
                    id: row.get(0)?,
                    folder_id: row.get(1)?,
                    url: row.get(2)?,
                    title: row.get(3)?,
                    description: row.get(4)?,
                    tags: decode_tags(row.get::<_, Option<String>>(5)?),
                    favorite: decode_flag(row.get_ref(6)?),
                    archived: decode_flag(row.get_ref(7)?),
                    position: row.get::<_, Option<i32>>(8)?.unwrap_or(0),
                    revision: row.get::<_, Option<i64>>(9)?.unwrap_or(1),
                    updated_at: row.get::<_, Option<String>>(10)?.unwrap_or_default(),
                })
            })
            .map_err(|error| format!("query bookmark snapshot: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("decode bookmark snapshot: {error}"))?
    };
    let pending_uploads = reader
        .query_row("SELECT COUNT(*) FROM ps_crud", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|error| format!("count pending bookmarks writes: {error}"))?
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
        folders: Arc::new(folders),
        bookmarks: Arc::new(bookmarks),
    })
}

fn decode_tags(value: Option<String>) -> Vec<String> {
    let Some(value) = value.filter(|item| !item.is_empty() && item != "null") else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<String>>(&value).unwrap_or_default()
}

fn decode_flag(value: rusqlite::types::ValueRef<'_>) -> bool {
    match value {
        rusqlite::types::ValueRef::Integer(number) => number != 0,
        rusqlite::types::ValueRef::Text(text) => {
            let text = String::from_utf8_lossy(text);
            matches!(text.as_ref(), "true" | "t" | "1")
        }
        rusqlite::types::ValueRef::Real(number) => number != 0.0,
        _ => false,
    }
}

pub async fn apply_local(db: &PowerSyncDatabase, command: Command) -> Result<(), String> {
    match &command {
        Command::MoveFolder { id, parent_id, .. } => {
            let snapshot = read_snapshot(db, &Mutex::new(None)).await?;
            snapshot.validate_folder_move(id, parent_id.as_deref())?;
        }
        Command::DeleteFolder { id, .. } => {
            let snapshot = read_snapshot(db, &Mutex::new(None)).await?;
            snapshot.validate_folder_delete(id)?;
        }
        _ => {}
    }
    let writer = db
        .writer()
        .await
        .map_err(|error| format!("write bookmarks replica: {error}"))?;
    let now = chrono::Utc::now().to_rfc3339();
    let operation_id = Uuid::new_v4().to_string();
    match command {
        Command::Refresh => return Ok(()),
        Command::CreateFolder { name, parent_id } => {
            let name = validation::required_title(name, "Folder name")?;
            writer.execute(
                "INSERT INTO bookmarks_folders \
                 (id, user_id, parent_id, name, position, revision, created_at, updated_at, \
                  client_operation, operation_id, expected_revision, deleted_local) \
                 VALUES (?, '', ?, ?, 0, 1, ?, ?, 'create', ?, NULL, 0)",
                params![
                    Uuid::new_v4().to_string(),
                    parent_id,
                    name,
                    now,
                    now,
                    operation_id
                ],
            )
        }
        Command::RenameFolder { id, revision, name } => {
            let name = validation::required_title(name, "Folder name")?;
            writer.execute(
                "UPDATE bookmarks_folders SET name = ?, revision = ?, updated_at = ?, \
                 client_operation = 'rename', operation_id = ?, expected_revision = ? WHERE id = ?",
                params![name, revision + 1, now, operation_id, revision, id],
            )
        }
        Command::MoveFolder {
            id,
            revision,
            parent_id,
        } => writer.execute(
            "UPDATE bookmarks_folders SET parent_id = ?, revision = ?, updated_at = ?, \
             client_operation = 'move', operation_id = ?, expected_revision = ? WHERE id = ?",
            params![parent_id, revision + 1, now, operation_id, revision, id],
        ),
        Command::DeleteFolder { id, revision } => writer.execute(
            "UPDATE bookmarks_folders SET deleted_local = 1, revision = ?, updated_at = ?, \
             client_operation = 'delete', operation_id = ?, expected_revision = ? WHERE id = ?",
            params![revision + 1, now, operation_id, revision, id],
        ),
        Command::CreateBookmark {
            folder_id,
            url,
            title,
            description,
            tags,
        } => {
            let url = validation::validate_bookmark_url(&url)?;
            let title = validation::required_title(title, "Bookmark title")?;
            let description = validation::lossless_description(description)?;
            let tags = validation::normalize_tags(&tags)?;
            writer.execute(
                "INSERT INTO bookmarks \
                 (id, user_id, folder_id, url, title, description, tags, favorite, archived, position, \
                  revision, created_at, updated_at, client_operation, operation_id, expected_revision, deleted_local) \
                 VALUES (?, '', ?, ?, ?, ?, ?, 0, 0, 0, 1, ?, ?, 'create', ?, NULL, 0)",
                params![
                    Uuid::new_v4().to_string(),
                    folder_id,
                    url,
                    title,
                    description,
                    serde_json::to_string(&tags).unwrap_or_else(|_| "[]".into()),
                    now,
                    now,
                    operation_id
                ],
            )
        }
        Command::UpdateBookmark {
            id,
            revision,
            url,
            title,
            description,
            tags,
        } => {
            let url = validation::validate_bookmark_url(&url)?;
            let title = validation::required_title(title, "Bookmark title")?;
            let description = validation::lossless_description(description)?;
            let tags = validation::normalize_tags(&tags)?;
            let payload = json!({
                "operationId": operation_id,
                "url": url,
                "title": title,
                "description": description,
                "tags": tags,
            })
            .to_string();
            writer.execute(
                "UPDATE bookmarks SET url = ?, title = ?, description = ?, tags = ?, revision = ?, updated_at = ?, \
                 client_operation = 'update', operation_id = ?, expected_revision = ?, \
                 client_payload = ? WHERE id = ?",
                params![
                    url,
                    title,
                    description,
                    serde_json::to_string(&tags).unwrap_or_else(|_| "[]".into()),
                    revision + 1,
                    now,
                    operation_id,
                    revision,
                    payload,
                    id
                ],
            )
        }
        Command::MoveBookmark {
            id,
            revision,
            folder_id,
        } => writer.execute(
            "UPDATE bookmarks SET folder_id = ?, revision = ?, updated_at = ?, \
             client_operation = 'move', operation_id = ?, expected_revision = ? WHERE id = ?",
            params![folder_id, revision + 1, now, operation_id, revision, id],
        ),
        Command::FavoriteBookmark {
            id,
            revision,
            favorite,
        } => {
            let payload = json!({
                "operationId": operation_id,
                "favorite": favorite,
            })
            .to_string();
            writer.execute(
                "UPDATE bookmarks SET favorite = ?, revision = ?, updated_at = ?, \
                 client_operation = 'favorite', operation_id = ?, expected_revision = ?, \
                 client_payload = ? WHERE id = ?",
                params![
                    i64::from(favorite),
                    revision + 1,
                    now,
                    operation_id,
                    revision,
                    payload,
                    id
                ],
            )
        }
        Command::ArchiveBookmark {
            id,
            revision,
            archived,
        } => {
            let payload = json!({
                "operationId": operation_id,
                "archived": archived,
            })
            .to_string();
            writer.execute(
                "UPDATE bookmarks SET archived = ?, revision = ?, updated_at = ?, \
                 client_operation = 'archive', operation_id = ?, expected_revision = ?, \
                 client_payload = ? WHERE id = ?",
                params![
                    i64::from(archived),
                    revision + 1,
                    now,
                    operation_id,
                    revision,
                    payload,
                    id
                ],
            )
        }
        Command::DeleteBookmark { id, revision } => writer.execute(
            "UPDATE bookmarks SET deleted_local = 1, revision = ?, updated_at = ?, \
             client_operation = 'delete', operation_id = ?, expected_revision = ? WHERE id = ?",
            params![revision + 1, now, operation_id, revision, id],
        ),
    }
    .map_err(|error| format!("apply local bookmarks command: {error}"))?;
    Ok(())
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
        ("bookmarks_folders", "create") => {
            api.create_folder(
                operation_id,
                &entry.id,
                required_text(data, "name")?,
                optional_text(data, "parent_id"),
                optional_i64(data, "position"),
            )
            .await?;
        }
        ("bookmarks_folders", "rename") => {
            api.rename_folder(
                operation_id,
                &entry.id,
                required_i64(data, EXPECTED_REVISION)?,
                required_text(data, "name")?,
            )
            .await?;
        }
        ("bookmarks_folders", "move") => {
            api.move_folder(
                operation_id,
                &entry.id,
                required_i64(data, EXPECTED_REVISION)?,
                optional_text(data, "parent_id"),
                optional_i64(data, "position"),
            )
            .await?;
        }
        ("bookmarks_folders", "delete") => {
            api.delete_folder(
                operation_id,
                &entry.id,
                required_i64(data, EXPECTED_REVISION)?,
            )
            .await?;
        }
        ("bookmarks", "create") => {
            api.create_bookmark(
                operation_id,
                &entry.id,
                required_text(data, "folder_id")?,
                required_text(data, "url")?,
                required_text(data, "title")?,
                optional_text(data, "description").unwrap_or_default(),
                &decode_tags(optional_text(data, "tags").map(str::to_string)),
                optional_bool(data, "favorite").unwrap_or(false),
                optional_bool(data, "archived").unwrap_or(false),
                optional_i64(data, "position"),
            )
            .await?;
        }
        ("bookmarks", "update") => {
            let payload = required_text(data, CLIENT_PAYLOAD)?;
            let payload =
                serde_json::from_str::<BookmarkUpdatePayload>(payload).map_err(|error| {
                    api::ApiError::InvalidCommand(format!(
                        "invalid durable bookmark update payload: {error}"
                    ))
                })?;
            api.update_bookmark(
                operation_id,
                &entry.id,
                required_i64(data, EXPECTED_REVISION)?,
                &payload.url,
                &payload.title,
                &payload.description,
                &payload.tags,
            )
            .await?;
        }
        ("bookmarks", "move") => {
            api.move_bookmark(
                operation_id,
                &entry.id,
                required_i64(data, EXPECTED_REVISION)?,
                required_text(data, "folder_id")?,
                optional_i64(data, "position"),
            )
            .await?;
        }
        ("bookmarks", "favorite") => {
            let favorite = payload_bool(data, "favorite")
                .or_else(|| optional_bool(data, "favorite"))
                .ok_or_else(|| api::ApiError::InvalidCommand("missing favorite flag".into()))?;
            api.favorite_bookmark(
                operation_id,
                &entry.id,
                required_i64(data, EXPECTED_REVISION)?,
                favorite,
            )
            .await?;
        }
        ("bookmarks", "archive") => {
            let archived = payload_bool(data, "archived")
                .or_else(|| optional_bool(data, "archived"))
                .ok_or_else(|| api::ApiError::InvalidCommand("missing archived flag".into()))?;
            api.archive_bookmark(
                operation_id,
                &entry.id,
                required_i64(data, EXPECTED_REVISION)?,
                archived,
            )
            .await?;
        }
        ("bookmarks", "delete") => {
            api.delete_bookmark(
                operation_id,
                &entry.id,
                required_i64(data, EXPECTED_REVISION)?,
            )
            .await?;
        }
        (table, operation) => {
            return Err(api::ApiError::InvalidCommand(format!(
                "unsupported local bookmarks command {table}/{operation}"
            )));
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct BookmarkUpdatePayload {
    url: String,
    title: String,
    description: String,
    tags: Vec<String>,
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

fn optional_bool(data: &Map<String, Value>, key: &str) -> Option<bool> {
    data.get(key).and_then(|value| match value {
        Value::Bool(flag) => Some(*flag),
        Value::Number(number) => number.as_i64().map(|item| item != 0),
        Value::String(text) => match text.as_str() {
            "true" | "t" | "1" => Some(true),
            "false" | "f" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    })
}

fn payload_bool(data: &Map<String, Value>, field: &str) -> Option<bool> {
    let payload = optional_text(data, CLIENT_PAYLOAD)?;
    let value: Value = serde_json::from_str(payload).ok()?;
    match value.get(field)? {
        Value::Bool(flag) => Some(*flag),
        Value::Number(number) => number.as_i64().map(|item| item != 0),
        _ => None,
    }
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
                    "folder_not_empty" => {
                        "Folder is not empty. Move or delete its bookmarks and folders first.".into()
                    }
                    "cycle" => "A folder cannot be moved into its own descendant.".into(),
                    "invalid_parent" => "That folder destination is not valid.".into(),
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

/// Describes the native replica boundary shown in the development UI.
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
        PowerSyncDatabase::new(environment, bookmarks_schema())
    }

    fn nested_snapshot() -> Snapshot {
        Snapshot {
            availability: Availability::Available,
            development: true,
            using_powersync: true,
            folders: Arc::new(vec![
                Folder {
                    id: "root".into(),
                    parent_id: None,
                    name: "Root".into(),
                    position: 0,
                    revision: 1,
                },
                Folder {
                    id: "child".into(),
                    parent_id: Some("root".into()),
                    name: "Child".into(),
                    position: 0,
                    revision: 1,
                },
                Folder {
                    id: "leaf".into(),
                    parent_id: Some("child".into()),
                    name: "Leaf".into(),
                    position: 0,
                    revision: 1,
                },
                Folder {
                    id: "other".into(),
                    parent_id: None,
                    name: "Other".into(),
                    position: 1,
                    revision: 1,
                },
            ]),
            bookmarks: Arc::new(vec![Bookmark {
                id: "b1".into(),
                folder_id: "child".into(),
                url: "https://example.com".into(),
                title: "Nested".into(),
                description: "body".into(),
                tags: vec!["work".into()],
                favorite: false,
                archived: false,
                position: 0,
                revision: 1,
                updated_at: String::new(),
            }]),
            ..Snapshot::default()
        }
    }

    #[test]
    fn folder_path_and_move_validation_exclude_descendants() {
        let snapshot = nested_snapshot();
        assert_eq!(
            snapshot
                .folder_path("leaf")
                .into_iter()
                .map(|folder| folder.id)
                .collect::<Vec<_>>(),
            vec!["root", "child", "leaf"]
        );
        assert_eq!(snapshot.folder_path_label("leaf"), "Root / Child / Leaf");
        assert_eq!(
            snapshot
                .valid_folder_move_targets("child")
                .into_iter()
                .map(|folder| folder.id)
                .collect::<Vec<_>>(),
            vec!["other", "root"]
        );
        assert_eq!(
            snapshot
                .validate_folder_move("child", Some("leaf"))
                .unwrap_err(),
            "A folder cannot be moved into its own descendant."
        );
        assert!(snapshot.validate_folder_move("child", Some("root")).is_ok());
        assert!(snapshot.validate_folder_delete("other").is_ok());
        assert_eq!(
            snapshot.validate_folder_delete("child").unwrap_err(),
            "Folder is not empty. Move or delete its bookmarks and folders first."
        );
    }

    #[test]
    fn search_and_filters_hide_archived_unless_requested() {
        let mut snapshot = nested_snapshot();
        snapshot.bookmarks = Arc::new(vec![
            Bookmark {
                id: "live".into(),
                folder_id: "child".into(),
                url: "https://doc.rust-lang.org".into(),
                title: "Rust docs".into(),
                description: "lang".into(),
                tags: vec!["rust".into()],
                favorite: false,
                archived: false,
                position: 0,
                revision: 1,
                updated_at: "2".into(),
            },
            Bookmark {
                id: "fav".into(),
                folder_id: "child".into(),
                url: "https://example.com/fav".into(),
                title: "Favorite".into(),
                description: "".into(),
                tags: vec!["work".into()],
                favorite: true,
                archived: false,
                position: 1,
                revision: 1,
                updated_at: "3".into(),
            },
            Bookmark {
                id: "old".into(),
                folder_id: "child".into(),
                url: "https://example.com/old".into(),
                title: "Archived rust".into(),
                description: "".into(),
                tags: vec!["rust".into()],
                favorite: false,
                archived: true,
                position: 2,
                revision: 1,
                updated_at: "1".into(),
            },
        ]);
        assert_eq!(
            snapshot
                .visible_bookmarks("", Filter::All, None, None)
                .into_iter()
                .map(|item| item.id)
                .collect::<Vec<_>>(),
            vec!["fav", "live"]
        );
        assert_eq!(
            snapshot
                .visible_bookmarks("", Filter::Favorites, None, None)
                .into_iter()
                .map(|item| item.id)
                .collect::<Vec<_>>(),
            vec!["fav"]
        );
        assert_eq!(
            snapshot
                .visible_bookmarks("doc.rust", Filter::All, None, None)
                .into_iter()
                .map(|item| item.id)
                .collect::<Vec<_>>(),
            vec!["live"]
        );
    }

    #[test]
    fn sync_banner_prefers_stale_revision_over_offline() {
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
        assert_eq!(
            sync_banner(Availability::Available, true, 3, None),
            Some(SyncBanner::Offline { pending: 3 })
        );
        assert_eq!(
            sync_banner(Availability::Available, false, 1, None),
            Some(SyncBanner::Pending { pending: 1 })
        );
        assert_eq!(sync_banner(Availability::Available, false, 0, None), None);
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
            "foyer-bookmarks-powersync-test-{}.sqlite3",
            Uuid::new_v4()
        ));
        let conflict = Mutex::new(None);
        let db = test_database(&path);
        apply_local(
            &db,
            Command::CreateFolder {
                name: "Offline".into(),
                parent_id: None,
            },
        )
        .await
        .unwrap();
        let first = read_snapshot(&db, &conflict).await.unwrap();
        let folder_id = first.folders[0].id.clone();
        apply_local(
            &db,
            Command::CreateBookmark {
                folder_id,
                url: "https://example.com/queued".into(),
                title: "Restart-safe".into(),
                description: "Keep <em>html</em>\n".into(),
                tags: vec!["Work".into(), "work".into()],
            },
        )
        .await
        .unwrap();
        drop(db);

        let reopened = test_database(&path);
        let snapshot = read_snapshot(&reopened, &conflict).await.unwrap();
        assert!(snapshot.using_powersync);
        assert!(snapshot.offline);
        assert_eq!(snapshot.pending_uploads, 2);
        assert_eq!(snapshot.folders[0].name, "Offline");
        assert_eq!(snapshot.bookmarks[0].title, "Restart-safe");
        assert_eq!(snapshot.bookmarks[0].description, "Keep <em>html</em>\n");
        assert_eq!(snapshot.bookmarks[0].tags, vec!["work"]);
        drop(reopened);

        for suffix in ["", "-shm", "-wal"] {
            let _ = fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn local_folder_move_and_nonempty_delete_follow_server_rules() {
        let path = env::temp_dir().join(format!(
            "foyer-bookmarks-powersync-move-{}.sqlite3",
            Uuid::new_v4()
        ));
        let conflict = Mutex::new(None);
        let db = test_database(&path);
        apply_local(
            &db,
            Command::CreateFolder {
                name: "Root".into(),
                parent_id: None,
            },
        )
        .await
        .unwrap();
        let root = read_snapshot(&db, &conflict).await.unwrap().folders[0].clone();
        apply_local(
            &db,
            Command::CreateFolder {
                name: "Child".into(),
                parent_id: Some(root.id.clone()),
            },
        )
        .await
        .unwrap();
        let child = read_snapshot(&db, &conflict)
            .await
            .unwrap()
            .folders
            .iter()
            .find(|folder| folder.name == "Child")
            .cloned()
            .unwrap();
        apply_local(
            &db,
            Command::CreateBookmark {
                folder_id: child.id.clone(),
                url: "https://example.com/keep".into(),
                title: "Keep".into(),
                description: "body".into(),
                tags: vec![],
            },
        )
        .await
        .unwrap();
        let rejected = apply_local(
            &db,
            Command::DeleteFolder {
                id: child.id.clone(),
                revision: child.revision,
            },
        )
        .await
        .unwrap_err();
        assert!(rejected.contains("not empty"));
        apply_local(
            &db,
            Command::MoveFolder {
                id: child.id.clone(),
                revision: child.revision,
                parent_id: None,
            },
        )
        .await
        .unwrap();
        let moved = read_snapshot(&db, &conflict)
            .await
            .unwrap()
            .folder(&child.id)
            .cloned()
            .unwrap();
        assert_eq!(moved.parent_id, None);
        drop(db);
        for suffix in ["", "-shm", "-wal"] {
            let _ = fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn local_url_validation_rejects_javascript() {
        let path = env::temp_dir().join(format!(
            "foyer-bookmarks-powersync-url-{}.sqlite3",
            Uuid::new_v4()
        ));
        let conflict = Mutex::new(None);
        let db = test_database(&path);
        apply_local(
            &db,
            Command::CreateFolder {
                name: "Inbox".into(),
                parent_id: None,
            },
        )
        .await
        .unwrap();
        let folder_id = read_snapshot(&db, &conflict).await.unwrap().folders[0]
            .id
            .clone();
        let rejected = apply_local(
            &db,
            Command::CreateBookmark {
                folder_id,
                url: "javascript:alert(1)".into(),
                title: "Nope".into(),
                description: "".into(),
                tags: vec![],
            },
        )
        .await
        .unwrap_err();
        assert!(rejected.contains("HTTP"));
        drop(db);
        for suffix in ["", "-shm", "-wal"] {
            let _ = fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }
}
