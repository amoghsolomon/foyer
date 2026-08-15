//! Device-key challenge authentication for Foyer Shell.
//!
//! This crate owns key generation, the Secret Service private-key boundary,
//! the operator-readable public enrollment file, and cached five-minute access
//! tokens. Domain crates use [`ApiSession`] instead of reading `FOYER_DEV_TOKEN`.

mod base64_url;
mod enrollment;
mod error;
mod jwk;
mod keystore;
mod secret_service;
mod session;
mod transport;

pub use error::{AuthError, EnrollmentStatus, KeyStoreError, RequestError};
pub use jwk::{EnrollmentMaterial, PublicJwk};
pub use keystore::{
    DeviceKeyStore, DeviceSigningKey, MemoryKeyStore, SecretServiceKeyStore, UnavailableKeyStore,
};
pub use session::{
    ApiSession, ApiSessionBuilder, development_auth_enabled, development_token_from_env,
};
pub use transport::{HttpRequest, HttpResponse, HttpTransport, ReqwestTransport, api_base_url};

pub use base64_url::{decode as decode_base64url, encode as encode_base64url};

#[cfg(test)]
mod contract_tests;
#[cfg(test)]
mod session_tests;
