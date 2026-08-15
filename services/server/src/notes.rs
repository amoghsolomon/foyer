use axum::{
    Json,
    extract::{Path, Query, State},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    AppState,
    auth::Principal,
    error::{ApiError, ApiResult},
};

pub const MAX_FOLDER_NAME: usize = 80;
pub const MAX_NOTE_TITLE: usize = 200;
pub const MAX_NOTE_BODY_BYTES: usize = 256 * 1024;
pub const MAX_FOLDER_DEPTH: usize = 8;
pub const MAX_FOLDERS_PER_USER: i64 = 256;
pub const MAX_NOTES_PER_USER: i64 = 2048;
pub const MAX_CHILDREN_PER_FOLDER: i64 = 256;
pub const MAX_POSITION: i32 = 100_000;

#[derive(Clone, Debug, Serialize, Deserialize, FromRow, PartialEq, Eq)]
pub struct Folder {
    pub id: String,
    #[serde(rename = "userId")]
    pub user_id: String,
    #[serde(rename = "parentId")]
    pub parent_id: Option<String>,
    pub name: String,
    pub position: i32,
    pub revision: i64,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
    #[serde(rename = "updatedAt")]
    pub updated_at: DateTime<Utc>,
    #[serde(rename = "deletedAt")]
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromRow, PartialEq, Eq)]
pub struct Note {
    pub id: String,
    #[serde(rename = "userId")]
    pub user_id: String,
    #[serde(rename = "folderId")]
    pub folder_id: String,
    pub title: String,
    pub body: String,
    pub revision: i64,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
    #[serde(rename = "updatedAt")]
    pub updated_at: DateTime<Utc>,
    #[serde(rename = "deletedAt")]
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct FolderList {
    pub folders: Vec<Folder>,
}

#[derive(Debug, Serialize)]
pub struct NoteList {
    pub notes: Vec<Note>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateFolderRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    pub id: String,
    pub name: String,
    #[serde(rename = "parentId")]
    pub parent_id: Option<String>,
    pub position: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RenameFolderRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    #[serde(rename = "expectedRevision")]
    pub expected_revision: i64,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MoveFolderRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    #[serde(rename = "expectedRevision")]
    pub expected_revision: i64,
    #[serde(rename = "parentId")]
    pub parent_id: Option<String>,
    pub position: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateNoteRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    pub id: String,
    #[serde(rename = "folderId")]
    pub folder_id: String,
    pub title: String,
    pub body: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateNoteRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    #[serde(rename = "expectedRevision")]
    pub expected_revision: i64,
    pub title: String,
    pub body: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MoveNoteRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    #[serde(rename = "expectedRevision")]
    pub expected_revision: i64,
    #[serde(rename = "folderId")]
    pub folder_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    #[serde(rename = "expectedRevision")]
    pub expected_revision: i64,
}

#[derive(Debug, Deserialize)]
pub struct NoteListQuery {
    #[serde(rename = "folderId")]
    pub folder_id: Option<String>,
}

pub async fn list_folders(
    State(state): State<AppState>,
    principal: Principal,
) -> ApiResult<Json<FolderList>> {
    let folders = sqlx::query_as::<_, Folder>(
        "SELECT id, user_id, parent_id, name, position, revision, created_at, updated_at, deleted_at
         FROM notes_folders
         WHERE user_id = $1 AND deleted_at IS NULL
         ORDER BY parent_id NULLS FIRST, position, name, id",
    )
    .bind(&principal.user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(database_error)?;
    Ok(Json(FolderList { folders }))
}

pub async fn get_folder(
    State(state): State<AppState>,
    principal: Principal,
    Path(folder_id): Path<String>,
) -> ApiResult<Json<Folder>> {
    let folder = load_folder(&state.pool, &folder_id).await?;
    visible_folder(&principal.user_id, folder).map(Json)
}

pub async fn create_folder(
    State(state): State<AppState>,
    principal: Principal,
    Json(request): Json<CreateFolderRequest>,
) -> ApiResult<Json<Folder>> {
    let request_body = operation_request(&request)?;
    let operation_id = parse_uuid("operationId", &request.operation_id)?;
    let id = parse_uuid("id", &request.id)?;
    let name = validate_folder_name(&request.name)?;
    let parent_id = optional_uuid("parentId", request.parent_id.as_deref())?;
    let position = optional_position(request.position)?;
    let user_id = principal.user_id.clone();
    with_operation(&state.pool, operation_binding(user_id.clone(), operation_id, "folder", id.clone(), "create", request_body), |tx| {
        Box::pin(async move {
            if let Some(existing) = load_folder_tx(tx, &id).await? {
                return Err(existing_identity_error(&user_id, existing.user_id, existing.deleted_at.is_some()));
            }
            if let Some(parent) = parent_id.as_ref() {
                ensure_live_parent(tx, &user_id, parent).await?;
                ensure_depth(tx, parent, 1).await?;
                ensure_child_capacity(tx, &user_id, Some(parent)).await?;
            } else {
                ensure_child_capacity(tx, &user_id, None).await?;
            }
            ensure_folder_quota(tx, &user_id).await?;
            let now = Utc::now();
            let position = match position {
                Some(value) => value,
                None => next_position(tx, &user_id, parent_id.as_deref()).await?,
            };
            sqlx::query(
                "INSERT INTO notes_folders
                    (id, user_id, parent_id, name, position, revision, created_at, updated_at, deleted_at)
                 VALUES ($1, $2, $3, $4, $5, 1, $6, $6, NULL)",
            )
            .bind(&id)
            .bind(&user_id)
            .bind(&parent_id)
            .bind(&name)
            .bind(position)
            .bind(now)
            .execute(&mut **tx)
            .await
            .map_err(database_error)?;
            load_required_folder(tx, &id).await
        })
    })
    .await
    .map(Json)
}

pub async fn rename_folder(
    State(state): State<AppState>,
    principal: Principal,
    Path(folder_id): Path<String>,
    Json(request): Json<RenameFolderRequest>,
) -> ApiResult<Json<Folder>> {
    let request_body = operation_request(&request)?;
    let operation_id = parse_uuid("operationId", &request.operation_id)?;
    let folder_id = parse_uuid("folderId", &folder_id)?;
    let name = validate_folder_name(&request.name)?;
    let expected = validate_revision(request.expected_revision)?;
    let user_id = principal.user_id.clone();
    with_operation(
        &state.pool,
        operation_binding(
            user_id.clone(),
            operation_id,
            "folder",
            folder_id.clone(),
            "rename",
            request_body,
        ),
        |tx| {
            Box::pin(async move {
                let folder = locked_live_folder(tx, &user_id, &folder_id, expected).await?;
                let now = Utc::now();
                sqlx::query(
                    "UPDATE notes_folders
                 SET name = $2, revision = revision + 1, updated_at = $3
                 WHERE id = $1",
                )
                .bind(&folder.id)
                .bind(&name)
                .bind(now)
                .execute(&mut **tx)
                .await
                .map_err(database_error)?;
                load_required_folder(tx, &folder.id).await
            })
        },
    )
    .await
    .map(Json)
}

pub async fn move_folder(
    State(state): State<AppState>,
    principal: Principal,
    Path(folder_id): Path<String>,
    Json(request): Json<MoveFolderRequest>,
) -> ApiResult<Json<Folder>> {
    let request_body = operation_request(&request)?;
    let operation_id = parse_uuid("operationId", &request.operation_id)?;
    let folder_id = parse_uuid("folderId", &folder_id)?;
    let parent_id = optional_uuid("parentId", request.parent_id.as_deref())?;
    let position = optional_position(request.position)?;
    let expected = validate_revision(request.expected_revision)?;
    let user_id = principal.user_id.clone();
    with_operation(
        &state.pool,
        operation_binding(
            user_id.clone(),
            operation_id,
            "folder",
            folder_id.clone(),
            "move",
            request_body,
        ),
        |tx| {
            Box::pin(async move {
                let folder = locked_live_folder(tx, &user_id, &folder_id, expected).await?;
                if parent_id.as_deref() == Some(folder.id.as_str()) {
                    return Err(ApiError::cycle("A folder cannot be its own parent."));
                }
                if let Some(parent) = parent_id.as_ref() {
                    ensure_live_parent(tx, &user_id, parent).await?;
                    if is_descendant(tx, parent, &folder.id).await? {
                        return Err(ApiError::cycle(
                            "A folder cannot be moved under one of its descendants.",
                        ));
                    }
                    let subtree_height = subtree_height(tx, &folder.id).await?;
                    ensure_depth(tx, parent, subtree_height).await?;
                }
                if parent_id != folder.parent_id {
                    ensure_child_capacity(tx, &user_id, parent_id.as_deref()).await?;
                }
                let now = Utc::now();
                let position = match position {
                    Some(value) => value,
                    None => next_position(tx, &user_id, parent_id.as_deref()).await?,
                };
                sqlx::query(
                    "UPDATE notes_folders
                 SET parent_id = $2, position = $3, revision = revision + 1, updated_at = $4
                 WHERE id = $1",
                )
                .bind(&folder.id)
                .bind(&parent_id)
                .bind(position)
                .bind(now)
                .execute(&mut **tx)
                .await
                .map_err(database_error)?;
                load_required_folder(tx, &folder.id).await
            })
        },
    )
    .await
    .map(Json)
}

pub async fn delete_folder(
    State(state): State<AppState>,
    principal: Principal,
    Path(folder_id): Path<String>,
    Json(request): Json<DeleteRequest>,
) -> ApiResult<Json<Folder>> {
    let request_body = operation_request(&request)?;
    let operation_id = parse_uuid("operationId", &request.operation_id)?;
    let folder_id = parse_uuid("folderId", &folder_id)?;
    let expected = validate_revision(request.expected_revision)?;
    let user_id = principal.user_id.clone();
    with_operation(
        &state.pool,
        operation_binding(
            user_id.clone(),
            operation_id,
            "folder",
            folder_id.clone(),
            "delete",
            request_body,
        ),
        |tx| {
            Box::pin(async move {
                let folder = locked_live_folder(tx, &user_id, &folder_id, expected).await?;
                let child_folders = sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM notes_folders
                 WHERE user_id = $1 AND parent_id = $2 AND deleted_at IS NULL",
                )
                .bind(&user_id)
                .bind(&folder.id)
                .fetch_one(&mut **tx)
                .await
                .map_err(database_error)?;
                let child_notes = sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM notes
                 WHERE user_id = $1 AND folder_id = $2 AND deleted_at IS NULL",
                )
                .bind(&user_id)
                .bind(&folder.id)
                .fetch_one(&mut **tx)
                .await
                .map_err(database_error)?;
                if child_folders > 0 || child_notes > 0 {
                    return Err(ApiError::folder_not_empty(
                        "A folder can be deleted only when it has no live child folders or notes.",
                    ));
                }
                let now = Utc::now();
                sqlx::query(
                    "UPDATE notes_folders
                 SET deleted_at = $2, revision = revision + 1, updated_at = $2
                 WHERE id = $1",
                )
                .bind(&folder.id)
                .bind(now)
                .execute(&mut **tx)
                .await
                .map_err(database_error)?;
                load_required_folder(tx, &folder.id).await
            })
        },
    )
    .await
    .map(Json)
}

