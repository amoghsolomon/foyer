//! Semantic Tasks API. Radicale CalDAV is canonical; PostgreSQL rows are projections.
#![allow(
    clippy::collapsible_if,
    clippy::collapsible_str_replace,
    clippy::new_without_default,
    clippy::too_many_arguments
)]

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    AppState,
    auth::Principal,
    dav::{
        CollectionKind, DavClient, DavHref, DavMediaType, DavPayload, ETag, NewCalendar,
        PutPrecondition,
    },
    error::{ApiError, ApiResult},
};

pub const MAX_LIST_NAME: usize = 80;
pub const MAX_TASK_TITLE: usize = 200;
pub const MAX_DESCRIPTION_BYTES: usize = 256 * 1024;
pub const MAX_LISTS_PER_USER: i64 = 64;
pub const MAX_TASKS_PER_USER: i64 = 4096;
pub const MAX_TASKS_PER_LIST: i64 = 512;
pub const MAX_POSITION: i32 = 100_000;
pub const MAX_ICAL_BYTES: usize = 512 * 1024;

const PRODID: &str = "-//Amazity//Foyer//EN";
const FOYER_ORDER: &str = "X-FOYER-ORDER";
const FOYER_OP: &str = "X-FOYER-OP";

static DAV_OVERRIDE: Mutex<Option<DavBackend>> = Mutex::new(None);

