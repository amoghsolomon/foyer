use foyer_shell_auth::{ApiSession, RequestError};
use reqwest::StatusCode;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use thiserror::Error;

use crate::{Folder, Note};

#[derive(Clone, Debug)]
pub struct Client {
    session: ApiSession,
}

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("notes request failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("invalid notes response: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("notes request failed ({status}): {body}")]
    Response { status: StatusCode, body: String },
    #[error("invalid durable notes command: {0}")]
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

#[derive(Debug, Deserialize)]
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

    pub async fn create_folder(
        &self,
        operation_id: &str,
        id: &str,
        name: &str,
        parent_id: Option<&str>,
        position: Option<i64>,
    ) -> Result<Folder, ApiError> {
        self.post(
            "/v1/folders",
            json!({
                "operationId": operation_id,
                "id": id,
                "name": name,
                "parentId": parent_id,
                "position": position,
            }),
        )
        .await
    }

    pub async fn rename_folder(
        &self,
        operation_id: &str,
        id: &str,
        revision: i64,
        name: &str,
    ) -> Result<Folder, ApiError> {
        self.post(
            &format!("/v1/folders/{id}/rename"),
            json!({
                "operationId": operation_id,
                "expectedRevision": revision,
                "name": name,
            }),
        )
        .await
    }

    pub async fn move_folder(
        &self,
        operation_id: &str,
        id: &str,
        revision: i64,
        parent_id: Option<&str>,
        position: Option<i64>,
    ) -> Result<Folder, ApiError> {
        self.post(
            &format!("/v1/folders/{id}/move"),
            json!({
                "operationId": operation_id,
                "expectedRevision": revision,
                "parentId": parent_id,
                "position": position,
            }),
        )
        .await
    }

    pub async fn delete_folder(
        &self,
        operation_id: &str,
        id: &str,
        revision: i64,
    ) -> Result<Folder, ApiError> {
        self.post(
            &format!("/v1/folders/{id}/delete"),
            json!({
                "operationId": operation_id,
                "expectedRevision": revision,
            }),
        )
        .await
    }

    pub async fn create_note(
        &self,
        operation_id: &str,
        id: &str,
        folder_id: &str,
        title: &str,
        body: &str,
    ) -> Result<Note, ApiError> {
        self.post(
            "/v1/notes",
            json!({
                "operationId": operation_id,
                "id": id,
                "folderId": folder_id,
                "title": title,
                "body": body,
            }),
        )
        .await
    }

    pub async fn update_note(
        &self,
        operation_id: &str,
        id: &str,
        revision: i64,
        title: &str,
        body: &str,
    ) -> Result<Note, ApiError> {
        self.post(
            &format!("/v1/notes/{id}/update"),
            json!({
                "operationId": operation_id,
                "expectedRevision": revision,
                "title": title,
                "body": body,
            }),
        )
        .await
    }

    pub async fn move_note(
        &self,
        operation_id: &str,
        id: &str,
        revision: i64,
        folder_id: &str,
    ) -> Result<Note, ApiError> {
        self.post(
            &format!("/v1/notes/{id}/move"),
            json!({
                "operationId": operation_id,
                "expectedRevision": revision,
                "folderId": folder_id,
            }),
        )
        .await
    }

    pub async fn delete_note(
        &self,
        operation_id: &str,
        id: &str,
        revision: i64,
    ) -> Result<Note, ApiError> {
        self.post(
            &format!("/v1/notes/{id}/delete"),
            json!({
                "operationId": operation_id,
                "expectedRevision": revision,
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
