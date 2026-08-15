use std::fmt::{Display, Formatter};
use std::path::PathBuf;

use reqwest::StatusCode;
use thiserror::Error;

use crate::jwk::EnrollmentMaterial;

#[derive(Clone, Debug, Error)]
pub enum KeyStoreError {
    #[error(
        "Secret Service is not available. Foyer Shell stores the device signing key in the desktop keyring and will not write it to a file. Start Secret Service (org.freedesktop.secrets), then try again."
    )]
    Unavailable,
    #[error(
        "The desktop keyring is locked or refused the Foyer Shell device signing key. Unlock Secret Service, then try again."
    )]
    Locked,
    #[error(
        "The device signing key stored in Secret Service is unusable. Remove the Foyer Shell keyring item and try again."
    )]
    Corrupt,
    #[error("{0}")]
    Failed(String),
}

impl KeyStoreError {
    pub fn unavailable(_detail: impl Display) -> Self {
        Self::Unavailable
    }
}

#[derive(Clone, Debug)]
pub struct EnrollmentStatus {
    pub path: PathBuf,
    pub fingerprint: String,
}

impl EnrollmentStatus {
    pub fn from_material(path: PathBuf, material: &EnrollmentMaterial) -> Self {
        Self {
            path,
            fingerprint: material.device_key_id.clone(),
        }
    }

    pub fn public_message(&self) -> String {
        format!(
            "This device is not enrolled yet. Public key file: {}. Fingerprint: {}. Ask the operator to add this public key with foyer-admin, then try again.",
            self.path.display(),
            self.fingerprint
        )
    }
}

impl Display for EnrollmentStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.public_message())
    }
}

#[derive(Clone, Debug, Error)]
pub enum AuthError {
    #[error("{0}")]
    NotEnrolled(EnrollmentStatus),
    #[error("{0}")]
    KeyStore(#[from] KeyStoreError),
    #[error("{0}")]
    Transport(String),
    #[error("{0}")]
    Protocol(String),
    #[error("{0}")]
    DevelopmentRefused(String),
}

impl AuthError {
    pub fn is_retryable(&self) -> bool {
        true
    }

    pub fn status(&self) -> Option<StatusCode> {
        match self {
            Self::NotEnrolled(_) | Self::DevelopmentRefused(_) => Some(StatusCode::UNAUTHORIZED),
            _ => None,
        }
    }

    pub fn is_not_enrolled(&self) -> bool {
        matches!(self, Self::NotEnrolled(_))
    }

    pub fn public_message(&self) -> String {
        self.to_string()
    }

    pub fn from_http_status(status: StatusCode, enrollment: Option<EnrollmentStatus>) -> Self {
        if matches!(
            status,
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::NOT_FOUND
        ) && let Some(enrollment) = enrollment
        {
            return Self::NotEnrolled(enrollment);
        }
        if status.is_server_error() {
            return Self::Transport("Foyer is unavailable. Try again shortly.".into());
        }
        Self::Protocol(
            "Couldn't authenticate this device. Try again after the operator adds the public key."
                .into(),
        )
    }
}

#[derive(Debug, Error)]
pub enum RequestError {
    #[error("request failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("invalid response: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("request failed ({status}): {body}")]
    Status { status: StatusCode, body: String },
    #[error("{0}")]
    Auth(#[from] AuthError),
}

impl RequestError {
    pub fn status(&self) -> Option<StatusCode> {
        match self {
            Self::Status { status, .. } => Some(*status),
            Self::Auth(error) => error.status(),
            Self::Transport(error) => error.status(),
            Self::Decode(_) => None,
        }
    }

    pub fn public_message(&self) -> String {
        match self {
            Self::Auth(error) => error.public_message(),
            Self::Status { status, .. } => format!("request failed ({status})"),
            Self::Transport(_) => {
                "Couldn't reach Foyer. Check the connection and try again.".into()
            }
            Self::Decode(_) => "invalid response".into(),
        }
    }
}
