use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
    details: Option<Value>,
}

impl ApiError {
    pub fn unauthenticated() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "unauthenticated",
            "A valid bearer token is required.",
        )
    }

    pub fn authentication_failed() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "unauthenticated",
            "Authentication failed.",
        )
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "forbidden", message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }

    pub fn gone(message: impl Into<String>) -> Self {
        Self::new(StatusCode::GONE, "gone", message)
    }

    pub fn stale_revision(expected: i64, actual: i64) -> Self {
        Self::new(
            StatusCode::CONFLICT,
            "stale_revision",
            "The expected revision does not match the current revision.",
        )
        .with_details(json!({
            "expectedRevision": expected,
            "actualRevision": actual,
        }))
    }

    pub fn stale_etag(expected: impl Into<String>, actual: Option<String>) -> Self {
        Self::new(
            StatusCode::CONFLICT,
            "stale_etag",
            "The expected ETag does not match the current DAV ETag.",
        )
        .with_details(json!({
            "expectedEtag": expected.into(),
            "actualEtag": actual,
        }))
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "conflict", message)
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_request", message)
    }

    pub fn invalid_parent(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, "invalid_parent", message)
    }

    pub fn cycle(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, "cycle", message)
    }

    pub fn folder_not_empty(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "folder_not_empty",
            message,
        )
    }

    pub fn limit_exceeded(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, "limit_exceeded", message)
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, "unavailable", message)
    }

    pub fn rate_limited() -> Self {
        Self::new(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "Too many authentication attempts.",
        )
    }

    pub fn status_code(&self) -> StatusCode {
        self.status
    }

    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            details: None,
        }
    }

    fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn status(self, status: StatusCode) -> Replay {
        Replay {
            status,
            body: ErrorBody::from(self),
        }
    }
}

pub struct Replay {
    pub status: StatusCode,
    pub body: ErrorBody,
}

#[derive(Clone, Debug, Serialize)]
pub struct ErrorBody {
    error: ErrorObject,
}

#[derive(Clone, Debug, Serialize)]
struct ErrorObject {
    code: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<Value>,
}

impl From<ApiError> for ErrorBody {
    fn from(error: ApiError) -> Self {
        Self {
            error: ErrorObject {
                code: error.code,
                message: error.message,
                details: error.details,
            },
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status;
        (status, Json(ErrorBody::from(self))).into_response()
    }
}

impl IntoResponse for Replay {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
