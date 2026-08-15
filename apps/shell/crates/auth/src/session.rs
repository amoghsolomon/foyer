use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::{Method, StatusCode};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::sync::Mutex;

use tokio::sync::broadcast;

use crate::base64_url;
use crate::enrollment::{default_enrollment_path, write_public_enrollment};
use crate::error::{AuthError, EnrollmentStatus, RequestError};
use crate::jwk::EnrollmentMaterial;
use crate::keystore::{DeviceKeyStore, DeviceSigningKey, SecretServiceKeyStore};
use crate::transport::{HttpRequest, HttpTransport, ReqwestTransport};

const REFRESH_SKEW: Duration = Duration::from_secs(45);

#[derive(Clone, Debug)]
struct CachedToken {
    access_token: String,
    expires_at: DateTime<Utc>,
}

impl CachedToken {
    fn is_fresh(&self, now: DateTime<Utc>, skew: Duration) -> bool {
        self.expires_at - chrono::Duration::from_std(skew).unwrap_or(chrono::Duration::seconds(45))
            > now
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChallengeBody {
    challenge_id: String,
    signing_payload: String,
    #[allow(dead_code)]
    expires_at: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionBody {
    access_token: String,
    #[allow(dead_code)]
    token_type: Option<String>,
    expires_at: String,
    #[allow(dead_code)]
    user_id: String,
    #[allow(dead_code)]
    device_key_id: String,
}

struct TokenState {
    cached: Option<CachedToken>,
    inflight: Option<broadcast::Sender<Result<CachedToken, AuthError>>>,
}

struct SessionInner {
    key: DeviceSigningKey,
    material: EnrollmentMaterial,
    enrollment_path: PathBuf,
    development_token: Option<String>,
    transport: Arc<dyn HttpTransport>,
    clock: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
    refresh_skew: Duration,
    state: Mutex<TokenState>,
}

#[derive(Clone)]
pub struct ApiSession {
    inner: Arc<SessionInner>,
}

impl std::fmt::Debug for ApiSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiSession")
            .field("device_key_id", &self.inner.material.device_key_id)
            .field("enrollment_path", &self.inner.enrollment_path)
            .field("development", &self.uses_development_token())
            .finish()
    }
}

impl ApiSession {
    pub async fn from_env() -> Result<Self, AuthError> {
        ApiSessionBuilder::new().build().await
    }

    pub fn uses_development_token(&self) -> bool {
        self.inner.development_token.is_some()
    }

    pub fn enrollment(&self) -> EnrollmentStatus {
        EnrollmentStatus::from_material(self.inner.enrollment_path.clone(), &self.inner.material)
    }

    pub fn material(&self) -> &EnrollmentMaterial {
        &self.inner.material
    }

    pub fn enrollment_path(&self) -> &std::path::Path {
        &self.inner.enrollment_path
    }

    pub async fn ensure_access(&self) -> Result<(), AuthError> {
        self.bearer_token(false).await.map(|_| ())
    }

    pub fn invalidate(&self) {
        self.inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .cached = None;
    }

    pub async fn bearer_token(&self, force_refresh: bool) -> Result<String, AuthError> {
        if let Some(token) = self.inner.development_token.as_ref() {
            return Ok(token.clone());
        }
        if !force_refresh && let Some(token) = self.cached_fresh() {
            return Ok(token);
        }
        Ok(self.refresh_session(force_refresh).await?.access_token)
    }

    pub async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, RequestError> {
        self.send_json(Method::GET, path, None).await
    }

    pub async fn post_json<T: DeserializeOwned>(
        &self,
        path: &str,
        body: Value,
    ) -> Result<T, RequestError> {
        self.send_json(Method::POST, path, Some(body)).await
    }

    async fn send_json<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        json: Option<Value>,
    ) -> Result<T, RequestError> {
        let mut refreshed = false;
        loop {
            let token = self.bearer_token(refreshed).await?;
            let response = self
                .inner
                .transport
                .exchange(HttpRequest {
                    method: method.clone(),
                    path: path.to_string(),
                    bearer: Some(token),
                    json: json.clone(),
                })
                .await?;
            if response.status == StatusCode::UNAUTHORIZED && !refreshed {
                self.clear_cached_token().await;
                refreshed = true;
                continue;
            }
            return decode_response(response);
        }
    }

