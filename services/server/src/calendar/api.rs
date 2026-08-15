//! Production Calendar HTTP surface. Radicale is canonical; PostgreSQL rows
//! are rebuildable projections written only after conditional DAV success.

use std::future::Future;
use std::pin::Pin;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool, Postgres, Transaction};

use crate::AppState;
use crate::auth::Principal;
use crate::calendar::{
    CalendarError, CreateCalendar, Date, DeleteCommand, EventDraft, EventRecord, MoveEvent,
    RenameCalendar, UpdateEvent, event_from_ical, expand_event, new_event_document, parse_ical,
    parse_uuid, patch_event_document, serialize_ical, unix_now,
};
use crate::dav::{
    CollectionKind, DavHref, DavMediaType, DavPayload, ETag, NewCalendar, PutPrecondition,
    SyncToken,
};
use crate::error::{ApiError, ApiResult};

const MAX_CALENDARS: i64 = 64;
const MAX_EVENTS: i64 = 4096;

#[derive(Clone, Debug, Serialize, Deserialize, FromRow, PartialEq, Eq)]
pub struct Calendar {
    pub id: String,
    #[serde(rename = "userId")]
    pub user_id: String,
    pub uid: String,
    pub href: String,
    pub etag: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    pub description: String,
    pub color: Option<String>,
    pub revision: i64,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
    #[serde(rename = "updatedAt")]
    pub updated_at: DateTime<Utc>,
    #[serde(rename = "deletedAt")]
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromRow, PartialEq, Eq)]
pub struct Event {
    pub id: String,
    #[serde(rename = "userId")]
    pub user_id: String,
    #[serde(rename = "calendarId")]
    pub calendar_id: String,
    pub uid: String,
    pub href: String,
    pub etag: String,
    pub summary: String,
    pub description: String,
    pub location: String,
    #[serde(rename = "allDay")]
    pub all_day: bool,
    pub dtstart: String,
    pub dtend: Option<String>,
    pub tzid: Option<String>,
    pub rrule: Option<String>,
    pub exdates: String,
    pub revision: i64,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
    #[serde(rename = "updatedAt")]
    pub updated_at: DateTime<Utc>,
    #[serde(rename = "deletedAt")]
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct CalendarList {
    pub calendars: Vec<Calendar>,
}

#[derive(Debug, Serialize)]
pub struct EventList {
    pub events: Vec<Event>,
}

#[derive(Debug, Serialize)]
pub struct OccurrenceList {
    pub occurrences: Vec<crate::calendar::Occurrence>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateCalendarRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    pub id: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    pub color: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RenameCalendarRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    #[serde(rename = "expectedRevision")]
    pub expected_revision: i64,
    #[serde(rename = "expectedEtag")]
    pub expected_etag: Option<String>,
    #[serde(rename = "displayName")]
    pub display_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    #[serde(rename = "expectedRevision")]
    pub expected_revision: i64,
    #[serde(rename = "expectedEtag")]
    pub expected_etag: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateEventRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    pub id: String,
    #[serde(rename = "calendarId")]
    pub calendar_id: String,
    pub uid: Option<String>,
    #[serde(flatten)]
    pub draft: EventDraftBody,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateEventRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    #[serde(rename = "expectedRevision")]
    pub expected_revision: i64,
    #[serde(rename = "expectedEtag")]
    pub expected_etag: Option<String>,
    #[serde(flatten)]
    pub draft: EventDraftBody,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MoveEventRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    #[serde(rename = "expectedRevision")]
    pub expected_revision: i64,
    #[serde(rename = "expectedEtag")]
    pub expected_etag: Option<String>,
    #[serde(rename = "calendarId")]
    pub calendar_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EventDraftBody {
    pub summary: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub location: String,
    #[serde(rename = "allDay")]
    pub all_day: bool,
    pub dtstart: String,
    pub dtend: Option<String>,
    pub tzid: Option<String>,
    pub rrule: Option<String>,
    #[serde(default)]
    pub exdates: Vec<String>,
}

impl EventDraftBody {
    fn into_draft(self) -> EventDraft {
        EventDraft {
            summary: self.summary,
            description: self.description,
            location: self.location,
            all_day: self.all_day,
            dtstart: self.dtstart,
            dtend: self.dtend,
            tzid: self.tzid,
            rrule: self.rrule,
            exdates: self.exdates,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct EventListQuery {
    #[serde(rename = "calendarId")]
    pub calendar_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ExpandQuery {
    #[serde(rename = "calendarId")]
    pub calendar_id: Option<String>,
    pub start: String,
    pub end: String,
    pub limit: Option<usize>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/calendars", get(list_calendars).post(create_calendar))
        .route("/v1/calendars/{calendarId}", get(get_calendar))
        .route("/v1/calendars/{calendarId}/rename", post(rename_calendar))
        .route("/v1/calendars/{calendarId}/delete", post(delete_calendar))
        .route("/v1/events", get(list_events).post(create_event))
        .route("/v1/events/expand", get(expand_events))
        .route("/v1/events/{eventId}", get(get_event))
        .route("/v1/events/{eventId}/update", post(update_event))
        .route("/v1/events/{eventId}/move", post(move_event))
        .route("/v1/events/{eventId}/delete", post(delete_event))
}

pub async fn list_calendars(
    State(state): State<AppState>,
    principal: Principal,
) -> ApiResult<Json<CalendarList>> {
    let calendars = sqlx::query_as::<_, Calendar>(
        "SELECT id, user_id, uid, href, etag, display_name, description, color, revision,
                created_at, updated_at, deleted_at
         FROM calendar_calendars
         WHERE user_id = $1 AND deleted_at IS NULL
         ORDER BY display_name, id",
    )
    .bind(&principal.user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(database_error)?;
    Ok(Json(CalendarList { calendars }))
}

pub async fn get_calendar(
    State(state): State<AppState>,
    principal: Principal,
    Path(calendar_id): Path<String>,
) -> ApiResult<Json<Calendar>> {
    visible_calendar(
        &principal.user_id,
        load_calendar(&state.pool, &calendar_id).await?,
    )
    .map(Json)
}

pub async fn create_calendar(
    State(state): State<AppState>,
    principal: Principal,
    Json(request): Json<CreateCalendarRequest>,
) -> ApiResult<Json<Calendar>> {
    let request_body = operation_request(&request)?;
    let operation_id = map_cal(parse_uuid("operationId", &request.operation_id))?;
    let id = map_cal(parse_uuid("id", &request.id))?;
    let command = CreateCalendar {
        operation_id: operation_id.clone(),
        id: id.clone(),
        display_name: request.display_name.clone(),
        description: request.description.clone(),
        color: request.color.clone(),
    };
    let user_id = principal.user_id.clone();
    let dav = state.dav_client()?.clone();
    with_operation(
        &state.pool,
        OperationBinding {
            user_id: user_id.clone(),
            operation_id,
            entity_type: "calendar",
            entity_id: id.clone(),
            operation: "create",
            request_body,
        },
        |tx| {
            Box::pin(async move {
                if let Some(existing) = load_calendar_tx(tx, &id).await? {
                    return Err(existing_identity(
                        &user_id,
                        existing.user_id,
                        existing.deleted_at.is_some(),
                    ));
                }
                ensure_calendar_quota(tx, &user_id).await?;
                let created = dav
                    .create_calendar(
                        &user_id,
                        &NewCalendar {
                            collection_id: id.clone(),
                            display_name: command.display_name.clone(),
                        },
                    )
                    .await
                    .map_err(map_dav)?;
                let now = Utc::now();
                sqlx::query(
                    "INSERT INTO calendar_calendars
                        (id, user_id, uid, href, etag, display_name, description, color, ctag,
                         sync_token, revision, created_at, updated_at, deleted_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NULL, NULL, 1, $9, $9, NULL)",
                )
                .bind(&id)
                .bind(&user_id)
                .bind(&id)
                .bind(created.href.as_str())
                .bind(created.etag.as_ref().map(ETag::as_str).unwrap_or("\"1\""))
                .bind(&command.display_name)
                .bind(&command.description)
                .bind(&command.color)
                .bind(now)
                .execute(&mut **tx)
                .await
                .map_err(database_error)?;
                load_required_calendar(tx, &id).await
            })
        },
    )
    .await
    .map(Json)
}

pub async fn rename_calendar(
    State(state): State<AppState>,
    principal: Principal,
    Path(calendar_id): Path<String>,
    Json(request): Json<RenameCalendarRequest>,
) -> ApiResult<Json<Calendar>> {
    let request_body = operation_request(&request)?;
    let operation_id = map_cal(parse_uuid("operationId", &request.operation_id))?;
    let calendar_id = map_cal(parse_uuid("calendarId", &calendar_id))?;
    let command = RenameCalendar {
        operation_id: operation_id.clone(),
        expected_revision: request.expected_revision,
        expected_etag: request.expected_etag.clone(),
        display_name: request.display_name.clone(),
    };
    let user_id = principal.user_id.clone();
    let dav = state.dav_client()?.clone();
    with_operation(
        &state.pool,
        OperationBinding {
            user_id: user_id.clone(),
            operation_id,
            entity_type: "calendar",
            entity_id: calendar_id.clone(),
            operation: "rename",
            request_body,
        },
        |tx| {
            Box::pin(async move {
                let current =
                    locked_live_calendar(tx, &user_id, &calendar_id, command.expected_revision)
                        .await?;
                let href = DavHref::parse(&current.href).map_err(map_dav)?;
                dav.set_display_name(&user_id, &href, &command.display_name, None)
                    .await
                    .map_err(map_dav)?;
                sqlx::query(
                    "UPDATE calendar_calendars
                     SET display_name = $2, revision = revision + 1, updated_at = $3
                     WHERE id = $1",
                )
                .bind(&current.id)
                .bind(&command.display_name)
                .bind(Utc::now())
                .execute(&mut **tx)
                .await
                .map_err(database_error)?;
                load_required_calendar(tx, &current.id).await
            })
        },
    )
    .await
    .map(Json)
}

pub async fn delete_calendar(
    State(state): State<AppState>,
    principal: Principal,
    Path(calendar_id): Path<String>,
    Json(request): Json<DeleteRequest>,
) -> ApiResult<Json<Calendar>> {
    let request_body = operation_request(&request)?;
    let operation_id = map_cal(parse_uuid("operationId", &request.operation_id))?;
    let calendar_id = map_cal(parse_uuid("calendarId", &calendar_id))?;
    let command = DeleteCommand {
        operation_id: operation_id.clone(),
        expected_revision: request.expected_revision,
        expected_etag: request.expected_etag.clone(),
    };
    let user_id = principal.user_id.clone();
    let dav = state.dav_client()?.clone();
    with_operation(
        &state.pool,
        OperationBinding {
            user_id: user_id.clone(),
            operation_id,
            entity_type: "calendar",
            entity_id: calendar_id.clone(),
            operation: "delete",
            request_body,
        },
        |tx| {
            Box::pin(async move {
                let current =
                    locked_live_calendar(tx, &user_id, &calendar_id, command.expected_revision)
                        .await?;
                let live = sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM calendar_events
                     WHERE user_id = $1 AND calendar_id = $2 AND deleted_at IS NULL",
                )
                .bind(&user_id)
                .bind(&current.id)
                .fetch_one(&mut **tx)
                .await
                .map_err(database_error)?;
                if live > 0 {
                    return Err(ApiError::conflict(
                        "A calendar can be deleted only when it has no live events.",
                    ));
                }
                let href = DavHref::parse(&current.href).map_err(map_dav)?;
                let expected = command
                    .expected_etag
                    .as_deref()
                    .or(Some(current.etag.as_str()))
                    .map(ETag::parse)
                    .transpose()
                    .map_err(map_dav)?;
                dav.delete_collection(&user_id, &href, expected.as_ref())
                    .await
                    .map_err(map_dav)?;
                sqlx::query(
                    "UPDATE calendar_calendars
                     SET deleted_at = NOW(), revision = revision + 1, updated_at = NOW()
                     WHERE id = $1",
                )
                .bind(&current.id)
                .execute(&mut **tx)
                .await
                .map_err(database_error)?;
                load_required_calendar(tx, &current.id).await
            })
        },
    )
    .await
    .map(Json)
}

pub async fn list_events(
    State(state): State<AppState>,
    principal: Principal,
    Query(query): Query<EventListQuery>,
) -> ApiResult<Json<EventList>> {
    let events = if let Some(calendar_id) = query.calendar_id.as_deref() {
        sqlx::query_as::<_, Event>(
            "SELECT id, user_id, calendar_id, uid, href, etag, summary, description, location,
                    all_day, dtstart, dtend, tzid, rrule, exdates, revision, created_at, updated_at, deleted_at
             FROM calendar_events
             WHERE user_id = $1 AND calendar_id = $2 AND deleted_at IS NULL
             ORDER BY dtstart, id",
        )
        .bind(&principal.user_id)
        .bind(calendar_id)
        .fetch_all(&state.pool)
        .await
        .map_err(database_error)?
    } else {
        sqlx::query_as::<_, Event>(
            "SELECT id, user_id, calendar_id, uid, href, etag, summary, description, location,
                    all_day, dtstart, dtend, tzid, rrule, exdates, revision, created_at, updated_at, deleted_at
             FROM calendar_events
             WHERE user_id = $1 AND deleted_at IS NULL
             ORDER BY dtstart, id",
        )
        .bind(&principal.user_id)
        .fetch_all(&state.pool)
        .await
        .map_err(database_error)?
    };
    Ok(Json(EventList { events }))
}

pub async fn get_event(
    State(state): State<AppState>,
    principal: Principal,
    Path(event_id): Path<String>,
) -> ApiResult<Json<Event>> {
    visible_event(
        &principal.user_id,
        load_event(&state.pool, &event_id).await?,
    )
    .map(Json)
}

pub async fn create_event(
    State(state): State<AppState>,
    principal: Principal,
    Json(request): Json<CreateEventRequest>,
) -> ApiResult<Json<Event>> {
    let request_body = operation_request(&request)?;
    let operation_id = map_cal(parse_uuid("operationId", &request.operation_id))?;
    let id = map_cal(parse_uuid("id", &request.id))?;
    let calendar_id = map_cal(parse_uuid("calendarId", &request.calendar_id))?;
    let uid = request
        .uid
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| id.clone());
    let draft = request.draft.into_draft();
    let user_id = principal.user_id.clone();
    let dav = state.dav_client()?.clone();
    with_operation(
        &state.pool,
        OperationBinding {
            user_id: user_id.clone(),
            operation_id,
            entity_type: "event",
            entity_id: id.clone(),
            operation: "create",
            request_body,
        },
        |tx| {
            Box::pin(async move {
                if let Some(existing) = load_event_tx(tx, &id).await? {
                    return Err(existing_identity(
                        &user_id,
                        existing.user_id,
                        existing.deleted_at.is_some(),
                    ));
                }
                let calendar = ensure_live_calendar(tx, &user_id, &calendar_id).await?;
                ensure_event_quota(tx, &user_id).await?;
                let href = format!("{}{}.ics", calendar.href, uid);
                let document = map_cal(new_event_document(&uid, &draft, unix_now()))?;
                let body = serialize_ical(&document);
                let parsed_href = DavHref::parse(&href).map_err(map_dav)?;
                let payload =
                    DavPayload::from_raw(DavMediaType::ICalendar, &body).map_err(map_dav)?;
                let write = dav
                    .put_resource(
                        &user_id,
                        &parsed_href,
                        &payload,
                        PutPrecondition::IfNoneMatchStar,
                    )
                    .await
                    .map_err(map_dav)?;
                let etag = write
                    .etag
                    .as_ref()
                    .map(ETag::as_str)
                    .unwrap_or("\"1\"")
                    .to_string();
                insert_event_projection(
                    tx,
                    &id,
                    &user_id,
                    &calendar_id,
                    &uid,
                    &href,
                    &etag,
                    &body,
                    1,
                )
                .await
            })
        },
    )
    .await
    .map(Json)
}

pub async fn update_event(
    State(state): State<AppState>,
    principal: Principal,
    Path(event_id): Path<String>,
    Json(request): Json<UpdateEventRequest>,
) -> ApiResult<Json<Event>> {
    let request_body = operation_request(&request)?;
    let operation_id = map_cal(parse_uuid("operationId", &request.operation_id))?;
    let event_id = map_cal(parse_uuid("eventId", &event_id))?;
    let command = UpdateEvent {
        operation_id: operation_id.clone(),
        expected_revision: request.expected_revision,
        expected_etag: request.expected_etag.clone(),
        draft: request.draft.into_draft(),
    };
    let user_id = principal.user_id.clone();
    let dav = state.dav_client()?.clone();
    with_operation(
        &state.pool,
        OperationBinding {
            user_id: user_id.clone(),
            operation_id,
            entity_type: "event",
            entity_id: event_id.clone(),
            operation: "update",
            request_body,
        },
        |tx| {
            Box::pin(async move {
                let current =
                    locked_live_event(tx, &user_id, &event_id, command.expected_revision).await?;
                if let Some(expected) = command.expected_etag.as_deref()
                    && normalize_etag(expected) != normalize_etag(&current.etag)
                {
                    return Err(ApiError::stale_etag(expected, Some(current.etag)));
                }
                let payload = load_payload(tx, &current.id).await?;
                let mut document = map_cal(parse_ical(&payload))?;
                map_cal(patch_event_document(
                    &mut document,
                    &command.draft,
                    unix_now(),
                ))?;
                let body = serialize_ical(&document);
                let href = DavHref::parse(&current.href).map_err(map_dav)?;
                let dav_payload =
                    DavPayload::from_raw(DavMediaType::ICalendar, &body).map_err(map_dav)?;
                let write = dav
                    .put_resource(
                        &user_id,
                        &href,
                        &dav_payload,
                        PutPrecondition::IfMatch(ETag::parse(&current.etag).map_err(map_dav)?),
                    )
                    .await
                    .map_err(map_dav)?;
                let etag = write
                    .etag
                    .as_ref()
                    .map(ETag::as_str)
                    .unwrap_or(current.etag.as_str())
                    .to_string();
                replace_event_projection(
                    tx,
                    &current.id,
                    &current.user_id,
                    &current.calendar_id,
                    &current.uid,
                    &current.href,
                    &etag,
                    &body,
                    current.revision + 1,
                    current.created_at,
                )
                .await
            })
        },
    )
    .await
    .map(Json)
}

pub async fn move_event(
    State(state): State<AppState>,
    principal: Principal,
    Path(event_id): Path<String>,
    Json(request): Json<MoveEventRequest>,
) -> ApiResult<Json<Event>> {
    let request_body = operation_request(&request)?;
    let operation_id = map_cal(parse_uuid("operationId", &request.operation_id))?;
    let event_id = map_cal(parse_uuid("eventId", &event_id))?;
    let command = MoveEvent {
        operation_id: operation_id.clone(),
        expected_revision: request.expected_revision,
        expected_etag: request.expected_etag.clone(),
        calendar_id: request.calendar_id.clone(),
    };
    let calendar_id = map_cal(parse_uuid("calendarId", &command.calendar_id))?;
    let user_id = principal.user_id.clone();
    let dav = state.dav_client()?.clone();
    with_operation(
        &state.pool,
        OperationBinding {
            user_id: user_id.clone(),
            operation_id,
            entity_type: "event",
            entity_id: event_id.clone(),
            operation: "move",
            request_body,
        },
        |tx| {
            Box::pin(async move {
                let current = locked_live_event(tx, &user_id, &event_id, command.expected_revision).await?;
                let calendar = ensure_live_calendar(tx, &user_id, &calendar_id).await?;
                let destination = format!("{}{}.ics", calendar.href, current.uid);
                let src = DavHref::parse(&current.href).map_err(map_dav)?;
                let dest = DavHref::parse(&destination).map_err(map_dav)?;
                let expected = ETag::parse(
                    command
                        .expected_etag
                        .as_deref()
                        .unwrap_or(current.etag.as_str()),
                )
                .map_err(map_dav)?;
                let write = dav
                    .move_resource(&user_id, &src, &dest, &expected)
                    .await
                    .map_err(map_dav)?;
                let etag = write
                    .etag
                    .as_ref()
                    .map(ETag::as_str)
                    .unwrap_or(current.etag.as_str())
                    .to_string();
                sqlx::query(
                    "UPDATE calendar_events
                     SET calendar_id = $2, href = $3, etag = $4, revision = revision + 1, updated_at = $5
                     WHERE id = $1",
                )
                .bind(&current.id)
                .bind(&calendar.id)
                .bind(write.href.as_str())
                .bind(&etag)
                .bind(Utc::now())
                .execute(&mut **tx)
                .await
                .map_err(database_error)?;
                load_required_event(tx, &current.id).await
            })
        },
    )
    .await
    .map(Json)
}

pub async fn delete_event(
    State(state): State<AppState>,
    principal: Principal,
    Path(event_id): Path<String>,
    Json(request): Json<DeleteRequest>,
) -> ApiResult<Json<Event>> {
    let request_body = operation_request(&request)?;
    let operation_id = map_cal(parse_uuid("operationId", &request.operation_id))?;
    let event_id = map_cal(parse_uuid("eventId", &event_id))?;
    let user_id = principal.user_id.clone();
    let dav = state.dav_client()?.clone();
    with_operation(
        &state.pool,
        OperationBinding {
            user_id: user_id.clone(),
            operation_id,
            entity_type: "event",
            entity_id: event_id.clone(),
            operation: "delete",
            request_body,
        },
        |tx| {
            Box::pin(async move {
                let current =
                    locked_live_event(tx, &user_id, &event_id, request.expected_revision).await?;
                let href = DavHref::parse(&current.href).map_err(map_dav)?;
                let expected = ETag::parse(
                    request
                        .expected_etag
                        .as_deref()
                        .unwrap_or(current.etag.as_str()),
                )
                .map_err(map_dav)?;
                match dav.delete_resource(&user_id, &href, &expected).await {
                    Ok(()) => {}
                    Err(crate::dav::DavError::NotFound(_)) => {}
                    Err(error) => return Err(map_dav(error)),
                }
                sqlx::query(
                    "UPDATE calendar_events
                     SET deleted_at = NOW(), revision = revision + 1, updated_at = NOW()
                     WHERE id = $1",
                )
                .bind(&current.id)
                .execute(&mut **tx)
                .await
                .map_err(database_error)?;
                load_required_event(tx, &current.id).await
            })
        },
    )
    .await
    .map(Json)
}

pub async fn expand_events(
    State(state): State<AppState>,
    principal: Principal,
    Query(query): Query<ExpandQuery>,
) -> ApiResult<Json<OccurrenceList>> {
    let start = map_cal(Date::parse_ical(&query.start.replace('-', "")))?;
    let end = map_cal(Date::parse_ical(&query.end.replace('-', "")))?;
    let limit = query
        .limit
        .unwrap_or(crate::calendar::MAX_EXPANSION_INSTANCES);
    let events = list_events(
        State(state.clone()),
        principal,
        Query(EventListQuery {
            calendar_id: query.calendar_id,
        }),
    )
    .await?
    .0
    .events;
    let mut occurrences = Vec::new();
    for event in events {
        let record = event_to_record(&event);
        occurrences.extend(map_cal(expand_event(&record, start, end, limit))?);
    }
    occurrences.sort_by(|a, b| {
        a.start_local
            .cmp(&b.start_local)
            .then(a.event_id.cmp(&b.event_id))
    });
    occurrences.truncate(limit);
    Ok(Json(OccurrenceList { occurrences }))
}

pub async fn reconcile_user(state: &AppState, user_id: &str) -> Result<(), String> {
    let client = match state.dav.as_ref() {
        Some(client) => client,
        None => return Ok(()),
    };
    let discovered = client
        .discover(user_id)
        .await
        .map_err(|error| error.to_string())?;
    for collection in discovered
        .collections
        .into_iter()
        .filter(|collection| collection.kind == CollectionKind::Calendar)
    {
        let collection_id = collection
            .href
            .as_str()
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("calendar")
            .to_string();
        let remembered = state
            .projector
            .remember_collection(user_id, &collection_id, &collection)
            .await
            .map_err(|error| error.to_string())?;
        let plan = match state
            .projector
            .plan_sync(client, user_id, &collection.href)
            .await
        {
            Ok(plan) => plan,
            Err(error) => {
                let _ = state
                    .projector
                    .record_collection_error(user_id, &collection.href, &error)
                    .await;
                continue;
            }
        };
        let mut tx = state
            .pool
            .begin()
            .await
            .map_err(|error| error.to_string())?;
        if plan.page.token_reset {
            sqlx::query(
                "UPDATE calendar_events
                 SET deleted_at = NOW(), revision = revision + 1, updated_at = NOW()
                 WHERE user_id = $1 AND calendar_id = $2 AND deleted_at IS NULL",
            )
            .bind(user_id)
            .bind(&remembered.collection_id)
            .execute(&mut *tx)
            .await
            .map_err(|error| error.to_string())?;
        }
        upsert_calendar_projection(&mut tx, user_id, &remembered.collection_id, &collection)
            .await
            .map_err(|error| error.to_string())?;
        for fetch in &plan.resources {
            match &fetch.result {
                Ok(resource) => {
                    if let Err(CalendarError::Parse(_)) = event_from_ical(
                        resource.href.as_str(),
                        user_id,
                        &remembered.collection_id,
                        resource.href.as_str(),
                        resource.etag.as_str(),
                        1,
                        &Utc::now().to_rfc3339(),
                        &Utc::now().to_rfc3339(),
                        resource.payload.raw(),
                    ) {
                        continue;
                    }
                    let id = stable_event_id(user_id, resource.href.as_str());
                    insert_event_projection(
                        &mut tx,
                        &id,
                        user_id,
                        &remembered.collection_id,
                        resource.payload.uid().as_deref().unwrap_or(&id),
                        resource.href.as_str(),
                        resource.etag.as_str(),
                        resource.payload.raw(),
                        1,
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                }
                Err(error) => {
                    tracing::warn!(href = %fetch.href, error = %error, "skipping malformed calendar resource");
                }
            }
        }
        for href in &plan.page.deletions {
            sqlx::query(
                "UPDATE calendar_events
                 SET deleted_at = NOW(), revision = revision + 1, updated_at = NOW()
                 WHERE user_id = $1 AND href = $2 AND deleted_at IS NULL",
            )
            .bind(user_id)
            .bind(href.as_str())
            .execute(&mut *tx)
            .await
            .map_err(|error| error.to_string())?;
        }
        state
            .projector
            .commit_checkpoint(
                &mut tx,
                user_id,
                &collection.href,
                plan.page.sync_token.as_ref(),
                collection.etag.as_ref(),
            )
            .await
            .map_err(|error| error.to_string())?;
        tx.commit().await.map_err(|error| error.to_string())?;
    }
    Ok(())
}

async fn upsert_calendar_projection(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    id: &str,
    collection: &crate::dav::DavCollection,
) -> ApiResult<()> {
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO calendar_calendars
            (id, user_id, uid, href, etag, display_name, description, color, ctag, sync_token,
             revision, created_at, updated_at, deleted_at)
         VALUES ($1, $2, $3, $4, $5, $6, '', NULL, NULL, $7, 1, $8, $8, NULL)
         ON CONFLICT (id) DO UPDATE SET
            href = EXCLUDED.href,
            etag = EXCLUDED.etag,
            display_name = EXCLUDED.display_name,
            sync_token = EXCLUDED.sync_token,
            deleted_at = NULL,
            revision = CASE
                WHEN calendar_calendars.etag IS DISTINCT FROM EXCLUDED.etag
                    OR calendar_calendars.deleted_at IS NOT NULL
                THEN calendar_calendars.revision + 1
                ELSE calendar_calendars.revision
            END,
            updated_at = EXCLUDED.updated_at",
    )
    .bind(id)
    .bind(user_id)
    .bind(id)
    .bind(collection.href.as_str())
    .bind(
        collection
            .etag
            .as_ref()
            .map(ETag::as_str)
            .unwrap_or("\"1\""),
    )
    .bind(collection.display_name.as_deref().unwrap_or(id))
    .bind(collection.sync_token.as_ref().map(SyncToken::as_str))
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(database_error)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_event_projection(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
    user_id: &str,
    calendar_id: &str,
    uid: &str,
    href: &str,
    etag: &str,
    ical: &str,
    revision: i64,
) -> ApiResult<Event> {
    replace_event_projection(
        tx,
        id,
        user_id,
        calendar_id,
        uid,
        href,
        etag,
        ical,
        revision,
        Utc::now(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn replace_event_projection(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
    user_id: &str,
    calendar_id: &str,
    _uid: &str,
    href: &str,
    etag: &str,
    ical: &str,
    revision: i64,
    created_at: DateTime<Utc>,
) -> ApiResult<Event> {
    let now = Utc::now();
    let record = map_cal(event_from_ical(
        id,
        user_id,
        calendar_id,
        href,
        etag,
        revision,
        &created_at.to_rfc3339(),
        &now.to_rfc3339(),
        ical,
    ))?;
    sqlx::query(
        "INSERT INTO calendar_events
            (id, user_id, calendar_id, uid, href, etag, summary, description, location, all_day,
             dtstart, dtend, tzid, rrule, exdates, revision, created_at, updated_at, deleted_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,NULL)
         ON CONFLICT (id) DO UPDATE SET
            calendar_id = EXCLUDED.calendar_id,
            uid = EXCLUDED.uid,
            href = EXCLUDED.href,
            etag = EXCLUDED.etag,
            summary = EXCLUDED.summary,
            description = EXCLUDED.description,
            location = EXCLUDED.location,
            all_day = EXCLUDED.all_day,
            dtstart = EXCLUDED.dtstart,
            dtend = EXCLUDED.dtend,
            tzid = EXCLUDED.tzid,
            rrule = EXCLUDED.rrule,
            exdates = EXCLUDED.exdates,
            revision = EXCLUDED.revision,
            updated_at = EXCLUDED.updated_at,
            deleted_at = NULL",
    )
    .bind(&record.id)
    .bind(&record.user_id)
    .bind(&record.calendar_id)
    .bind(&record.uid)
    .bind(&record.href)
    .bind(&record.etag)
    .bind(&record.summary)
    .bind(&record.description)
    .bind(&record.location)
    .bind(record.all_day)
    .bind(&record.dtstart)
    .bind(&record.dtend)
    .bind(&record.tzid)
    .bind(&record.rrule)
    .bind(&record.exdates)
    .bind(record.revision)
    .bind(created_at)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(database_error)?;
    sqlx::query(
        "INSERT INTO calendar_event_payloads (event_id, ical_text)
         VALUES ($1, $2)
         ON CONFLICT (event_id) DO UPDATE SET ical_text = EXCLUDED.ical_text",
    )
    .bind(&record.id)
    .bind(ical)
    .execute(&mut **tx)
    .await
    .map_err(database_error)?;
    load_required_event(tx, &record.id).await
}

fn event_to_record(event: &Event) -> EventRecord {
    EventRecord {
        id: event.id.clone(),
        user_id: event.user_id.clone(),
        calendar_id: event.calendar_id.clone(),
        uid: event.uid.clone(),
        href: event.href.clone(),
        etag: event.etag.clone(),
        summary: event.summary.clone(),
        description: event.description.clone(),
        location: event.location.clone(),
        all_day: event.all_day,
        dtstart: event.dtstart.clone(),
        dtend: event.dtend.clone(),
        tzid: event.tzid.clone(),
        rrule: event.rrule.clone(),
        exdates: event.exdates.clone(),
        revision: event.revision,
        created_at: event.created_at.to_rfc3339(),
        updated_at: event.updated_at.to_rfc3339(),
        deleted_at: event.deleted_at.map(|value| value.to_rfc3339()),
    }
}

fn stable_event_id(_user_id: &str, _href: &str) -> String {
    uuid::Uuid::new_v4().to_string()
}

struct OperationBinding {
    user_id: String,
    operation_id: String,
    entity_type: &'static str,
    entity_id: String,
    operation: &'static str,
    request_body: Value,
}

async fn with_operation<T, F>(pool: &PgPool, binding: OperationBinding, work: F) -> ApiResult<T>
where
    T: Serialize + for<'de> Deserialize<'de> + Send,
    F: for<'c> FnOnce(
        &'c mut Transaction<'_, Postgres>,
    ) -> Pin<Box<dyn Future<Output = ApiResult<T>> + Send + 'c>>,
{
    let mut tx = pool.begin().await.map_err(database_error)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(&binding.operation_id)
        .execute(&mut *tx)
        .await
        .map_err(database_error)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 1))")
        .bind(&binding.user_id)
        .execute(&mut *tx)
        .await
        .map_err(database_error)?;
    if let Some((user_id, entity_type, entity_id, operation, request_body, status, result_body)) =
        sqlx::query_as::<_, (String, String, String, String, String, i32, String)>(
            "SELECT user_id, entity_type, entity_id, operation, request_body, result_status, result_body
             FROM calendar_operations WHERE operation_id = $1",
        )
        .bind(&binding.operation_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(database_error)?
    {
        let stored_body: Value = serde_json::from_str(&request_body).unwrap_or(Value::Null);
        if user_id != binding.user_id
            || entity_type != binding.entity_type
            || entity_id != binding.entity_id
            || operation != binding.operation
            || stored_body != binding.request_body
        {
            return Err(ApiError::conflict(
                "This operation id is already bound to a different request.",
            ));
        }
        tx.commit().await.map_err(database_error)?;
        if status != 200 {
            return Err(ApiError::conflict(
                "This operation id already produced a non-success result.",
            ));
        }
        return serde_json::from_str(&result_body)
            .map_err(|error| ApiError::unavailable(format!("stored calendar operation is invalid: {error}")));
    }
    let result = work(&mut tx).await?;
    let body = serde_json::to_string(&result).map_err(|error| {
        ApiError::unavailable(format!("failed to store calendar operation: {error}"))
    })?;
    sqlx::query(
        "INSERT INTO calendar_operations
            (operation_id, user_id, entity_type, entity_id, operation, request_body,
             result_status, result_body, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, 200, $7, $8)",
    )
    .bind(&binding.operation_id)
    .bind(&binding.user_id)
    .bind(binding.entity_type)
    .bind(&binding.entity_id)
    .bind(binding.operation)
    .bind(binding.request_body.to_string())
    .bind(body)
    .bind(Utc::now())
    .execute(&mut *tx)
    .await
    .map_err(database_error)?;
    tx.commit().await.map_err(database_error)?;
    Ok(result)
}

fn operation_request<T: Serialize>(value: &T) -> ApiResult<Value> {
    serde_json::to_value(value)
        .map_err(|error| ApiError::invalid_request(format!("invalid request body: {error}")))
}

async fn load_calendar(pool: &PgPool, id: &str) -> ApiResult<Option<Calendar>> {
    sqlx::query_as::<_, Calendar>(
        "SELECT id, user_id, uid, href, etag, display_name, description, color, revision,
                created_at, updated_at, deleted_at
         FROM calendar_calendars WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(database_error)
}

async fn load_calendar_tx(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
) -> ApiResult<Option<Calendar>> {
    sqlx::query_as::<_, Calendar>(
        "SELECT id, user_id, uid, href, etag, display_name, description, color, revision,
                created_at, updated_at, deleted_at
         FROM calendar_calendars WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)
}

async fn load_required_calendar(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
) -> ApiResult<Calendar> {
    load_calendar_tx(tx, id)
        .await?
        .ok_or_else(|| ApiError::unavailable("calendar projection disappeared after write"))
}

async fn load_event(pool: &PgPool, id: &str) -> ApiResult<Option<Event>> {
    sqlx::query_as::<_, Event>(
        "SELECT id, user_id, calendar_id, uid, href, etag, summary, description, location,
                all_day, dtstart, dtend, tzid, rrule, exdates, revision, created_at, updated_at, deleted_at
         FROM calendar_events WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(database_error)
}

async fn load_event_tx(tx: &mut Transaction<'_, Postgres>, id: &str) -> ApiResult<Option<Event>> {
    sqlx::query_as::<_, Event>(
        "SELECT id, user_id, calendar_id, uid, href, etag, summary, description, location,
                all_day, dtstart, dtend, tzid, rrule, exdates, revision, created_at, updated_at, deleted_at
         FROM calendar_events WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)
}

async fn load_required_event(tx: &mut Transaction<'_, Postgres>, id: &str) -> ApiResult<Event> {
    load_event_tx(tx, id)
        .await?
        .ok_or_else(|| ApiError::unavailable("event projection disappeared after write"))
}

async fn load_payload(tx: &mut Transaction<'_, Postgres>, event_id: &str) -> ApiResult<String> {
    sqlx::query_scalar::<_, String>(
        "SELECT ical_text FROM calendar_event_payloads WHERE event_id = $1",
    )
    .bind(event_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)?
    .ok_or_else(|| ApiError::unavailable("missing iCalendar payload for this event"))
}

async fn locked_live_calendar(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    id: &str,
    expected: i64,
) -> ApiResult<Calendar> {
    let calendar = sqlx::query_as::<_, Calendar>(
        "SELECT id, user_id, uid, href, etag, display_name, description, color, revision,
                created_at, updated_at, deleted_at
         FROM calendar_calendars WHERE id = $1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)?;
    let calendar = visible_calendar(user_id, calendar)?;
    if calendar.revision != expected {
        return Err(ApiError::stale_revision(expected, calendar.revision));
    }
    Ok(calendar)
}

async fn locked_live_event(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    id: &str,
    expected: i64,
) -> ApiResult<Event> {
    let event = sqlx::query_as::<_, Event>(
        "SELECT id, user_id, calendar_id, uid, href, etag, summary, description, location,
                all_day, dtstart, dtend, tzid, rrule, exdates, revision, created_at, updated_at, deleted_at
         FROM calendar_events WHERE id = $1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)?;
    let event = visible_event(user_id, event)?;
    if event.revision != expected {
        return Err(ApiError::stale_revision(expected, event.revision));
    }
    Ok(event)
}

async fn ensure_live_calendar(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    id: &str,
) -> ApiResult<Calendar> {
    visible_calendar(user_id, load_calendar_tx(tx, id).await?)
}

async fn ensure_calendar_quota(tx: &mut Transaction<'_, Postgres>, user_id: &str) -> ApiResult<()> {
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM calendar_calendars WHERE user_id = $1 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(database_error)?;
    if count >= MAX_CALENDARS {
        return Err(ApiError::limit_exceeded(format!(
            "A user may have at most {MAX_CALENDARS} calendars."
        )));
    }
    Ok(())
}

async fn ensure_event_quota(tx: &mut Transaction<'_, Postgres>, user_id: &str) -> ApiResult<()> {
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM calendar_events WHERE user_id = $1 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(database_error)?;
    if count >= MAX_EVENTS {
        return Err(ApiError::limit_exceeded(format!(
            "A user may have at most {MAX_EVENTS} events."
        )));
    }
    Ok(())
}

fn visible_calendar(user_id: &str, calendar: Option<Calendar>) -> ApiResult<Calendar> {
    match calendar {
        Some(row) if row.user_id != user_id => Err(ApiError::not_found("Calendar not found.")),
        Some(row) if row.deleted_at.is_some() => {
            Err(ApiError::gone("This calendar has been deleted."))
        }
        Some(row) => Ok(row),
        None => Err(ApiError::not_found("Calendar not found.")),
    }
}

fn visible_event(user_id: &str, event: Option<Event>) -> ApiResult<Event> {
    match event {
        Some(row) if row.user_id != user_id => Err(ApiError::not_found("Event not found.")),
        Some(row) if row.deleted_at.is_some() => {
            Err(ApiError::gone("This event has been deleted."))
        }
        Some(row) => Ok(row),
        None => Err(ApiError::not_found("Event not found.")),
    }
}

fn existing_identity(user_id: &str, owner_id: String, deleted: bool) -> ApiError {
    if owner_id != user_id {
        ApiError::conflict("This identifier is already in use.")
    } else if deleted {
        ApiError::gone("This identifier has been deleted and cannot be reused.")
    } else {
        ApiError::conflict("This identifier is already in use.")
    }
}

fn map_cal<T>(result: Result<T, CalendarError>) -> ApiResult<T> {
    result.map_err(|error| match error {
        CalendarError::InvalidRequest(message) => ApiError::invalid_request(message),
        CalendarError::NotFound(message) => ApiError::not_found(message),
        CalendarError::Gone(message) => ApiError::gone(message),
        CalendarError::Conflict(message) => ApiError::conflict(message),
        CalendarError::StaleEtag { expected, actual } => {
            ApiError::stale_etag(expected, Some(actual))
        }
        CalendarError::StaleRevision { expected, actual } => {
            ApiError::stale_revision(expected, actual)
        }
        CalendarError::LimitExceeded(message) => ApiError::limit_exceeded(message),
        CalendarError::Dav(message) | CalendarError::Parse(message) => {
            ApiError::unavailable(message)
        }
    })
}

fn map_dav(error: crate::dav::DavError) -> ApiError {
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
        crate::dav::DavError::Gone(message) => ApiError::gone(message),
        other => ApiError::unavailable(other.to_string()),
    }
}

fn normalize_etag(value: &str) -> String {
    crate::calendar::normalize_etag(value)
}

fn database_error(error: sqlx::Error) -> ApiError {
    ApiError::unavailable(format!("database error: {error}"))
}