pub async fn list_notes(
    State(state): State<AppState>,
    principal: Principal,
    Query(query): Query<NoteListQuery>,
) -> ApiResult<Json<NoteList>> {
    let folder_id = optional_uuid("folderId", query.folder_id.as_deref())?;
    let notes = if let Some(folder_id) = folder_id {
        sqlx::query_as::<_, Note>(
            "SELECT id, user_id, folder_id, title, body, revision, created_at, updated_at, deleted_at
             FROM notes
             WHERE user_id = $1 AND folder_id = $2 AND deleted_at IS NULL
             ORDER BY updated_at DESC, id",
        )
        .bind(&principal.user_id)
        .bind(folder_id)
        .fetch_all(&state.pool)
        .await
        .map_err(database_error)?
    } else {
        sqlx::query_as::<_, Note>(
            "SELECT id, user_id, folder_id, title, body, revision, created_at, updated_at, deleted_at
             FROM notes
             WHERE user_id = $1 AND deleted_at IS NULL
             ORDER BY updated_at DESC, id",
        )
        .bind(&principal.user_id)
        .fetch_all(&state.pool)
        .await
        .map_err(database_error)?
    };
    Ok(Json(NoteList { notes }))
}

pub async fn get_note(
    State(state): State<AppState>,
    principal: Principal,
    Path(note_id): Path<String>,
) -> ApiResult<Json<Note>> {
    let note = load_note(&state.pool, &note_id).await?;
    visible_note(&principal.user_id, note).map(Json)
}