    fn now(&self) -> DateTime<Utc> {
        (self.inner.clock)()
    }

    fn cached_fresh(&self) -> Option<String> {
        let state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state
            .cached
            .as_ref()
            .filter(|cached| cached.is_fresh(self.now(), self.inner.refresh_skew))
            .map(|cached| cached.access_token.clone())
    }

    async fn clear_cached_token(&self) {
        self.invalidate();
    }

    async fn refresh_session(&self, force_refresh: bool) -> Result<CachedToken, AuthError> {
        let mut waiter;
        let leader;
        {
            let mut state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if !force_refresh
                && let Some(cached) = state
                    .cached
                    .as_ref()
                    .filter(|cached| cached.is_fresh(self.now(), self.inner.refresh_skew))
            {
                return Ok(cached.clone());
            }
            if let Some(inflight) = &state.inflight {
                waiter = inflight.subscribe();
                leader = false;
            } else {
                let (tx, rx) = broadcast::channel(1);
                state.inflight = Some(tx);
                waiter = rx;
                leader = true;
            }
        }
        if !leader {
            return waiter
                .recv()
                .await
                .map_err(|_| AuthError::Protocol("token refresh was cancelled".into()))?;
        }
        let result = self.exchange_challenge().await;
        {
            let mut state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match &result {
                Ok(cached) => state.cached = Some(cached.clone()),
                Err(_) => state.cached = None,
            }
            if let Some(tx) = state.inflight.take() {
                let _ = tx.send(result.clone());
            }
        }
        result
    }

    async fn exchange_challenge(&self) -> Result<CachedToken, AuthError> {
        let enrollment = self.enrollment();
        let challenge = self
            .post_unauthenticated::<ChallengeBody>(
                "/v1/auth/challenges",
                json!({ "deviceKeyId": self.inner.material.device_key_id }),
            )
            .await
            .map_err(|error| map_auth_status(error, Some(enrollment.clone())))?;
        let payload = base64_url::decode(&challenge.signing_payload).map_err(|_| {
            AuthError::Protocol("challenge signing payload is not valid base64url".into())
        })?;
        let signature = base64_url::encode(&self.inner.key.sign_sha256(&payload));
        let session = self
            .post_unauthenticated::<SessionBody>(
                "/v1/auth/sessions",
                json!({
                    "challengeId": challenge.challenge_id,
                    "signature": signature,
                }),
            )
            .await
            .map_err(|error| map_auth_status(error, Some(enrollment)))?;
        let expires_at = DateTime::parse_from_rfc3339(&session.expires_at)
            .map_err(|_| AuthError::Protocol("session expiry is not a valid timestamp".into()))?
            .with_timezone(&Utc);
        Ok(CachedToken {
            access_token: session.access_token,
            expires_at,
        })
    }

    async fn post_unauthenticated<T: DeserializeOwned>(
        &self,
        path: &str,
        body: Value,
    ) -> Result<T, RequestError> {
        let response = self
            .inner
            .transport
            .exchange(HttpRequest {
                method: Method::POST,
                path: path.to_string(),
                bearer: None,
                json: Some(body),
            })
            .await?;
        decode_response(response)
    }

    async fn validate_development_token(&self, token: &str) -> Result<(), AuthError> {
        let response = self
            .inner
            .transport
            .exchange(HttpRequest {
                method: Method::GET,
                path: "/v1/session".into(),
                bearer: Some(token.to_string()),
                json: None,
            })
            .await
            .map_err(|_| {
                AuthError::DevelopmentRefused(
                    "Couldn't reach Foyer to validate the development token. Try again.".into(),
                )
            })?;
        if response.is_success() {
            return Ok(());
        }
        Err(AuthError::DevelopmentRefused(
            "The development token was rejected. It is accepted only when Foyer Server runs in development.".into(),
        ))
    }
}

fn decode_response<T: DeserializeOwned>(
    response: crate::transport::HttpResponse,
) -> Result<T, RequestError> {
    if !response.is_success() {
        return Err(RequestError::Status {
            status: response.status,
            body: response.text(),
        });
    }
    Ok(serde_json::from_slice(&response.body)?)
}

