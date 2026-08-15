use foyer_shell_auth::{ApiSession, RequestError};
use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use thiserror::Error;

use crate::{Calendar, Event, EventDraft};

#[derive(Clone, Debug)]
pub struct Client {
    session: ApiSession,
}

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("calendar request failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("invalid calendar response: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("calendar request failed ({status}): {body}")]
    Response { status: StatusCode, body: String },
    #[error("invalid durable calendar command: {0}")]
    InvalidCommand(String),
}

impl ApiError {
    pub fn is_permanent_command_rejection(&self) -> bool {
        matches!(self, Self::InvalidCommand(_))
            || matches!(
                self,
                Self::Response { status, .. }
                    if matches!(
                        *status,
                        StatusCode::BAD_REQUEST
                            | StatusCode::NOT_FOUND
                            | StatusCode::CONFLICT
                            | StatusCode::GONE
                            | StatusCode::UNPROCESSABLE_ENTITY
                    )
            )
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncCredentials {
    pub endpoint: String,
    pub token: String,
}

impl From<RequestError> for ApiError {
    fn from(error: RequestError) -> Self {
        match error {
            RequestError::Transport(error) => Self::Transport(error),
            RequestError::Decode(error) => Self::Decode(error),
            RequestError::Status { status, body } => Self::Response { status, body },
            RequestError::Auth(error) => Self::Response {
                status: error.status().unwrap_or(StatusCode::UNAUTHORIZED),
                body: error.public_message(),
            },
        }
    }
}

impl Client {
    pub async fn from_env() -> Result<Self, String> {
        Ok(Self {
            session: ApiSession::from_env()
                .await
                .map_err(|error| error.public_message())?,
        })
    }

    pub fn from_session(session: ApiSession) -> Self {
        Self { session }
    }

    pub async fn sync_credentials(&self) -> Result<SyncCredentials, ApiError> {
        self.get("/v1/sync/credentials").await
    }

    pub async fn create_calendar(
        &self,
        operation_id: &str,
        id: &str,
        display_name: &str,
        description: &str,
        color: Option<&str>,
    ) -> Result<Calendar, ApiError> {
        self.post(
            "/v1/calendars",
            json!({
                "operationId": operation_id,
                "id": id,
                "displayName": display_name,
                "description": description,
                "color": color,
            }),
        )
        .await
    }

    pub async fn rename_calendar(
        &self,
        operation_id: &str,
        id: &str,
        revision: i64,
        etag: Option<&str>,
        display_name: &str,
    ) -> Result<Calendar, ApiError> {
        self.post(
            &format!("/v1/calendars/{id}/rename"),
            json!({
                "operationId": operation_id,
                "expectedRevision": revision,
                "expectedEtag": etag,
                "displayName": display_name,
            }),
        )
        .await
    }

    pub async fn delete_calendar(
        &self,
        operation_id: &str,
        id: &str,
        revision: i64,
        etag: Option<&str>,
    ) -> Result<Calendar, ApiError> {
        self.post(
            &format!("/v1/calendars/{id}/delete"),
            json!({
                "operationId": operation_id,
                "expectedRevision": revision,
                "expectedEtag": etag,
            }),
        )
        .await
    }

    pub async fn create_event(
        &self,
        operation_id: &str,
        id: &str,
        uid: Option<&str>,
        draft: &EventDraft,
    ) -> Result<Event, ApiError> {
        self.post(
            "/v1/events",
            event_body(operation_id, id, uid, draft, None, None),
        )
        .await
    }

    pub async fn update_event(
        &self,
        operation_id: &str,
        id: &str,
        revision: i64,
        etag: Option<&str>,
        draft: &EventDraft,
    ) -> Result<Event, ApiError> {
        self.post(
            &format!("/v1/events/{id}/update"),
            event_body(operation_id, id, None, draft, Some(revision), etag),
        )
        .await
    }

    pub async fn move_event(
        &self,
        operation_id: &str,
        id: &str,
        revision: i64,
        etag: Option<&str>,
        calendar_id: &str,
    ) -> Result<Event, ApiError> {
        self.post(
            &format!("/v1/events/{id}/move"),
            json!({
                "operationId": operation_id,
                "expectedRevision": revision,
                "expectedEtag": etag,
                "calendarId": calendar_id,
            }),
        )
        .await
    }

    pub async fn delete_event(
        &self,
        operation_id: &str,
        id: &str,
        revision: i64,
        etag: Option<&str>,
    ) -> Result<Event, ApiError> {
        self.post(
            &format!("/v1/events/{id}/delete"),
            json!({
                "operationId": operation_id,
                "expectedRevision": revision,
                "expectedEtag": etag,
            }),
        )
        .await
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, ApiError> {
        Ok(self.session.get_json(path).await?)
    }

    async fn post<T: DeserializeOwned>(&self, path: &str, body: Value) -> Result<T, ApiError> {
        Ok(self.session.post_json(path, body).await?)
    }
}

fn event_body(
    operation_id: &str,
    id: &str,
    uid: Option<&str>,
    draft: &EventDraft,
    revision: Option<i64>,
    etag: Option<&str>,
) -> Value {
    json!({
        "operationId": operation_id,
        "id": id,
        "calendarId": draft.calendar_id,
        "uid": uid,
        "summary": draft.summary,
        "description": draft.description,
        "location": draft.location,
        "allDay": draft.all_day,
        "dtstart": draft.dtstart,
        "dtend": draft.dtend,
        "tzid": draft.tzid,
        "rrule": draft.rrule,
        "exdates": draft.exdates,
        "expectedRevision": revision,
        "expectedEtag": etag,
    })
}
