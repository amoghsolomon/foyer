//! Required live regressions. scripts/test-server runs these against isolated PostgreSQL and Radicale.
//! Missing services are failures, never silently successful tests.
use axum::{
    Json,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use foyer_server::config::{DavSettings, DevUser};
use foyer_server::dav::{
    CollectionKind, DavHref, DavMediaType, DavPayload, NewAddressBook, NewCalendar, PutPrecondition,
};
use foyer_server::{AppState, Config, app_state, calendar, contacts, router, tasks};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::future::IntoFuture;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU8, Ordering},
};
use tower::ServiceExt;
use uuid::Uuid;

fn id() -> String {
    Uuid::new_v4().to_string()
}
async fn state() -> AppState {
    let database = std::env::var("FOYER_TEST_DATABASE_URL")
        .expect("Run scripts/test-server: PostgreSQL is required");
    let mut config = Config::test_development(database);
    config.dev_users = vec![DevUser {
        user_id: format!("live-{}", id()),
        token: "test-token".into(),
    }];
    config.dav = Some(DavSettings {
        base_url: std::env::var("FOYER_DAV_URL").expect("Radicale is required"),
        username: std::env::var("FOYER_DAV_USERNAME").expect("DAV user required"),
        password: std::env::var("FOYER_DAV_PASSWORD").expect("DAV password required"),
    });
    app_state(config)
        .await
        .expect("Live services and migrations must work")
}
fn user(state: &AppState) -> &str {
    &state.config.dev_users[0].user_id
}
async fn api(state: &AppState, path: &str, body: Value) -> (StatusCode, Value) {
    let request = axum::http::Request::post(path)
        .header("content-type", "application/json")
        .header("authorization", "Bearer test-token")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = router(state.clone()).oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&body).unwrap())
}
async fn put(state: &AppState, owner: &str, href: &DavHref, text: String, kind: DavMediaType) {
    state
        .dav_client()
        .unwrap()
        .put_resource(
            owner,
            href,
            &DavPayload::from_raw(kind, text).unwrap(),
            PutPrecondition::IfNoneMatchStar,
        )
        .await
        .unwrap();
}
fn event(uid: &str, summary: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Foyer tests//EN\r\nBEGIN:VEVENT\r\nUID:{uid}\r\nDTSTAMP:20261102T000000Z\r\nDTSTART:20261103T100000Z\r\nSUMMARY:{summary}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
    )
}
fn todo(uid: &str, title: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Foyer tests//EN\r\nBEGIN:VTODO\r\nUID:{uid}\r\nDTSTAMP:20261102T000000Z\r\nSUMMARY:{title}\r\nEND:VTODO\r\nEND:VCALENDAR\r\n"
    )
}

