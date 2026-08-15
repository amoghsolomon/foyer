use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use reqwest::StatusCode;
use serde_json::{Value, json};
use tokio::sync::Notify;

use crate::error::{AuthError, RequestError};
use crate::jwk::PublicJwk;
use crate::keystore::{DeviceSigningKey, MemoryKeyStore, UnavailableKeyStore};
use crate::transport::{HttpRequest, HttpResponse, HttpTransport};
use crate::{ApiSession, ApiSessionBuilder, decode_base64url, encode_base64url};

struct MutableClock {
    now: Mutex<DateTime<Utc>>,
}

impl MutableClock {
    fn new(now: DateTime<Utc>) -> Arc<Self> {
        Arc::new(Self {
            now: Mutex::new(now),
        })
    }

    fn get(&self) -> DateTime<Utc> {
        *self.now.lock().expect("clock")
    }

    fn set(&self, now: DateTime<Utc>) {
        *self.now.lock().expect("clock") = now;
    }
}

struct ChallengeGate {
    released: std::sync::atomic::AtomicBool,
    notify: Notify,
}

impl ChallengeGate {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            released: std::sync::atomic::AtomicBool::new(false),
            notify: Notify::new(),
        })
    }

    async fn wait(&self) {
        if self.released.load(Ordering::SeqCst) {
            return;
        }
        let notified = self.notify.notified();
        if self.released.load(Ordering::SeqCst) {
            return;
        }
        notified.await;
    }

    fn release(&self) {
        self.released.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }
}

struct ScriptedTransport {
    started: AtomicUsize,
    challenges: AtomicUsize,
    requests: Mutex<Vec<HttpRequest>>,
    responses: Mutex<VecDeque<Result<HttpResponse, RequestError>>>,
    before_challenge: Option<Arc<ChallengeGate>>,
}

impl ScriptedTransport {
    fn new(responses: Vec<Result<HttpResponse, RequestError>>) -> Arc<Self> {
        Arc::new(Self {
            started: AtomicUsize::new(0),
            challenges: AtomicUsize::new(0),
            requests: Mutex::new(Vec::new()),
            responses: Mutex::new(responses.into()),
            before_challenge: None,
        })
    }

    fn gated(
        responses: Vec<Result<HttpResponse, RequestError>>,
        gate: Arc<ChallengeGate>,
    ) -> Arc<Self> {
        Arc::new(Self {
            started: AtomicUsize::new(0),
            challenges: AtomicUsize::new(0),
            requests: Mutex::new(Vec::new()),
            responses: Mutex::new(responses.into()),
            before_challenge: Some(gate),
        })
    }

    fn json(status: StatusCode, body: Value) -> Result<HttpResponse, RequestError> {
        Ok(HttpResponse {
            status,
            body: serde_json::to_vec(&body).expect("json"),
        })
    }

    fn challenge(n: u32) -> Result<HttpResponse, RequestError> {
        Self::json(
            StatusCode::OK,
            json!({
                "challengeId": format!("challenge-{n}"),
                "signingPayload": encode_base64url(format!("payload-{n}").as_bytes()),
                "expiresAt": "2026-08-15T12:01:00Z",
            }),
        )
    }

    fn session(n: u32) -> Result<HttpResponse, RequestError> {
        Self::json(
            StatusCode::OK,
            json!({
                "accessToken": format!("token-{n}"),
                "tokenType": "Bearer",
                "expiresAt": "2026-08-15T12:05:00Z",
                "userId": "user-1",
                "deviceKeyId": "cn-I_WNMClehiVp51i_0VpOENW1upEerA8sEam5hn-s",
            }),
        )
    }
}

