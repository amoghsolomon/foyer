use axum::{
    Json,
    extract::{FromRequestParts, State},
    http::{StatusCode, header::AUTHORIZATION, request::Parts},
};
use chrono::SecondsFormat;
use serde::Serialize;

use crate::{
    AppState,
    config::Config,
    error::{ApiError, ApiResult},
};

pub mod admin;
pub mod challenge;
pub mod jwk;
pub mod rate;
pub mod tokens;

pub use admin::{
    AddedDevice, DeviceAddStatus, DeviceRecord, DeviceRevokeStatus, RevokedDevice, add_device,
    list_devices, public_jwk_from_bytes, revoke_device,
};
pub use challenge::{
    ChallengeBody, CreateChallengeRequest, CreateSessionRequest, SessionIssuedBody,
    build_signing_payload, issue_challenge, redeem_session,
};
pub use jwk::{PublicJwk, decode_p1363, is_device_key_id, parse_public_jwk, verify_p1363};
pub use rate::AuthLimiter;
pub use tokens::{TokenSigner, looks_like_jwt};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Principal {
    pub user_id: String,
    pub device_key_id: Option<String>,
}

impl FromRequestParts<AppState> for Principal {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        authenticate(
            &state.config,
            &state.signer,
            parts
                .headers
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
        )
    }
}

pub fn authenticate(
    config: &Config,
    signer: &TokenSigner,
    authorization: Option<&str>,
) -> ApiResult<Principal> {
    let Some(header) = authorization else {
        return Err(ApiError::unauthenticated());
    };
    let token = header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "))
        .ok_or_else(ApiError::unauthenticated)?;
    if token.is_empty() {
        return Err(ApiError::unauthenticated());
    }
    if looks_like_jwt(token) {
        return signer.authenticate_access(token);
    }
    if !config.is_development() {
        return Err(ApiError::forbidden(
            "Development tokens are disabled outside FOYER_SERVER_ENV=development.",
        ));
    }
    config
        .user_for_token(token)
        .map(|user| Principal {
            user_id: user.user_id.clone(),
            device_key_id: None,
        })
        .ok_or_else(ApiError::unauthenticated)
}

#[derive(Debug, Serialize)]
pub struct SessionBody {
    #[serde(rename = "userId")]
    pub user_id: String,
    #[serde(rename = "deviceKeyId", skip_serializing_if = "Option::is_none")]
    pub device_key_id: Option<String>,
    pub environment: &'static str,
    pub development: bool,
}

#[derive(Debug, Serialize)]
pub struct SyncCredentialsBody {
    pub endpoint: String,
    pub token: String,
    #[serde(rename = "expiresAt")]
    pub expires_at: String,
    #[serde(rename = "userId")]
    pub user_id: String,
}

#[derive(Debug, Serialize)]
pub struct JwksBody {
    pub keys: Vec<serde_json::Value>,
}

pub fn session_body(config: &Config, principal: &Principal) -> SessionBody {
    SessionBody {
        user_id: principal.user_id.clone(),
        device_key_id: principal.device_key_id.clone(),
        environment: config.runtime_env.as_str(),
        development: config.is_development(),
    }
}

pub fn sync_credentials(
    config: &Config,
    signer: &TokenSigner,
    principal: &Principal,
) -> ApiResult<SyncCredentialsBody> {
    let endpoint = config.powersync_url.clone().ok_or_else(|| {
        ApiError::forbidden("FOYER_POWERSYNC_URL is required to issue sync credentials.")
    })?;
    let issued = signer.issue_powersync(&principal.user_id, principal.device_key_id.as_deref())?;
    Ok(SyncCredentialsBody {
        endpoint,
        token: issued.access_token,
        expires_at: issued.expires_at.to_rfc3339_opts(SecondsFormat::Secs, true),
        user_id: principal.user_id.clone(),
    })
}

pub fn jwks_body(signer: &TokenSigner) -> JwksBody {
    let document = signer.jwks();
    JwksBody {
        keys: document
            .get("keys")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default(),
    }
}

