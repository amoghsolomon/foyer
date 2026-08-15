use foyer_shell_auth::{ApiSession, RequestError};
use reqwest::StatusCode;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use thiserror::Error;

use crate::{Due, Task, TaskList};

#[derive(Clone, Debug)]
pub struct Client {
    session: ApiSession,
}

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("tasks request failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("invalid tasks response: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("tasks request failed ({status}): {body}")]
    Response { status: StatusCode, body: String },
    #[error("invalid durable tasks command: {0}")]
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

    pub async fn create_list(
        &self,
        operation_id: &str,
        id: &str,
        name: &str,
        position: Option<i64>,
    ) -> Result<TaskList, ApiError> {
        self.post(
            "/v1/task-lists",
            json!({
                "operationId": operation_id,
                "id": id,
                "name": name,
                "position": position,
            }),
        )
        .await
    }

    pub async fn rename_list(
        &self,
        operation_id: &str,
        id: &str,
        revision: i64,
        name: &str,
    ) -> Result<TaskList, ApiError> {
        self.post(
            &format!("/v1/task-lists/{id}/rename"),
            json!({
                "operationId": operation_id,
                "expectedRevision": revision,
                "name": name,
            }),
        )
        .await
    }

    pub async fn delete_list(
        &self,
        operation_id: &str,
        id: &str,
        revision: i64,
    ) -> Result<TaskList, ApiError> {
        self.post(
            &format!("/v1/task-lists/{id}/delete"),
            json!({
                "operationId": operation_id,
                "expectedRevision": revision,
            }),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_task(
        &self,
        operation_id: &str,
        id: &str,
        list_id: &str,
        title: &str,
        description: &str,
        due: Option<&Due>,
        priority: i32,
        position: Option<i64>,
    ) -> Result<Task, ApiError> {
        self.post(
            "/v1/tasks",
            json!({
                "operationId": operation_id,
                "id": id,
                "listId": list_id,
                "title": title,
                "description": description,
                "due": due,
                "priority": priority,
                "position": position,
            }),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_task(
        &self,
        operation_id: &str,
        id: &str,
        revision: i64,
        title: &str,
        description: &str,
        due: Option<&Due>,
        priority: i32,
        position: i32,
    ) -> Result<Task, ApiError> {
        self.post(
            &format!("/v1/tasks/{id}/update"),
            json!({
                "operationId": operation_id,
                "expectedRevision": revision,
                "title": title,
                "description": description,
                "due": due,
                "priority": priority,
                "position": position,
            }),
        )
        .await
    }

    pub async fn move_task(
        &self,
        operation_id: &str,
        id: &str,
        revision: i64,
        list_id: &str,
        position: Option<i64>,
    ) -> Result<Task, ApiError> {
        self.post(
            &format!("/v1/tasks/{id}/move"),
            json!({
                "operationId": operation_id,
                "expectedRevision": revision,
                "listId": list_id,
                "position": position,
            }),
        )
        .await
    }

    pub async fn complete_task(
        &self,
        operation_id: &str,
        id: &str,
        revision: i64,
    ) -> Result<Task, ApiError> {
        self.post(
            &format!("/v1/tasks/{id}/complete"),
            json!({
                "operationId": operation_id,
                "expectedRevision": revision,
            }),
        )
        .await
    }

    pub async fn reopen_task(
        &self,
        operation_id: &str,
        id: &str,
        revision: i64,
    ) -> Result<Task, ApiError> {
        self.post(
            &format!("/v1/tasks/{id}/reopen"),
            json!({
                "operationId": operation_id,
                "expectedRevision": revision,
            }),
        )
        .await
    }

    pub async fn delete_task(
        &self,
        operation_id: &str,
        id: &str,
        revision: i64,
    ) -> Result<Task, ApiError> {
        self.post(
            &format!("/v1/tasks/{id}/delete"),
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