#[async_trait]
impl HttpTransport for ScriptedTransport {
    async fn exchange(&self, request: HttpRequest) -> Result<HttpResponse, RequestError> {
        if request.path == "/v1/auth/challenges" {
            self.started.fetch_add(1, Ordering::SeqCst);
            if let Some(gate) = &self.before_challenge {
                gate.wait().await;
            }
            self.challenges.fetch_add(1, Ordering::SeqCst);
        }
        self.requests.lock().expect("requests").push(request);
        self.responses
            .lock()
            .expect("responses")
            .pop_front()
            .unwrap_or_else(|| {
                Err(RequestError::Status {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    body: "no scripted response".into(),
                })
            })
    }
}

fn fixture_key() -> DeviceSigningKey {
    DeviceSigningKey::generate().expect("key")
}

fn enrollment_path() -> std::path::PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    std::env::temp_dir().join(format!("foyer-auth-session-{unique}.json"))
}

async fn session_with(transport: Arc<ScriptedTransport>, clock: Arc<MutableClock>) -> ApiSession {
    let path = enrollment_path();
    ApiSessionBuilder::new()
        .keystore(MemoryKeyStore::with_key(fixture_key()))
        .transport(OwnedTransport(transport))
        .enrollment_path(path)
        .development_token(None)
        .clock(move || clock.get())
        .refresh_skew(Duration::from_secs(60))
        .validate_development(false)
        .build()
        .await
        .expect("session")
}

struct OwnedTransport(Arc<ScriptedTransport>);

#[async_trait]
impl HttpTransport for OwnedTransport {
    async fn exchange(&self, request: HttpRequest) -> Result<HttpResponse, RequestError> {
        self.0.exchange(request).await
    }
}

