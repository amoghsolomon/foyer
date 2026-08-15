use std::fmt::{Display, Formatter};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DavError {
    InvalidRequest(String),
    PreconditionFailed {
        expected: Option<String>,
        detail: String,
    },
    Conflict(String),
    NotFound(String),
    Unauthorized,
    Forbidden(String),
    Gone(String),
    Unavailable(String),
    MalformedRemote(String),
    ResponseTooLarge {
        limit: usize,
    },
    XmlBound {
        detail: String,
    },
    UnsafeUrl(String),
    UnsafePath(String),
    Timeout,
    InvalidSyncToken,
    OperationConflict,
    Protocol {
        status: u16,
        detail: String,
    },
    Transport(String),
}

impl DavError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidRequest(_) => "invalid_request",
            Self::PreconditionFailed { .. } => "precondition_failed",
            Self::Conflict(_) | Self::OperationConflict => "conflict",
            Self::NotFound(_) => "not_found",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden(_) => "forbidden",
            Self::Gone(_) => "gone",
            Self::Unavailable(_) | Self::Timeout | Self::Transport(_) => "unavailable",
            Self::MalformedRemote(_) => "malformed_remote",
            Self::ResponseTooLarge { .. } | Self::XmlBound { .. } => "limit_exceeded",
            Self::UnsafeUrl(_) | Self::UnsafePath(_) => "unsafe_url",
            Self::InvalidSyncToken => "invalid_sync_token",
            Self::Protocol { .. } => "protocol",
        }
    }

    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Unavailable(_) | Self::Timeout | Self::Transport(_)
        )
    }

    pub fn is_stale(&self) -> bool {
        matches!(
            self,
            Self::PreconditionFailed { .. } | Self::InvalidSyncToken
        )
    }

    pub fn public_message(&self) -> String {
        self.to_string()
    }
}

impl Display for DavError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(message) => f.write_str(message),
            Self::PreconditionFailed { detail, .. } => {
                write!(f, "DAV precondition failed: {detail}")
            }
            Self::Conflict(message) => write!(f, "DAV conflict: {message}"),
            Self::NotFound(message) => f.write_str(message),
            Self::Unauthorized => f.write_str("DAV authentication failed."),
            Self::Forbidden(message) => f.write_str(message),
            Self::Gone(message) => f.write_str(message),
            Self::Unavailable(message) => f.write_str(message),
            Self::MalformedRemote(message) => {
                write!(f, "remote DAV resource is malformed: {message}")
            }
            Self::ResponseTooLarge { limit } => {
                write!(f, "DAV response exceeded the {limit} byte bound")
            }
            Self::XmlBound { detail } => write!(f, "DAV XML exceeded safety bounds: {detail}"),
            Self::UnsafeUrl(message) => write!(f, "DAV URL is not safe: {message}"),
            Self::UnsafePath(message) => write!(f, "DAV path is not safe: {message}"),
            Self::Timeout => f.write_str("DAV request timed out"),
            Self::InvalidSyncToken => f.write_str("DAV sync token is no longer valid"),
            Self::OperationConflict => {
                f.write_str("this DAV operation id is already bound to a different request")
            }
            Self::Protocol { status, detail } => {
                write!(f, "DAV protocol error ({status}): {detail}")
            }
            Self::Transport(message) => write!(f, "DAV transport error: {message}"),
        }
    }
}

impl std::error::Error for DavError {}

pub fn classify_http(status: u16, dav_error: Option<&str>, body_hint: &str) -> DavError {
    let token = dav_error.unwrap_or("");
    if token.eq_ignore_ascii_case("valid-sync-token") {
        return DavError::InvalidSyncToken;
    }
    match status {
        401 => DavError::Unauthorized,
        403 if token.eq_ignore_ascii_case("valid-sync-token") => DavError::InvalidSyncToken,
        403 => DavError::Forbidden(safe_hint(body_hint, "DAV access denied.")),
        404 => DavError::NotFound(safe_hint(body_hint, "DAV resource not found.")),
        409 if token.eq_ignore_ascii_case("valid-sync-token") => DavError::InvalidSyncToken,
        409 => DavError::Conflict(conflict_detail(token, body_hint)),
        412 => DavError::PreconditionFailed {
            expected: None,
            detail: safe_hint(body_hint, "the stored ETag no longer matches"),
        },
        415 => DavError::InvalidRequest("DAV rejected the resource media type.".into()),
        423 => DavError::Conflict("the DAV resource is locked.".into()),
        507 => DavError::Unavailable("DAV storage is exhausted.".into()),
        400 => DavError::InvalidRequest(safe_hint(body_hint, "DAV rejected the request.")),
        405 => DavError::Protocol {
            status,
            detail: safe_hint(body_hint, "method not allowed"),
        },
        500..=599 => DavError::Unavailable(format!("DAV server returned {status}")),
        _ => DavError::Protocol {
            status,
            detail: safe_hint(body_hint, "unexpected DAV status"),
        },
    }
}

fn conflict_detail(token: &str, body_hint: &str) -> String {
    if token.eq_ignore_ascii_case("no-uid-conflict") {
        return "a resource with this UID already exists.".into();
    }
    if !token.is_empty() {
        return format!("DAV reported {token}");
    }
    safe_hint(body_hint, "the DAV collection rejected the write.")
}

fn safe_hint(hint: &str, fallback: &str) -> String {
    let trimmed = hint.trim();
    if trimmed.is_empty() || trimmed.len() > 200 || trimmed.contains('\0') {
        return fallback.to_string();
    }
    if trimmed.chars().any(|ch| ch.is_control() && ch != ' ') {
        return fallback.to_string();
    }
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_sync_token_is_classified() {
        assert_eq!(
            classify_http(403, Some("valid-sync-token"), ""),
            DavError::InvalidSyncToken
        );
        assert_eq!(
            classify_http(409, Some("valid-sync-token"), ""),
            DavError::InvalidSyncToken
        );
    }

    #[test]
    fn stale_etag_is_precondition_failed() {
        let error = classify_http(412, None, "");
        assert!(error.is_stale());
        assert_eq!(error.code(), "precondition_failed");
    }

    #[test]
    fn display_does_not_echo_authorization() {
        let error = DavError::Transport("basic abcdef".into());
        assert!(!error.to_string().contains("Authorization"));
    }
}