pub async fn create_note(
    State(state): State<AppState>,
    principal: Principal,
    Json(request): Json<CreateNoteRequest>,
) -> ApiResult<Json<Note>> {
    let request_body = operation_request(&request)?;
    let operation_id = parse_uuid("operationId", &request.operation_id)?;
    let id = parse_uuid("id", &request.id)?;
    let folder_id = parse_uuid("folderId", &request.folder_id)?;
    let title = validate_note_title(&request.title)?;
    let body = validate_note_body(&request.body)?;
    let user_id = principal.user_id.clone();
    with_operation(&state.pool, operation_binding(user_id.clone(), operation_id, "note", id.clone(), "create", request_body), |tx| {
        Box::pin(async move {
            if let Some(existing) = load_note_tx(tx, &id).await? {
                return Err(existing_identity_error(
                    &user_id,
                    existing.user_id,
                    existing.deleted_at.is_some(),
                ));
            }
            ensure_live_parent(tx, &user_id, &folder_id).await?;
            ensure_note_quota(tx, &user_id).await?;
            let now = Utc::now();
            sqlx::query(
                "INSERT INTO notes
                    (id, user_id, folder_id, title, body, revision, created_at, updated_at, deleted_at)
                 VALUES ($1, $2, $3, $4, $5, 1, $6, $6, NULL)",
            )
            .bind(&id)
            .bind(&user_id)
            .bind(&folder_id)
            .bind(&title)
            .bind(&body)
            .bind(now)
            .execute(&mut **tx)
            .await
            .map_err(database_error)?;
            load_required_note(tx, &id).await
        })
    })
    .await
    .map(Json)
}

