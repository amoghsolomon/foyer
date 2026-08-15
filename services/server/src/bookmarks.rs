use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool, Postgres, Transaction, types::Json as SqlJson};
use uuid::Uuid;

use crate::{
    AppState,
    auth::Principal,
    error::{ApiError, ApiResult},
};

pub const MAX_FOLDER_NAME: usize = 80;
pub const MAX_BOOKMARK_TITLE: usize = 200;
pub const MAX_DESCRIPTION_BYTES: usize = 64 * 1024;
pub const MAX_URL_BYTES: usize = 2048;
pub const MAX_FOLDER_DEPTH: usize = 8;
pub const MAX_FOLDERS_PER_USER: i64 = 256;
pub const MAX_BOOKMARKS_PER_USER: i64 = 4096;
pub const MAX_CHILDREN_PER_FOLDER: i64 = 256;
pub const MAX_BOOKMARKS_PER_FOLDER: i64 = 512;
pub const MAX_POSITION: i32 = 100_000;
pub const MAX_TAGS: usize = 16;
pub const MAX_TAG_LENGTH: usize = 32;

/// Bookmark routes ready to merge into the Foyer Server router.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/v1/bookmark-folders",
            get(list_folders).post(create_folder),
        )
        .route("/v1/bookmark-folders/{folderId}", get(get_folder))
        .route(
            "/v1/bookmark-folders/{folderId}/rename",
            post(rename_folder),
        )
        .route("/v1/bookmark-folders/{folderId}/move", post(move_folder))
        .route(
            "/v1/bookmark-folders/{folderId}/delete",
            post(delete_folder),
        )
        .route("/v1/bookmarks", get(list_bookmarks).post(create_bookmark))
        .route("/v1/bookmarks/{bookmarkId}", get(get_bookmark))
        .route("/v1/bookmarks/{bookmarkId}/update", post(update_bookmark))
        .route("/v1/bookmarks/{bookmarkId}/move", post(move_bookmark))
        .route(
            "/v1/bookmarks/{bookmarkId}/favorite",
            post(favorite_bookmark),
        )
        .route("/v1/bookmarks/{bookmarkId}/archive", post(archive_bookmark))
        .route("/v1/bookmarks/{bookmarkId}/delete", post(delete_bookmark))
}

pub fn router(state: AppState) -> Router {
    routes().with_state(state)
}

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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Bookmark {
    pub id: String,
    #[serde(rename = "userId")]
    pub user_id: String,
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
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
    #[serde(rename = "updatedAt")]
    pub updated_at: DateTime<Utc>,
    #[serde(rename = "deletedAt")]
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, FromRow)]
struct BookmarkRow {
    id: String,
    user_id: String,
    folder_id: String,
    url: String,
    title: String,
    description: String,
    tags: SqlJson<Vec<String>>,
    favorite: bool,
    archived: bool,
    position: i32,
    revision: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
}