#[tokio::test]
async fn calendar_reconciles_twice_preserves_client_id_and_cascades_external_deletion() {
    let state = state().await;
    let uid = user(&state);
    let calendar_id = id();
    let event_id = id();
    let (status, collection) = api(
        &state,
        "/v1/calendars",
        json!({"operationId":id(),"id":calendar_id,"displayName":"Regression"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{collection}");
    let (status, created) = api(&state,"/v1/events",json!({"operationId":id(),"id":event_id,"calendarId":calendar_id,"summary":"Original","allDay":false,"dtstart":"20261103T100000","tzid":"UTC"})).await;
    assert_eq!(status, StatusCode::OK, "{created}");
    for _ in 0..2 {
        calendar::reconcile_user(&state, uid).await.unwrap();
    }
    let (stored_id,revision):(String,i64)=sqlx::query_as("SELECT id,revision FROM calendar_events WHERE user_id=$1 AND uid=$2 AND deleted_at IS NULL").bind(uid).bind(&event_id).fetch_one(&state.pool).await.unwrap();
    assert_eq!(stored_id, event_id);
    assert_eq!(revision, 1);
    let client = state.dav_client().unwrap();
    let href = DavHref::parse(created["href"].as_str().unwrap()).unwrap();
    let resource = client
        .get_resource(uid, &href, CollectionKind::Calendar)
        .await
        .unwrap();
    let changed = resource
        .payload
        .raw()
        .replace("SUMMARY:Original", "SUMMARY:External update");
    client
        .put_resource(
            uid,
            &href,
            &DavPayload::from_raw(DavMediaType::ICalendar, changed).unwrap(),
            PutPrecondition::IfMatch(resource.etag),
        )
        .await
        .unwrap();
    for _ in 0..2 {
        calendar::reconcile_user(&state, uid).await.unwrap();
    }
    let row: (String, i64, String) = sqlx::query_as(
        "SELECT id,revision,summary FROM calendar_events WHERE user_id=$1 AND deleted_at IS NULL",
    )
    .bind(uid)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(row, (event_id, 2, "External update".into()));
    client
        .delete_collection(
            uid,
            &DavHref::parse(collection["href"].as_str().unwrap()).unwrap(),
            None,
        )
        .await
        .unwrap();
    calendar::reconcile_user(&state, uid).await.unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM calendar_events WHERE user_id=$1 AND deleted_at IS NULL",
    )
    .bind(uid)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn same_dav_uids_and_collection_names_are_isolated_between_users() {
    let state = state().await;
    let a = user(&state).to_owned();
    let b = format!("live-{}", id());
    let shared = id();
    let collection = id();
    let client = state.dav_client().unwrap();
    for (owner, title) in [(&a, "Alice"), (&b, "Bob")] {
        let list = client
            .create_task_list(
                owner,
                &NewCalendar {
                    collection_id: collection.clone(),
                    display_name: "Tasks".into(),
                },
            )
            .await
            .unwrap();
        put(
            &state,
            owner,
            &DavHref::parse(&format!("{}task.ics", list.href.as_str())).unwrap(),
            todo(&shared, title),
            DavMediaType::ICalendar,
        )
        .await;
        let cal = client
            .create_calendar(
                owner,
                &NewCalendar {
                    collection_id: collection.clone(),
                    display_name: "Calendar".into(),
                },
            )
            .await
            .unwrap();
        put(
            &state,
            owner,
            &DavHref::parse(&format!("{}event.ics", cal.href.as_str())).unwrap(),
            event(&shared, title),
            DavMediaType::ICalendar,
        )
        .await;
        let book = client
            .create_address_book(
                owner,
                &NewAddressBook {
                    collection_id: collection.clone(),
                    display_name: "Contacts".into(),
                },
            )
            .await
            .unwrap();
        put(
            &state,
            owner,
            &DavHref::parse(&format!("{}contact.vcf", book.href.as_str())).unwrap(),
            format!("BEGIN:VCARD\r\nVERSION:3.0\r\nUID:{shared}\r\nFN:{title}\r\nEND:VCARD\r\n"),
            DavMediaType::VCard,
        )
        .await;
        tasks::reconcile_user(&state, owner).await.unwrap();
        calendar::reconcile_user(&state, owner).await.unwrap();
        contacts::reconcile_user(&state, owner).await.unwrap();
    }
    for table in ["tasks", "calendar_events", "contacts"] {
        let rows: Vec<(String, String)> = sqlx::query_as(&format!(
            "SELECT id,user_id FROM {table} WHERE user_id=ANY($1) AND deleted_at IS NULL"
        ))
        .bind(vec![&a, &b])
        .fetch_all(&state.pool)
        .await
        .unwrap();
        assert_eq!(rows.len(), 2, "{table}");
        assert_ne!(rows[0].0, rows[1].0, "{table}");
    }
    let title: String =
        sqlx::query_scalar("SELECT title FROM tasks WHERE user_id=$1 AND deleted_at IS NULL")
            .bind(&a)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert_eq!(title, "Alice");
    client
        .delete_collection(
            &a,
            &DavHref::parse(&format!("/{a}/tasks/{collection}/")).unwrap(),
            None,
        )
        .await
        .unwrap();
    tasks::reconcile_user(&state, &a).await.unwrap();
    let counts:Vec<(String,i64)>=sqlx::query_as("SELECT user_id,COUNT(*) FROM tasks WHERE user_id=ANY($1) AND deleted_at IS NULL GROUP BY user_id")
        .bind(vec![&a,&b]).fetch_all(&state.pool).await.unwrap();
    assert_eq!(counts, vec![(b, 1)]);
}

#[derive(Clone)]
struct FaultProxy {
    target: String,
    username: String,
    password: String,
    mode: Arc<AtomicU8>,
    href: Arc<Mutex<String>>,
}
async fn forward(State(proxy): State<FaultProxy>, request: Request) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let body = to_bytes(request.into_body(), 1024 * 1024).await.unwrap();
    let mode = proxy.mode.load(Ordering::SeqCst);
    if mode == 1 && method == "GET" {
        return (StatusCode::SERVICE_UNAVAILABLE, "injected read outage").into_response();
    }
    if mode == 2
        && method == "REPORT"
        && String::from_utf8_lossy(&body).contains("calendar-multiget")
    {
        let href = proxy.href.lock().unwrap().clone();
        return (StatusCode::MULTI_STATUS,[("content-type","application/xml")],format!("<d:multistatus xmlns:d=\"DAV:\"><d:response><d:href>{href}</d:href><d:status>HTTP/1.1 503 Service Unavailable</d:status></d:response></d:multistatus>")).into_response();
    }
    // This proxy fixture intentionally handles reads only; writes go directly to real Radicale.
    let upstream = reqwest::Client::new()
        .request(method, format!("{}{path}", proxy.target))
        .basic_auth(&proxy.username, Some(&proxy.password))
        .header("Depth", "1")
        .header("content-type", "application/xml")
        .body(body)
        .send()
        .await
        .unwrap();
    let status = upstream.status();
    let content_type = upstream.headers().get("content-type").cloned();
    let bytes = upstream.bytes().await.unwrap();
    let mut response = (status, bytes).into_response();
    if let Some(content_type) = content_type {
        response.headers_mut().insert("content-type", content_type);
    }
    response
}

#[tokio::test]
async fn transient_resource_read_failures_preserve_rows_and_retry_checkpoint() {
    let state = state().await;
    let owner = user(&state);
    let client = state.dav_client().unwrap();
    let collection = id();
    let event_uid = id();
    let cal = client
        .create_calendar(
            owner,
            &NewCalendar {
                collection_id: collection.clone(),
                display_name: "Faults".into(),
            },
        )
        .await
        .unwrap();
    let href = DavHref::parse(&format!("{}event.ics", cal.href.as_str())).unwrap();
    put(
        &state,
        owner,
        &href,
        event(&event_uid, "Before"),
        DavMediaType::ICalendar,
    )
    .await;
    let list = client
        .create_task_list(
            owner,
            &NewCalendar {
                collection_id: collection.clone(),
                display_name: "Faults".into(),
            },
        )
        .await
        .unwrap();
    let task_href = DavHref::parse(&format!("{}task.ics", list.href.as_str())).unwrap();
    put(
        &state,
        owner,
        &task_href,
        todo(&id(), "Keep me"),
        DavMediaType::ICalendar,
    )
    .await;
    calendar::reconcile_user(&state, owner).await.unwrap();
    tasks::reconcile_user(&state, owner).await.unwrap();
    let token: Option<String> = sqlx::query_scalar(
        "SELECT sync_token FROM dav_collection_checkpoints WHERE user_id=$1 AND collection_href=$2",
    )
    .bind(owner)
    .bind(cal.href.as_str())
    .fetch_one(&state.pool)
    .await
    .unwrap();
    let settings = state.config.dav.as_ref().unwrap();
    let proxy = FaultProxy {
        target: settings.base_url.clone(),
        username: settings.username.clone(),
        password: settings.password.clone(),
        mode: Arc::new(AtomicU8::new(0)),
        href: Arc::new(Mutex::new(href.as_str().to_owned())),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(
        axum::serve(
            listener,
            axum::Router::new()
                .fallback(forward)
                .with_state(proxy.clone()),
        )
        .into_future(),
    );
    let mut faulty = state.clone();
    faulty.dav = Some(
        foyer_server::dav::DavClient::new(
            foyer_server::dav::DavConfig::new(
                format!("http://{address}"),
                settings.username.clone(),
                settings.password.clone(),
            )
            .unwrap(),
        )
        .unwrap(),
    );
    proxy.mode.store(1, Ordering::SeqCst);
    assert!(tasks::reconcile_user(&faulty, owner).await.is_err());
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tasks WHERE user_id=$1 AND deleted_at IS NULL")
            .bind(owner)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
    let resource = client
        .get_resource(owner, &href, CollectionKind::Calendar)
        .await
        .unwrap();
    client
        .put_resource(
            owner,
            &href,
            &DavPayload::from_raw(DavMediaType::ICalendar, event(&event_uid, "After")).unwrap(),
            PutPrecondition::IfMatch(resource.etag),
        )
        .await
        .unwrap();
    proxy.mode.store(2, Ordering::SeqCst);
    assert!(calendar::reconcile_user(&faulty, owner).await.is_err());
    let after: Option<String> = sqlx::query_scalar(
        "SELECT sync_token FROM dav_collection_checkpoints WHERE user_id=$1 AND collection_href=$2",
    )
    .bind(owner)
    .bind(cal.href.as_str())
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(token, after);
    let summary: String = sqlx::query_scalar(
        "SELECT summary FROM calendar_events WHERE user_id=$1 AND deleted_at IS NULL",
    )
    .bind(owner)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(summary, "Before");
    calendar::reconcile_user(&state, owner).await.unwrap();
    let summary: String = sqlx::query_scalar(
        "SELECT summary FROM calendar_events WHERE user_id=$1 AND deleted_at IS NULL",
    )
    .bind(owner)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(summary, "After");
    server.abort();
}

#[tokio::test]
async fn accepted_dav_write_retries_after_projection_rollback_and_rejects_changed_arguments() {
    let state = state().await;
    let calendar_id = id();
    let event_id = id();
    let (_, calendar) = api(
        &state,
        "/v1/calendars",
        json!({"operationId":id(),"id":calendar_id,"displayName":"Crash"}),
    )
    .await;
    assert!(calendar["id"].is_string());
    let (_,event)=api(&state,"/v1/events",json!({"operationId":id(),"id":event_id,"calendarId":calendar_id,"summary":"Before","allDay":true,"dtstart":"20261103"})).await;
    assert!(event["id"].is_string());
    sqlx::raw_sql("CREATE OR REPLACE FUNCTION foyer_test_reject_projection() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.summary = 'Accepted before rollback' THEN RAISE EXCEPTION 'injected projection failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER foyer_test_reject_projection BEFORE UPDATE ON calendar_events FOR EACH ROW EXECUTE FUNCTION foyer_test_reject_projection();").execute(&state.pool).await.unwrap();
    let operation = id();
    let body = json!({"operationId":operation,"expectedRevision":1,"summary":"Accepted before rollback","allDay":true,"dtstart":"20261103"});
    let (status, _) = api(
        &state,
        &format!("/v1/events/{event_id}/update"),
        body.clone(),
    )
    .await;
    assert!(status.is_server_error());
    sqlx::raw_sql("DROP TRIGGER foyer_test_reject_projection ON calendar_events; DROP FUNCTION foyer_test_reject_projection();").execute(&state.pool).await.unwrap();
    let (status, recovered) = api(
        &state,
        &format!("/v1/events/{event_id}/update"),
        body.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{recovered}");
    assert_eq!(recovered["revision"], 2);
    let mut changed = body;
    changed["summary"] = json!("Different arguments");
    assert_eq!(
        api(&state, &format!("/v1/events/{event_id}/update"), changed)
            .await
            .0,
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn agent_proxy_requires_authentication_and_isolates_the_single_owner() {
    let mut state = state().await;
    let owner = user(&state).to_owned();
    state.config.dev_users.push(DevUser {
        user_id: "foreign-user".into(),
        token: "foreign-token".into(),
    });
    async fn stub(request: Request) -> Json<Value> {
        assert!(
            request.headers().get("authorization").is_none(),
            "Device tokens must not reach the sidecar"
        );
        Json(json!({"threadId":"main","configured":false}))
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server =
        tokio::spawn(axum::serve(listener, axum::Router::new().fallback(stub)).into_future());
    state.agent =
        Some(foyer_server::agent::Client::new(&format!("http://{address}/"), owner).unwrap());
    for (token, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some("foreign-token"), StatusCode::NOT_FOUND),
        (Some("test-token"), StatusCode::OK),
    ] {
        let mut request = axum::http::Request::get("/v1/agent");
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        let response = router(state.clone())
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    server.abort();
}

#[tokio::test]
async fn contact_update_and_move_recover_after_projection_rollback() {
    let state = state().await;
    let book_a = id();
    let book_b = id();
    for book in [&book_a, &book_b] {
        let (status, value) = api(
            &state,
            "/v1/address-books",
            json!({"operationId":id(),"id":book,"displayName":"Recovery"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{value}");
    }
    let contact_id = id();
    let (status, value) = api(&state, "/v1/contacts", json!({"operationId":id(),"id":contact_id,"addressBookId":book_a,"displayName":"Before rollback"})).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    sqlx::raw_sql("CREATE OR REPLACE FUNCTION foyer_test_reject_contact() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.display_name = 'Recovered contact' AND OLD.display_name <> NEW.display_name THEN RAISE EXCEPTION 'injected'; END IF; RETURN NEW; END $$; CREATE TRIGGER foyer_test_reject_contact BEFORE UPDATE ON contacts FOR EACH ROW EXECUTE FUNCTION foyer_test_reject_contact();").execute(&state.pool).await.unwrap();
    let request =
        json!({"operationId":id(),"expectedRevision":1,"displayName":"Recovered contact"});
    let (failed, _) = api(
        &state,
        &format!("/v1/contacts/{contact_id}/update"),
        request.clone(),
    )
    .await;
    sqlx::raw_sql("DROP TRIGGER foyer_test_reject_contact ON contacts; DROP FUNCTION foyer_test_reject_contact();").execute(&state.pool).await.unwrap();
    assert!(failed.is_server_error(), "{failed}");
    let (status, recovered) = api(
        &state,
        &format!("/v1/contacts/{contact_id}/update"),
        request,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{recovered}");
    assert_eq!(recovered["revision"], 2);
    sqlx::raw_sql("CREATE OR REPLACE FUNCTION foyer_test_reject_contact_move() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.address_book_id <> OLD.address_book_id THEN RAISE EXCEPTION 'injected'; END IF; RETURN NEW; END $$; CREATE TRIGGER foyer_test_reject_contact_move BEFORE UPDATE ON contacts FOR EACH ROW EXECUTE FUNCTION foyer_test_reject_contact_move();").execute(&state.pool).await.unwrap();
    let request = json!({"operationId":id(),"expectedRevision":2,"addressBookId":book_b});
    let (failed, _) = api(
        &state,
        &format!("/v1/contacts/{contact_id}/move"),
        request.clone(),
    )
    .await;
    sqlx::raw_sql("DROP TRIGGER foyer_test_reject_contact_move ON contacts; DROP FUNCTION foyer_test_reject_contact_move();").execute(&state.pool).await.unwrap();
    assert!(failed.is_server_error(), "{failed}");
    let (status, recovered) =
        api(&state, &format!("/v1/contacts/{contact_id}/move"), request).await;
    assert_eq!(status, StatusCode::OK, "{recovered}");
    assert_eq!(recovered["addressBookId"], book_b);
    assert_eq!(recovered["revision"], 3);
}

#[tokio::test]
async fn moved_resource_retry_requires_the_unchanged_destination() {
    let state = state().await;
    let owner = user(&state);
    let client = state.dav_client().unwrap();
    let a = client
        .create_calendar(
            owner,
            &NewCalendar {
                collection_id: id(),
                display_name: "A".into(),
            },
        )
        .await
        .unwrap();
    let b = client
        .create_calendar(
            owner,
            &NewCalendar {
                collection_id: id(),
                display_name: "B".into(),
            },
        )
        .await
        .unwrap();
    let uid = id();
    let source = DavHref::parse(&format!("{}{uid}.ics", a.href)).unwrap();
    let destination = DavHref::parse(&format!("{}{uid}.ics", b.href)).unwrap();
    put(
        &state,
        owner,
        &source,
        event(&uid, "Before move"),
        DavMediaType::ICalendar,
    )
    .await;
    let before = client
        .get_resource(owner, &source, CollectionKind::Calendar)
        .await
        .unwrap();
    client
        .move_resource(owner, &source, &destination, &before.etag)
        .await
        .unwrap();
    client
        .move_resource(owner, &source, &destination, &before.etag)
        .await
        .expect("retry recovers the same resource");
    let current = client
        .get_resource(owner, &destination, CollectionKind::Calendar)
        .await
        .unwrap();
    client
        .put_resource(
            owner,
            &destination,
            &DavPayload::from_raw(DavMediaType::ICalendar, event(&uid, "External edit")).unwrap(),
            PutPrecondition::IfMatch(current.etag),
        )
        .await
        .unwrap();
    assert!(
        client
            .move_resource(owner, &source, &destination, &before.etag)
            .await
            .is_err(),
        "an external edit must not be claimed as a successful retry"
    );
}