pub async fn update_note(
    State(state): State<AppState>,
    principal: Principal,
    Path(note_id): Path<String>,
    Json(request): Json<UpdateNoteRequest>,
) -> ApiResult<Json<Note>> {
    let request_body = operation_request(&request)?;
    let operation_id = parse_uuid("operationId", &request.operation_id)?;
    let note_id = parse_uuid("noteId", &note_id)?;
    let title = validate_note_title(&request.title)?;
    let body = validate_note_body(&request.body)?;
    let expected = validate_revision(request.expected_revision)?;
    let user_id = principal.user_id.clone();
    with_operation(
        &state.pool,
        operation_binding(
            user_id.clone(),
            operation_id,
            "note",
            note_id.clone(),
            "update",
            request_body,
        ),
        |tx| {
            Box::pin(async move {
                let note = locked_live_note(tx, &user_id, &note_id, expected).await?;
                let now = Utc::now();
                sqlx::query(
                    "UPDATE notes
                 SET title = $2, body = $3, revision = revision + 1, updated_at = $4
                 WHERE id = $1",
                )
                .bind(&note.id)
                .bind(&title)
                .bind(&body)
                .bind(now)
                .execute(&mut **tx)
                .await
                .map_err(database_error)?;
                load_required_note(tx, &note.id).await
            })
        },
    )
    .await
    .map(Json)
}

pub async fn move_note(
    State(state): State<AppState>,
    principal: Principal,
    Path(note_id): Path<String>,
    Json(request): Json<MoveNoteRequest>,
) -> ApiResult<Json<Note>> {
    let request_body = operation_request(&request)?;
    let operation_id = parse_uuid("operationId", &request.operation_id)?;
    let note_id = parse_uuid("noteId", &note_id)?;
    let folder_id = parse_uuid("folderId", &request.folder_id)?;
    let expected = validate_revision(request.expected_revision)?;
    let user_id = principal.user_id.clone();
    with_operation(
        &state.pool,
        operation_binding(
            user_id.clone(),
            operation_id,
            "note",
            note_id.clone(),
            "move",
            request_body,
        ),
        |tx| {
            Box::pin(async move {
                let note = locked_live_note(tx, &user_id, &note_id, expected).await?;
                ensure_live_parent(tx, &user_id, &folder_id).await?;
                let now = Utc::now();
                sqlx::query(
                    "UPDATE notes
                 SET folder_id = $2, revision = revision + 1, updated_at = $3
                 WHERE id = $1",
                )
                .bind(&note.id)
                .bind(&folder_id)
                .bind(now)
                .execute(&mut **tx)
                .await
                .map_err(database_error)?;
                load_required_note(tx, &note.id).await
            })
        },
    )
    .await
    .map(Json)
}

