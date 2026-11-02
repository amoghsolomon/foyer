//! The public agent boundary authenticates the same enrolled devices as personal data.
//! Its single-thread runtime is a private loopback sidecar, never a public service.
use crate::{
    AppState,
    auth::Principal,
    error::{ApiError, ApiResult},
};
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::StreamExt;

#[derive(Clone)]
pub struct Client {
    pub owner: String,
    base: reqwest::Url,
    http: reqwest::Client,
}
impl Client {
    pub fn new(base: &str, owner: String) -> Result<Self, String> {
        let base = reqwest::Url::parse(base).map_err(|_| "Invalid FOYER_AGENT_URL")?;
        // Keep the unauthenticated sidecar on this pod's loopback boundary.
        if base.scheme() != "http"
            || !matches!(base.host_str(), Some("127.0.0.1" | "[::1]"))
            || base.path() != "/"
            || base.query().is_some()
            || base.fragment().is_some()
            || !base.username().is_empty()
            || base.password().is_some()
            || owner.trim().is_empty()
        {
            return Err("FOYER_AGENT_URL must be an HTTP loopback origin and FOYER_AGENT_OWNER_ID is required".into());
        }
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(3))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| "Create agent HTTP client")?;
        Ok(Self { owner, base, http })
    }
    pub fn from_env() -> Result<Option<Self>, String> {
        match (
            std::env::var("FOYER_AGENT_URL").ok(),
            std::env::var("FOYER_AGENT_OWNER_ID").ok(),
        ) {
            (None, None) => Ok(None),
            (Some(url), Some(owner)) => Self::new(&url, owner).map(Some),
            _ => Err("Set FOYER_AGENT_URL and FOYER_AGENT_OWNER_ID together".into()),
        }
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/agent", get(proxy))
        .route("/v1/agent/history", get(proxy))
        .route("/v1/agent/events", get(proxy))
        .route("/v1/agent/messages", post(proxy))
        .route("/v1/agent/cancel", post(proxy))
        .route("/v1/agent/submissions/{id}", get(proxy))
}
async fn proxy(
    State(state): State<AppState>,
    principal: Principal,
    request: Request,
) -> ApiResult<Response> {
    let agent = state
        .agent
        .as_ref()
        .ok_or_else(|| ApiError::unavailable("Agent runtime is not configured."))?;
    if principal.user_id != agent.owner {
        return Err(ApiError::not_found("Agent thread not found."));
    }
    let stream_deadline = tokio::time::Instant::now() + stream_lifetime(&state, &request)?;
    let path = request
        .uri()
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/v1/agent");
    let target = agent
        .base
        .join(path)
        .map_err(|_| ApiError::invalid_request("Invalid agent route."))?;
    let is_not_stream = request_is_not_stream(path);
    let method = request.method().clone();
    let body = to_bytes(request.into_body(), 32768)
        .await
        .map_err(|_| ApiError::invalid_request("Request exceeds 32768 bytes."))?;
    let mut upstream_request = agent.http.request(method, target);
    if is_not_stream {
        upstream_request = upstream_request.timeout(std::time::Duration::from_secs(30));
    }
    let upstream = upstream_request
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|_| ApiError::unavailable("Agent runtime is unavailable."))?;
    let status = upstream.status();
    let content_type = upstream.headers().get("content-type").cloned();
    // End the stream at the signed token expiry, or sooner at the 240-second ceiling.
    let stream = upstream
        .bytes_stream()
        .take_until(tokio::time::sleep_until(stream_deadline));
    let mut response = (status, Body::from_stream(stream)).into_response();
    if let Some(value) = content_type {
        response.headers_mut().insert("content-type", value);
    }
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response
        .headers_mut()
        .insert("x-accel-buffering", "no".parse().unwrap());
    Ok(response)
}

fn request_is_not_stream(path: &str) -> bool {
    path.split('?').next() != Some("/v1/agent/events")
}
fn stream_lifetime(state: &AppState, request: &Request) -> ApiResult<std::time::Duration> {
    let maximum = std::time::Duration::from_secs(240);
    let token = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.strip_prefix("Bearer ")
                .or_else(|| v.strip_prefix("bearer "))
        });
    if let Some(token) = token.filter(|t| crate::auth::looks_like_jwt(t)) {
        let claims = state
            .signer
            .decode(token, &state.config.auth_api_audience)?;
        let remaining = claims.exp.saturating_mul(1000) - chrono::Utc::now().timestamp_millis();
        if remaining <= 0 {
            return Err(ApiError::authentication_failed());
        }
        return Ok(maximum.min(std::time::Duration::from_millis(remaining as u64)));
    }
    Ok(maximum)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sidecar_origin_is_strictly_loopback() {
        for url in [
            "http://evil.test/",
            "https://127.0.0.1/",
            "http://127.0.0.1/foo",
            "http://x@127.0.0.1/",
            "http://127.0.0.1/?x",
        ] {
            assert!(Client::new(url, "owner".into()).is_err());
        }
        assert!(Client::new("http://127.0.0.1:3584/", "owner".into()).is_ok());
    }
    #[tokio::test]
    async fn a_signed_stream_cannot_outlive_its_token() {
        use p256::pkcs8::EncodePrivateKey;
        let secret = p256::SecretKey::random(&mut rand::rngs::OsRng);
        let pem = secret.to_pkcs8_pem(p256::pkcs8::LineEnding::LF).unwrap();
        let config = crate::Config::test_development("postgres://test:test@127.0.0.1/test");
        let signer = crate::auth::TokenSigner::from_pem(
            &pem,
            config.auth_key_id.clone(),
            config.auth_issuer.clone(),
            config.auth_api_audience.clone(),
            config.powersync_audience.clone(),
        )
        .unwrap();
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy(&config.database_url)
            .unwrap();
        let state = crate::AppState {
            config,
            signer,
            pool: pool.clone(),
            auth_limiter: crate::auth::AuthLimiter::new(),
            http_guard: crate::http_guard::HttpGuard::default(),
            dav: None,
            projector: crate::dav::Projector::new(pool),
            agent: None,
        };
        let now = chrono::Utc::now().timestamp();
        let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::ES256);
        header.kid = Some(state.signer.key_id.clone());
        for (expires, expected_max) in [(now + 2, 2), (now + 300, 240)] {
            let claims = serde_json::json!({"sub":"owner","aud":state.signer.api_audience,"iss":state.signer.issuer,"iat":now,"exp":expires,"jti":"stream-test","deviceKeyId":"test-device"});
            let token = jsonwebtoken::encode(
                &header,
                &claims,
                &jsonwebtoken::EncodingKey::from_ec_pem(pem.as_bytes()).unwrap(),
            )
            .unwrap();
            let request = Request::builder()
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap();
            assert!(
                stream_lifetime(&state, &request).unwrap()
                    <= std::time::Duration::from_secs(expected_max)
            );
        }
    }
}