#[derive(Clone, Debug, Serialize, Deserialize, FromRow, PartialEq, Eq)]
pub struct TaskList {
    pub id: String,
    #[serde(rename = "userId")]
    pub user_id: String,
    pub name: String,
    pub position: i32,
    pub href: String,
    pub etag: Option<String>,
    pub revision: i64,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
    #[serde(rename = "updatedAt")]
    pub updated_at: DateTime<Utc>,
    #[serde(rename = "deletedAt")]
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Due {
    pub local: String,
    #[serde(rename = "timeZone", skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,
    #[serde(rename = "allDay")]
    pub all_day: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Task {
    pub id: String,
    #[serde(rename = "userId")]
    pub user_id: String,
    #[serde(rename = "listId")]
    pub list_id: String,
    pub title: String,
    pub description: String,
    pub due: Option<Due>,
    pub priority: i32,
    pub completed: bool,
    #[serde(rename = "completedAt")]
    pub completed_at: Option<DateTime<Utc>>,
    pub position: i32,
    pub href: String,
    pub etag: String,
    pub revision: i64,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
    #[serde(rename = "updatedAt")]
    pub updated_at: DateTime<Utc>,
    #[serde(rename = "deletedAt")]
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, FromRow)]
struct TaskRow {
    id: String,
    user_id: String,
    list_id: String,
    title: String,
    description: String,
    due_at: Option<DateTime<Utc>>,
    due_local: Option<String>,
    due_time_zone: Option<String>,
    due_all_day: bool,
    priority: i32,
    completed: bool,
    completed_at: Option<DateTime<Utc>>,
    position: i32,
    href: String,
    etag: String,
    revision: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
}

impl Task {
    fn from_row(row: TaskRow) -> Self {
        let due = row
            .due_local
            .as_deref()
            .filter(|value| !value.is_empty())
            .map(|local| Due {
                local: local.to_string(),
                time_zone: row.due_time_zone,
                all_day: row.due_all_day,
                at: row.due_at,
            });
        Self {
            id: row.id,
            user_id: row.user_id,
            list_id: row.list_id,
            title: row.title,
            description: row.description,
            due,
            priority: row.priority,
            completed: row.completed,
            completed_at: row.completed_at,
            position: row.position,
            href: row.href,
            etag: row.etag,
            revision: row.revision,
            created_at: row.created_at,
            updated_at: row.updated_at,
            deleted_at: row.deleted_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct TaskListCollection {
    pub lists: Vec<TaskList>,
}

#[derive(Debug, Serialize)]
pub struct TaskCollection {
    pub tasks: Vec<Task>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateTaskListRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    pub id: String,
    pub name: String,
    pub position: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RenameTaskListRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    #[serde(rename = "expectedRevision")]
    pub expected_revision: i64,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateTaskRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    pub id: String,
    #[serde(rename = "listId")]
    pub list_id: String,
    pub title: String,
    pub description: String,
    pub due: Option<Due>,
    pub priority: Option<i32>,
    pub position: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateTaskRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    #[serde(rename = "expectedRevision")]
    pub expected_revision: i64,
    pub title: String,
    pub description: String,
    pub due: Option<Due>,
    pub priority: i32,
    pub position: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MoveTaskRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    #[serde(rename = "expectedRevision")]
    pub expected_revision: i64,
    #[serde(rename = "listId")]
    pub list_id: String,
    pub position: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    #[serde(rename = "expectedRevision")]
    pub expected_revision: i64,
}

#[derive(Debug, Deserialize)]
pub struct TaskListQuery {
    #[serde(rename = "listId")]
    pub list_id: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/v1/task-lists",
            get(list_task_lists).post(create_task_list),
        )
        .route("/v1/task-lists/{listId}", get(get_task_list))
        .route("/v1/task-lists/{listId}/rename", post(rename_task_list))
        .route("/v1/task-lists/{listId}/delete", post(delete_task_list))
        .route("/v1/tasks", get(list_tasks).post(create_task))
        .route("/v1/tasks/{taskId}", get(get_task))
        .route("/v1/tasks/{taskId}/update", post(update_task))
        .route("/v1/tasks/{taskId}/move", post(move_task))
        .route("/v1/tasks/{taskId}/complete", post(complete_task))
        .route("/v1/tasks/{taskId}/reopen", post(reopen_task))
        .route("/v1/tasks/{taskId}/delete", post(delete_task))
}

pub async fn list_task_lists(
    State(state): State<AppState>,
    principal: Principal,
) -> ApiResult<Json<TaskListCollection>> {
    let lists = sqlx::query_as::<_, TaskList>(
        "SELECT id, user_id, name, position, href, etag, revision, created_at, updated_at, deleted_at
         FROM task_lists
         WHERE user_id = $1 AND deleted_at IS NULL
         ORDER BY position, name, id",
    )
    .bind(&principal.user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(database_error)?;
    Ok(Json(TaskListCollection { lists }))
}

pub async fn get_task_list(
    State(state): State<AppState>,
    principal: Principal,
    Path(list_id): Path<String>,
) -> ApiResult<Json<TaskList>> {
    let list = load_list(&state.pool, &list_id).await?;
    visible_list(&principal.user_id, list).map(Json)
}

pub async fn create_task_list(
    State(state): State<AppState>,
    principal: Principal,
    Json(request): Json<CreateTaskListRequest>,
) -> ApiResult<Json<TaskList>> {
    let request_body = operation_request(&request)?;
    let operation_id = parse_uuid("operationId", &request.operation_id)?;
    let id = parse_uuid("id", &request.id)?;
    let name = validate_list_name(&request.name)?;
    let position = optional_position(request.position)?;
    let user_id = principal.user_id.clone();
    let dav = dav_from_state(&state)?;
    with_operation(
        &state.pool,
        operation_binding(
            user_id.clone(),
            operation_id.clone(),
            "task_list",
            id.clone(),
            "create",
            request_body,
        ),
        |tx| {
            Box::pin(async move {
                if let Some(existing) = load_list_tx(tx, &id).await? {
                    return Err(existing_identity_error(
                        &user_id,
                        existing.user_id,
                        existing.deleted_at.is_some(),
                    ));
                }
                ensure_list_quota(tx, &user_id).await?;
                let position = match position {
                    Some(value) => value,
                    None => next_list_position(tx, &user_id).await?,
                };
                ensure_user_home(&dav, &user_id).await?;
                let href = list_href(&user_id, &id);
                let created = dav
                    .mkcol(&href, &name, position, DavPrecondition::None)
                    .await;
                match created {
                    Ok(()) => {}
                    Err(error) if is_precondition(error.status()) => {
                        if !operation_already_applied(&dav, &href, &operation_id).await? {
                            return Err(ApiError::conflict("This identifier is already in use."));
                        }
                    }
                    Err(error) if error.status() == Some(405) || error.status() == Some(409) => {
                        if !operation_already_applied(&dav, &href, &operation_id).await? {
                            return Err(ApiError::conflict("This identifier is already in use."));
                        }
                    }
                    Err(error) => return Err(error.into_api()),
                }
                let _ = dav.proppatch(&href, &name, position).await;
                project_list_from_dav(tx, &dav, &user_id, &id, &href, &name, position).await
            })
        },
    )
    .await
    .map(Json)
}

pub async fn rename_task_list(
    State(state): State<AppState>,
    principal: Principal,
    Path(list_id): Path<String>,
    Json(request): Json<RenameTaskListRequest>,
) -> ApiResult<Json<TaskList>> {
    let request_body = operation_request(&request)?;
    let operation_id = parse_uuid("operationId", &request.operation_id)?;
    let list_id = parse_uuid("listId", &list_id)?;
    let name = validate_list_name(&request.name)?;
    let expected = validate_revision(request.expected_revision)?;
    let user_id = principal.user_id.clone();
    let dav = dav_from_state(&state)?;
    with_operation(
        &state.pool,
        operation_binding(
            user_id.clone(),
            operation_id,
            "task_list",
            list_id.clone(),
            "rename",
            request_body,
        ),
        |tx| {
            Box::pin(async move {
                let list = locked_live_list(tx, &user_id, &list_id, expected).await?;
                dav.proppatch(&list.href, &name, list.position)
                    .await
                    .map_err(DavError::into_api)?;
                project_list_from_dav(
                    tx,
                    &dav,
                    &user_id,
                    &list.id,
                    &list.href,
                    &name,
                    list.position,
                )
                .await
            })
        },
    )
    .await
    .map(Json)
}

pub async fn delete_task_list(
    State(state): State<AppState>,
    principal: Principal,
    Path(list_id): Path<String>,
    Json(request): Json<DeleteRequest>,
) -> ApiResult<Json<TaskList>> {
    let request_body = operation_request(&request)?;
    let operation_id = parse_uuid("operationId", &request.operation_id)?;
    let list_id = parse_uuid("listId", &list_id)?;
    let expected = validate_revision(request.expected_revision)?;
    let user_id = principal.user_id.clone();
    let dav = dav_from_state(&state)?;
    with_operation(
        &state.pool,
        operation_binding(
            user_id.clone(),
            operation_id,
            "task_list",
            list_id.clone(),
            "delete",
            request_body,
        ),
        |tx| {
            Box::pin(async move {
                let list = locked_live_list(tx, &user_id, &list_id, expected).await?;
                let live_tasks = sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM tasks
                     WHERE user_id = $1 AND list_id = $2 AND deleted_at IS NULL",
                )
                .bind(&user_id)
                .bind(&list.id)
                .fetch_one(&mut **tx)
                .await
                .map_err(database_error)?;
                if live_tasks > 0 {
                    return Err(ApiError::conflict(
                        "A task list can be deleted only when it has no live tasks.",
                    ));
                }
                match dav
                    .delete(
                        &list.href,
                        DavPrecondition::IfMatch(list.etag.clone()),
                        true,
                    )
                    .await
                {
                    Ok(()) => {}
                    Err(error) if error.status() == Some(404) => {}
                    Err(error) if is_precondition(error.status()) => {
                        return Err(stale_dav(&list.revision));
                    }
                    Err(error) => return Err(error.into_api()),
                }
                tombstone_list(tx, &list.id).await
            })
        },
    )
    .await
    .map(Json)
}

pub async fn list_tasks(
    State(state): State<AppState>,
    principal: Principal,
    Query(query): Query<TaskListQuery>,
) -> ApiResult<Json<TaskCollection>> {
    let list_id = optional_uuid("listId", query.list_id.as_deref())?;
    let rows = if let Some(list_id) = list_id {
        sqlx::query_as::<_, TaskRow>(
            "SELECT id, user_id, list_id, title, description, due_at, due_local, due_time_zone,
                    due_all_day, priority, completed, completed_at, position, href, etag,
                    revision, created_at, updated_at, deleted_at
             FROM tasks
             WHERE user_id = $1 AND list_id = $2 AND deleted_at IS NULL
             ORDER BY completed, position, title, id",
        )
        .bind(&principal.user_id)
        .bind(list_id)
        .fetch_all(&state.pool)
        .await
        .map_err(database_error)?
    } else {
        sqlx::query_as::<_, TaskRow>(
            "SELECT id, user_id, list_id, title, description, due_at, due_local, due_time_zone,
                    due_all_day, priority, completed, completed_at, position, href, etag,
                    revision, created_at, updated_at, deleted_at
             FROM tasks
             WHERE user_id = $1 AND deleted_at IS NULL
             ORDER BY completed, position, title, id",
        )
        .bind(&principal.user_id)
        .fetch_all(&state.pool)
        .await
        .map_err(database_error)?
    };
    Ok(Json(TaskCollection {
        tasks: rows.into_iter().map(Task::from_row).collect(),
    }))
}

pub async fn get_task(
    State(state): State<AppState>,
    principal: Principal,
    Path(task_id): Path<String>,
) -> ApiResult<Json<Task>> {
    let task = load_task(&state.pool, &task_id).await?;
    visible_task(&principal.user_id, task).map(Json)
}

pub async fn create_task(
    State(state): State<AppState>,
    principal: Principal,
    Json(request): Json<CreateTaskRequest>,
) -> ApiResult<Json<Task>> {
    let request_body = operation_request(&request)?;
    let operation_id = parse_uuid("operationId", &request.operation_id)?;
    let id = parse_uuid("id", &request.id)?;
    let list_id = parse_uuid("listId", &request.list_id)?;
    let title = validate_task_title(&request.title)?;
    let description = validate_description(&request.description)?;
    let due = optional_due(request.due.as_ref())?;
    let priority = optional_priority(request.priority)?;
    let position = optional_position(request.position)?;
    let user_id = principal.user_id.clone();
    let dav = dav_from_state(&state)?;
    with_operation(
        &state.pool,
        operation_binding(
            user_id.clone(),
            operation_id.clone(),
            "task",
            id.clone(),
            "create",
            request_body,
        ),
        |tx| {
            Box::pin(async move {
                if let Some(existing) = load_task_tx(tx, &id).await? {
                    return Err(existing_identity_error(
                        &user_id,
                        existing.user_id,
                        existing.deleted_at.is_some(),
                    ));
                }
                let list = ensure_live_list(tx, &user_id, &list_id).await?;
                ensure_task_quota(tx, &user_id, &list.id).await?;
                let position = match position {
                    Some(value) => value,
                    None => next_task_position(tx, &user_id, &list.id).await?,
                };
                let href = task_href(&user_id, &list.id, &id);
                let fields = TodoFields {
                    uid: id.clone(),
                    title: title.clone(),
                    description: description.clone(),
                    due: due.clone(),
                    priority,
                    completed: false,
                    completed_at: None,
                    position,
                    operation_id: Some(operation_id.clone()),
                };
                let body = serialize_calendar(&new_todo_calendar(&fields));
                match dav
                    .put(&href, &body, DavPrecondition::IfNoneMatchStar)
                    .await
                {
                    Ok(_) => {}
                    Err(error) if is_precondition(error.status()) => {
                        if !operation_already_applied(&dav, &href, &operation_id).await? {
                            return Err(ApiError::conflict("This identifier is already in use."));
                        }
                    }
                    Err(error) => return Err(error.into_api()),
                }
                project_task_href(tx, &dav, &user_id, &list.id, &href).await
            })
        },
    )
    .await
    .map(Json)
}

pub async fn update_task(
    State(state): State<AppState>,
    principal: Principal,
    Path(task_id): Path<String>,
    Json(request): Json<UpdateTaskRequest>,
) -> ApiResult<Json<Task>> {
    let request_body = operation_request(&request)?;
    let operation_id = parse_uuid("operationId", &request.operation_id)?;
    let task_id = parse_uuid("taskId", &task_id)?;
    let title = validate_task_title(&request.title)?;
    let description = validate_description(&request.description)?;
    let due = optional_due(request.due.as_ref())?;
    let priority = validate_priority(request.priority)?;
    let position = required_position(request.position)?;
    let expected = validate_revision(request.expected_revision)?;
    let user_id = principal.user_id.clone();
    let dav = dav_from_state(&state)?;
    with_operation(
        &state.pool,
        operation_binding(
            user_id.clone(),
            operation_id.clone(),
            "task",
            task_id.clone(),
            "update",
            request_body,
        ),
        |tx| {
            Box::pin(async move {
                let task = locked_live_task(tx, &user_id, &task_id, expected).await?;
                let patch = TodoPatch {
                    title: Some(title),
                    description: Some(description),
                    due: Some(due),
                    priority: Some(priority),
                    position: Some(position),
                    completed: None,
                    operation_id: Some(operation_id),
                };
                put_patched_todo(&dav, &task, &patch).await?;
                project_task_href(tx, &dav, &user_id, &task.list_id, &task.href).await
            })
        },
    )
    .await
    .map(Json)
}

pub async fn move_task(
    State(state): State<AppState>,
    principal: Principal,
    Path(task_id): Path<String>,
    Json(request): Json<MoveTaskRequest>,
) -> ApiResult<Json<Task>> {
    let request_body = operation_request(&request)?;
    let operation_id = parse_uuid("operationId", &request.operation_id)?;
    let task_id = parse_uuid("taskId", &task_id)?;
    let list_id = parse_uuid("listId", &request.list_id)?;
    let position = optional_position(request.position)?;
    let expected = validate_revision(request.expected_revision)?;
    let user_id = principal.user_id.clone();
    let dav = dav_from_state(&state)?;
    with_operation(
        &state.pool,
        operation_binding(
            user_id.clone(),
            operation_id.clone(),
            "task",
            task_id.clone(),
            "move",
            request_body,
        ),
        |tx| {
            Box::pin(async move {
                let task = locked_live_task(tx, &user_id, &task_id, expected).await?;
                let dest = ensure_live_list(tx, &user_id, &list_id).await?;
                if dest.id != task.list_id {
                    ensure_list_capacity(tx, &user_id, &dest.id).await?;
                }
                let dest_href = task_href(&user_id, &dest.id, &task.id);
                if dest_href != task.href {
                    match dav
                        .r#move(
                            &task.href,
                            &dest_href,
                            DavPrecondition::IfMatch(Some(task.etag.clone())),
                        )
                        .await
                    {
                        Ok(()) => {}
                        Err(error) if is_precondition(error.status()) => {
                            if !operation_already_applied(&dav, &dest_href, &operation_id).await? {
                                return Err(stale_dav(&task.revision));
                            }
                        }
                        Err(error) => return Err(error.into_api()),
                    }
                }
                if let Some(position) = position {
                    let observed = dav.get(&dest_href).await.map_err(DavError::into_api)?;
                    let mut calendar = parse_calendar(&observed.body).map_err(format_error)?;
                    patch_todo(
                        &mut calendar,
                        &TodoPatch {
                            title: None,
                            description: None,
                            due: None,
                            priority: None,
                            position: Some(position),
                            completed: None,
                            operation_id: Some(operation_id),
                        },
                    )
                    .map_err(format_error)?;
                    dav.put(
                        &dest_href,
                        &serialize_calendar(&calendar),
                        DavPrecondition::IfMatch(Some(observed.etag)),
                    )
                    .await
                    .map_err(DavError::into_api)?;
                }
                project_task_href(tx, &dav, &user_id, &dest.id, &dest_href).await
            })
        },
    )
    .await
    .map(Json)
}

pub async fn complete_task(
    State(state): State<AppState>,
    principal: Principal,
    Path(task_id): Path<String>,
    Json(request): Json<DeleteRequest>,
) -> ApiResult<Json<Task>> {
    mutate_completion(state, principal, task_id, request, true).await
}

pub async fn reopen_task(
    State(state): State<AppState>,
    principal: Principal,
    Path(task_id): Path<String>,
    Json(request): Json<DeleteRequest>,
) -> ApiResult<Json<Task>> {
    mutate_completion(state, principal, task_id, request, false).await
}

async fn mutate_completion(
    state: AppState,
    principal: Principal,
    task_id: String,
    request: DeleteRequest,
    completed: bool,
) -> ApiResult<Json<Task>> {
    let request_body = operation_request(&request)?;
    let operation_id = parse_uuid("operationId", &request.operation_id)?;
    let task_id = parse_uuid("taskId", &task_id)?;
    let expected = validate_revision(request.expected_revision)?;
    let user_id = principal.user_id.clone();
    let dav = dav_from_state(&state)?;
    let operation = if completed { "complete" } else { "reopen" };
    with_operation(
        &state.pool,
        operation_binding(
            user_id.clone(),
            operation_id.clone(),
            "task",
            task_id.clone(),
            operation,
            request_body,
        ),
        |tx| {
            Box::pin(async move {
                let task = locked_live_task(tx, &user_id, &task_id, expected).await?;
                if task.completed == completed {
                    return Ok(task);
                }
                let patch = TodoPatch {
                    title: None,
                    description: None,
                    due: None,
                    priority: None,
                    position: None,
                    completed: Some(completed),
                    operation_id: Some(operation_id),
                };
                put_patched_todo(&dav, &task, &patch).await?;
                project_task_href(tx, &dav, &user_id, &task.list_id, &task.href).await
            })
        },
    )
    .await
    .map(Json)
}

pub async fn delete_task(
    State(state): State<AppState>,
    principal: Principal,
    Path(task_id): Path<String>,
    Json(request): Json<DeleteRequest>,
) -> ApiResult<Json<Task>> {
    let request_body = operation_request(&request)?;
    let operation_id = parse_uuid("operationId", &request.operation_id)?;
    let task_id = parse_uuid("taskId", &task_id)?;
    let expected = validate_revision(request.expected_revision)?;
    let user_id = principal.user_id.clone();
    let dav = dav_from_state(&state)?;
    with_operation(
        &state.pool,
        operation_binding(
            user_id.clone(),
            operation_id,
            "task",
            task_id.clone(),
            "delete",
            request_body,
        ),
        |tx| {
            Box::pin(async move {
                let task = locked_live_task(tx, &user_id, &task_id, expected).await?;
                match dav
                    .delete(
                        &task.href,
                        DavPrecondition::IfMatch(Some(task.etag.clone())),
                        false,
                    )
                    .await
                {
                    Ok(()) => {}
                    Err(error) if error.status() == Some(404) => {}
                    Err(error) if is_precondition(error.status()) => {
                        return Err(stale_dav(&task.revision));
                    }
                    Err(error) => return Err(error.into_api()),
                }
                tombstone_task(tx, &task.id).await
            })
        },
    )
    .await
    .map(Json)
}

async fn put_patched_todo(dav: &DavBackend, task: &Task, patch: &TodoPatch) -> ApiResult<()> {
    let observed = match dav.get(&task.href).await {
        Ok(resource) => resource,
        Err(error) if error.status() == Some(404) => {
            return Err(ApiError::gone("This task has been deleted."));
        }
        Err(error) => return Err(error.into_api()),
    };
    if observed.etag != task.etag {
        return Err(ApiError::conflict(
            "The DAV resource changed. Refresh and retry.",
        ));
    }
    let mut calendar = parse_calendar(&observed.body).map_err(format_error)?;
    patch_todo(&mut calendar, patch).map_err(format_error)?;
    let body = serialize_calendar(&calendar);
    match dav
        .put(
            &task.href,
            &body,
            DavPrecondition::IfMatch(Some(task.etag.clone())),
        )
        .await
    {
        Ok(_) => Ok(()),
        Err(error) if is_precondition(error.status()) => {
            if let Some(operation_id) = patch.operation_id.as_deref() {
                if operation_already_applied(dav, &task.href, operation_id).await? {
                    return Ok(());
                }
            }
            Err(stale_dav(&task.revision))
        }
        Err(error) => Err(error.into_api()),
    }
}

async fn operation_already_applied(
    dav: &DavBackend,
    href: &str,
    operation_id: &str,
) -> ApiResult<bool> {
    let Ok(resource) = dav.get(href).await else {
        return Ok(false);
    };
    let Ok(calendar) = parse_calendar(&resource.body) else {
        return Ok(false);
    };
    Ok(extract_todo(&calendar)
        .ok()
        .and_then(|todo| todo.operation_id)
        .as_deref()
        == Some(operation_id))
}

pub async fn reconcile_user(state: &AppState, user_id: &str) -> ApiResult<()> {
    let dav = dav_from_state(state)?;
    let mut tx = state.pool.begin().await.map_err(database_error)?;
    project_user(&mut tx, &dav, user_id).await?;
    tx.commit().await.map_err(database_error)?;
    if let Ok(client) = state.dav_client() {
        commit_task_checkpoints(state, client, user_id).await?;
    }
    Ok(())
}

async fn commit_task_checkpoints(
    state: &AppState,
    client: &DavClient,
    user_id: &str,
) -> ApiResult<()> {
    let lists = sqlx::query_as::<_, TaskList>(
        "SELECT id, user_id, name, position, href, etag, revision, created_at, updated_at, deleted_at
         FROM task_lists
         WHERE user_id = $1 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(database_error)?;
    for list in lists {
        let href = DavHref::parse(&list.href).map_err(dav_error_to_api)?;
        let collection = crate::dav::DavCollection {
            href: href.clone(),
            kind: CollectionKind::TaskList,
            display_name: Some(list.name.clone()),
            etag: list
                .etag
                .as_deref()
                .and_then(|value| ETag::parse(value).ok()),
            sync_token: None,
            supported_components: vec!["VTODO".into()],
        };
        let remembered = state
            .projector
            .remember_collection(user_id, &list.id, &collection)
            .await
            .map_err(dav_error_to_api)?;
        if let Ok(plan) = state.projector.plan_sync(client, user_id, &href).await {
            let mut tx = state.pool.begin().await.map_err(database_error)?;
            state
                .projector
                .commit_checkpoint(
                    &mut tx,
                    user_id,
                    &href,
                    plan.page.sync_token.as_ref(),
                    remembered
                        .collection_etag
                        .as_deref()
                        .and_then(|value| ETag::parse(value).ok())
                        .as_ref(),
                )
                .await
                .map_err(dav_error_to_api)?;
            tx.commit().await.map_err(database_error)?;
        }
    }
    Ok(())
}

fn dav_error_to_api(error: crate::dav::DavError) -> ApiError {
    match error {
        crate::dav::DavError::PreconditionFailed { expected, .. } => {
            ApiError::stale_etag(expected.unwrap_or_default(), None)
        }
        crate::dav::DavError::NotFound(message) => ApiError::not_found(message),
        crate::dav::DavError::Conflict(message) => ApiError::conflict(message),
        crate::dav::DavError::OperationConflict => {
            ApiError::conflict("this DAV operation id is already bound to a different request")
        }
        crate::dav::DavError::InvalidRequest(message) => ApiError::invalid_request(message),
        other => ApiError::unavailable(other.to_string()),
    }
}

async fn project_user(
    tx: &mut Transaction<'_, Postgres>,
    dav: &DavBackend,
    user_id: &str,
) -> ApiResult<()> {
    let root = tasks_root(user_id);
    let listing = match dav.propfind(&root, 1).await {
        Ok(listing) => listing,
        Err(error) if error.status() == Some(404) => return Ok(()),
        Err(error) => return Err(error.into_api()),
    };
    let mut seen_lists = Vec::new();
    for item in listing.items {
        if item.href == root || !item.collection {
            continue;
        }
        let Some(list_id) = list_id_from_href(user_id, &item.href) else {
            continue;
        };
        let name = item
            .displayname
            .clone()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| list_id.clone());
        let position = item.calendar_order.unwrap_or(0).clamp(0, MAX_POSITION);
        let list =
            project_list_from_dav(tx, dav, user_id, &list_id, &item.href, &name, position).await?;
        project_list_members(tx, dav, user_id, &list).await?;
        seen_lists.push(list.id);
    }
    sqlx::query(
        "UPDATE task_lists SET deleted_at = NOW(), revision = revision + 1, updated_at = NOW()
         WHERE user_id = $1 AND deleted_at IS NULL AND NOT (id = ANY($2))",
    )
    .bind(user_id)
    .bind(&seen_lists)
    .execute(&mut **tx)
    .await
    .map_err(database_error)?;
    Ok(())
}

async fn project_list_members(
    tx: &mut Transaction<'_, Postgres>,
    dav: &DavBackend,
    user_id: &str,
    list: &TaskList,
) -> ApiResult<()> {
    let listing = dav
        .propfind(&list.href, 1)
        .await
        .map_err(DavError::into_api)?;
    let mut seen = Vec::new();
    for item in listing.items {
        if item.collection || item.href == list.href {
            continue;
        }
        match project_task_href(tx, dav, user_id, &list.id, &item.href).await {
            Ok(task) => seen.push(task.id),
            Err(error) => {
                tracing::warn!(href = %item.href, error = ?error, "skipping unreadable VTODO");
            }
        }
    }
    sqlx::query(
        "UPDATE tasks SET deleted_at = NOW(), revision = revision + 1, updated_at = NOW()
         WHERE user_id = $1 AND list_id = $2 AND deleted_at IS NULL AND NOT (id = ANY($3))",
    )
    .bind(user_id)
    .bind(&list.id)
    .bind(&seen)
    .execute(&mut **tx)
    .await
    .map_err(database_error)?;
    sqlx::query(
        "INSERT INTO tasks_dav_checkpoints (user_id, collection_href, sync_token, projected_at)
         VALUES ($1, $2, $3, NOW())
         ON CONFLICT (user_id, collection_href)
         DO UPDATE SET sync_token = EXCLUDED.sync_token, projected_at = EXCLUDED.projected_at",
    )
    .bind(user_id)
    .bind(&list.href)
    .bind(listing.sync_token)
    .execute(&mut **tx)
    .await
    .map_err(database_error)?;
    Ok(())
}

async fn project_list_from_dav(
    tx: &mut Transaction<'_, Postgres>,
    dav: &DavBackend,
    user_id: &str,
    id: &str,
    href: &str,
    name: &str,
    position: i32,
) -> ApiResult<TaskList> {
    let listing = dav.propfind(href, 0).await.ok();
    let etag = listing
        .as_ref()
        .and_then(|value| value.items.first())
        .and_then(|item| item.etag.clone());
    let ctag = listing.as_ref().and_then(|value| value.ctag.clone());
    let sync_token = listing.as_ref().and_then(|value| value.sync_token.clone());
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO task_lists
            (id, user_id, name, position, href, etag, ctag, sync_token, revision, created_at, updated_at, deleted_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 1, $9, $9, NULL)
         ON CONFLICT (id) DO UPDATE SET
            name = EXCLUDED.name,
            position = EXCLUDED.position,
            href = EXCLUDED.href,
            etag = EXCLUDED.etag,
            ctag = EXCLUDED.ctag,
            sync_token = EXCLUDED.sync_token,
            revision = CASE
                WHEN task_lists.deleted_at IS NOT NULL
                    OR task_lists.name IS DISTINCT FROM EXCLUDED.name
                    OR task_lists.position IS DISTINCT FROM EXCLUDED.position
                    OR task_lists.href IS DISTINCT FROM EXCLUDED.href
                    OR task_lists.etag IS DISTINCT FROM EXCLUDED.etag
                THEN task_lists.revision + 1
                ELSE task_lists.revision
            END,
            updated_at = CASE
                WHEN task_lists.deleted_at IS NOT NULL
                    OR task_lists.name IS DISTINCT FROM EXCLUDED.name
                    OR task_lists.position IS DISTINCT FROM EXCLUDED.position
                    OR task_lists.href IS DISTINCT FROM EXCLUDED.href
                    OR task_lists.etag IS DISTINCT FROM EXCLUDED.etag
                THEN EXCLUDED.updated_at
                ELSE task_lists.updated_at
            END,
            deleted_at = NULL",
    )
    .bind(id)
    .bind(user_id)
    .bind(name)
    .bind(position)
    .bind(href)
    .bind(etag)
    .bind(ctag)
    .bind(sync_token)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(database_error)?;
    load_required_list(tx, id).await
}

async fn project_task_href(
    tx: &mut Transaction<'_, Postgres>,
    dav: &DavBackend,
    user_id: &str,
    list_id: &str,
    href: &str,
) -> ApiResult<Task> {
    let resource = dav.get(href).await.map_err(DavError::into_api)?;
    let calendar = parse_calendar(&resource.body).map_err(format_error)?;
    let fields = extract_todo(&calendar).map_err(format_error)?;
    let now = Utc::now();
    let due_local = fields.due.as_ref().map(|due| due.local.clone());
    let due_time_zone = fields.due.as_ref().and_then(|due| due.time_zone.clone());
    let due_all_day = fields.due.as_ref().map(|due| due.all_day).unwrap_or(false);
    let due_at = fields.due.as_ref().and_then(|due| due.at);
    sqlx::query(
        "INSERT INTO tasks
            (id, user_id, list_id, title, description, due_at, due_local, due_time_zone, due_all_day,
             priority, completed, completed_at, position, href, etag, revision, created_at, updated_at, deleted_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, 1, $16, $16, NULL)
         ON CONFLICT (id) DO UPDATE SET
            list_id = EXCLUDED.list_id,
            title = EXCLUDED.title,
            description = EXCLUDED.description,
            due_at = EXCLUDED.due_at,
            due_local = EXCLUDED.due_local,
            due_time_zone = EXCLUDED.due_time_zone,
            due_all_day = EXCLUDED.due_all_day,
            priority = EXCLUDED.priority,
            completed = EXCLUDED.completed,
            completed_at = EXCLUDED.completed_at,
            position = EXCLUDED.position,
            href = EXCLUDED.href,
            etag = EXCLUDED.etag,
            revision = CASE
                WHEN tasks.deleted_at IS NOT NULL
                    OR tasks.list_id IS DISTINCT FROM EXCLUDED.list_id
                    OR tasks.title IS DISTINCT FROM EXCLUDED.title
                    OR tasks.description IS DISTINCT FROM EXCLUDED.description
                    OR tasks.due_at IS DISTINCT FROM EXCLUDED.due_at
                    OR tasks.due_local IS DISTINCT FROM EXCLUDED.due_local
                    OR tasks.due_time_zone IS DISTINCT FROM EXCLUDED.due_time_zone
                    OR tasks.due_all_day IS DISTINCT FROM EXCLUDED.due_all_day
                    OR tasks.priority IS DISTINCT FROM EXCLUDED.priority
                    OR tasks.completed IS DISTINCT FROM EXCLUDED.completed
                    OR tasks.completed_at IS DISTINCT FROM EXCLUDED.completed_at
                    OR tasks.position IS DISTINCT FROM EXCLUDED.position
                    OR tasks.href IS DISTINCT FROM EXCLUDED.href
                    OR tasks.etag IS DISTINCT FROM EXCLUDED.etag
                THEN tasks.revision + 1
                ELSE tasks.revision
            END,
            updated_at = CASE
                WHEN tasks.deleted_at IS NOT NULL
                    OR tasks.etag IS DISTINCT FROM EXCLUDED.etag
                    OR tasks.list_id IS DISTINCT FROM EXCLUDED.list_id
                THEN EXCLUDED.updated_at
                ELSE tasks.updated_at
            END,
            deleted_at = NULL",
    )
    .bind(&fields.uid)
    .bind(user_id)
    .bind(list_id)
    .bind(&fields.title)
    .bind(&fields.description)
    .bind(due_at)
    .bind(due_local)
    .bind(due_time_zone)
    .bind(due_all_day)
    .bind(fields.priority)
    .bind(fields.completed)
    .bind(fields.completed_at)
    .bind(fields.position)
    .bind(href)
    .bind(&resource.etag)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(database_error)?;
    load_required_task(tx, &fields.uid).await
}

async fn tombstone_list(tx: &mut Transaction<'_, Postgres>, id: &str) -> ApiResult<TaskList> {
    sqlx::query(
        "UPDATE task_lists
         SET deleted_at = NOW(), revision = revision + 1, updated_at = NOW()
         WHERE id = $1",
    )
    .bind(id)
    .execute(&mut **tx)
    .await
    .map_err(database_error)?;
    load_required_list(tx, id).await
}

async fn tombstone_task(tx: &mut Transaction<'_, Postgres>, id: &str) -> ApiResult<Task> {
    sqlx::query(
        "UPDATE tasks
         SET deleted_at = NOW(), revision = revision + 1, updated_at = NOW()
         WHERE id = $1",
    )
    .bind(id)
    .execute(&mut **tx)
    .await
    .map_err(database_error)?;
    load_required_task(tx, id).await
}

async fn with_operation<T, F>(pool: &PgPool, binding: OperationBinding, work: F) -> ApiResult<T>
where
    T: Serialize + for<'de> Deserialize<'de> + Send,
    F: for<'c> FnOnce(
        &'c mut Transaction<'_, Postgres>,
    ) -> Pin<Box<dyn Future<Output = ApiResult<T>> + Send + 'c>>,
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
        "INSERT INTO tasks_operations
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

