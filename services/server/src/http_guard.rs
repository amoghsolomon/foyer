//! Bounded personal-deployment HTTP admission. Never trust forwarded IP headers for quotas.
use axum::{
    Json,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use futures_util::StreamExt;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;

const PER_MINUTE: usize = 300;
const CONCURRENT: usize = 64;
const BODY_BYTES: usize = 1024 * 1024;
#[derive(Clone)]
pub struct HttpGuard {
    admissions: Arc<Mutex<VecDeque<Instant>>>,
    active: Arc<Semaphore>,
}
impl Default for HttpGuard {
    fn default() -> Self {
        Self {
            admissions: Arc::new(Mutex::new(VecDeque::new())),
            active: Arc::new(Semaphore::new(CONCURRENT)),
        }
    }
}
impl HttpGuard {
    fn admit(&self, now: Instant) -> bool {
        let Ok(mut q) = self.admissions.lock() else {
            return false;
        };
        while q
            .front()
            .is_some_and(|t| now.duration_since(*t) >= Duration::from_secs(60))
        {
            q.pop_front();
        }
        if q.len() >= PER_MINUTE {
            return false;
        }
        q.push_back(now);
        true
    }
}
fn failure(status: StatusCode, code: &'static str, message: &'static str) -> Response {
    let mut response = (
        status,
        Json(serde_json::json!({"error":{"code":code,"message":message}})),
    )
        .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    if status == StatusCode::TOO_MANY_REQUESTS {
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, "60".parse().unwrap());
    }
    response
}
pub async fn guard(State(guard): State<HttpGuard>, request: Request, next: Next) -> Response {
    // Internal readiness must remain available during an API flood. The public Tunnel blocks it.
    if matches!(request.uri().path(), "/health/live" | "/health/ready") {
        return next.run(request).await;
    }
    if !guard.admit(Instant::now()) {
        return failure(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "Request budget exceeded. Retry later.",
        );
    }
    let Ok(permit) = guard.active.clone().try_acquire_owned() else {
        return failure(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "Too many active requests.",
        );
    };
    if request
        .headers()
        .get(header::AUTHORIZATION)
        .is_some_and(|v| v.as_bytes().len() > 4096)
    {
        return failure(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Authorization header exceeds the limit.",
        );
    }
    let (parts, body) = request.into_parts();
    let body = match tokio::time::timeout(Duration::from_secs(10), to_bytes(body, BODY_BYTES)).await
    {
        Err(_) => {
            return failure(
                StatusCode::REQUEST_TIMEOUT,
                "request_timeout",
                "Request body timed out.",
            );
        }
        Ok(Err(_)) => {
            return failure(
                StatusCode::PAYLOAD_TOO_LARGE,
                "too_large",
                "Request body exceeds the limit.",
            );
        }
        Ok(Ok(body)) => body,
    };
    let response = match tokio::time::timeout(
        Duration::from_secs(60),
        next.run(Request::from_parts(parts, Body::from(body))),
    )
    .await
    {
        Ok(response) => response,
        Err(_) => {
            return failure(
                StatusCode::GATEWAY_TIMEOUT,
                "request_timeout",
                "Request timed out.",
            );
        }
    };
    let (mut parts, body) = response.into_parts();
    parts
        .headers
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    parts
        .headers
        .insert("x-content-type-options", "nosniff".parse().unwrap());
    parts
        .headers
        .insert("referrer-policy", "no-referrer".parse().unwrap());
    // Retain the permit for the response lifetime, including slow readers and SSE streams.
    let stream = body.into_data_stream().map(move |chunk| {
        let _keep = &permit;
        chunk
    });
    Response::from_parts(parts, Body::from_stream(stream))
}
#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, routing::post};
    use tower::ServiceExt;
    fn app() -> Router {
        Router::new().route("/test", post(|| async { "ok" })).layer(
            axum::middleware::from_fn_with_state(HttpGuard::default(), guard),
        )
    }
    #[test]
    fn aggregate_budget_is_bounded_and_expires() {
        let g = HttpGuard::default();
        let now = Instant::now();
        for _ in 0..PER_MINUTE {
            assert!(g.admit(now));
        }
        assert!(!g.admit(now));
        assert!(g.admit(now + Duration::from_secs(61)));
        assert_eq!(g.admissions.lock().unwrap().len(), 1);
    }
    #[tokio::test]
    async fn slow_reader_and_oversize_body_are_bounded() {
        let app = app();
        let r = app
            .clone()
            .oneshot(
                Request::post("/test")
                    .body(Body::from(vec![0u8; BODY_BYTES + 1]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let r = app
            .clone()
            .oneshot(
                Request::post("/test")
                    .header("authorization", "x".repeat(4097))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        let mut retained = Vec::new();
        for _ in 0..CONCURRENT {
            retained.push(
                app.clone()
                    .oneshot(Request::post("/test").body(Body::empty()).unwrap())
                    .await
                    .unwrap(),
            );
        }
        let r = app
            .clone()
            .oneshot(Request::post("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::TOO_MANY_REQUESTS);
        drop(retained);
        assert_eq!(
            app.oneshot(Request::post("/test").body(Body::empty()).unwrap())
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
    }
    #[tokio::test(start_paused = true)]
    async fn stalled_upload_times_out() {
        let body = Body::from_stream(futures_util::stream::pending::<
            Result<axum::body::Bytes, std::io::Error>,
        >());
        let r = app()
            .oneshot(Request::post("/test").body(body).unwrap())
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::REQUEST_TIMEOUT);
    }
}