pub async fn delete_note(
    State(state): State<AppState>,
    principal: Principal,
    Path(note_id): Path<String>,
    Json(request): Json<DeleteRequest>,
) -> ApiResult<Json<Note>> {
    let request_body = operation_request(&request)?;
    let operation_id = parse_uuid("operationId", &request.operation_id)?;
    let note_id = parse_uuid("noteId", &note_id)?;
    let expected = validate_revision(request.expected_revision)?;
    let user_id = principal.user_id.clone();
    with_operation(
        &state.pool,
        operation_binding(
            user_id.clone(),
            operation_id,
            "note",
            note_id.clone(),
            "delete",
            request_body,
        ),
        |tx| {
            Box::pin(async move {
                let note = locked_live_note(tx, &user_id, &note_id, expected).await?;
                let now = Utc::now();
                sqlx::query(
                    "UPDATE notes
                 SET deleted_at = $2, revision = revision + 1, updated_at = $2
                 WHERE id = $1",
                )
                .bind(&note.id)
                .bind(now)
                .execute(&mut **tx)
                .await
                .map_err(database_error)?;
                load_required_note(tx, &note.id).await
            })
        },
    )
    .await
    .map(Json)
}

async fn with_operation<T, F>(pool: &PgPool, binding: OperationBinding, work: F) -> ApiResult<T>
where
    T: Serialize + for<'de> Deserialize<'de> + Send,
    F: for<'c> FnOnce(
        &'c mut Transaction<'_, Postgres>,
    ) -> std::pin::Pin<Box<dyn Future<Output = ApiResult<T>> + Send + 'c>>,
{
    let OperationBinding {
        user_id,
        operation_id,
        entity_type,
        entity_id,
        operation,
        request_body,
    } = binding;
    let mut tx = pool.begin().await.map_err(database_error)?;
    // Serialize operation-id checks globally and all writes for a user. The user lock makes
    // folder cycle/quota validation race-safe; the operation lock makes concurrent retries replay
    // the committed result instead of surfacing a uniqueness error after doing rolled-back work.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(&operation_id)
        .execute(&mut *tx)
        .await
        .map_err(database_error)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 1))")
        .bind(&user_id)
        .execute(&mut *tx)
        .await
        .map_err(database_error)?;
    if let Some(stored) = load_operation(&mut tx, &operation_id).await? {
        if stored.user_id != user_id
            || stored.entity_type != entity_type
            || stored.entity_id != entity_id
            || stored.operation != operation
            || stored.request_body != request_body
        {
            return Err(ApiError::conflict(
                "This operation id is already bound to a different request.",
            ));
        }
        tx.commit().await.map_err(database_error)?;
        if stored.result_status != 200 {
            return Err(ApiError::conflict(
                "This operation id already produced a non-success result.",
            ));
        }
        return serde_json::from_value(stored.result_body).map_err(|error| {
            ApiError::unavailable(format!("stored operation payload is invalid: {error}"))
        });
    }
    let result = work(&mut tx).await?;
    let body = serde_json::to_value(&result)
        .map_err(|error| ApiError::unavailable(format!("failed to store operation: {error}")))?;
    sqlx::query(
        "INSERT INTO notes_operations
            (operation_id, user_id, entity_type, entity_id, operation, request_body,
             result_status, result_body, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, 200, $7, $8)",
    )
    .bind(operation_id)
    .bind(user_id)
    .bind(entity_type)
    .bind(entity_id)
    .bind(operation)
    .bind(request_body)
    .bind(body)
    .bind(Utc::now())
    .execute(&mut *tx)
    .await
    .map_err(database_error)?;
    tx.commit().await.map_err(database_error)?;
    Ok(result)
}

struct OperationBinding {
    user_id: String,
    operation_id: String,
    entity_type: &'static str,
    entity_id: String,
    operation: &'static str,
    request_body: Value,
}

fn operation_binding(
    user_id: String,
    operation_id: String,
    entity_type: &'static str,
    entity_id: String,
    operation: &'static str,
    request_body: Value,
) -> OperationBinding {
    OperationBinding {
        user_id,
        operation_id,
        entity_type,
        entity_id,
        operation,
        request_body,
    }
}

