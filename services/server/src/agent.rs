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
    let path = request
        .uri()
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/v1/agent");
    let target = agent
        .base
        .join(path)
        .map_err(|_| ApiError::invalid_request("Invalid agent route."))?;
    let method = request.method().clone();
    let body = to_bytes(request.into_body(), 32768)
        .await
        .map_err(|_| ApiError::invalid_request("Request exceeds 32768 bytes."))?;
    let upstream = agent
        .http
        .request(method, target)
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|_| ApiError::unavailable("Agent runtime is unavailable."))?;
    let status = upstream.status();
    let content_type = upstream.headers().get("content-type").cloned();
    // Reconnect periodically so an expired/revoked device cannot keep a stream forever.
    let stream = upstream
        .bytes_stream()
        .take_until(tokio::time::sleep(std::time::Duration::from_secs(240)));
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
}
