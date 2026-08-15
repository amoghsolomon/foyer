use std::path::PathBuf;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{TimeZone, Utc};
use foyer_server::auth::{
    DeviceAddStatus, DeviceRevokeStatus, TokenSigner, add_device, build_signing_payload,
    list_devices, parse_public_jwk, public_jwk_from_bytes, revoke_device, verify_p1363,
};
use foyer_server::config::RuntimeEnv;
use foyer_server::{AppState, Config, app_state, router};
use http_body_util::BodyExt;
use p256::SecretKey;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

const RFC_PUBLIC: &str =
    include_str!("../../../contracts/auth/v1/fixtures/rfc7517-public.jwk.json");
const RFC_PRIVATE: &str =
    include_str!("../../../contracts/auth/v1/fixtures/rfc7517-private.jwk.json");
const RFC_THUMBPRINT: &str = include_str!("../../../contracts/auth/v1/fixtures/thumbprint.txt");
const RFC_CANONICAL: &str = include_str!("../../../contracts/auth/v1/fixtures/canonical-jwk.json");

async fn test_database_url() -> Option<String> {
    if let Ok(url) = std::env::var("FOYER_TEST_DATABASE_URL")
        && !url.is_empty()
    {
        return Some(url);
    }
    start_postgres_container().await
}

async fn start_postgres_container() -> Option<String> {
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::postgres::Postgres;

    let container = Postgres::default().start().await.ok()?;
    let host = container.get_host().await.ok()?;
    let port = container.get_host_port_ipv4(5432).await.ok()?;
    let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");
    std::mem::forget(container);
    Some(url)
}

async fn wait_for_postgres(url: &str) -> Result<(), sqlx::Error> {
    for _ in 0..40 {
        if PgPool::connect(url).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    PgPool::connect(url).await.map(|_| ())
}

async fn test_state() -> Option<AppState> {
    let url = test_database_url().await?;
    wait_for_postgres(&url).await.ok()?;
    Some(
        app_state(Config::test_development(url))
            .await
            .expect("app state"),
    )
}

async fn call(
    state: &AppState,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let request_body = if let Some(body) = body {
        builder = builder.header("content-type", "application/json");
        Body::from(body.to_string())
    } else {
        Body::empty()
    };
    let response = router(state.clone())
        .oneshot(builder.body(request_body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).into()))
    };
    (status, json)
}

fn rfc_public() -> foyer_server::auth::PublicJwk {
    parse_public_jwk(&serde_json::from_str(RFC_PUBLIC).unwrap()).expect("rfc public")
}

fn rfc_signing_key() -> SigningKey {
    let value: Value = serde_json::from_str(RFC_PRIVATE).unwrap();
    let d = URL_SAFE_NO_PAD
        .decode(value["d"].as_str().unwrap())
        .expect("d");
    SigningKey::from(SecretKey::from_slice(&d).expect("secret"))
}

fn sign_payload(payload: &[u8]) -> String {
    let signature: Signature = rfc_signing_key().sign(payload);
    URL_SAFE_NO_PAD.encode(signature.to_bytes())
}

fn fixture_payload() -> Vec<u8> {
    let expires = Utc.with_ymd_and_hms(2026, 1, 1, 0, 1, 0).unwrap();
    build_signing_payload(RFC_THUMBPRINT.trim(), "foyer-api", expires, &[0x11; 32])
}

#[test]
fn fixture_thumbprint_and_signature_are_stable() {
    let jwk = rfc_public();
    assert_eq!(jwk.canonical_json().trim(), RFC_CANONICAL.trim());
    assert_eq!(jwk.device_key_id(), RFC_THUMBPRINT.trim());
    let payload = fixture_payload();
    let signature = sign_payload(&payload);
    verify_p1363(&jwk, &payload, &signature).expect("rfc signature");
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/auth/v1/fixtures/signature.b64");
    let expected = std::fs::read_to_string(fixture_path).expect("signature fixture");
    assert_eq!(signature, expected.trim());
    let payload_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/auth/v1/fixtures/signing-payload.b64");
    let stored_payload = std::fs::read_to_string(payload_path).expect("payload fixture");
    assert_eq!(URL_SAFE_NO_PAD.encode(&payload), stored_payload.trim());
}