pub async fn create_challenge(
    State(state): State<AppState>,
    Json(body): Json<CreateChallengeRequest>,
) -> ApiResult<Json<ChallengeBody>> {
    let device_key_id = body.device_key_id.trim();
    if !is_device_key_id(device_key_id) {
        return Err(ApiError::invalid_request(
            "deviceKeyId must be a P-256 JWK thumbprint.",
        ));
    }
    if !state.auth_limiter.allow_challenge(device_key_id) {
        return Err(ApiError::rate_limited());
    }
    issue_challenge(&state.pool, &state.signer, device_key_id)
        .await
        .map(Json)
}

pub async fn create_session(
    State(state): State<AppState>,
    Json(body): Json<CreateSessionRequest>,
) -> ApiResult<Json<SessionIssuedBody>> {
    let challenge_id = body.challenge_id.trim();
    let signature = body.signature.trim();
    if uuid::Uuid::parse_str(challenge_id).is_err() || decode_p1363(signature).is_err() {
        return Err(ApiError::authentication_failed());
    }
    if !state.auth_limiter.allow_session(challenge_id) {
        return Err(ApiError::rate_limited());
    }
    redeem_session(&state.pool, &state.signer, challenge_id, signature)
        .await
        .map(Json)
}

pub async fn jwks(State(state): State<AppState>) -> Json<JwksBody> {
    Json(jwks_body(&state.signer))
}

pub async fn development_jwks(State(state): State<AppState>) -> Result<Json<JwksBody>, StatusCode> {
    if !state.config.is_development() {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(Json(jwks_body(&state.signer)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DevUser, RuntimeEnv};

    fn development_config() -> Config {
        Config::test_development("postgres://foyer:foyer@127.0.0.1:5432/foyer")
    }

    fn signer() -> TokenSigner {
        TokenSigner::ephemeral(
            "foyer-dev".into(),
            "foyer-server".into(),
            "foyer-api".into(),
            "foyer-powersync".into(),
        )
        .expect("signer")
    }

    #[test]
    fn missing_bearer_is_unauthenticated() {
        assert!(authenticate(&development_config(), &signer(), None).is_err());
    }

    #[test]
    fn development_token_maps_to_principal() {
        let principal = authenticate(&development_config(), &signer(), Some("Bearer dev-token"))
            .expect("principal");
        assert_eq!(principal.user_id, "dev-user");
        assert_eq!(principal.device_key_id, None);
    }

    #[test]
    fn production_rejects_static_token() {
        let mut config = development_config();
        config.runtime_env = RuntimeEnv::Production;
        config.dev_users.clear();
        let error = authenticate(&config, &signer(), Some("Bearer dev-token")).expect_err("denied");
        assert_eq!(error.status_code(), axum::http::StatusCode::FORBIDDEN);
    }

    #[test]
    fn development_accepts_signed_access_jwt() {
        let signer = signer();
        let issued = signer.issue_access("owner", Some("device-1")).expect("jwt");
        let principal = authenticate(
            &development_config(),
            &signer,
            Some(&format!("Bearer {}", issued.access_token)),
        )
        .expect("principal");
        assert_eq!(principal.user_id, "owner");
        assert_eq!(principal.device_key_id.as_deref(), Some("device-1"));
    }

    #[test]
    fn powersync_token_is_not_an_access_token() {
        let signer = signer();
        let issued = signer
            .issue_powersync("owner", Some("device-1"))
            .expect("jwt");
        assert!(
            authenticate(
                &development_config(),
                &signer,
                Some(&format!("Bearer {}", issued.access_token)),
            )
            .is_err()
        );
    }

    #[test]
    fn extra_dev_user_is_resolved() {
        let mut config = development_config();
        config.dev_users.push(DevUser {
            user_id: "other".into(),
            token: "other-token".into(),
        });
        let principal = authenticate(&config, &signer(), Some("Bearer other-token")).expect("ok");
        assert_eq!(principal.user_id, "other");
    }
}
