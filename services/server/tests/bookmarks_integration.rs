//! Bookmarks semantic-command suite.
//!
//! Mounts `bookmarks::router` locally so this file can run before `lib.rs` is wired.
//! After the shared integration step, `foyer_server::router` can replace the local mount.

use std::time::Duration;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use foyer_server::bookmarks;
use foyer_server::config::DevUser;
use foyer_server::{AppState, Config, app_state};
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
    let response = bookmarks::router(state.clone())
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

fn create_bookmark_body(
    id: &str,
    folder_id: &str,
    url: &str,
    title: &str,
    description: &str,
    tags: &[&str],
) -> Value {
    json!({
        "operationId": uuid(),
        "id": id,
        "folderId": folder_id,
        "url": url,
        "title": title,
        "description": description,
        "tags": tags,
    })
}

#[tokio::test]
async fn bookmarks_vertical_slice() {
    let Some(state) = test_state().await else {
        eprintln!("skipping bookmarks_vertical_slice: PostgreSQL is unavailable");
        return;
    };

    let reading_id = uuid();
    let (status, reading) = call(
        &state,
        "POST",
        "/v1/bookmark-folders",
        DEV_TOKEN,
        Some(create_folder_body(&reading_id, "Reading", None)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reading}");
    assert_eq!(reading["name"], "Reading");
    assert_eq!(reading["revision"], 1);
    assert!(reading["deletedAt"].is_null());

    let nested_id = uuid();
    let (status, nested) = call(
        &state,
        "POST",
        "/v1/bookmark-folders",
        DEV_TOKEN,
        Some(create_folder_body(&nested_id, "Rust", Some(&reading_id))),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{nested}");
    assert_eq!(nested["parentId"], reading_id);

    let description = "Keep <script>alert(1)</script> and a trailing newline.\n";
    let bookmark_id = uuid();
    let create = create_bookmark_body(
        &bookmark_id,
        &reading_id,
        "HTTPS://Example.COM/docs?q=1",
        "Example docs",
        description,
        &["  Work ", "WORK", "docs"],
    );
    let (status, created) = call(
        &state,
        "POST",
        "/v1/bookmarks",
        DEV_TOKEN,
        Some(create.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    assert_eq!(created["url"], "https://Example.COM/docs?q=1");
    assert_eq!(created["description"], description);
    assert_eq!(created["tags"], json!(["work", "docs"]));
    assert_eq!(created["favorite"], false);
    assert_eq!(created["archived"], false);
    assert_eq!(created["revision"], 1);

    let (status, replay) = call(
        &state,
        "POST",
        "/v1/bookmarks",
        DEV_TOKEN,
        Some(create.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["revision"], created["revision"]);
    assert_eq!(replay["description"], description);
    assert_eq!(replay["tags"], json!(["work", "docs"]));

    let mut changed_retry = create_bookmark_body(
        &bookmark_id,
        &reading_id,
        "https://example.com/changed",
        "Changed retry",
        "different",
        &["other"],
    );
    changed_retry["operationId"] = create["operationId"].clone();
    let (status, changed_retry_error) = call(
        &state,
        "POST",
        "/v1/bookmarks",
        DEV_TOKEN,
        Some(changed_retry),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{changed_retry_error}");

    let (status, cross_user_replay) =
        call(&state, "POST", "/v1/bookmarks", OTHER_TOKEN, Some(create)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{cross_user_replay}");

    let (status, conflict) = call(
        &state,
        "POST",
        &format!("/v1/bookmarks/{bookmark_id}/update"),
        DEV_TOKEN,
        Some(json!({
            "operationId": uuid(),
            "expectedRevision": 99,
            "url": "https://example.com/stale",
            "title": "stale",
            "description": "stale",
            "tags": []
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
    assert_eq!(conflict["error"]["code"], "stale_revision");

    let (status, invalid_url) = call(
        &state,
        "POST",
        "/v1/bookmarks",
        DEV_TOKEN,
        Some(create_bookmark_body(
            &uuid(),
            &reading_id,
            "javascript:alert(1)",
            "Nope",
            "",
            &[],
        )),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{invalid_url}");

    let (status, ftp) = call(
        &state,
        "POST",
        "/v1/bookmarks",
        DEV_TOKEN,
        Some(create_bookmark_body(
            &uuid(),
            &reading_id,
            "ftp://example.com/file",
            "FTP",
            "",
            &[],
        )),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{ftp}");

    let missing_parent = uuid();
    let (status, invalid_parent) = call(
        &state,
        "POST",
        "/v1/bookmark-folders",
        DEV_TOKEN,
        Some(create_folder_body(&uuid(), "Orphan", Some(&missing_parent))),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{invalid_parent}");
    assert_eq!(invalid_parent["error"]["code"], "invalid_parent");

    let (status, cycle) = call(
        &state,
        "POST",
        &format!("/v1/bookmark-folders/{reading_id}/move"),
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
        &format!("/v1/bookmark-folders/{reading_id}/delete"),
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
        &format!("/v1/bookmarks/{bookmark_id}/move"),
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
    assert_eq!(moved["description"], description);

    let (status, favored) = call(
        &state,
        "POST",
        &format!("/v1/bookmarks/{bookmark_id}/favorite"),
        DEV_TOKEN,
        Some(json!({
            "operationId": uuid(),
            "expectedRevision": 2,
            "favorite": true
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{favored}");
    assert_eq!(favored["favorite"], true);
    assert_eq!(favored["revision"], 3);
    assert_eq!(favored["description"], description);

    let (status, archived) = call(
        &state,
        "POST",
        &format!("/v1/bookmarks/{bookmark_id}/archive"),
        DEV_TOKEN,
        Some(json!({
            "operationId": uuid(),
            "expectedRevision": 3,
            "archived": true
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{archived}");
    assert_eq!(archived["archived"], true);
    assert_eq!(archived["revision"], 4);

    let (status, favorites) = call(
        &state,
        "GET",
        "/v1/bookmarks?favorite=true&archived=true",
        DEV_TOKEN,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{favorites}");
    assert_eq!(favorites["bookmarks"].as_array().unwrap().len(), 1);
    assert_eq!(favorites["bookmarks"][0]["id"], bookmark_id);

    let (status, tagged) = call(&state, "GET", "/v1/bookmarks?tag=work", DEV_TOKEN, None).await;
    assert_eq!(status, StatusCode::OK, "{tagged}");
    assert!(
        tagged["bookmarks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == bookmark_id)
    );

    let (status, searched) = call(
        &state,
        "GET",
        "/v1/bookmarks?q=trailing%20newline",
        DEV_TOKEN,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{searched}");
    assert_eq!(searched["bookmarks"][0]["description"], description);

    let (status, deleted) = call(
        &state,
        "POST",
        &format!("/v1/bookmarks/{bookmark_id}/delete"),
        DEV_TOKEN,
        Some(json!({
            "operationId": uuid(),
            "expectedRevision": 4
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{deleted}");
    assert!(!deleted["deletedAt"].is_null());

    let (status, listed) = call(&state, "GET", "/v1/bookmarks", DEV_TOKEN, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        listed["bookmarks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|bookmark| bookmark["id"] != bookmark_id)
    );

    let (status, gone) = call(
        &state,
        "GET",
        &format!("/v1/bookmarks/{bookmark_id}"),
        DEV_TOKEN,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::GONE, "{gone}");

    let (status, resurrect) = call(
        &state,
        "POST",
        "/v1/bookmarks",
        DEV_TOKEN,
        Some(create_bookmark_body(
            &bookmark_id,
            &nested_id,
            "https://example.com/resurrect",
            "Resurrect",
            "nope",
            &[],
        )),
    )
    .await;
    assert_eq!(status, StatusCode::GONE, "{resurrect}");

    let foreign_folder = uuid();
    let (status, other_folder) = call(
        &state,
        "POST",
        "/v1/bookmark-folders",
        OTHER_TOKEN,
        Some(create_folder_body(&foreign_folder, "Other inbox", None)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{other_folder}");

    let (status, isolated) = call(
        &state,
        "GET",
        &format!("/v1/bookmark-folders/{foreign_folder}"),
        DEV_TOKEN,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{isolated}");

    let (status, cross_write) = call(
        &state,
        "POST",
        "/v1/bookmarks",
        DEV_TOKEN,
        Some(create_bookmark_body(
            &uuid(),
            &foreign_folder,
            "https://example.com/secret",
            "Nope",
            "secret",
            &[],
        )),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{cross_write}");
    assert_eq!(cross_write["error"]["code"], "invalid_parent");

    let (status, other_list) = call(&state, "GET", "/v1/bookmark-folders", OTHER_TOKEN, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        other_list["folders"]
            .as_array()
            .unwrap()
            .iter()
            .all(|folder| folder["id"] == foreign_folder)
    );

    let persisted_id = uuid();
    let lossless = "Line one\n\nKeep trailing spaces   \n";
    let (status, _) = call(
        &state,
        "POST",
        "/v1/bookmarks",
        DEV_TOKEN,
        Some(create_bookmark_body(
            &persisted_id,
            &nested_id,
            "https://example.com/persisted",
            "Persisted",
            lossless,
            &["keep"],
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
        &format!("/v1/bookmarks/{persisted_id}"),
        DEV_TOKEN,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(after_restart["description"], lossless);
    assert_eq!(after_restart["tags"], json!(["keep"]));

    let (status, folders) = call(&restarted, "GET", "/v1/bookmark-folders", DEV_TOKEN, None).await;
    let (status_bookmarks, bookmarks) =
        call(&restarted, "GET", "/v1/bookmarks", DEV_TOKEN, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(status_bookmarks, StatusCode::OK);
    let rebuilt_folders = folders["folders"].as_array().unwrap();
    let rebuilt_bookmarks = bookmarks["bookmarks"].as_array().unwrap();
    assert!(
        rebuilt_folders
            .iter()
            .any(|folder| folder["id"] == reading_id)
    );
    assert!(
        rebuilt_folders
            .iter()
            .any(|folder| folder["id"] == nested_id)
    );
    assert!(
        rebuilt_bookmarks
            .iter()
            .any(|bookmark| bookmark["id"] == persisted_id)
    );
    assert!(
        rebuilt_bookmarks
            .iter()
            .all(|bookmark| bookmark["id"] != bookmark_id)
    );

    let oversized = "n".repeat(bookmarks::MAX_DESCRIPTION_BYTES + 1);
    let (status, too_big) = call(
        &restarted,
        "POST",
        "/v1/bookmarks",
        DEV_TOKEN,
        Some(create_bookmark_body(
            &uuid(),
            &nested_id,
            "https://example.com/huge",
            "Huge",
            &oversized,
            &[],
        )),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{too_big}");

    let empty_nested = uuid();
    let (status, _) = call(
        &restarted,
        "POST",
        "/v1/bookmark-folders",
        DEV_TOKEN,
        Some(create_folder_body(
            &empty_nested,
            "Empty",
            Some(&reading_id),
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, removed) = call(
        &restarted,
        "POST",
        &format!("/v1/bookmark-folders/{empty_nested}/delete"),
        DEV_TOKEN,
        Some(json!({
            "operationId": uuid(),
            "expectedRevision": 1
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{removed}");
    assert!(!removed["deletedAt"].is_null());
}

#[test]
fn local_validators_cover_url_tags_and_description() {
    assert!(bookmarks::validate_bookmark_url("https://example.com").is_ok());
    assert!(bookmarks::validate_bookmark_url("http://127.0.0.1/a").is_ok());
    assert!(bookmarks::validate_bookmark_url("javascript:alert(1)").is_err());
    assert!(bookmarks::validate_bookmark_url("https://").is_err());
    let description = "lossless\n";
    assert_eq!(
        bookmarks::validate_description(description).unwrap(),
        description
    );
    assert_eq!(
        bookmarks::normalize_tags(&["A".into(), "a".into(), "B".into()]).unwrap(),
        vec!["a", "b"]
    );
}