#[test]
fn malformed_jwk_and_signature_are_rejected() {
    assert!(public_jwk_from_bytes(br#"{"kty":"oct","k":"QQ"}"#).is_err());
    assert!(public_jwk_from_bytes(br#""not-an-object""#).is_err());
    let payload = fixture_payload();
    assert!(verify_p1363(&rfc_public(), &payload, "$$$$").is_err());
    assert!(verify_p1363(&rfc_public(), &payload, "YWJjZA").is_err());
    let der_looking = URL_SAFE_NO_PAD.encode([0x30_u8; 70]);
    assert!(verify_p1363(&rfc_public(), &payload, &der_looking).is_err());
}

#[tokio::test]
async fn challenge_session_and_admin_lifecycle() {
    let Some(state) = test_state().await else {
        eprintln!("skipping challenge_session_and_admin_lifecycle: PostgreSQL is unavailable");
        return;
    };
    let jwk = rfc_public();
    let added = add_device(&state.pool, "owner", "phone", &jwk)
        .await
        .expect("enroll");
    assert_eq!(added.status, DeviceAddStatus::Created);
    assert_eq!(added.device_key_id, RFC_THUMBPRINT.trim());

    let again = add_device(&state.pool, "owner", "phone", &jwk)
        .await
        .expect("idempotent");
    assert_eq!(again.status, DeviceAddStatus::Unchanged);

    let relabeled = add_device(&state.pool, "owner", "Pixel", &jwk)
        .await
        .expect("relabel");
    assert_eq!(relabeled.status, DeviceAddStatus::Updated);

    let (status, unknown) = call(
        &state,
        "POST",
        "/v1/auth/challenges",
        None,
        Some(json!({ "deviceKeyId": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{unknown}");
    assert!(unknown["challengeId"].as_str().is_some());
    let (status, unknown_session) = call(
        &state,
        "POST",
        "/v1/auth/sessions",
        None,
        Some(json!({
            "challengeId": unknown["challengeId"],
            "signature": sign_payload(&fixture_payload()),
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{unknown_session}");
    assert_eq!(unknown_session["error"]["code"], "unauthenticated");

    let (status, challenge) = call(
        &state,
        "POST",
        "/v1/auth/challenges",
        None,
        Some(json!({ "deviceKeyId": RFC_THUMBPRINT.trim() })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{challenge}");
    let payload = URL_SAFE_NO_PAD
        .decode(challenge["signingPayload"].as_str().unwrap())
        .expect("payload");
    assert!(payload.starts_with(b"FOYER-AUTH-CHALLENGE-V1"));
    let signature = sign_payload(&payload);

    let (status, malformed) = call(
        &state,
        "POST",
        "/v1/auth/sessions",
        None,
        Some(json!({
            "challengeId": challenge["challengeId"],
            "signature": "not-a-signature",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{malformed}");

    let (status, session) = call(
        &state,
        "POST",
        "/v1/auth/sessions",
        None,
        Some(json!({
            "challengeId": challenge["challengeId"],
            "signature": signature,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    assert_eq!(session["tokenType"], "Bearer");
    assert_eq!(session["userId"], "owner");
    assert_eq!(session["deviceKeyId"], RFC_THUMBPRINT.trim());
    let access = session["accessToken"].as_str().unwrap();

    let (status, replay) = call(
        &state,
        "POST",
        "/v1/auth/sessions",
        None,
        Some(json!({
            "challengeId": challenge["challengeId"],
            "signature": signature,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{replay}");

    let (status, me) = call(&state, "GET", "/v1/session", Some(access), None).await;
    assert_eq!(status, StatusCode::OK, "{me}");
    assert_eq!(me["userId"], "owner");
    assert_eq!(me["deviceKeyId"], RFC_THUMBPRINT.trim());
    assert_eq!(me["development"], true);

    let (status, creds) = call(&state, "GET", "/v1/sync/credentials", Some(access), None).await;
    assert_eq!(status, StatusCode::OK, "{creds}");
    let header = jsonwebtoken::decode_header(creds["token"].as_str().unwrap()).expect("header");
    assert_eq!(header.alg, jsonwebtoken::Algorithm::ES256);
    assert_eq!(header.kid.as_deref(), Some("foyer-dev"));
    let sync_claims = state
        .signer
        .decode(creds["token"].as_str().unwrap(), "foyer-powersync")
        .expect("powersync claims");
    assert_eq!(sync_claims.sub, "owner");
    assert_eq!(sync_claims.aud, "foyer-powersync");
    assert_eq!(
        sync_claims.device_key_id.as_deref(),
        Some(RFC_THUMBPRINT.trim())
    );
    assert!(
        state
            .signer
            .decode(creds["token"].as_str().unwrap(), "foyer-api")
            .is_err()
    );

    let (status, _) = call(
        &state,
        "GET",
        "/v1/sync/credentials",
        Some(creds["token"].as_str().unwrap()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let expired_id = "00000000-0000-4000-8000-000000000001";
    sqlx::query(
        "INSERT INTO auth_challenges (
            challenge_id, device_key_id, user_id, signing_payload, payload_sha256,
            expires_at, created_at
         ) VALUES ($1, $2, $3, $4, $5, now() - interval '2 minutes', now() - interval '3 minutes')",
    )
    .bind(expired_id)
    .bind(RFC_THUMBPRINT.trim())
    .bind("owner")
    .bind(&payload)
    .bind(&[0_u8; 32][..])
    .execute(&state.pool)
    .await
    .expect("expired challenge");
    let (status, expired) = call(
        &state,
        "POST",
        "/v1/auth/sessions",
        None,
        Some(json!({
            "challengeId": expired_id,
            "signature": signature,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{expired}");

    revoke_device(&state.pool, RFC_THUMBPRINT.trim())
        .await
        .expect("revoke");
    let (status, after_revoke) = call(
        &state,
        "POST",
        "/v1/auth/challenges",
        None,
        Some(json!({ "deviceKeyId": RFC_THUMBPRINT.trim() })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{after_revoke}");
    let revoked_payload = URL_SAFE_NO_PAD
        .decode(after_revoke["signingPayload"].as_str().unwrap())
        .unwrap();
    let (status, revoked_session) = call(
        &state,
        "POST",
        "/v1/auth/sessions",
        None,
        Some(json!({
            "challengeId": after_revoke["challengeId"],
            "signature": sign_payload(&revoked_payload),
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{revoked_session}");

    let listed = list_devices(&state.pool, Some("owner"))
        .await
        .expect("list");
    assert_eq!(listed.len(), 1);
    assert!(listed[0].revoked_at.is_some());
    let again = revoke_device(&state.pool, RFC_THUMBPRINT.trim())
        .await
        .expect("already revoked");
    assert_eq!(again.status, DeviceRevokeStatus::AlreadyRevoked);
}

#[tokio::test]
async fn concurrent_challenge_consumption_is_single_use() {
    let Some(state) = test_state().await else {
        eprintln!("skipping concurrent_challenge_consumption_is_single_use");
        return;
    };
    add_device(&state.pool, "owner", "phone", &rfc_public())
        .await
        .expect("enroll");
    let (status, challenge) = call(
        &state,
        "POST",
        "/v1/auth/challenges",
        None,
        Some(json!({ "deviceKeyId": RFC_THUMBPRINT.trim() })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{challenge}");
    let payload = URL_SAFE_NO_PAD
        .decode(challenge["signingPayload"].as_str().unwrap())
        .unwrap();
    let signature = sign_payload(&payload);
    let body = json!({
        "challengeId": challenge["challengeId"],
        "signature": signature,
    });
    let first = call(
        &state,
        "POST",
        "/v1/auth/sessions",
        None,
        Some(body.clone()),
    );
    let second = call(&state, "POST", "/v1/auth/sessions", None, Some(body));
    let (first, second) = tokio::join!(first, second);
    let statuses = [first.0, second.0];
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == StatusCode::OK)
            .count(),
        1,
        "{first:?} {second:?}"
    );
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == StatusCode::UNAUTHORIZED)
            .count(),
        1,
        "{first:?} {second:?}"
    );
}

#[tokio::test]
async fn development_static_token_is_isolated_from_production() {
    let Some(mut state) = test_state().await else {
        eprintln!("skipping development_static_token_is_isolated_from_production");
        return;
    };
    let (status, session) = call(&state, "GET", "/v1/session", Some("dev-token"), None).await;
    assert_eq!(status, StatusCode::OK, "{session}");
    assert_eq!(session["userId"], "dev-user");
    assert!(session.get("deviceKeyId").is_none());

    let (status, creds) = call(
        &state,
        "GET",
        "/v1/sync/credentials",
        Some("dev-token"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{creds}");
    let claims = state
        .signer
        .decode(creds["token"].as_str().unwrap(), "foyer-powersync")
        .expect("dev powersync");
    assert_eq!(claims.sub, "dev-user");
    assert!(claims.device_key_id.is_none());

    state.config.runtime_env = RuntimeEnv::Production;
    state.config.dev_users.clear();
    let (status, denied) = call(&state, "GET", "/v1/session", Some("dev-token"), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{denied}");
}

#[test]
fn production_signer_requires_a_real_pem() {
    let mut config = Config::test_development("postgres://foyer:foyer@127.0.0.1:5432/foyer");
    config.runtime_env = RuntimeEnv::Production;
    config.dev_users.clear();
    assert!(TokenSigner::from_config(&config).is_err());
}
