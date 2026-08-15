use std::time::Duration;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use foyer_server::config::{DevUser, RuntimeEnv};
use foyer_server::{AppState, Config, app_state, notes::MAX_NOTE_BODY_BYTES, router};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

const DEV_TOKEN: &str = "dev-token";
const OTHER_TOKEN: &str = "other-token";

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
    // Keep the container alive for the process by leaking it.
    std::mem::forget(container);
    Some(url)
}

async fn test_state() -> Option<AppState> {
    let url = test_database_url().await?;
    wait_for_postgres(&url).await.ok()?;
    let mut config = Config::test_development(url);
    config.dev_users = vec![
        DevUser {
            user_id: "dev-user".into(),
            token: DEV_TOKEN.into(),
        },
        DevUser {
            user_id: "other-user".into(),
            token: OTHER_TOKEN.into(),
        },
    ];
    Some(app_state(config).await.expect("app state"))
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

async fn call(
    state: &AppState,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"));
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

fn uuid() -> String {
    Uuid::new_v4().to_string()
}

fn create_folder_body(id: &str, name: &str, parent_id: Option<&str>) -> Value {
    let mut body = json!({
        "operationId": uuid(),
        "id": id,
        "name": name,
    });
    if let Some(parent_id) = parent_id {
        body["parentId"] = json!(parent_id);
    }
    body
}

fn create_note_body(id: &str, folder_id: &str, title: &str, body: &str) -> Value {
    json!({
        "operationId": uuid(),
        "id": id,
        "folderId": folder_id,
        "title": title,
        "body": body,
    })
}

#[tokio::test]
async fn notes_vertical_slice() {
    let Some(state) = test_state().await else {
        eprintln!("skipping notes_vertical_slice: PostgreSQL is unavailable");
        return;
    };

    let (status, live) = call(&state, "GET", "/health/live", DEV_TOKEN, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(live["status"], "ok");

    let (status, ready) = call(&state, "GET", "/health/ready", DEV_TOKEN, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ready["status"], "ok");

    let (status, session) = call(&state, "GET", "/v1/session", DEV_TOKEN, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(session["userId"], "dev-user");
    assert_eq!(session["development"], true);

    let (status, _) = call(&state, "GET", "/v1/session", "wrong", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let inbox_id = uuid();
    let (status, inbox) = call(
        &state,
        "POST",
        "/v1/folders",
        DEV_TOKEN,
        Some(create_folder_body(&inbox_id, "Inbox", None)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{inbox}");
    assert_eq!(inbox["name"], "Inbox");
    assert_eq!(inbox["revision"], 1);
    assert!(inbox["deletedAt"].is_null());

    let nested_id = uuid();
    let (status, nested) = call(
        &state,
        "POST",
        "/v1/folders",
        DEV_TOKEN,
        Some(create_folder_body(&nested_id, "Projects", Some(&inbox_id))),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{nested}");
    assert_eq!(nested["parentId"], inbox_id);

    let markdown = "# Heading\n\nKeep <script>alert(1)</script> and **bold** losslessly.\n\n- item";
    let note_id = uuid();
    let create_note = create_note_body(&note_id, &inbox_id, "First note", markdown);
    let (status, created) = call(
        &state,
        "POST",
        "/v1/notes",
        DEV_TOKEN,
        Some(create_note.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    assert_eq!(created["body"], markdown);
    assert_eq!(created["title"], "First note");

    let (status, replay) = call(
        &state,
        "POST",
        "/v1/notes",
        DEV_TOKEN,
        Some(create_note.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay["revision"], created["revision"]);
    assert_eq!(replay["body"], markdown);

    let mut changed_retry = create_note_body(&note_id, &inbox_id, "Changed retry", "different");
    // Bind a known operation id explicitly: changing any part of the request must not replay.
    changed_retry["operationId"] = create_note["operationId"].clone();
    let (status, changed_retry_error) = call(
        &state,
        "POST",
        "/v1/notes",
        DEV_TOKEN,
        Some(changed_retry.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{changed_retry_error}");

    let (status, cross_user_replay) =
        call(&state, "POST", "/v1/notes", OTHER_TOKEN, Some(create_note)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{cross_user_replay}");

    let (status, conflict) = call(
        &state,
        "POST",
        &format!("/v1/notes/{note_id}/update"),
        DEV_TOKEN,
        Some(json!({
            "operationId": uuid(),
            "expectedRevision": 99,
            "title": "stale",
            "body": "stale"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
    assert_eq!(conflict["error"]["code"], "stale_revision");

    let missing_parent = uuid();
    let (status, invalid_parent) = call(
        &state,
        "POST",
        "/v1/folders",
        DEV_TOKEN,
        Some(create_folder_body(&uuid(), "Orphan", Some(&missing_parent))),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{invalid_parent}");
    assert_eq!(invalid_parent["error"]["code"], "invalid_parent");

    let (status, cycle) = call(
        &state,
        "POST",
        &format!("/v1/folders/{inbox_id}/move"),
        DEV_TOKEN,
        Some(json!({
            "operationId": uuid(),
            "expectedRevision": 1,
            "parentId": nested_id
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{cycle}");
    assert_eq!(cycle["error"]["code"], "cycle");

    let (status, nonempty) = call(
        &state,
        "POST",
        &format!("/v1/folders/{inbox_id}/delete"),
        DEV_TOKEN,
        Some(json!({
            "operationId": uuid(),
            "expectedRevision": 1
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{nonempty}");
    assert_eq!(nonempty["error"]["code"], "folder_not_empty");

    let (status, moved) = call(
        &state,
        "POST",
        &format!("/v1/notes/{note_id}/move"),
        DEV_TOKEN,
        Some(json!({
            "operationId": uuid(),
            "expectedRevision": 1,
            "folderId": nested_id
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(moved["folderId"], nested_id);
    assert_eq!(moved["revision"], 2);
    assert_eq!(moved["body"], markdown);

    let (status, deleted) = call(
        &state,
        "POST",
        &format!("/v1/notes/{note_id}/delete"),
        DEV_TOKEN,
        Some(json!({
            "operationId": uuid(),
            "expectedRevision": 2
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{deleted}");
    assert!(!deleted["deletedAt"].is_null());

    let (status, listed) = call(&state, "GET", "/v1/notes", DEV_TOKEN, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        listed["notes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|note| note["id"] != note_id)
    );

    let (status, gone) = call(
        &state,
        "GET",
        &format!("/v1/notes/{note_id}"),
        DEV_TOKEN,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::GONE, "{gone}");

    let (status, resurrect) = call(
        &state,
        "POST",
        "/v1/notes",
        DEV_TOKEN,
        Some(create_note_body(&note_id, &nested_id, "Resurrect", "nope")),
    )
    .await;
    assert_eq!(status, StatusCode::GONE, "{resurrect}");

    let foreign_folder = uuid();
    let (status, other_folder) = call(
        &state,
        "POST",
        "/v1/folders",
        OTHER_TOKEN,
        Some(create_folder_body(&foreign_folder, "Other inbox", None)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{other_folder}");

    let (status, isolated) = call(
        &state,
        "GET",
        &format!("/v1/folders/{foreign_folder}"),
        DEV_TOKEN,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{isolated}");

    let (status, cross_write) = call(
        &state,
        "POST",
        "/v1/notes",
        DEV_TOKEN,
        Some(create_note_body(&uuid(), &foreign_folder, "Nope", "secret")),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{cross_write}");
    assert_eq!(cross_write["error"]["code"], "invalid_parent");

    let (status, other_list) = call(&state, "GET", "/v1/folders", OTHER_TOKEN, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        other_list["folders"]
            .as_array()
            .unwrap()
            .iter()
            .all(|folder| folder["id"] == foreign_folder)
    );

    let persisted_note = uuid();
    let (status, _) = call(
        &state,
        "POST",
        "/v1/notes",
        DEV_TOKEN,
        Some(create_note_body(
            &persisted_note,
            &nested_id,
            "Persisted",
            "survives restart",
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let restarted = app_state(state.config.clone())
        .await
        .expect("reopened pool");
    let (status, after_restart) = call(
        &restarted,
        "GET",
        &format!("/v1/notes/{persisted_note}"),
        DEV_TOKEN,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(after_restart["body"], "survives restart");

    let (status, folders) = call(&restarted, "GET", "/v1/folders", DEV_TOKEN, None).await;
    let (status_notes, notes) = call(&restarted, "GET", "/v1/notes", DEV_TOKEN, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(status_notes, StatusCode::OK);
    let rebuilt_folders = folders["folders"].as_array().unwrap();
    let rebuilt_notes = notes["notes"].as_array().unwrap();
    assert!(
        rebuilt_folders
            .iter()
            .any(|folder| folder["id"] == inbox_id)
    );
    assert!(
        rebuilt_folders
            .iter()
            .any(|folder| folder["id"] == nested_id)
    );
    assert!(
        rebuilt_notes
            .iter()
            .any(|note| note["id"] == persisted_note)
    );
    assert!(rebuilt_notes.iter().all(|note| note["id"] != note_id));
    assert!(
        rebuilt_notes
            .iter()
            .find(|note| note["id"] == persisted_note)
            .unwrap()["body"]
            == "survives restart"
    );

    let oversized = "n".repeat(MAX_NOTE_BODY_BYTES + 1);
    let (status, too_big) = call(
        &restarted,
        "POST",
        "/v1/notes",
        DEV_TOKEN,
        Some(create_note_body(&uuid(), &nested_id, "Huge", &oversized)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{too_big}");
}

#[tokio::test]
async fn development_jwks_is_hidden_outside_development() {
    let Some(mut state) = test_state().await else {
        eprintln!("skipping development_jwks_is_hidden_outside_development");
        return;
    };
    let (status, jwks) = call(&state, "GET", "/v1/dev/jwks", DEV_TOKEN, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(jwks["keys"][0]["kty"], "EC");
    assert!(jwks["keys"][0].get("k").is_none());

    let (status, public_jwks) = call(&state, "GET", "/v1/auth/jwks", DEV_TOKEN, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(public_jwks["keys"][0]["alg"], "ES256");

    state.config.runtime_env = RuntimeEnv::Production;
    state.config.dev_users.clear();
    let (status, _) = call(&state, "GET", "/v1/dev/jwks", DEV_TOKEN, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, production_jwks) = call(&state, "GET", "/v1/auth/jwks", DEV_TOKEN, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(production_jwks["keys"][0]["kty"], "EC");
    let (status, _) = call(&state, "GET", "/v1/session", DEV_TOKEN, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