impl From<BookmarkRow> for Bookmark {
    fn from(row: BookmarkRow) -> Self {
        Self {
            id: row.id,
            user_id: row.user_id,
            folder_id: row.folder_id,
            url: row.url,
            title: row.title,
            description: row.description,
            tags: row.tags.0,
            favorite: row.favorite,
            archived: row.archived,
            position: row.position,
            revision: row.revision,
            created_at: row.created_at,
            updated_at: row.updated_at,
            deleted_at: row.deleted_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct FolderList {
    pub folders: Vec<Folder>,
}

#[derive(Debug, Serialize)]
pub struct BookmarkList {
    pub bookmarks: Vec<Bookmark>,
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
pub struct CreateBookmarkRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    pub id: String,
    #[serde(rename = "folderId")]
    pub folder_id: String,
    pub url: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub archived: bool,
    pub position: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateBookmarkRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    #[serde(rename = "expectedRevision")]
    pub expected_revision: i64,
    pub url: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MoveBookmarkRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    #[serde(rename = "expectedRevision")]
    pub expected_revision: i64,
    #[serde(rename = "folderId")]
    pub folder_id: String,
    pub position: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FavoriteRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    #[serde(rename = "expectedRevision")]
    pub expected_revision: i64,
    pub favorite: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ArchiveRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    #[serde(rename = "expectedRevision")]
    pub expected_revision: i64,
    pub archived: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    #[serde(rename = "expectedRevision")]
    pub expected_revision: i64,
}

#[derive(Debug, Deserialize)]
pub struct BookmarkListQuery {
    #[serde(rename = "folderId")]
    pub folder_id: Option<String>,
    pub favorite: Option<bool>,
    pub archived: Option<bool>,
    pub tag: Option<String>,
    pub q: Option<String>,
}

const BOOKMARK_COLUMNS: &str = "id, user_id, folder_id, url, title, description, tags, favorite, \
     archived, position, revision, created_at, updated_at, deleted_at";

const FOLDER_COLUMNS: &str =
    "id, user_id, parent_id, name, position, revision, created_at, updated_at, deleted_at";

pub async fn list_folders(
    State(state): State<AppState>,
    principal: Principal,
) -> ApiResult<Json<FolderList>> {
    let folders = sqlx::query_as::<_, Folder>(&format!(
        "SELECT {FOLDER_COLUMNS}
         FROM bookmarks_folders
         WHERE user_id = $1 AND deleted_at IS NULL
         ORDER BY parent_id NULLS FIRST, position, name, id"
    ))
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
    with_operation(
        &state.pool,
        operation_binding(
            user_id.clone(),
            operation_id,
            "folder",
            id.clone(),
            "create",
            request_body,
        ),
        |tx| {
            Box::pin(async move {
                if let Some(existing) = load_folder_tx(tx, &id).await? {
                    return Err(existing_identity_error(
                        &user_id,
                        existing.user_id,
                        existing.deleted_at.is_some(),
                    ));
                }
                if let Some(parent) = parent_id.as_ref() {
                    ensure_live_parent(tx, &user_id, parent).await?;
                    ensure_depth(tx, parent, 1).await?;
                    ensure_child_folder_capacity(tx, &user_id, Some(parent)).await?;
                } else {
                    ensure_child_folder_capacity(tx, &user_id, None).await?;
                }
                ensure_folder_quota(tx, &user_id).await?;
                let now = Utc::now();
                let position = match position {
                    Some(value) => value,
                    None => next_folder_position(tx, &user_id, parent_id.as_deref()).await?,
                };
                sqlx::query(
                    "INSERT INTO bookmarks_folders
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
        },
    )
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
                    "UPDATE bookmarks_folders
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
                    ensure_child_folder_capacity(tx, &user_id, parent_id.as_deref()).await?;
                }
                let now = Utc::now();
                let position = match position {
                    Some(value) => value,
                    None => next_folder_position(tx, &user_id, parent_id.as_deref()).await?,
                };
                sqlx::query(
                    "UPDATE bookmarks_folders
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
                    "SELECT COUNT(*) FROM bookmarks_folders
                     WHERE user_id = $1 AND parent_id = $2 AND deleted_at IS NULL",
                )
                .bind(&user_id)
                .bind(&folder.id)
                .fetch_one(&mut **tx)
                .await
                .map_err(database_error)?;
                let child_bookmarks = sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM bookmarks
                     WHERE user_id = $1 AND folder_id = $2 AND deleted_at IS NULL",
                )
                .bind(&user_id)
                .bind(&folder.id)
                .fetch_one(&mut **tx)
                .await
                .map_err(database_error)?;
                if child_folders > 0 || child_bookmarks > 0 {
                    return Err(ApiError::folder_not_empty(
                        "A folder can be deleted only when it has no live child folders or bookmarks.",
                    ));
                }
                let now = Utc::now();
                sqlx::query(
                    "UPDATE bookmarks_folders
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

pub async fn list_bookmarks(
    State(state): State<AppState>,
    principal: Principal,
    Query(query): Query<BookmarkListQuery>,
) -> ApiResult<Json<BookmarkList>> {
    let folder_id = optional_uuid("folderId", query.folder_id.as_deref())?;
    let tag = match query.tag.as_deref() {
        None | Some("") => None,
        Some(value) => Some(
            normalize_tag(value)
                .map_err(|_| ApiError::invalid_request("tag must be a normalized bookmark tag."))?,
        ),
    };
    let search = match query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        None => None,
        Some(value) => {
            if value.chars().count() > 200 {
                return Err(ApiError::invalid_request(
                    "q must be at most 200 characters.",
                ));
            }
            Some(escape_like(value))
        }
    };
    let rows = sqlx::query_as::<_, BookmarkRow>(&format!(
        "SELECT {BOOKMARK_COLUMNS}
         FROM bookmarks
         WHERE user_id = $1
           AND deleted_at IS NULL
           AND ($2::text IS NULL OR folder_id = $2)
           AND ($3::bool IS NULL OR favorite = $3)
           AND ($4::bool IS NULL OR archived = $4)
           AND ($5::text IS NULL OR tags @> jsonb_build_array($5::text))
           AND ($6::text IS NULL OR (
                title ILIKE '%' || $6 || '%' ESCAPE '\\'
                OR url ILIKE '%' || $6 || '%' ESCAPE '\\'
                OR description ILIKE '%' || $6 || '%' ESCAPE '\\'
                OR tags::text ILIKE '%' || $6 || '%' ESCAPE '\\'
           ))
         ORDER BY archived ASC, favorite DESC, position, title, id"
    ))
    .bind(&principal.user_id)
    .bind(folder_id)
    .bind(query.favorite)
    .bind(query.archived)
    .bind(tag)
    .bind(search)
    .fetch_all(&state.pool)
    .await
    .map_err(database_error)?;
    Ok(Json(BookmarkList {
        bookmarks: rows.into_iter().map(Bookmark::from).collect(),
    }))
}

pub async fn get_bookmark(
    State(state): State<AppState>,
    principal: Principal,
    Path(bookmark_id): Path<String>,
) -> ApiResult<Json<Bookmark>> {
    let bookmark = load_bookmark(&state.pool, &bookmark_id).await?;
    visible_bookmark(&principal.user_id, bookmark).map(Json)
}

pub async fn create_bookmark(
    State(state): State<AppState>,
    principal: Principal,
    Json(request): Json<CreateBookmarkRequest>,
) -> ApiResult<Json<Bookmark>> {
    let request_body = operation_request(&request)?;
    let operation_id = parse_uuid("operationId", &request.operation_id)?;
    let id = parse_uuid("id", &request.id)?;
    let folder_id = parse_uuid("folderId", &request.folder_id)?;
    let url = validate_bookmark_url(&request.url)?;
    let title = validate_bookmark_title(&request.title)?;
    let description = validate_description(&request.description)?;
    let tags = normalize_tags(&request.tags)?;
    let position = optional_position(request.position)?;
    let favorite = request.favorite;
    let archived = request.archived;
    let user_id = principal.user_id.clone();
    with_operation(
        &state.pool,
        operation_binding(
            user_id.clone(),
            operation_id,
            "bookmark",
            id.clone(),
            "create",
            request_body,
        ),
        |tx| {
            Box::pin(async move {
                if let Some(existing) = load_bookmark_tx(tx, &id).await? {
                    return Err(existing_identity_error(
                        &user_id,
                        existing.user_id,
                        existing.deleted_at.is_some(),
                    ));
                }
                ensure_live_parent(tx, &user_id, &folder_id).await?;
                ensure_bookmark_quota(tx, &user_id).await?;
                ensure_bookmark_folder_capacity(tx, &user_id, &folder_id).await?;
                let now = Utc::now();
                let position = match position {
                    Some(value) => value,
                    None => next_bookmark_position(tx, &user_id, &folder_id).await?,
                };
                sqlx::query(
                    "INSERT INTO bookmarks
                        (id, user_id, folder_id, url, title, description, tags, favorite, archived,
                         position, revision, created_at, updated_at, deleted_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 1, $11, $11, NULL)",
                )
                .bind(&id)
                .bind(&user_id)
                .bind(&folder_id)
                .bind(&url)
                .bind(&title)
                .bind(&description)
                .bind(SqlJson(&tags))
                .bind(favorite)
                .bind(archived)
                .bind(position)
                .bind(now)
                .execute(&mut **tx)
                .await
                .map_err(database_error)?;
                load_required_bookmark(tx, &id).await
            })
        },
    )
    .await
    .map(Json)
}

pub async fn update_bookmark(
    State(state): State<AppState>,
    principal: Principal,
    Path(bookmark_id): Path<String>,
    Json(request): Json<UpdateBookmarkRequest>,
) -> ApiResult<Json<Bookmark>> {
    let request_body = operation_request(&request)?;
    let operation_id = parse_uuid("operationId", &request.operation_id)?;
    let bookmark_id = parse_uuid("bookmarkId", &bookmark_id)?;
    let url = validate_bookmark_url(&request.url)?;
    let title = validate_bookmark_title(&request.title)?;
    let description = validate_description(&request.description)?;
    let tags = normalize_tags(&request.tags)?;
    let expected = validate_revision(request.expected_revision)?;
    let user_id = principal.user_id.clone();
    with_operation(
        &state.pool,
        operation_binding(
            user_id.clone(),
            operation_id,
            "bookmark",
            bookmark_id.clone(),
            "update",
            request_body,
        ),
        |tx| {
            Box::pin(async move {
                let bookmark = locked_live_bookmark(tx, &user_id, &bookmark_id, expected).await?;
                let now = Utc::now();
                sqlx::query(
                    "UPDATE bookmarks
                     SET url = $2, title = $3, description = $4, tags = $5,
                         revision = revision + 1, updated_at = $6
                     WHERE id = $1",
                )
                .bind(&bookmark.id)
                .bind(&url)
                .bind(&title)
                .bind(&description)
                .bind(SqlJson(&tags))
                .bind(now)
                .execute(&mut **tx)
                .await
                .map_err(database_error)?;
                load_required_bookmark(tx, &bookmark.id).await
            })
        },
    )
    .await
    .map(Json)
}

pub async fn move_bookmark(
    State(state): State<AppState>,
    principal: Principal,
    Path(bookmark_id): Path<String>,
    Json(request): Json<MoveBookmarkRequest>,
) -> ApiResult<Json<Bookmark>> {
    let request_body = operation_request(&request)?;
    let operation_id = parse_uuid("operationId", &request.operation_id)?;
    let bookmark_id = parse_uuid("bookmarkId", &bookmark_id)?;
    let folder_id = parse_uuid("folderId", &request.folder_id)?;
    let position = optional_position(request.position)?;
    let expected = validate_revision(request.expected_revision)?;
    let user_id = principal.user_id.clone();
    with_operation(
        &state.pool,
        operation_binding(
            user_id.clone(),
            operation_id,
            "bookmark",
            bookmark_id.clone(),
            "move",
            request_body,
        ),
        |tx| {
            Box::pin(async move {
                let bookmark = locked_live_bookmark(tx, &user_id, &bookmark_id, expected).await?;
                ensure_live_parent(tx, &user_id, &folder_id).await?;
                if folder_id != bookmark.folder_id {
                    ensure_bookmark_folder_capacity(tx, &user_id, &folder_id).await?;
                }
                let now = Utc::now();
                let position = match position {
                    Some(value) => value,
                    None => next_bookmark_position(tx, &user_id, &folder_id).await?,
                };
                sqlx::query(
                    "UPDATE bookmarks
                     SET folder_id = $2, position = $3, revision = revision + 1, updated_at = $4
                     WHERE id = $1",
                )
                .bind(&bookmark.id)
                .bind(&folder_id)
                .bind(position)
                .bind(now)
                .execute(&mut **tx)
                .await
                .map_err(database_error)?;
                load_required_bookmark(tx, &bookmark.id).await
            })
        },
    )
    .await
    .map(Json)
}

pub async fn favorite_bookmark(
    State(state): State<AppState>,
    principal: Principal,
    Path(bookmark_id): Path<String>,
    Json(request): Json<FavoriteRequest>,
) -> ApiResult<Json<Bookmark>> {
    let request_body = operation_request(&request)?;
    let operation_id = parse_uuid("operationId", &request.operation_id)?;
    let bookmark_id = parse_uuid("bookmarkId", &bookmark_id)?;
    let expected = validate_revision(request.expected_revision)?;
    let favorite = request.favorite;
    let user_id = principal.user_id.clone();
    with_operation(
        &state.pool,
        operation_binding(
            user_id.clone(),
            operation_id,
            "bookmark",
            bookmark_id.clone(),
            "favorite",
            request_body,
        ),
        |tx| {
            Box::pin(async move {
                let bookmark = locked_live_bookmark(tx, &user_id, &bookmark_id, expected).await?;
                let now = Utc::now();
                sqlx::query(
                    "UPDATE bookmarks
                     SET favorite = $2, revision = revision + 1, updated_at = $3
                     WHERE id = $1",
                )
                .bind(&bookmark.id)
                .bind(favorite)
                .bind(now)
                .execute(&mut **tx)
                .await
                .map_err(database_error)?;
                load_required_bookmark(tx, &bookmark.id).await
            })
        },
    )
    .await
    .map(Json)
}

pub async fn archive_bookmark(
    State(state): State<AppState>,
    principal: Principal,
    Path(bookmark_id): Path<String>,
    Json(request): Json<ArchiveRequest>,
) -> ApiResult<Json<Bookmark>> {
    let request_body = operation_request(&request)?;
    let operation_id = parse_uuid("operationId", &request.operation_id)?;
    let bookmark_id = parse_uuid("bookmarkId", &bookmark_id)?;
    let expected = validate_revision(request.expected_revision)?;
    let archived = request.archived;
    let user_id = principal.user_id.clone();
    with_operation(
        &state.pool,
        operation_binding(
            user_id.clone(),
            operation_id,
            "bookmark",
            bookmark_id.clone(),
            "archive",
            request_body,
        ),
        |tx| {
            Box::pin(async move {
                let bookmark = locked_live_bookmark(tx, &user_id, &bookmark_id, expected).await?;
                let now = Utc::now();
                sqlx::query(
                    "UPDATE bookmarks
                     SET archived = $2, revision = revision + 1, updated_at = $3
                     WHERE id = $1",
                )
                .bind(&bookmark.id)
                .bind(archived)
                .bind(now)
                .execute(&mut **tx)
                .await
                .map_err(database_error)?;
                load_required_bookmark(tx, &bookmark.id).await
            })
        },
    )
    .await
    .map(Json)
}

pub async fn delete_bookmark(
    State(state): State<AppState>,
    principal: Principal,
    Path(bookmark_id): Path<String>,
    Json(request): Json<DeleteRequest>,
) -> ApiResult<Json<Bookmark>> {
    let request_body = operation_request(&request)?;
    let operation_id = parse_uuid("operationId", &request.operation_id)?;
    let bookmark_id = parse_uuid("bookmarkId", &bookmark_id)?;
    let expected = validate_revision(request.expected_revision)?;
    let user_id = principal.user_id.clone();
    with_operation(
        &state.pool,
        operation_binding(
            user_id.clone(),
            operation_id,
            "bookmark",
            bookmark_id.clone(),
            "delete",
            request_body,
        ),
        |tx| {
            Box::pin(async move {
                let bookmark = locked_live_bookmark(tx, &user_id, &bookmark_id, expected).await?;
                let now = Utc::now();
                sqlx::query(
                    "UPDATE bookmarks
                     SET deleted_at = $2, revision = revision + 1, updated_at = $2
                     WHERE id = $1",
                )
                .bind(&bookmark.id)
                .bind(now)
                .execute(&mut **tx)
                .await
                .map_err(database_error)?;
                load_required_bookmark(tx, &bookmark.id).await
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
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 2))")
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
        "INSERT INTO bookmarks_operations
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
         FROM bookmarks_operations WHERE operation_id = $1",
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
    sqlx::query_as::<_, Folder>(&format!(
        "SELECT {FOLDER_COLUMNS} FROM bookmarks_folders WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(database_error)
}

async fn load_folder_tx(tx: &mut Transaction<'_, Postgres>, id: &str) -> ApiResult<Option<Folder>> {
    sqlx::query_as::<_, Folder>(&format!(
        "SELECT {FOLDER_COLUMNS} FROM bookmarks_folders WHERE id = $1"
    ))
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

async fn load_bookmark(pool: &PgPool, id: &str) -> ApiResult<Option<Bookmark>> {
    sqlx::query_as::<_, BookmarkRow>(&format!(
        "SELECT {BOOKMARK_COLUMNS} FROM bookmarks WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(database_error)
    .map(|row| row.map(Bookmark::from))
}

async fn load_bookmark_tx(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
) -> ApiResult<Option<Bookmark>> {
    sqlx::query_as::<_, BookmarkRow>(&format!(
        "SELECT {BOOKMARK_COLUMNS} FROM bookmarks WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)
    .map(|row| row.map(Bookmark::from))
}

async fn load_required_bookmark(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
) -> ApiResult<Bookmark> {
    load_bookmark_tx(tx, id)
        .await?
        .ok_or_else(|| ApiError::unavailable("bookmark disappeared after write"))
}

async fn locked_live_folder(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    id: &str,
    expected: i64,
) -> ApiResult<Folder> {
    let folder = sqlx::query_as::<_, Folder>(&format!(
        "SELECT {FOLDER_COLUMNS} FROM bookmarks_folders WHERE id = $1 FOR UPDATE"
    ))
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

async fn locked_live_bookmark(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    id: &str,
    expected: i64,
) -> ApiResult<Bookmark> {
    let bookmark = sqlx::query_as::<_, BookmarkRow>(&format!(
        "SELECT {BOOKMARK_COLUMNS} FROM bookmarks WHERE id = $1 FOR UPDATE"
    ))
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)?
    .map(Bookmark::from);
    let bookmark = visible_bookmark(user_id, bookmark)?;
    if bookmark.revision != expected {
        return Err(ApiError::stale_revision(expected, bookmark.revision));
    }
    Ok(bookmark)
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

fn visible_bookmark(user_id: &str, bookmark: Option<Bookmark>) -> ApiResult<Bookmark> {
    match bookmark {
        Some(bookmark) if bookmark.user_id != user_id => {
            Err(ApiError::not_found("Bookmark not found."))
        }
        Some(bookmark) if bookmark.deleted_at.is_some() => {
            Err(ApiError::gone("This bookmark has been deleted."))
        }
        Some(bookmark) => Ok(bookmark),
        None => Err(ApiError::not_found("Bookmark not found.")),
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
            "SELECT parent_id FROM bookmarks_folders WHERE id = $1",
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
            "SELECT id FROM bookmarks_folders WHERE parent_id = $1 AND deleted_at IS NULL",
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
            "SELECT parent_id FROM bookmarks_folders WHERE id = $1",
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
        "SELECT COUNT(*) FROM bookmarks_folders WHERE user_id = $1 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(database_error)?;
    if count >= MAX_FOLDERS_PER_USER {
        return Err(ApiError::limit_exceeded(format!(
            "A user may have at most {MAX_FOLDERS_PER_USER} bookmark folders."
        )));
    }
    Ok(())
}

async fn ensure_bookmark_quota(tx: &mut Transaction<'_, Postgres>, user_id: &str) -> ApiResult<()> {
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM bookmarks WHERE user_id = $1 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(database_error)?;
    if count >= MAX_BOOKMARKS_PER_USER {
        return Err(ApiError::limit_exceeded(format!(
            "A user may have at most {MAX_BOOKMARKS_PER_USER} bookmarks."
        )));
    }
    Ok(())
}

async fn ensure_child_folder_capacity(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    parent_id: Option<&str>,
) -> ApiResult<()> {
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM bookmarks_folders
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

async fn ensure_bookmark_folder_capacity(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    folder_id: &str,
) -> ApiResult<()> {
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM bookmarks
         WHERE user_id = $1 AND folder_id = $2 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .bind(folder_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(database_error)?;
    if count >= MAX_BOOKMARKS_PER_FOLDER {
        return Err(ApiError::limit_exceeded(format!(
            "A folder may have at most {MAX_BOOKMARKS_PER_FOLDER} bookmarks."
        )));
    }
    Ok(())
}

async fn next_folder_position(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    parent_id: Option<&str>,
) -> ApiResult<i32> {
    let max = sqlx::query_scalar::<_, Option<i32>>(
        "SELECT MAX(position) FROM bookmarks_folders
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

async fn next_bookmark_position(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    folder_id: &str,
) -> ApiResult<i32> {
    let max = sqlx::query_scalar::<_, Option<i32>>(
        "SELECT MAX(position) FROM bookmarks
         WHERE user_id = $1 AND folder_id = $2 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .bind(folder_id)
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

pub fn validate_bookmark_title(value: &str) -> ApiResult<String> {
    validate_text("title", value, 1, MAX_BOOKMARK_TITLE)
}

pub fn validate_description(value: &str) -> ApiResult<String> {
    if value.len() > MAX_DESCRIPTION_BYTES {
        return Err(ApiError::invalid_request(format!(
            "description must be at most {MAX_DESCRIPTION_BYTES} bytes."
        )));
    }
    if value.contains('\0') {
        return Err(ApiError::invalid_request(
            "description cannot contain NUL bytes.",
        ));
    }
    Ok(value.to_string())
}

pub fn validate_bookmark_url(value: &str) -> ApiResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ApiError::invalid_request("url is required."));
    }
    if trimmed.len() > MAX_URL_BYTES {
        return Err(ApiError::invalid_request(format!(
            "url must be at most {MAX_URL_BYTES} bytes."
        )));
    }
    if trimmed.contains('\0')
        || trimmed
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err(ApiError::invalid_request(
            "url cannot contain whitespace or control characters.",
        ));
    }
    let (scheme, rest) = if let Some(rest) = strip_prefix_ignore_ascii_case(trimmed, "https://") {
        ("https", rest)
    } else if let Some(rest) = strip_prefix_ignore_ascii_case(trimmed, "http://") {
        ("http", rest)
    } else {
        return Err(ApiError::invalid_request(
            "url must be an HTTP or HTTPS URL.",
        ));
    };
    if rest.is_empty() {
        return Err(ApiError::invalid_request("url must include a host."));
    }
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() {
        return Err(ApiError::invalid_request("url must include a host."));
    }
    let hostport = authority
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(authority);
    if hostport.is_empty() {
        return Err(ApiError::invalid_request("url must include a host."));
    }
    let host = if let Some(inner) = hostport.strip_prefix('[') {
        let end = inner
            .find(']')
            .ok_or_else(|| ApiError::invalid_request("url host is invalid."))?;
        if inner[..end].is_empty() {
            return Err(ApiError::invalid_request("url must include a host."));
        }
        &inner[..end]
    } else {
        hostport
            .split_once(':')
            .map(|(host, _)| host)
            .unwrap_or(hostport)
    };
    if host.is_empty() || host == "." || host == ".." || host.contains('/') {
        return Err(ApiError::invalid_request("url must include a host."));
    }
    Ok(format!("{scheme}://{rest}"))
}

pub fn normalize_tags(values: &[String]) -> ApiResult<Vec<String>> {
    let mut tags = Vec::new();
    for value in values {
        let tag = normalize_tag(value)?;
        if !tags.iter().any(|existing| existing == &tag) {
            tags.push(tag);
        }
    }
    if tags.len() > MAX_TAGS {
        return Err(ApiError::invalid_request(format!(
            "a bookmark may have at most {MAX_TAGS} tags."
        )));
    }
    Ok(tags)
}

pub fn normalize_tag(value: &str) -> ApiResult<String> {
    let collapsed = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    if collapsed.is_empty() {
        return Err(ApiError::invalid_request("tags cannot be empty."));
    }
    if collapsed.chars().count() > MAX_TAG_LENGTH {
        return Err(ApiError::invalid_request(format!(
            "each tag must be at most {MAX_TAG_LENGTH} characters."
        )));
    }
    if collapsed.contains('\0') || collapsed.chars().any(char::is_control) {
        return Err(ApiError::invalid_request(
            "tags cannot contain control characters.",
        ));
    }
    Ok(collapsed)
}

fn strip_prefix_ignore_ascii_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    if value.len() >= prefix.len() && value[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&value[prefix.len()..])
    } else {
        None
    }
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
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
        assert_eq!(validate_folder_name("  Reading  ").unwrap(), "Reading");
        assert!(validate_folder_name("").is_err());
        assert!(validate_folder_name(&"x".repeat(MAX_FOLDER_NAME + 1)).is_err());
    }

    #[test]
    fn description_is_preserved_including_html_and_trailing_newline() {
        let description = "Keep <script>alert(1)</script> and **bold** losslessly.\n";
        assert_eq!(validate_description(description).unwrap(), description);
    }

    #[test]
    fn description_rejects_oversize_and_nul() {
        assert!(validate_description(&"a".repeat(MAX_DESCRIPTION_BYTES + 1)).is_err());
        assert!(validate_description("ok\0no").is_err());
    }

    #[test]
    fn url_accepts_only_http_and_https() {
        assert_eq!(
            validate_bookmark_url("  HTTPS://Example.COM/path?q=1  ").unwrap(),
            "https://Example.COM/path?q=1"
        );
        assert_eq!(
            validate_bookmark_url("http://localhost:8080/a").unwrap(),
            "http://localhost:8080/a"
        );
        assert!(validate_bookmark_url("javascript:alert(1)").is_err());
        assert!(validate_bookmark_url("ftp://example.com").is_err());
        assert!(validate_bookmark_url("file:///etc/passwd").is_err());
        assert!(validate_bookmark_url("https:///no-host").is_err());
        assert!(validate_bookmark_url("https://exam ple.com").is_err());
        assert!(validate_bookmark_url("not-a-url").is_err());
    }

    #[test]
    fn tags_are_normalized_and_deduplicated() {
        let tags = normalize_tags(&[
            "  Work  ".into(),
            "WORK".into(),
            "docs".into(),
            "  Docs ".into(),
        ])
        .unwrap();
        assert_eq!(tags, vec!["work", "docs"]);
        assert!(normalize_tags(&["   ".into()]).is_err());
        assert!(normalize_tags(&["x".repeat(MAX_TAG_LENGTH + 1)]).is_err());
        assert!(
            normalize_tags(&(0..=MAX_TAGS).map(|i| format!("t{i}")).collect::<Vec<_>>()).is_err()
        );
    }

    #[test]
    fn uuid_fields_must_be_canonical() {
        let id = parse_uuid("id", "550e8400-e29b-41d4-a716-446655440000").unwrap();
        assert_eq!(id, "550e8400-e29b-41d4-a716-446655440000");
        assert!(parse_uuid("id", "not-a-uuid").is_err());
    }
}