async fn load_operation(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: &str,
) -> ApiResult<Option<StoredOperation>> {
    sqlx::query_as::<_, StoredOperation>(
        "SELECT user_id, entity_type, entity_id, operation, request_body, result_status, result_body
         FROM notes_operations WHERE operation_id = $1",
    )
    .bind(operation_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)
}

#[derive(Debug, FromRow)]
struct StoredOperation {
    user_id: String,
    entity_type: String,
    entity_id: String,
    operation: String,
    request_body: Value,
    result_status: i32,
    result_body: Value,
}

fn operation_request<T: Serialize>(request: &T) -> ApiResult<Value> {
    serde_json::to_value(request)
        .map_err(|error| ApiError::unavailable(format!("failed to encode operation: {error}")))
}

async fn load_folder(pool: &PgPool, id: &str) -> ApiResult<Option<Folder>> {
    sqlx::query_as::<_, Folder>(
        "SELECT id, user_id, parent_id, name, position, revision, created_at, updated_at, deleted_at
         FROM notes_folders WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(database_error)
}

async fn load_folder_tx(tx: &mut Transaction<'_, Postgres>, id: &str) -> ApiResult<Option<Folder>> {
    sqlx::query_as::<_, Folder>(
        "SELECT id, user_id, parent_id, name, position, revision, created_at, updated_at, deleted_at
         FROM notes_folders WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)
}

async fn load_required_folder(tx: &mut Transaction<'_, Postgres>, id: &str) -> ApiResult<Folder> {
    load_folder_tx(tx, id)
        .await?
        .ok_or_else(|| ApiError::unavailable("folder disappeared after write"))
}

async fn load_note(pool: &PgPool, id: &str) -> ApiResult<Option<Note>> {
    sqlx::query_as::<_, Note>(
        "SELECT id, user_id, folder_id, title, body, revision, created_at, updated_at, deleted_at
         FROM notes WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(database_error)
}

async fn load_note_tx(tx: &mut Transaction<'_, Postgres>, id: &str) -> ApiResult<Option<Note>> {
    sqlx::query_as::<_, Note>(
        "SELECT id, user_id, folder_id, title, body, revision, created_at, updated_at, deleted_at
         FROM notes WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)
}

async fn load_required_note(tx: &mut Transaction<'_, Postgres>, id: &str) -> ApiResult<Note> {
    load_note_tx(tx, id)
        .await?
        .ok_or_else(|| ApiError::unavailable("note disappeared after write"))
}

async fn locked_live_folder(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    id: &str,
    expected: i64,
) -> ApiResult<Folder> {
    let folder = sqlx::query_as::<_, Folder>(
        "SELECT id, user_id, parent_id, name, position, revision, created_at, updated_at, deleted_at
         FROM notes_folders WHERE id = $1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)?;
    let folder = visible_folder(user_id, folder)?;
    if folder.revision != expected {
        return Err(ApiError::stale_revision(expected, folder.revision));
    }
    Ok(folder)
}

async fn locked_live_note(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    id: &str,
    expected: i64,
) -> ApiResult<Note> {
    let note = sqlx::query_as::<_, Note>(
        "SELECT id, user_id, folder_id, title, body, revision, created_at, updated_at, deleted_at
         FROM notes WHERE id = $1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)?;
    let note = visible_note(user_id, note)?;
    if note.revision != expected {
        return Err(ApiError::stale_revision(expected, note.revision));
    }
    Ok(note)
}

fn visible_folder(user_id: &str, folder: Option<Folder>) -> ApiResult<Folder> {
    match folder {
        Some(folder) if folder.user_id != user_id => Err(ApiError::not_found("Folder not found.")),
        Some(folder) if folder.deleted_at.is_some() => {
            Err(ApiError::gone("This folder has been deleted."))
        }
        Some(folder) => Ok(folder),
        None => Err(ApiError::not_found("Folder not found.")),
    }
}

fn visible_note(user_id: &str, note: Option<Note>) -> ApiResult<Note> {
    match note {
        Some(note) if note.user_id != user_id => Err(ApiError::not_found("Note not found.")),
        Some(note) if note.deleted_at.is_some() => {
            Err(ApiError::gone("This note has been deleted."))
        }
        Some(note) => Ok(note),
        None => Err(ApiError::not_found("Note not found.")),
    }
}

fn existing_identity_error(user_id: &str, owner_id: String, deleted: bool) -> ApiError {
    if owner_id != user_id {
        return ApiError::conflict("This identifier is already in use.");
    }
    if deleted {
        ApiError::gone("A tombstoned item cannot be resurrected.")
    } else {
        ApiError::conflict("This identifier is already in use.")
    }
}

async fn ensure_live_parent(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    parent_id: &str,
) -> ApiResult<Folder> {
    let parent = load_folder_tx(tx, parent_id).await?;
    match parent {
        None => Err(ApiError::invalid_parent(
            "The parent folder does not exist.",
        )),
        Some(parent) if parent.user_id != user_id => Err(ApiError::invalid_parent(
            "The parent folder does not exist.",
        )),
        Some(parent) if parent.deleted_at.is_some() => Err(ApiError::invalid_parent(
            "The parent folder has been deleted.",
        )),
        Some(parent) => Ok(parent),
    }
}

async fn ensure_depth(
    tx: &mut Transaction<'_, Postgres>,
    parent_id: &str,
    extra_levels: usize,
) -> ApiResult<()> {
    let parent_depth = folder_depth(tx, parent_id).await?;
    if parent_depth + extra_levels > MAX_FOLDER_DEPTH {
        return Err(ApiError::limit_exceeded(format!(
            "Folders may be nested at most {MAX_FOLDER_DEPTH} levels."
        )));
    }
    Ok(())
}

async fn folder_depth(tx: &mut Transaction<'_, Postgres>, id: &str) -> ApiResult<usize> {
    let mut current = Some(id.to_string());
    let mut depth = 0;
    let mut seen = 0;
    while let Some(folder_id) = current {
        seen += 1;
        if seen > MAX_FOLDER_DEPTH + 2 {
            return Err(ApiError::cycle("Existing folder tree contains a cycle."));
        }
        depth += 1;
        current = sqlx::query_scalar::<_, Option<String>>(
            "SELECT parent_id FROM notes_folders WHERE id = $1",
        )
        .bind(folder_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(database_error)?
        .flatten();
    }
    Ok(depth)
}

async fn subtree_height(tx: &mut Transaction<'_, Postgres>, root_id: &str) -> ApiResult<usize> {
    async fn walk(tx: &mut Transaction<'_, Postgres>, id: &str, depth: usize) -> ApiResult<usize> {
        let children: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM notes_folders WHERE parent_id = $1 AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_all(&mut **tx)
        .await
        .map_err(database_error)?;
        let mut max = depth;
        for child in children {
            max = max.max(Box::pin(walk(tx, &child, depth + 1)).await?);
        }
        Ok(max)
    }
    walk(tx, root_id, 1).await
}

async fn is_descendant(
    tx: &mut Transaction<'_, Postgres>,
    maybe_descendant: &str,
    ancestor: &str,
) -> ApiResult<bool> {
    let mut current = Some(maybe_descendant.to_string());
    let mut seen = 0;
    while let Some(folder_id) = current {
        if folder_id == ancestor {
            return Ok(true);
        }
        seen += 1;
        if seen > MAX_FOLDER_DEPTH + 2 {
            return Err(ApiError::cycle("Existing folder tree contains a cycle."));
        }
        current = sqlx::query_scalar::<_, Option<String>>(
            "SELECT parent_id FROM notes_folders WHERE id = $1",
        )
        .bind(folder_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(database_error)?
        .flatten();
    }
    Ok(false)
}

async fn ensure_folder_quota(tx: &mut Transaction<'_, Postgres>, user_id: &str) -> ApiResult<()> {
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM notes_folders WHERE user_id = $1 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(database_error)?;
    if count >= MAX_FOLDERS_PER_USER {
        return Err(ApiError::limit_exceeded(format!(
            "A user may have at most {MAX_FOLDERS_PER_USER} folders."
        )));
    }
    Ok(())
}

async fn ensure_note_quota(tx: &mut Transaction<'_, Postgres>, user_id: &str) -> ApiResult<()> {
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM notes WHERE user_id = $1 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(database_error)?;
    if count >= MAX_NOTES_PER_USER {
        return Err(ApiError::limit_exceeded(format!(
            "A user may have at most {MAX_NOTES_PER_USER} notes."
        )));
    }
    Ok(())
}

async fn ensure_child_capacity(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    parent_id: Option<&str>,
) -> ApiResult<()> {
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM notes_folders
         WHERE user_id = $1 AND parent_id IS NOT DISTINCT FROM $2 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .bind(parent_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(database_error)?;
    if count >= MAX_CHILDREN_PER_FOLDER {
        return Err(ApiError::limit_exceeded(format!(
            "A folder may have at most {MAX_CHILDREN_PER_FOLDER} child folders."
        )));
    }
    Ok(())
}

async fn next_position(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    parent_id: Option<&str>,
) -> ApiResult<i32> {
    let max = sqlx::query_scalar::<_, Option<i32>>(
        "SELECT MAX(position) FROM notes_folders
         WHERE user_id = $1 AND parent_id IS NOT DISTINCT FROM $2 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .bind(parent_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(database_error)?;
    Ok(max
        .map(|value| value.saturating_add(1))
        .unwrap_or(0)
        .min(MAX_POSITION))
}

pub fn parse_uuid(field: &str, value: &str) -> ApiResult<String> {
    Uuid::parse_str(value)
        .map(|uuid| uuid.to_string())
        .map_err(|_| ApiError::invalid_request(format!("{field} must be a UUID.")))
}

fn optional_uuid(field: &str, value: Option<&str>) -> ApiResult<Option<String>> {
    match value {
        None | Some("") => Ok(None),
        Some(value) => parse_uuid(field, value).map(Some),
    }
}

fn optional_position(value: Option<i32>) -> ApiResult<Option<i32>> {
    match value {
        None => Ok(None),
        Some(value) if (0..=MAX_POSITION).contains(&value) => Ok(Some(value)),
        Some(_) => Err(ApiError::invalid_request(format!(
            "position must be between 0 and {MAX_POSITION}."
        ))),
    }
}

fn validate_revision(value: i64) -> ApiResult<i64> {
    if value < 1 {
        return Err(ApiError::invalid_request(
            "expectedRevision must be at least 1.",
        ));
    }
    Ok(value)
}

pub fn validate_folder_name(value: &str) -> ApiResult<String> {
    validate_text("name", value, 1, MAX_FOLDER_NAME)
}

pub fn validate_note_title(value: &str) -> ApiResult<String> {
    validate_text("title", value, 1, MAX_NOTE_TITLE)
}

pub fn validate_note_body(value: &str) -> ApiResult<String> {
    if value.len() > MAX_NOTE_BODY_BYTES {
        return Err(ApiError::invalid_request(format!(
            "body must be at most {MAX_NOTE_BODY_BYTES} bytes."
        )));
    }
    if value.contains('\0') {
        return Err(ApiError::invalid_request("body cannot contain NUL bytes."));
    }
    Ok(value.to_string())
}

fn validate_text(field: &str, value: &str, min: usize, max: usize) -> ApiResult<String> {
    let trimmed = value.trim();
    if trimmed.chars().count() < min || trimmed.chars().count() > max {
        return Err(ApiError::invalid_request(format!(
            "{field} must be between {min} and {max} characters."
        )));
    }
    if trimmed.contains('\0') || trimmed.chars().any(char::is_control) {
        return Err(ApiError::invalid_request(format!(
            "{field} cannot contain control characters."
        )));
    }
    Ok(trimmed.to_string())
}

fn database_error(error: sqlx::Error) -> ApiError {
    ApiError::unavailable(format!("database error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_name_is_trimmed_and_bounded() {
        assert_eq!(validate_folder_name("  Inbox  ").unwrap(), "Inbox");
        assert!(validate_folder_name("").is_err());
        assert!(validate_folder_name(&"x".repeat(MAX_FOLDER_NAME + 1)).is_err());
    }

    #[test]
    fn note_body_is_preserved_including_html() {
        let body = "# Title\n\n<script>alert(1)</script>\n\n**bold**";
        assert_eq!(validate_note_body(body).unwrap(), body);
    }

    #[test]
    fn note_body_rejects_oversize_and_nul() {
        assert!(validate_note_body(&"a".repeat(MAX_NOTE_BODY_BYTES + 1)).is_err());
        assert!(validate_note_body("ok\0no").is_err());
    }

    #[test]
    fn uuid_fields_must_be_canonical() {
        let id = parse_uuid("id", "550e8400-e29b-41d4-a716-446655440000").unwrap();
        assert_eq!(id, "550e8400-e29b-41d4-a716-446655440000");
        assert!(parse_uuid("id", "not-a-uuid").is_err());
    }
}