fn map_auth_status(error: RequestError, enrollment: Option<EnrollmentStatus>) -> AuthError {
    match error {
        RequestError::Status { status, .. } => AuthError::from_http_status(status, enrollment),
        RequestError::Auth(error) => error,
        RequestError::Transport(_) => {
            AuthError::Transport("Couldn't reach Foyer. Check the connection and try again.".into())
        }
        RequestError::Decode(_) => AuthError::Protocol("invalid authentication response".into()),
    }
}

pub struct ApiSessionBuilder {
    keystore: Option<Arc<dyn DeviceKeyStore>>,
    transport: Option<Arc<dyn HttpTransport>>,
    enrollment_path: Option<PathBuf>,
    development_token: Option<String>,
    clock: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
    refresh_skew: Duration,
    validate_development: bool,
}

impl Default for ApiSessionBuilder {
    fn default() -> Self {
        Self {
            keystore: None,
            transport: None,
            enrollment_path: None,
            development_token: development_token_from_env(),
            clock: Arc::new(Utc::now),
            refresh_skew: REFRESH_SKEW,
            validate_development: true,
        }
    }
}

impl ApiSessionBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn keystore(mut self, keystore: impl DeviceKeyStore + 'static) -> Self {
        self.keystore = Some(Arc::new(keystore));
        self
    }

    pub fn transport(mut self, transport: impl HttpTransport + 'static) -> Self {
        self.transport = Some(Arc::new(transport));
        self
    }

    pub fn enrollment_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.enrollment_path = Some(path.into());
        self
    }

    pub fn development_token(mut self, token: Option<String>) -> Self {
        self.development_token = token.filter(|value| !value.is_empty());
        self
    }

    pub fn clock(mut self, clock: impl Fn() -> DateTime<Utc> + Send + Sync + 'static) -> Self {
        self.clock = Arc::new(clock);
        self
    }

    pub fn refresh_skew(mut self, skew: Duration) -> Self {
        self.refresh_skew = skew;
        self
    }

    pub fn validate_development(mut self, validate: bool) -> Self {
        self.validate_development = validate;
        self
    }

    pub async fn build(self) -> Result<ApiSession, AuthError> {
        let keystore = self
            .keystore
            .unwrap_or_else(|| Arc::new(SecretServiceKeyStore));
        let transport = match self.transport {
            Some(transport) => transport,
            None => Arc::new(ReqwestTransport::from_env()?),
        };
        let enrollment_path = self.enrollment_path.unwrap_or_else(default_enrollment_path);
        let key = keystore.load_or_create().await?;
        let material = key.material().clone();
        write_public_enrollment(&enrollment_path, &material)?;
        let session = ApiSession {
            inner: Arc::new(SessionInner {
                key,
                material,
                enrollment_path,
                development_token: self.development_token,
                transport,
                clock: self.clock,
                refresh_skew: self.refresh_skew,
                state: Mutex::new(TokenState {
                    cached: None,
                    inflight: None,
                }),
            }),
        };
        if session.uses_development_token() && self.validate_development {
            let token = session
                .inner
                .development_token
                .as_deref()
                .expect("development token is present");
            session.validate_development_token(token).await?;
        }
        Ok(session)
    }
}

pub fn development_auth_enabled() -> bool {
    env::var("FOYER_DEVELOPMENT_AUTH")
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

pub fn development_token_from_env() -> Option<String> {
    development_token_from(
        development_auth_enabled(),
        env::var("FOYER_DEV_TOKEN")
            .ok()
            .filter(|value| !value.is_empty()),
    )
}

pub(crate) fn development_token_from(
    development_auth: bool,
    token: Option<String>,
) -> Option<String> {
    if development_auth {
        return token.filter(|value| !value.is_empty());
    }
    if token.as_ref().is_some_and(|value| !value.is_empty()) {
        tracing::warn!(
            "FOYER_DEV_TOKEN is ignored unless FOYER_DEVELOPMENT_AUTH is set; production cannot silently use the development token"
        );
    }
    None
}