async fn load_operation(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: &str,
) -> ApiResult<Option<StoredOperation>> {
    sqlx::query_as::<_, StoredOperation>(
        "SELECT user_id, entity_type, entity_id, operation, request_body, result_status, result_body
         FROM tasks_operations WHERE operation_id = $1",
    )
    .bind(operation_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)
}

fn operation_request<T: Serialize>(request: &T) -> ApiResult<Value> {
    serde_json::to_value(request)
        .map_err(|error| ApiError::unavailable(format!("failed to encode operation: {error}")))
}

async fn load_list(pool: &PgPool, id: &str) -> ApiResult<Option<TaskList>> {
    sqlx::query_as::<_, TaskList>(
        "SELECT id, user_id, name, position, href, etag, revision, created_at, updated_at, deleted_at
         FROM task_lists WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(database_error)
}

async fn load_list_tx(tx: &mut Transaction<'_, Postgres>, id: &str) -> ApiResult<Option<TaskList>> {
    sqlx::query_as::<_, TaskList>(
        "SELECT id, user_id, name, position, href, etag, revision, created_at, updated_at, deleted_at
         FROM task_lists WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)
}

async fn load_required_list(tx: &mut Transaction<'_, Postgres>, id: &str) -> ApiResult<TaskList> {
    load_list_tx(tx, id)
        .await?
        .ok_or_else(|| ApiError::unavailable("task list disappeared after write"))
}

async fn load_task(pool: &PgPool, id: &str) -> ApiResult<Option<Task>> {
    sqlx::query_as::<_, TaskRow>(
        "SELECT id, user_id, list_id, title, description, due_at, due_local, due_time_zone,
                due_all_day, priority, completed, completed_at, position, href, etag,
                revision, created_at, updated_at, deleted_at
         FROM tasks WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(database_error)
    .map(|row| row.map(Task::from_row))
}

async fn load_task_tx(tx: &mut Transaction<'_, Postgres>, id: &str) -> ApiResult<Option<Task>> {
    sqlx::query_as::<_, TaskRow>(
        "SELECT id, user_id, list_id, title, description, due_at, due_local, due_time_zone,
                due_all_day, priority, completed, completed_at, position, href, etag,
                revision, created_at, updated_at, deleted_at
         FROM tasks WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)
    .map(|row| row.map(Task::from_row))
}

async fn load_required_task(tx: &mut Transaction<'_, Postgres>, id: &str) -> ApiResult<Task> {
    load_task_tx(tx, id)
        .await?
        .ok_or_else(|| ApiError::unavailable("task disappeared after write"))
}

async fn locked_live_list(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    id: &str,
    expected: i64,
) -> ApiResult<TaskList> {
    let list = sqlx::query_as::<_, TaskList>(
        "SELECT id, user_id, name, position, href, etag, revision, created_at, updated_at, deleted_at
         FROM task_lists WHERE id = $1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)?;
    let list = visible_list(user_id, list)?;
    if list.revision != expected {
        return Err(ApiError::stale_revision(expected, list.revision));
    }
    Ok(list)
}

async fn locked_live_task(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    id: &str,
    expected: i64,
) -> ApiResult<Task> {
    let task = sqlx::query_as::<_, TaskRow>(
        "SELECT id, user_id, list_id, title, description, due_at, due_local, due_time_zone,
                due_all_day, priority, completed, completed_at, position, href, etag,
                revision, created_at, updated_at, deleted_at
         FROM tasks WHERE id = $1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)?;
    let task = visible_task(user_id, task.map(Task::from_row))?;
    if task.revision != expected {
        return Err(ApiError::stale_revision(expected, task.revision));
    }
    Ok(task)
}

fn visible_list(user_id: &str, list: Option<TaskList>) -> ApiResult<TaskList> {
    match list {
        Some(list) if list.user_id != user_id => Err(ApiError::not_found("Task list not found.")),
        Some(list) if list.deleted_at.is_some() => {
            Err(ApiError::gone("This task list has been deleted."))
        }
        Some(list) => Ok(list),
        None => Err(ApiError::not_found("Task list not found.")),
    }
}

fn visible_task(user_id: &str, task: Option<Task>) -> ApiResult<Task> {
    match task {
        Some(task) if task.user_id != user_id => Err(ApiError::not_found("Task not found.")),
        Some(task) if task.deleted_at.is_some() => {
            Err(ApiError::gone("This task has been deleted."))
        }
        Some(task) => Ok(task),
        None => Err(ApiError::not_found("Task not found.")),
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

async fn ensure_live_list(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    list_id: &str,
) -> ApiResult<TaskList> {
    match load_list_tx(tx, list_id).await? {
        None => Err(ApiError::invalid_parent("The task list does not exist.")),
        Some(list) if list.user_id != user_id => {
            Err(ApiError::invalid_parent("The task list does not exist."))
        }
        Some(list) if list.deleted_at.is_some() => {
            Err(ApiError::invalid_parent("The task list has been deleted."))
        }
        Some(list) => Ok(list),
    }
}

async fn ensure_list_quota(tx: &mut Transaction<'_, Postgres>, user_id: &str) -> ApiResult<()> {
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM task_lists WHERE user_id = $1 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(database_error)?;
    if count >= MAX_LISTS_PER_USER {
        return Err(ApiError::limit_exceeded(format!(
            "A user may have at most {MAX_LISTS_PER_USER} task lists."
        )));
    }
    Ok(())
}

async fn ensure_task_quota(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    list_id: &str,
) -> ApiResult<()> {
    let total = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM tasks WHERE user_id = $1 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(database_error)?;
    if total >= MAX_TASKS_PER_USER {
        return Err(ApiError::limit_exceeded(format!(
            "A user may have at most {MAX_TASKS_PER_USER} tasks."
        )));
    }
    ensure_list_capacity(tx, user_id, list_id).await
}

async fn ensure_list_capacity(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    list_id: &str,
) -> ApiResult<()> {
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM tasks
         WHERE user_id = $1 AND list_id = $2 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .bind(list_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(database_error)?;
    if count >= MAX_TASKS_PER_LIST {
        return Err(ApiError::limit_exceeded(format!(
            "A task list may have at most {MAX_TASKS_PER_LIST} tasks."
        )));
    }
    Ok(())
}

async fn next_list_position(tx: &mut Transaction<'_, Postgres>, user_id: &str) -> ApiResult<i32> {
    let max = sqlx::query_scalar::<_, Option<i32>>(
        "SELECT MAX(position) FROM task_lists WHERE user_id = $1 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(database_error)?;
    Ok(max
        .map(|value| value.saturating_add(1))
        .unwrap_or(0)
        .min(MAX_POSITION))
}

async fn next_task_position(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    list_id: &str,
) -> ApiResult<i32> {
    let max = sqlx::query_scalar::<_, Option<i32>>(
        "SELECT MAX(position) FROM tasks
         WHERE user_id = $1 AND list_id = $2 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .bind(list_id)
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
        Some(value) => required_position(value).map(Some),
    }
}

fn required_position(value: i32) -> ApiResult<i32> {
    if (0..=MAX_POSITION).contains(&value) {
        Ok(value)
    } else {
        Err(ApiError::invalid_request(format!(
            "position must be between 0 and {MAX_POSITION}."
        )))
    }
}

fn optional_priority(value: Option<i32>) -> ApiResult<i32> {
    validate_priority(value.unwrap_or(0))
}

fn validate_priority(value: i32) -> ApiResult<i32> {
    if (0..=9).contains(&value) {
        Ok(value)
    } else {
        Err(ApiError::invalid_request(
            "priority must be between 0 (unset) and 9 (lowest).",
        ))
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

pub fn validate_list_name(value: &str) -> ApiResult<String> {
    validate_text("name", value, 1, MAX_LIST_NAME)
}

pub fn validate_task_title(value: &str) -> ApiResult<String> {
    validate_text("title", value, 1, MAX_TASK_TITLE)
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

fn optional_due(due: Option<&Due>) -> ApiResult<Option<Due>> {
    match due {
        None => Ok(None),
        Some(due) => validate_due(due).map(Some),
    }
}

pub fn validate_due(due: &Due) -> ApiResult<Due> {
    let local = due.local.trim();
    if local.is_empty() {
        return Err(ApiError::invalid_request("due.local is required."));
    }
    let time_zone = due
        .time_zone
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if let Some(zone) = time_zone.as_deref() {
        if zone != "UTC" && !zone.contains('/') {
            return Err(ApiError::invalid_request(
                "due.timeZone must be UTC or an IANA name such as America/New_York.",
            ));
        }
        if zone.len() > 64 || zone.contains('\0') || zone.chars().any(char::is_control) {
            return Err(ApiError::invalid_request("due.timeZone is invalid."));
        }
    }
    if due.all_day {
        NaiveDate::parse_from_str(local, "%Y-%m-%d")
            .map_err(|_| ApiError::invalid_request("all-day due.local must be YYYY-MM-DD."))?;
        return Ok(Due {
            local: local.to_string(),
            time_zone,
            all_day: true,
            at: None,
        });
    }
    let parsed = NaiveDateTime::parse_from_str(local, "%Y-%m-%dT%H:%M:%S").map_err(|_| {
        ApiError::invalid_request("due.local must be YYYY-MM-DDTHH:MM:SS when not all-day.")
    })?;
    let at = if time_zone.as_deref() == Some("UTC") {
        Some(parsed.and_utc())
    } else {
        due.at
    };
    Ok(Due {
        local: local.to_string(),
        time_zone,
        all_day: false,
        at,
    })
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

fn format_error(error: CalendarError) -> ApiError {
    ApiError::invalid_request(error.to_string())
}

fn stale_dav(revision: &i64) -> ApiError {
    ApiError::stale_revision(*revision, *revision)
}

fn is_precondition(status: Option<u16>) -> bool {
    matches!(status, Some(412))
}

fn user_root(user_id: &str) -> String {
    format!("/{user_id}/")
}

fn tasks_root(user_id: &str) -> String {
    format!("/{user_id}/tasks/")
}

fn list_href(user_id: &str, list_id: &str) -> String {
    format!("/{user_id}/tasks/{list_id}/")
}

fn task_href(user_id: &str, list_id: &str, uid: &str) -> String {
    format!("/{user_id}/tasks/{list_id}/{uid}.ics")
}

fn list_id_from_href(user_id: &str, href: &str) -> Option<String> {
    let prefix = tasks_root(user_id);
    let rest = href.strip_prefix(&prefix)?.trim_matches('/');
    if rest.is_empty() || rest.contains('/') {
        return None;
    }
    Uuid::parse_str(rest).ok().map(|uuid| uuid.to_string())
}

async fn ensure_user_home(dav: &DavBackend, user_id: &str) -> ApiResult<()> {
    for href in [user_root(user_id), tasks_root(user_id)] {
        match dav.mkcol(&href, "Tasks", 0, DavPrecondition::None).await {
            Ok(()) => {}
            Err(error) if matches!(error.status(), Some(405 | 409 | 412)) => {}
            Err(error) => return Err(error.into_api()),
        }
    }
    Ok(())
}

// --- iCalendar / VTODO -------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Calendar {
    pub components: Vec<Component>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Component {
    pub name: String,
    pub properties: Vec<Property>,
    pub children: Vec<Component>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Property {
    pub name: String,
    pub params: Vec<(String, String)>,
    pub value: String,
    pub raw: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TodoFields {
    pub uid: String,
    pub title: String,
    pub description: String,
    pub due: Option<Due>,
    pub priority: i32,
    pub completed: bool,
    pub completed_at: Option<DateTime<Utc>>,
    pub position: i32,
    pub operation_id: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct TodoPatch {
    pub title: Option<String>,
    pub description: Option<String>,
    pub due: Option<Option<Due>>,
    pub priority: Option<i32>,
    pub position: Option<i32>,
    pub completed: Option<bool>,
    pub operation_id: Option<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CalendarError {
    #[error("{0}")]
    Message(String),
}

impl CalendarError {
    fn new(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

pub fn parse_calendar(input: &str) -> Result<Calendar, CalendarError> {
    if input.len() > MAX_ICAL_BYTES {
        return Err(CalendarError::new("iCalendar payload is too large."));
    }
    let lines = unfold(input);
    if lines.is_empty() {
        return Err(CalendarError::new("iCalendar document is empty."));
    }
    let mut stack: Vec<Component> = Vec::new();
    let mut roots = Vec::new();
    for line in &lines {
        if let Some(name) = line.strip_prefix("BEGIN:") {
            stack.push(Component {
                name: name.trim().to_string(),
                properties: Vec::new(),
                children: Vec::new(),
            });
            continue;
        }
        if let Some(name) = line.strip_prefix("END:") {
            let finished = stack
                .pop()
                .ok_or_else(|| CalendarError::new("unbalanced END"))?;
            if !finished.name.eq_ignore_ascii_case(name.trim()) {
                return Err(CalendarError::new("mismatched BEGIN/END"));
            }
            if let Some(parent) = stack.last_mut() {
                parent.children.push(finished);
            } else {
                roots.push(finished);
            }
            continue;
        }
        let current = stack
            .last_mut()
            .ok_or_else(|| CalendarError::new("property outside a component"))?;
        current.properties.push(parse_property(line)?);
    }
    if !stack.is_empty() {
        return Err(CalendarError::new("unclosed iCalendar component"));
    }
    Ok(Calendar { components: roots })
}

pub fn serialize_calendar(calendar: &Calendar) -> String {
    let mut out = String::new();
    for component in &calendar.components {
        write_component(&mut out, component);
    }
    if !out.ends_with("\r\n") {
        out.push_str("\r\n");
    }
    out
}

pub fn new_todo_calendar(fields: &TodoFields) -> Calendar {
    let mut todo = Component {
        name: "VTODO".into(),
        properties: vec![
            property("UID", &fields.uid),
            property("DTSTAMP", &utc_stamp(Utc::now())),
            property("CREATED", &utc_stamp(Utc::now())),
            property("LAST-MODIFIED", &utc_stamp(Utc::now())),
            property("SUMMARY", &fields.title),
        ],
        children: Vec::new(),
    };
    apply_todo_fields(&mut todo, fields);
    Calendar {
        components: vec![Component {
            name: "VCALENDAR".into(),
            properties: vec![
                property("VERSION", "2.0"),
                property("PRODID", PRODID),
                property("CALSCALE", "GREGORIAN"),
            ],
            children: vec![todo],
        }],
    }
}

pub fn extract_todo(calendar: &Calendar) -> Result<TodoFields, CalendarError> {
    let todo = find_todo(calendar).ok_or_else(|| CalendarError::new("VTODO is missing."))?;
    let uid = required_prop(todo, "UID")?;
    let title = prop_value(todo, "SUMMARY").unwrap_or_default();
    let description = prop_value(todo, "DESCRIPTION").unwrap_or_default();
    let due = prop(todo, "DUE").map(due_from_property).transpose()?;
    let priority = prop_value(todo, "PRIORITY")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let status = prop_value(todo, "STATUS").unwrap_or_default();
    let percent = prop_value(todo, "PERCENT-COMPLETE")
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(0);
    let completed_at = prop_value(todo, "COMPLETED").and_then(|value| parse_utc_stamp(&value));
    let completed =
        status.eq_ignore_ascii_case("COMPLETED") || completed_at.is_some() || percent >= 100;
    let position = prop_value(todo, FOYER_ORDER)
        .or_else(|| prop_value(todo, "X-APPLE-SORT-ORDER"))
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let operation_id = prop_value(todo, FOYER_OP);
    Ok(TodoFields {
        uid,
        title,
        description,
        due,
        priority,
        completed,
        completed_at,
        position,
        operation_id,
    })
}

pub fn patch_todo(calendar: &mut Calendar, patch: &TodoPatch) -> Result<(), CalendarError> {
    let todo = find_todo_mut(calendar).ok_or_else(|| CalendarError::new("VTODO is missing."))?;
    let now = utc_stamp(Utc::now());
    set_property(todo, "DTSTAMP", Vec::new(), now.clone());
    set_property(todo, "LAST-MODIFIED", Vec::new(), now);
    if let Some(title) = patch.title.as_ref() {
        set_property(todo, "SUMMARY", Vec::new(), title.clone());
    }
    if let Some(description) = patch.description.as_ref() {
        if description.is_empty() {
            remove_property(todo, "DESCRIPTION");
        } else {
            set_property(todo, "DESCRIPTION", Vec::new(), description.clone());
        }
    }
    if let Some(due) = patch.due.as_ref() {
        remove_property(todo, "DUE");
        if let Some(due) = due {
            todo.properties.push(due_to_property(due));
        }
    }
    if let Some(priority) = patch.priority {
        if priority == 0 {
            remove_property(todo, "PRIORITY");
        } else {
            set_property(todo, "PRIORITY", Vec::new(), priority.to_string());
        }
    }
    if let Some(position) = patch.position {
        set_property(todo, FOYER_ORDER, Vec::new(), position.to_string());
    }
    if let Some(completed) = patch.completed {
        if completed {
            set_property(todo, "STATUS", Vec::new(), "COMPLETED".into());
            set_property(todo, "PERCENT-COMPLETE", Vec::new(), "100".into());
            set_property(todo, "COMPLETED", Vec::new(), utc_stamp(Utc::now()));
        } else {
            set_property(todo, "STATUS", Vec::new(), "NEEDS-ACTION".into());
            remove_property(todo, "PERCENT-COMPLETE");
            remove_property(todo, "COMPLETED");
        }
    }
    if let Some(operation_id) = patch.operation_id.as_ref() {
        set_property(todo, FOYER_OP, Vec::new(), operation_id.clone());
    }
    Ok(())
}

fn apply_todo_fields(todo: &mut Component, fields: &TodoFields) {
    if !fields.description.is_empty() {
        set_property(todo, "DESCRIPTION", Vec::new(), fields.description.clone());
    }
    if let Some(due) = fields.due.as_ref() {
        remove_property(todo, "DUE");
        todo.properties.push(due_to_property(due));
    }
    if fields.priority > 0 {
        set_property(todo, "PRIORITY", Vec::new(), fields.priority.to_string());
    }
    set_property(todo, FOYER_ORDER, Vec::new(), fields.position.to_string());
    if fields.completed {
        set_property(todo, "STATUS", Vec::new(), "COMPLETED".into());
        set_property(todo, "PERCENT-COMPLETE", Vec::new(), "100".into());
        set_property(
            todo,
            "COMPLETED",
            Vec::new(),
            utc_stamp(fields.completed_at.unwrap_or_else(Utc::now)),
        );
    } else {
        set_property(todo, "STATUS", Vec::new(), "NEEDS-ACTION".into());
    }
    if let Some(operation_id) = fields.operation_id.as_ref() {
        set_property(todo, FOYER_OP, Vec::new(), operation_id.clone());
    }
}

fn find_todo(calendar: &Calendar) -> Option<&Component> {
    calendar.components.iter().find_map(find_todo_in)
}

fn find_todo_in(component: &Component) -> Option<&Component> {
    if component.name.eq_ignore_ascii_case("VTODO") {
        return Some(component);
    }
    component.children.iter().find_map(find_todo_in)
}

fn find_todo_mut(calendar: &mut Calendar) -> Option<&mut Component> {
    calendar.components.iter_mut().find_map(find_todo_in_mut)
}

fn find_todo_in_mut(component: &mut Component) -> Option<&mut Component> {
    if component.name.eq_ignore_ascii_case("VTODO") {
        return Some(component);
    }
    component.children.iter_mut().find_map(find_todo_in_mut)
}

fn prop<'a>(component: &'a Component, name: &str) -> Option<&'a Property> {
    component
        .properties
        .iter()
        .find(|property| property.name.eq_ignore_ascii_case(name))
}

fn prop_value(component: &Component, name: &str) -> Option<String> {
    prop(component, name).map(|property| property.value.clone())
}

fn required_prop(component: &Component, name: &str) -> Result<String, CalendarError> {
    prop_value(component, name)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CalendarError::new(format!("{name} is required on VTODO.")))
}

fn property(name: &str, value: &str) -> Property {
    Property {
        name: name.to_string(),
        params: Vec::new(),
        value: value.to_string(),
        raw: None,
    }
}

fn set_property(
    component: &mut Component,
    name: &str,
    params: Vec<(String, String)>,
    value: String,
) {
    if let Some(existing) = component
        .properties
        .iter_mut()
        .find(|property| property.name.eq_ignore_ascii_case(name))
    {
        existing.name = name.to_string();
        existing.params = params;
        existing.value = value;
        existing.raw = None;
        return;
    }
    component.properties.push(Property {
        name: name.to_string(),
        params,
        value,
        raw: None,
    });
}

fn remove_property(component: &mut Component, name: &str) {
    component
        .properties
        .retain(|property| !property.name.eq_ignore_ascii_case(name));
}

fn due_to_property(due: &Due) -> Property {
    if due.all_day {
        return Property {
            name: "DUE".into(),
            params: vec![("VALUE".into(), "DATE".into())],
            value: due.local.replace('-', ""),
            raw: None,
        };
    }
    let value = ical_local_datetime(&due.local, due.time_zone.as_deref() == Some("UTC"));
    let params = match due.time_zone.as_deref() {
        Some("UTC") | None => Vec::new(),
        Some(zone) => vec![("TZID".into(), zone.to_string())],
    };
    Property {
        name: "DUE".into(),
        params,
        value,
        raw: None,
    }
}

fn due_from_property(property: &Property) -> Result<Due, CalendarError> {
    let value_param = param(property, "VALUE");
    let tzid = param(property, "TZID");
    if value_param
        .as_deref()
        .is_some_and(|value| value.eq_ignore_ascii_case("DATE"))
        || property.value.len() == 8
    {
        let local = parse_ical_date(&property.value)?;
        return Ok(Due {
            local,
            time_zone: tzid,
            all_day: true,
            at: None,
        });
    }
    let utc = property.value.ends_with('Z');
    let local = parse_ical_datetime(&property.value)?;
    let time_zone = if utc { Some("UTC".into()) } else { tzid };
    let at = if utc {
        NaiveDateTime::parse_from_str(&local, "%Y-%m-%dT%H:%M:%S")
            .ok()
            .map(|value| value.and_utc())
    } else {
        None
    };
    Ok(Due {
        local,
        time_zone,
        all_day: false,
        at,
    })
}

fn param(property: &Property, name: &str) -> Option<String> {
    property
        .params
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
}

fn parse_ical_date(value: &str) -> Result<String, CalendarError> {
    if value.len() == 8 && value.chars().all(|ch| ch.is_ascii_digit()) {
        Ok(format!(
            "{}-{}-{}",
            &value[0..4],
            &value[4..6],
            &value[6..8]
        ))
    } else {
        Err(CalendarError::new("invalid DATE value"))
    }
}

fn parse_ical_datetime(value: &str) -> Result<String, CalendarError> {
    let trimmed = value.trim_end_matches('Z');
    let naive = NaiveDateTime::parse_from_str(trimmed, "%Y%m%dT%H%M%S")
        .or_else(|_| NaiveDateTime::parse_from_str(trimmed, "%Y%m%dT%H%M%S%.f"))
        .map_err(|_| CalendarError::new("invalid DATE-TIME value"))?;
    Ok(naive.format("%Y-%m-%dT%H:%M:%S").to_string())
}

fn ical_local_datetime(local: &str, utc: bool) -> String {
    let compact = local
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect::<String>();
    let mut value = if compact.len() >= 14 {
        format!("{}T{}", &compact[0..8], &compact[8..14])
    } else {
        local.replace('-', "").replace(':', "")
    };
    if utc && !value.ends_with('Z') {
        value.push('Z');
    }
    value
}

fn utc_stamp(when: DateTime<Utc>) -> String {
    when.format("%Y%m%dT%H%M%SZ").to_string()
}

fn parse_utc_stamp(value: &str) -> Option<DateTime<Utc>> {
    let trimmed = value.trim_end_matches('Z');
    NaiveDateTime::parse_from_str(trimmed, "%Y%m%dT%H%M%S")
        .ok()
        .map(|value| value.and_utc())
}

fn unfold(input: &str) -> Vec<String> {
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines: Vec<String> = Vec::new();
    for line in normalized.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(last) = lines.last_mut() {
                last.push_str(&line[1..]);
                continue;
            }
        }
        if !line.is_empty() {
            lines.push(line.to_string());
        }
    }
    lines
}

fn parse_property(line: &str) -> Result<Property, CalendarError> {
    let colon = line
        .find(':')
        .ok_or_else(|| CalendarError::new("iCalendar property is missing a value"))?;
    let meta = &line[..colon];
    let raw_value = &line[colon + 1..];
    let mut parts = meta.split(';');
    let name = parts
        .next()
        .ok_or_else(|| CalendarError::new("iCalendar property is missing a name"))?
        .to_string();
    let mut params = Vec::new();
    for part in parts {
        let (key, value) = part.split_once('=').unwrap_or((part, ""));
        let value = value.trim_matches('"').to_string();
        params.push((key.to_string(), value));
    }
    Ok(Property {
        name,
        params,
        value: unescape_text(raw_value),
        raw: Some(line.to_string()),
    })
}

fn unescape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn escape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            other => out.push(other),
        }
    }
    out
}

fn write_component(out: &mut String, component: &Component) {
    push_folded(out, &format!("BEGIN:{}", component.name));
    for property in &component.properties {
        if let Some(raw) = property.raw.as_ref() {
            push_folded(out, raw);
        } else {
            push_folded(out, &format_property(property));
        }
    }
    for child in &component.children {
        write_component(out, child);
    }
    push_folded(out, &format!("END:{}", component.name));
}

fn format_property(property: &Property) -> String {
    let mut line = property.name.clone();
    for (key, value) in &property.params {
        if value
            .chars()
            .any(|ch| matches!(ch, ';' | ':' | ',' | ' ' | '"'))
        {
            line.push_str(&format!(";{key}=\"{value}\""));
        } else {
            line.push_str(&format!(";{key}={value}"));
        }
    }
    line.push(':');
    line.push_str(&escape_text(&property.value));
    line
}

fn push_folded(out: &mut String, line: &str) {
    let bytes = line.as_bytes();
    if bytes.len() <= 75 {
        out.push_str(line);
        out.push_str("\r\n");
        return;
    }
    let mut start = 0;
    let mut first = true;
    while start < bytes.len() {
        let budget = if first { 75 } else { 74 };
        let mut end = (start + budget).min(bytes.len());
        while end > start && !line.is_char_boundary(end) {
            end -= 1;
        }
        if end == start {
            end = (start + 1).min(bytes.len());
            while end < bytes.len() && !line.is_char_boundary(end) {
                end += 1;
            }
        }
        if !first {
            out.push(' ');
        }
        out.push_str(&line[start..end]);
        out.push_str("\r\n");
        start = end;
        first = false;
    }
}

// --- DAV ---------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum DavPrecondition {
    None,
    IfMatch(Option<String>),
    IfNoneMatchStar,
}

#[derive(Clone, Debug)]
pub struct DavResource {
    pub href: String,
    pub etag: String,
    pub body: String,
}

#[derive(Clone, Debug, Default)]
pub struct DavListing {
    pub items: Vec<DavItem>,
    pub sync_token: Option<String>,
    pub ctag: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct DavItem {
    pub href: String,
    pub etag: Option<String>,
    pub displayname: Option<String>,
    pub collection: bool,
    pub calendar_order: Option<i32>,
}

#[derive(Debug)]
pub struct DavError {
    status: Option<u16>,
    message: String,
}

impl DavError {
    fn new(status: Option<u16>, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    pub fn status(&self) -> Option<u16> {
        self.status
    }

    fn into_api(self) -> ApiError {
        match self.status {
            Some(401) | Some(403) => {
                ApiError::unavailable("DAV authority rejected the service credential.")
            }
            Some(404) => ApiError::not_found(self.message),
            Some(409) => ApiError::conflict(self.message),
            Some(412) => ApiError::conflict(self.message),
            Some(423) => ApiError::conflict("The DAV resource is locked."),
            _ => ApiError::unavailable(self.message),
        }
    }
}

#[derive(Clone)]
pub enum DavBackend {
    Memory(MemoryDav),
    Client(DavClient),
}

impl DavBackend {
    pub async fn get(&self, href: &str) -> Result<DavResource, DavError> {
        match self {
            Self::Memory(dav) => dav.get(href).map(|response| DavResource {
                href: href.to_string(),
                etag: response.etag.unwrap_or_default(),
                body: response.body,
            }),
            Self::Client(client) => {
                let user_id = user_id_from_href(href)?;
                let parsed = DavHref::parse(href).map_err(shared_dav_error)?;
                let resource = client
                    .get_resource(&user_id, &parsed, kind_from_href(href))
                    .await
                    .map_err(shared_dav_error)?;
                Ok(DavResource {
                    href: resource.href.as_str().to_string(),
                    etag: resource.etag.as_str().to_string(),
                    body: resource.payload.raw().to_string(),
                })
            }
        }
    }

    pub async fn put(
        &self,
        href: &str,
        body: &str,
        precondition: DavPrecondition,
    ) -> Result<String, DavError> {
        match self {
            Self::Memory(dav) => dav
                .put(href, body, precondition)
                .map(|response| response.etag.unwrap_or_default()),
            Self::Client(client) => {
                let user_id = user_id_from_href(href)?;
                let parsed = DavHref::parse(href).map_err(shared_dav_error)?;
                let payload = DavPayload::from_raw(DavMediaType::ICalendar, body)
                    .map_err(shared_dav_error)?;
                let result = client
                    .put_resource(
                        &user_id,
                        &parsed,
                        &payload,
                        shared_precondition(precondition)?,
                    )
                    .await
                    .map_err(shared_dav_error)?;
                Ok(result
                    .etag
                    .map(|etag| etag.as_str().to_string())
                    .unwrap_or_default())
            }
        }
    }

    pub async fn delete(
        &self,
        href: &str,
        precondition: DavPrecondition,
        infinity: bool,
    ) -> Result<(), DavError> {
        match self {
            Self::Memory(dav) => dav.delete(href, precondition),
            Self::Client(client) => {
                let user_id = user_id_from_href(href)?;
                let parsed = DavHref::parse(href).map_err(shared_dav_error)?;
                if infinity {
                    let expected = match precondition {
                        DavPrecondition::IfMatch(Some(etag)) => {
                            Some(ETag::parse(&etag).map_err(shared_dav_error)?)
                        }
                        _ => None,
                    };
                    client
                        .delete_collection(&user_id, &parsed, expected.as_ref())
                        .await
                        .map_err(shared_dav_error)
                } else {
                    let expected = match precondition {
                        DavPrecondition::IfMatch(Some(etag)) => {
                            ETag::parse(&etag).map_err(shared_dav_error)?
                        }
                        _ => {
                            return Err(DavError::new(
                                Some(412),
                                "DELETE requires an If-Match ETag",
                            ));
                        }
                    };
                    match client.delete_resource(&user_id, &parsed, &expected).await {
                        Ok(()) => Ok(()),
                        Err(crate::dav::DavError::NotFound(_)) => Ok(()),
                        Err(error) => Err(shared_dav_error(error)),
                    }
                }
            }
        }
    }

    pub async fn r#move(
        &self,
        from: &str,
        to: &str,
        precondition: DavPrecondition,
    ) -> Result<(), DavError> {
        match self {
            Self::Memory(dav) => dav.r#move(from, to, precondition),
            Self::Client(client) => {
                let user_id = user_id_from_href(from)?;
                let src = DavHref::parse(from).map_err(shared_dav_error)?;
                let dest = DavHref::parse(to).map_err(shared_dav_error)?;
                let expected = match precondition {
                    DavPrecondition::IfMatch(Some(etag)) => {
                        ETag::parse(&etag).map_err(shared_dav_error)?
                    }
                    _ => {
                        return Err(DavError::new(Some(412), "MOVE requires an If-Match ETag"));
                    }
                };
                client
                    .move_resource(&user_id, &src, &dest, &expected)
                    .await
                    .map(|_| ())
                    .map_err(shared_dav_error)
            }
        }
    }

    pub async fn mkcol(
        &self,
        href: &str,
        name: &str,
        order: i32,
        _precondition: DavPrecondition,
    ) -> Result<(), DavError> {
        match self {
            Self::Memory(dav) => dav.mkcol(href, name, order, _precondition),
            Self::Client(client) => {
                let user_id = user_id_from_href(href)?;
                let collection_id = collection_id_from_href(href)?;
                client
                    .create_task_list(
                        &user_id,
                        &NewCalendar {
                            collection_id,
                            display_name: name.to_string(),
                        },
                    )
                    .await
                    .map(|_| ())
                    .map_err(shared_dav_error)?;
                let _ = client
                    .set_display_name(
                        &user_id,
                        &DavHref::parse(href).map_err(shared_dav_error)?,
                        name,
                        Some(order),
                    )
                    .await;
                Ok(())
            }
        }
    }

    pub async fn proppatch(&self, href: &str, name: &str, order: i32) -> Result<(), DavError> {
        match self {
            Self::Memory(dav) => dav.proppatch(href, name, order),
            Self::Client(client) => {
                let user_id = user_id_from_href(href)?;
                let parsed = DavHref::parse(href).map_err(shared_dav_error)?;
                client
                    .set_display_name(&user_id, &parsed, name, Some(order))
                    .await
                    .map_err(shared_dav_error)
            }
        }
    }

    pub async fn propfind(&self, href: &str, depth: u8) -> Result<DavListing, DavError> {
        match self {
            Self::Memory(dav) => dav.propfind(href, depth),
            Self::Client(client) => client_propfind(client, href, depth).await,
        }
    }
}

async fn client_propfind(
    client: &DavClient,
    href: &str,
    depth: u8,
) -> Result<DavListing, DavError> {
    let user_id = user_id_from_href(href)?;
    let parsed = DavHref::parse(href).map_err(shared_dav_error)?;
    if depth == 0 {
        let collection = client
            .load_collection(&user_id, &parsed, kind_from_href(href))
            .await
            .map_err(shared_dav_error)?;
        return Ok(DavListing {
            items: vec![DavItem {
                href: collection.href.as_str().to_string(),
                etag: collection
                    .etag
                    .as_ref()
                    .map(|etag| etag.as_str().to_string()),
                displayname: collection.display_name,
                collection: true,
                calendar_order: None,
            }],
            sync_token: collection
                .sync_token
                .as_ref()
                .map(|token| token.as_str().to_string()),
            ctag: None,
        });
    }
    let page = client
        .sync_collection(&user_id, &parsed, None)
        .await
        .map_err(shared_dav_error)?;
    let mut items = Vec::new();
    if href.contains("/tasks/") && href.matches('/').count() <= 3 {
        let collections = client
            .list_collection(&user_id, &parsed)
            .await
            .map_err(shared_dav_error)?;
        for collection in collections {
            items.push(DavItem {
                href: collection.href.as_str().to_string(),
                etag: collection
                    .etag
                    .as_ref()
                    .map(|etag| etag.as_str().to_string()),
                displayname: collection.display_name,
                collection: true,
                calendar_order: None,
            });
        }
    }
    for change in page.upserts {
        items.push(DavItem {
            href: change.href.as_str().to_string(),
            etag: change.etag.as_ref().map(|etag| etag.as_str().to_string()),
            displayname: None,
            collection: change.href.is_collection(),
            calendar_order: None,
        });
    }
    Ok(DavListing {
        items,
        sync_token: page.sync_token.map(|token| token.as_str().to_string()),
        ctag: None,
    })
}

fn user_id_from_href(href: &str) -> Result<String, DavError> {
    href.trim_start_matches('/')
        .split('/')
        .next()
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| DavError::new(None, "DAV href is missing a user segment"))
}

fn collection_id_from_href(href: &str) -> Result<String, DavError> {
    href.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| DavError::new(None, "DAV href is missing a collection id"))
}

fn kind_from_href(href: &str) -> CollectionKind {
    if href.contains("/addressbooks/") {
        CollectionKind::AddressBook
    } else if href.contains("/tasks/") {
        CollectionKind::TaskList
    } else {
        CollectionKind::Calendar
    }
}

fn shared_precondition(precondition: DavPrecondition) -> Result<PutPrecondition, DavError> {
    match precondition {
        DavPrecondition::IfNoneMatchStar => Ok(PutPrecondition::IfNoneMatchStar),
        DavPrecondition::IfMatch(Some(etag)) => Ok(PutPrecondition::IfMatch(
            ETag::parse(&etag).map_err(shared_dav_error)?,
        )),
        DavPrecondition::IfMatch(None) | DavPrecondition::None => Err(DavError::new(
            Some(412),
            "conditional DAV write is required",
        )),
    }
}

fn shared_dav_error(error: crate::dav::DavError) -> DavError {
    let status = match &error {
        crate::dav::DavError::PreconditionFailed { .. } => Some(412),
        crate::dav::DavError::NotFound(_) => Some(404),
        crate::dav::DavError::Conflict(_) | crate::dav::DavError::OperationConflict => Some(409),
        crate::dav::DavError::Unauthorized => Some(401),
        crate::dav::DavError::Forbidden(_) => Some(403),
        crate::dav::DavError::InvalidRequest(_) => Some(400),
        _ => None,
    };
    DavError::new(status, error.to_string())
}

#[derive(Debug)]
struct HttpExchange {
    etag: Option<String>,
    body: String,
}

#[derive(Clone)]
pub struct MemoryDav {
    inner: Arc<Mutex<MemoryInner>>,
}

#[derive(Default)]
struct MemoryInner {
    next: u64,
    collections: BTreeMap<String, MemoryCollection>,
    resources: BTreeMap<String, MemoryResource>,
}

#[derive(Clone)]
struct MemoryCollection {
    displayname: String,
    order: i32,
    etag: String,
}

#[derive(Clone)]
struct MemoryResource {
    body: String,
    etag: String,
}

impl MemoryDav {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(MemoryInner::default())),
        }
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, MemoryInner>, DavError> {
        self.inner
            .lock()
            .map_err(|_| DavError::new(None, "memory DAV lock poisoned"))
    }

    fn get(&self, href: &str) -> Result<HttpExchange, DavError> {
        let inner = self.lock()?;
        let resource = inner
            .resources
            .get(href)
            .ok_or_else(|| DavError::new(Some(404), "DAV resource not found"))?;
        Ok(HttpExchange {
            etag: Some(resource.etag.clone()),
            body: resource.body.clone(),
        })
    }

    fn put(
        &self,
        href: &str,
        body: &str,
        precondition: DavPrecondition,
    ) -> Result<HttpExchange, DavError> {
        let mut inner = self.lock()?;
        check_resource_precondition(
            inner.resources.get(href).map(|item| item.etag.as_str()),
            &precondition,
        )?;
        if body.len() > MAX_ICAL_BYTES {
            return Err(DavError::new(Some(413), "VTODO payload is too large"));
        }
        let etag = inner.next_etag();
        inner.resources.insert(
            href.to_string(),
            MemoryResource {
                body: body.to_string(),
                etag: etag.clone(),
            },
        );
        Ok(HttpExchange {
            etag: Some(etag),
            body: String::new(),
        })
    }

    fn delete(&self, href: &str, precondition: DavPrecondition) -> Result<(), DavError> {
        let mut inner = self.lock()?;
        if href.ends_with('/') {
            let collection = inner
                .collections
                .get(href)
                .ok_or_else(|| DavError::new(Some(404), "DAV collection not found"))?;
            check_resource_precondition(Some(collection.etag.as_str()), &precondition)?;
            inner.collections.remove(href);
            inner.resources.retain(|key, _| !key.starts_with(href));
            return Ok(());
        }
        let resource = inner
            .resources
            .get(href)
            .ok_or_else(|| DavError::new(Some(404), "DAV resource not found"))?;
        check_resource_precondition(Some(resource.etag.as_str()), &precondition)?;
        inner.resources.remove(href);
        Ok(())
    }

    fn r#move(&self, from: &str, to: &str, precondition: DavPrecondition) -> Result<(), DavError> {
        let mut inner = self.lock()?;
        if inner.resources.contains_key(to) {
            return Err(DavError::new(Some(412), "destination already exists"));
        }
        let resource = inner
            .resources
            .get(from)
            .ok_or_else(|| DavError::new(Some(404), "DAV resource not found"))?
            .clone();
        check_resource_precondition(Some(resource.etag.as_str()), &precondition)?;
        inner.resources.remove(from);
        inner.resources.insert(to.to_string(), resource);
        Ok(())
    }

    fn mkcol(
        &self,
        href: &str,
        name: &str,
        order: i32,
        _precondition: DavPrecondition,
    ) -> Result<(), DavError> {
        let mut inner = self.lock()?;
        if inner.collections.contains_key(href) {
            return Err(DavError::new(Some(405), "collection exists"));
        }
        let etag = inner.next_etag();
        inner.collections.insert(
            normalize_collection(href),
            MemoryCollection {
                displayname: name.to_string(),
                order,
                etag,
            },
        );
        Ok(())
    }

    fn proppatch(&self, href: &str, name: &str, order: i32) -> Result<(), DavError> {
        let mut inner = self.lock()?;
        let href = normalize_collection(href);
        if !inner.collections.contains_key(&href) {
            return Err(DavError::new(Some(404), "DAV collection not found"));
        }
        inner.next += 1;
        let etag = format!("\"mem-{}\"", inner.next);
        let collection = inner.collections.get_mut(&href).expect("collection exists");
        collection.displayname = name.to_string();
        collection.order = order;
        collection.etag = etag;
        Ok(())
    }

    fn propfind(&self, href: &str, depth: u8) -> Result<DavListing, DavError> {
        let inner = self.lock()?;
        let href = if inner.collections.contains_key(href) || href.ends_with('/') {
            normalize_collection(href)
        } else {
            href.to_string()
        };
        let mut items = Vec::new();
        if let Some(collection) = inner.collections.get(&href) {
            items.push(DavItem {
                href: href.clone(),
                etag: Some(collection.etag.clone()),
                displayname: Some(collection.displayname.clone()),
                collection: true,
                calendar_order: Some(collection.order),
            });
            if depth > 0 {
                for (child, collection) in &inner.collections {
                    if child != &href && child.starts_with(&href) {
                        let rest = &child[href.len()..];
                        if rest.matches('/').count() <= 1 {
                            items.push(DavItem {
                                href: child.clone(),
                                etag: Some(collection.etag.clone()),
                                displayname: Some(collection.displayname.clone()),
                                collection: true,
                                calendar_order: Some(collection.order),
                            });
                        }
                    }
                }
                for (child, resource) in &inner.resources {
                    if child.starts_with(&href) {
                        items.push(DavItem {
                            href: child.clone(),
                            etag: Some(resource.etag.clone()),
                            displayname: None,
                            collection: false,
                            calendar_order: None,
                        });
                    }
                }
            }
            return Ok(DavListing {
                items,
                sync_token: Some(format!("sync-{}", inner.next)),
                ctag: Some(format!("ctag-{}", inner.next)),
            });
        }
        if let Some(resource) = inner.resources.get(&href) {
            items.push(DavItem {
                href,
                etag: Some(resource.etag.clone()),
                displayname: None,
                collection: false,
                calendar_order: None,
            });
            return Ok(DavListing {
                items,
                sync_token: None,
                ctag: None,
            });
        }
        Err(DavError::new(Some(404), "DAV path not found"))
    }
}

impl MemoryInner {
    fn next_etag(&mut self) -> String {
        self.next += 1;
        format!("\"mem-{}\"", self.next)
    }
}

fn check_resource_precondition(
    current: Option<&str>,
    precondition: &DavPrecondition,
) -> Result<(), DavError> {
    match precondition {
        DavPrecondition::None => Ok(()),
        DavPrecondition::IfNoneMatchStar if current.is_some() => {
            Err(DavError::new(Some(412), "resource already exists"))
        }
        DavPrecondition::IfNoneMatchStar => Ok(()),
        DavPrecondition::IfMatch(expected) => match (current, expected.as_deref()) {
            (Some(actual), Some(expected)) if actual == expected => Ok(()),
            (Some(_), Some(_)) => Err(DavError::new(Some(412), "stale DAV ETag")),
            (None, _) => Err(DavError::new(Some(412), "DAV resource missing")),
            (Some(_), None) => Ok(()),
        },
    }
}

fn normalize_collection(href: &str) -> String {
    if href.ends_with('/') {
        href.to_string()
    } else {
        format!("{href}/")
    }
}

fn dav_from_state(state: &AppState) -> ApiResult<DavBackend> {
    if let Some(dav) = DAV_OVERRIDE.lock().expect("DAV override lock").clone() {
        return Ok(dav);
    }
    Ok(DavBackend::Client(state.dav_client()?.clone()))
}

pub fn install_dav_backend(dav: DavBackend) {
    *DAV_OVERRIDE.lock().expect("DAV override lock") = Some(dav);
}

pub fn clear_dav_backend() {
    *DAV_OVERRIDE.lock().expect("DAV override lock") = None;
}

pub fn memory_backend() -> DavBackend {
    DavBackend::Memory(MemoryDav::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vtodo_round_trip_preserves_markdown_and_unknown_properties() {
        let markdown =
            "# Heading\n\nKeep <script>alert(1)</script> and **bold** losslessly.\n\n- item\n";
        let ics = format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Other//Client//EN\r\nBEGIN:VTODO\r\nUID:abc-123\r\nSUMMARY:Old\r\nDESCRIPTION:{}\r\nX-UNKNOWN-CLIENT:keep-me\r\nPRIORITY:5\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nDESCRIPTION:Ping\r\nTRIGGER:-PT15M\r\nEND:VALARM\r\nEND:VTODO\r\nEND:VCALENDAR\r\n",
            escape_text(markdown)
        );
        let mut calendar = parse_calendar(&ics).unwrap();
        patch_todo(
            &mut calendar,
            &TodoPatch {
                title: Some("Renamed".into()),
                description: Some(markdown.into()),
                due: Some(Some(Due {
                    local: "2026-08-15T18:00:00".into(),
                    time_zone: Some("America/New_York".into()),
                    all_day: false,
                    at: None,
                })),
                ..TodoPatch::default()
            },
        )
        .unwrap();
        let serialized = serialize_calendar(&calendar);
        assert!(serialized.contains("X-UNKNOWN-CLIENT:keep-me"));
        assert!(serialized.contains("BEGIN:VALARM"));
        assert!(serialized.contains("PRODID:-//Other//Client//EN"));
        assert!(serialized.contains("TZID=America/New_York"));
        let fields = extract_todo(&parse_calendar(&serialized).unwrap()).unwrap();
        assert_eq!(fields.title, "Renamed");
        assert_eq!(fields.description, markdown);
        assert_eq!(
            fields.due.as_ref().map(|due| due.local.as_str()),
            Some("2026-08-15T18:00:00")
        );
        assert_eq!(
            fields.due.as_ref().and_then(|due| due.time_zone.as_deref()),
            Some("America/New_York")
        );
    }

    #[test]
    fn all_day_and_utc_due_values_round_trip() {
        let mut calendar = new_todo_calendar(&TodoFields {
            uid: "11111111-1111-1111-1111-111111111111".into(),
            title: "Due".into(),
            description: String::new(),
            due: Some(Due {
                local: "2026-08-20".into(),
                time_zone: None,
                all_day: true,
                at: None,
            }),
            priority: 1,
            completed: false,
            completed_at: None,
            position: 3,
            operation_id: None,
        });
        let all_day = extract_todo(&calendar).unwrap().due.unwrap();
        assert!(all_day.all_day);
        assert_eq!(all_day.local, "2026-08-20");
        patch_todo(
            &mut calendar,
            &TodoPatch {
                due: Some(Some(Due {
                    local: "2026-08-20T13:30:00".into(),
                    time_zone: Some("UTC".into()),
                    all_day: false,
                    at: None,
                })),
                ..TodoPatch::default()
            },
        )
        .unwrap();
        let utc = extract_todo(&calendar).unwrap().due.unwrap();
        assert!(!utc.all_day);
        assert_eq!(utc.time_zone.as_deref(), Some("UTC"));
        assert_eq!(utc.at.unwrap().to_rfc3339(), "2026-08-20T13:30:00+00:00");
    }

    #[test]
    fn complete_and_reopen_preserve_unknown_fields() {
        let mut calendar = parse_calendar(
            "BEGIN:VCALENDAR\nBEGIN:VTODO\nUID:u1\nSUMMARY:Pay\nX-CUSTOM:1\nSTATUS:NEEDS-ACTION\nEND:VTODO\nEND:VCALENDAR\n",
        )
        .unwrap();
        patch_todo(
            &mut calendar,
            &TodoPatch {
                completed: Some(true),
                ..TodoPatch::default()
            },
        )
        .unwrap();
        let completed = extract_todo(&calendar).unwrap();
        assert!(completed.completed);
        assert!(serialize_calendar(&calendar).contains("X-CUSTOM:1"));
        patch_todo(
            &mut calendar,
            &TodoPatch {
                completed: Some(false),
                ..TodoPatch::default()
            },
        )
        .unwrap();
        let reopened = extract_todo(&calendar).unwrap();
        assert!(!reopened.completed);
        assert!(serialize_calendar(&calendar).contains("STATUS:NEEDS-ACTION"));
    }

    #[test]
    fn due_validation_rejects_malformed_inputs() {
        assert!(
            validate_due(&Due {
                local: "08/15/2026".into(),
                time_zone: None,
                all_day: true,
                at: None,
            })
            .is_err()
        );
        assert!(
            validate_due(&Due {
                local: "2026-08-15T18:00:00".into(),
                time_zone: Some("not a zone".into()),
                all_day: false,
                at: None,
            })
            .is_err()
        );
        assert!(validate_description(&"a".repeat(MAX_DESCRIPTION_BYTES + 1)).is_err());
        assert!(validate_task_title("").is_err());
    }

    #[tokio::test]
    async fn memory_dav_stale_etag_does_not_overwrite() {
        let dav = MemoryDav::new();
        dav.mkcol("/u/tasks/l/", "Inbox", 0, DavPrecondition::None)
            .unwrap();
        let first = dav
            .put(
                "/u/tasks/l/a.ics",
                "BEGIN:VCALENDAR\nEND:VCALENDAR\n",
                DavPrecondition::IfNoneMatchStar,
            )
            .unwrap();
        let conflict = dav
            .put(
                "/u/tasks/l/a.ics",
                "CHANGED",
                DavPrecondition::IfMatch(Some("\"stale\"".into())),
            )
            .unwrap_err();
        assert_eq!(conflict.status(), Some(412));
        let current = dav.get("/u/tasks/l/a.ics").unwrap();
        assert_eq!(current.etag, first.etag);
        assert!(current.body.contains("VCALENDAR"));
    }
}