#[tokio::test]
async fn refreshes_shortly_before_expiry_and_reuses_fresh_token() {
    let transport = ScriptedTransport::new(vec![
        ScriptedTransport::challenge(1),
        ScriptedTransport::session(1),
        ScriptedTransport::challenge(2),
        ScriptedTransport::session(2),
    ]);
    let clock = MutableClock::new(Utc.with_ymd_and_hms(2026, 8, 15, 12, 0, 0).unwrap());
    let session = session_with(transport.clone(), clock.clone()).await;
    assert_eq!(session.bearer_token(false).await.expect("token"), "token-1");
    assert_eq!(transport.challenges.load(Ordering::SeqCst), 1);
    clock.set(Utc.with_ymd_and_hms(2026, 8, 15, 12, 2, 0).unwrap());
    assert_eq!(session.bearer_token(false).await.expect("token"), "token-1");
    assert_eq!(transport.challenges.load(Ordering::SeqCst), 1);
    clock.set(Utc.with_ymd_and_hms(2026, 8, 15, 12, 4, 20).unwrap());
    assert_eq!(session.bearer_token(false).await.expect("token"), "token-2");
    assert_eq!(transport.challenges.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn coalesces_concurrent_refresh() {
    let gate = ChallengeGate::new();
    let transport = ScriptedTransport::gated(
        vec![
            ScriptedTransport::challenge(1),
            ScriptedTransport::session(1),
        ],
        gate.clone(),
    );
    let clock = MutableClock::new(Utc.with_ymd_and_hms(2026, 8, 15, 12, 0, 0).unwrap());
    let session = session_with(transport.clone(), clock).await;
    let first = {
        let session = session.clone();
        tokio::spawn(async move { session.bearer_token(false).await })
    };
    let second = {
        let session = session.clone();
        tokio::spawn(async move { session.bearer_token(false).await })
    };
    while transport.started.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
    }
    assert_eq!(transport.started.load(Ordering::SeqCst), 1);
    gate.release();
    assert_eq!(first.await.expect("join").expect("token"), "token-1");
    assert_eq!(second.await.expect("join").expect("token"), "token-1");
    assert_eq!(transport.challenges.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn force_refresh_after_invalidate_issues_a_new_challenge() {
    let transport = ScriptedTransport::new(vec![
        ScriptedTransport::challenge(1),
        ScriptedTransport::session(1),
        ScriptedTransport::challenge(2),
        ScriptedTransport::session(2),
    ]);
    let clock = MutableClock::new(Utc.with_ymd_and_hms(2026, 8, 15, 12, 0, 0).unwrap());
    let session = session_with(transport.clone(), clock).await;
    assert_eq!(session.bearer_token(false).await.expect("token"), "token-1");
    session.invalidate();
    assert_eq!(session.bearer_token(true).await.expect("token"), "token-2");
    assert_eq!(transport.challenges.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn unauthorized_challenge_is_not_enrolled() {
    let transport = ScriptedTransport::new(vec![ScriptedTransport::json(
        StatusCode::UNAUTHORIZED,
        json!({"error":{"code":"unknown_device"}}),
    )]);
    let clock = MutableClock::new(Utc.with_ymd_and_hms(2026, 8, 15, 12, 0, 0).unwrap());
    let session = session_with(transport, clock).await;
    let error = session.ensure_access().await.expect_err("not enrolled");
    assert!(error.is_not_enrolled());
    let message = error.public_message();
    assert!(message.contains("not enrolled"));
    assert!(message.contains("Fingerprint"));
    assert!(message.contains(session.enrollment_path().to_string_lossy().as_ref()));
    assert!(!message.contains("unknown_device"));
    assert!(!message.to_ascii_lowercase().contains("private"));
}

#[tokio::test]
async fn retries_unauthorized_data_request_once() {
    let transport = ScriptedTransport::new(vec![
        ScriptedTransport::challenge(1),
        ScriptedTransport::session(1),
        ScriptedTransport::json(StatusCode::UNAUTHORIZED, json!({"error":"stale"})),
        ScriptedTransport::challenge(2),
        ScriptedTransport::session(2),
        ScriptedTransport::json(StatusCode::OK, json!({"ok":true})),
    ]);
    let clock = MutableClock::new(Utc.with_ymd_and_hms(2026, 8, 15, 12, 0, 0).unwrap());
    let session = session_with(transport.clone(), clock).await;
    let body: Value = session.get_json("/v1/notes").await.expect("notes");
    assert_eq!(body["ok"], true);
    let requests = transport.requests.lock().expect("requests");
    let notes = requests
        .iter()
        .filter(|request| request.path == "/v1/notes")
        .collect::<Vec<_>>();
    assert_eq!(notes.len(), 2);
    assert_eq!(notes[0].bearer.as_deref(), Some("token-1"));
    assert_eq!(notes[1].bearer.as_deref(), Some("token-2"));
}

#[tokio::test]
async fn does_not_retry_a_second_unauthorized_response() {
    let transport = ScriptedTransport::new(vec![
        ScriptedTransport::challenge(1),
        ScriptedTransport::session(1),
        ScriptedTransport::json(StatusCode::UNAUTHORIZED, json!({})),
        ScriptedTransport::challenge(2),
        ScriptedTransport::session(2),
        ScriptedTransport::json(StatusCode::UNAUTHORIZED, json!({})),
    ]);
    let clock = MutableClock::new(Utc.with_ymd_and_hms(2026, 8, 15, 12, 0, 0).unwrap());
    let session = session_with(transport.clone(), clock).await;
    let error = session
        .get_json::<Value>("/v1/notes")
        .await
        .expect_err("still unauthorized");
    assert_eq!(error.status(), Some(StatusCode::UNAUTHORIZED));
    let notes = transport
        .requests
        .lock()
        .expect("requests")
        .iter()
        .filter(|request| request.path == "/v1/notes")
        .count();
    assert_eq!(notes, 2);
}

#[tokio::test]
async fn development_token_is_ignored_without_explicit_mode() {
    let token = crate::development_token_from_env();
    if std::env::var("FOYER_DEVELOPMENT_AUTH").is_err() {
        assert!(token.is_none() || crate::development_auth_enabled());
    }
}

#[tokio::test]
async fn development_token_skips_challenge() {
    let transport = ScriptedTransport::new(vec![ScriptedTransport::json(
        StatusCode::OK,
        json!({"userId":"dev-user","development":true}),
    )]);
    let path = enrollment_path();
    let session = ApiSessionBuilder::new()
        .keystore(MemoryKeyStore::with_key(fixture_key()))
        .transport(OwnedTransport(transport.clone()))
        .enrollment_path(&path)
        .development_token(Some("dev-token".into()))
        .validate_development(true)
        .build()
        .await
        .expect("dev session");
    assert!(session.uses_development_token());
    assert_eq!(
        session.bearer_token(false).await.expect("token"),
        "dev-token"
    );
    assert_eq!(transport.challenges.load(Ordering::SeqCst), 0);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn missing_secret_service_fails_visibly() {
    let error = ApiSessionBuilder::new()
        .keystore(UnavailableKeyStore)
        .transport(OwnedTransport(ScriptedTransport::new(vec![])))
        .enrollment_path(enrollment_path())
        .development_token(None)
        .validate_development(false)
        .build()
        .await
        .expect_err("secret service");
    assert!(matches!(error, AuthError::KeyStore(_)));
    assert!(error.public_message().contains("Secret Service"));
    assert!(
        !error
            .public_message()
            .to_ascii_lowercase()
            .contains("private")
    );
}

#[tokio::test]
async fn challenge_signature_is_p1363_base64url() {
    let key = fixture_key();
    let expected = encode_base64url(&key.sign_sha256(b"payload-1"));
    let transport = ScriptedTransport::new(vec![
        ScriptedTransport::challenge(1),
        ScriptedTransport::session(1),
    ]);
    let clock = MutableClock::new(Utc.with_ymd_and_hms(2026, 8, 15, 12, 0, 0).unwrap());
    let path = enrollment_path();
    let session = ApiSessionBuilder::new()
        .keystore(MemoryKeyStore::with_key(key))
        .transport(OwnedTransport(transport.clone()))
        .enrollment_path(&path)
        .development_token(None)
        .clock(move || clock.get())
        .validate_development(false)
        .build()
        .await
        .expect("session");
    session.ensure_access().await.expect("token");
    let requests = transport.requests.lock().expect("requests");
    let session_req = requests
        .iter()
        .find(|request| request.path == "/v1/auth/sessions")
        .expect("session request");
    let signature = session_req
        .json
        .as_ref()
        .and_then(|body| body.get("signature"))
        .and_then(Value::as_str)
        .expect("signature");
    assert_eq!(signature, expected);
    assert!(!signature.contains('='));
    assert_eq!(decode_base64url(signature).expect("sig").len(), 64);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn enrollment_errors_never_include_secrets() {
    let transport = ScriptedTransport::new(vec![ScriptedTransport::json(
        StatusCode::FORBIDDEN,
        json!({"error":{"code":"planted-secret-token-value","signature":"abc"}}),
    )]);
    let clock = MutableClock::new(Utc.with_ymd_and_hms(2026, 8, 15, 12, 0, 0).unwrap());
    let session = session_with(transport, clock).await;
    let error = session.ensure_access().await.expect_err("forbidden");
    let message = error.public_message();
    assert!(!message.contains("planted-secret-token-value"));
    assert!(!message.contains("signature"));
    assert!(
        PublicJwk::p256(
            "MKBCTNIcKUSDii11ySs3526iDZ8AiTo7Tu6KPAqv7D4",
            "4Etl6SRW2YiLUrN5vfvVHuhp7x8PxltmWWlbbM4IFyM",
        )
        .is_ok()
    );
}

#[test]
fn development_token_requires_explicit_mode() {
    assert_eq!(
        crate::session::development_token_from(false, Some("secret-token".into())),
        None
    );
    assert_eq!(
        crate::session::development_token_from(true, Some("secret-token".into())),
        Some("secret-token".into())
    );
    assert_eq!(crate::session::development_token_from(true, None), None);
}
