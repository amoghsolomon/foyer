use foyer_shell_auth::{ApiSession, RequestError};
use reqwest::StatusCode;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use thiserror::Error;

use crate::{AddressBook, Contact, ContactDraft};

#[derive(Clone, Debug)]
pub struct Client {
    session: ApiSession,
}

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("contacts request failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("invalid contacts response: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("contacts request failed ({status}): {body}")]
    Response { status: StatusCode, body: String },
    #[error("invalid durable contacts command: {0}")]
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

    pub async fn create_address_book(
        &self,
        operation_id: &str,
        id: &str,
        display_name: &str,
        description: Option<&str>,
    ) -> Result<AddressBook, ApiError> {
        self.post(
            "/v1/address-books",
            json!({
                "operationId": operation_id,
                "id": id,
                "displayName": display_name,
                "description": description,
            }),
        )
        .await
    }

    pub async fn update_address_book(
        &self,
        operation_id: &str,
        id: &str,
        expected_etag: Option<&str>,
        expected_revision: i64,
        display_name: &str,
    ) -> Result<AddressBook, ApiError> {
        self.post(
            &format!("/v1/address-books/{id}/update"),
            json!({
                "operationId": operation_id,
                "expectedEtag": expected_etag,
                "expectedRevision": expected_revision,
                "displayName": display_name,
            }),
        )
        .await
    }

    pub async fn delete_address_book(
        &self,
        operation_id: &str,
        id: &str,
        expected_etag: Option<&str>,
        expected_revision: i64,
    ) -> Result<AddressBook, ApiError> {
        self.post(
            &format!("/v1/address-books/{id}/delete"),
            json!({
                "operationId": operation_id,
                "expectedEtag": expected_etag,
                "expectedRevision": expected_revision,
            }),
        )
        .await
    }

    pub async fn create_contact(
        &self,
        operation_id: &str,
        id: &str,
        address_book_id: &str,
        draft: &ContactDraft,
    ) -> Result<Contact, ApiError> {
        let mut body = draft.to_json();
        body["operationId"] = json!(operation_id);
        body["id"] = json!(id);
        body["addressBookId"] = json!(address_book_id);
        self.post("/v1/contacts", body).await
    }

    pub async fn update_contact(
        &self,
        operation_id: &str,
        id: &str,
        expected_etag: Option<&str>,
        expected_revision: i64,
        draft: &ContactDraft,
    ) -> Result<Contact, ApiError> {
        let mut body = draft.to_json();
        body["operationId"] = json!(operation_id);
        body["expectedEtag"] = json!(expected_etag);
        body["expectedRevision"] = json!(expected_revision);
        self.post(&format!("/v1/contacts/{id}/update"), body).await
    }

    pub async fn move_contact(
        &self,
        operation_id: &str,
        id: &str,
        expected_etag: Option<&str>,
        expected_revision: i64,
        address_book_id: &str,
    ) -> Result<Contact, ApiError> {
        self.post(
            &format!("/v1/contacts/{id}/move"),
            json!({
                "operationId": operation_id,
                "expectedEtag": expected_etag,
                "expectedRevision": expected_revision,
                "addressBookId": address_book_id,
            }),
        )
        .await
    }

    pub async fn delete_contact(
        &self,
        operation_id: &str,
        id: &str,
        expected_etag: Option<&str>,
        expected_revision: i64,
    ) -> Result<Contact, ApiError> {
        self.post(
            &format!("/v1/contacts/{id}/delete"),
            json!({
                "operationId": operation_id,
                "expectedEtag": expected_etag,
                "expectedRevision": expected_revision,
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
