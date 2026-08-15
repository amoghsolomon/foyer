use foyer_server::dav;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dav::{
    BoundDavWrite, CollectionKind, DavClient, DavConfig, DavError, DavHref, DavMediaType,
    DavPayload, ETag, NewAddressBook, NewCalendar, OperationBinding, Projector, PropertyUpdate,
    PutPrecondition, SyncToken, UserPaths,
};
use serde_json::json;
use sqlx::PgPool;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use uuid::Uuid;

struct MockResponse {
    status: u16,
    reason: &'static str,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl MockResponse {
    fn xml(status: u16, reason: &'static str, body: &str) -> Self {
        Self {
            status,
            reason,
            headers: vec![
                (
                    "Content-Type".into(),
                    "application/xml; charset=utf-8".into(),
                ),
                ("Content-Length".into(), body.len().to_string()),
            ],
            body: body.as_bytes().to_vec(),
        }
    }

    fn empty(status: u16, reason: &'static str) -> Self {
        Self {
            status,
            reason,
            headers: vec![("Content-Length".into(), "0".into())],
            body: Vec::new(),
        }
    }

    fn with_etag(mut self, etag: &str) -> Self {
        self.headers.push(("ETag".into(), etag.into()));
        self
    }
}

struct MockRequest {
    method: String,
    target: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

impl MockRequest {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

struct MockServer {
    url: String,
    requests: Arc<Mutex<Vec<MockRequest>>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl MockServer {
    async fn start<F>(handler: F) -> Self
    where
        F: Fn(&MockRequest) -> MockResponse + Send + Sync + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock DAV");
        let addr = listener.local_addr().expect("mock addr");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        let handler = Arc::new(handler);
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut rx => break,
                    accepted = listener.accept() => {
                        let Ok((mut stream, _)) = accepted else { break };
                        let handler = handler.clone();
                        let seen = seen.clone();
                        tokio::spawn(async move {
                            if let Ok(request) = read_request(&mut stream).await {
                                seen.lock().expect("request log").push(MockRequest {
                                    method: request.method.clone(),
                                    target: request.target.clone(),
                                    headers: request.headers.clone(),
                                    body: request.body.clone(),
                                });
                                let response = handler(&request);
                                let _ = write_response(&mut stream, &response).await;
                            }
                        });
                    }
                }
            }
        });
        Self {
            url: format!("http://{addr}"),
            requests,
            shutdown: Some(tx),
        }
    }

    fn client(&self) -> DavClient {
        DavClient::new(DavConfig::new(&self.url, "foyer", "service-secret").unwrap()).unwrap()
    }

    fn requests(&self) -> Vec<(String, String)> {
        self.requests
            .lock()
            .expect("request log")
            .iter()
            .map(|request| (request.method.clone(), request.target.clone()))
            .collect()
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> Result<MockRequest, ()> {
    let mut buf = Vec::new();
    let mut chunk = [0_u8; 1024];
    let header_end = loop {
        let read = stream.read(&mut chunk).await.map_err(|_| ())?;
        if read == 0 {
            return Err(());
        }
        buf.extend_from_slice(&chunk[..read]);
        if let Some(index) = buf.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
        if buf.len() > 64 * 1024 {
            return Err(());
        }
    };
    let header_text = std::str::from_utf8(&buf[..header_end]).map_err(|_| ())?;
    let mut lines = header_text.split("\r\n");
    let start = lines.next().unwrap_or("");
    let mut start_parts = start.split_whitespace();
    let method = start_parts.next().unwrap_or("").to_string();
    let target = start_parts.next().unwrap_or("").to_string();
    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_string(), value.trim().to_string());
        }
    }
    let length = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = buf.split_off(header_end);
    while body.len() < length {
        let read = stream.read(&mut chunk).await.map_err(|_| ())?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(length);
    Ok(MockRequest {
        method,
        target,
        headers,
        body,
    })
}

async fn write_response(
    stream: &mut tokio::net::TcpStream,
    response: &MockResponse,
) -> Result<(), ()> {
    let mut out = format!(
        "HTTP/1.1 {} {}\r\nConnection: close\r\n",
        response.status, response.reason
    );
    for (name, value) in &response.headers {
        out.push_str(name);
        out.push_str(": ");
        out.push_str(value);
        out.push_str("\r\n");
    }
    out.push_str("\r\n");
    stream.write_all(out.as_bytes()).await.map_err(|_| ())?;
    stream.write_all(&response.body).await.map_err(|_| ())?;
    stream.flush().await.map_err(|_| ())?;
    Ok(())
}

fn discovery_multistatus() -> String {
    r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:cr="urn:ietf:params:xml:ns:carddav">
  <d:response>
    <d:href>/alice/</d:href>
    <d:propstat>
      <d:prop>
        <d:current-user-principal><d:href>/alice/</d:href></d:current-user-principal>
        <c:calendar-home-set><d:href>/alice/calendars/</d:href></c:calendar-home-set>
        <cr:addressbook-home-set><d:href>/alice/addressbooks/</d:href></cr:addressbook-home-set>
        <d:resourcetype><d:collection/><d:principal/></d:resourcetype>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#
        .into()
}

fn home_listing(kind: &str) -> String {
    let (href, restype, comps) = match kind {
        "tasks" => (
            "/alice/tasks/chores/",
            "<d:collection/><c:calendar/>",
            r#"<c:supported-calendar-component-set><c:comp name="VTODO"/></c:supported-calendar-component-set>"#,
        ),
        "addressbooks" => (
            "/alice/addressbooks/people/",
            "<d:collection/><cr:addressbook/>",
            "",
        ),
        _ => (
            "/alice/calendars/home/",
            "<d:collection/><c:calendar/>",
            r#"<c:supported-calendar-component-set><c:comp name="VEVENT"/></c:supported-calendar-component-set>"#,
        ),
    };
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:cr="urn:ietf:params:xml:ns:carddav">
  <d:response>
    <d:href>{href}</d:href>
    <d:propstat>
      <d:prop>
        <d:displayname>Listed</d:displayname>
        <d:getetag>"col-1"</d:getetag>
        <d:resourcetype>{restype}</d:resourcetype>
        {comps}
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#
    )
}

fn event_ics() -> &'static str {
    "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Foyer//EN\r\nBEGIN:VEVENT\r\nUID:evt-1\r\nSUMMARY:Lunch\r\nX-UNKNOWN:keep\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
}

#[tokio::test]
async fn discovery_lists_per_user_collections() {
    let server = MockServer::start(|request| {
        assert!(
            request
                .header("authorization")
                .unwrap()
                .starts_with("Basic ")
        );
        match (request.method.as_str(), request.target.as_str()) {
            ("PROPFIND", "/.well-known/caldav") => {
                MockResponse::xml(207, "Multi-Status", &discovery_multistatus())
            }
            ("PROPFIND", "/alice/") => {
                MockResponse::xml(207, "Multi-Status", &discovery_multistatus())
            }
            ("PROPFIND", "/alice/calendars/") => {
                MockResponse::xml(207, "Multi-Status", &home_listing("calendars"))
            }
            ("PROPFIND", "/alice/addressbooks/") => {
                MockResponse::xml(207, "Multi-Status", &home_listing("addressbooks"))
            }
            ("PROPFIND", "/alice/tasks/") => {
                MockResponse::xml(207, "Multi-Status", &home_listing("tasks"))
            }
            _ => MockResponse::empty(404, "Not Found"),
        }
    })
    .await;

    let discovered = server.client().discover("alice").await.unwrap();
    assert_eq!(discovered.principal.as_str(), "/alice/");
    assert_eq!(discovered.calendar_home.as_str(), "/alice/calendars/");
    assert!(
        discovered
            .collections
            .iter()
            .any(|collection| collection.kind == CollectionKind::Calendar
                && collection.href.as_str() == "/alice/calendars/home/")
    );
    assert!(
        discovered
            .collections
            .iter()
            .any(|collection| collection.kind == CollectionKind::TaskList)
    );
    assert!(
        discovered
            .collections
            .iter()
            .any(|collection| collection.kind == CollectionKind::AddressBook)
    );
}

#[tokio::test]
async fn mkcalendar_and_addressbook_creation_use_safe_paths() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let server = MockServer::start(move |request| {
        log.lock().unwrap().push((
            request.method.clone(),
            request.target.clone(),
            request.body_text(),
        ));
        match (request.method.as_str(), request.target.as_str()) {
            ("PROPFIND", _) => MockResponse::empty(404, "Not Found"),
            ("MKCOL", "/alice/" | "/alice/calendars/" | "/alice/addressbooks/") => {
                MockResponse::empty(201, "Created")
            }
            ("MKCALENDAR", "/alice/calendars/work/") => MockResponse::empty(201, "Created"),
            ("MKCOL", "/alice/addressbooks/people/") => MockResponse::empty(201, "Created"),
            _ => MockResponse::empty(500, "Error"),
        }
    })
    .await;

    let client = server.client();
    let calendar = client
        .create_calendar(
            "alice",
            &NewCalendar {
                collection_id: "work".into(),
                display_name: "Work <Home>".into(),
            },
        )
        .await
        .unwrap();
    let book = client
        .create_address_book(
            "alice",
            &NewAddressBook {
                collection_id: "people".into(),
                display_name: "People".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(calendar.href.as_str(), "/alice/calendars/work/");
    assert_eq!(book.href.as_str(), "/alice/addressbooks/people/");
    let bodies = seen.lock().unwrap();
    assert!(bodies.iter().any(|(method, target, body)| {
        method == "MKCALENDAR"
            && target == "/alice/calendars/work/"
            && body.contains("Work &lt;Home&gt;")
            && body.contains(r#"name="VEVENT""#)
    }));
    assert!(bodies.iter().any(|(method, _, body)| {
        method == "MKCOL" && body.contains("addressbook") && body.contains("People")
    }));
}

#[tokio::test]
async fn sync_token_report_and_multiget_preserve_unknown_properties() {
    let ics = event_ics();
    let server = MockServer::start(move |request| {
        match (request.method.as_str(), request.target.as_str()) {
            ("REPORT", "/alice/calendars/home/")
                if request.body_text().contains("sync-collection") =>
            {
                MockResponse::xml(
                    207,
                    "Multi-Status",
                    r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/alice/calendars/home/evt-1.ics</d:href>
    <d:propstat>
      <d:prop><d:getetag>"e1"</d:getetag><d:resourcetype/></d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
  <d:response>
    <d:href>/alice/calendars/home/gone.ics</d:href>
    <d:status>HTTP/1.1 404 Not Found</d:status>
  </d:response>
  <d:sync-token>http://radicale.org/ns/sync/2</d:sync-token>
</d:multistatus>"#,
                )
            }
            ("REPORT", "/alice/calendars/home/")
                if request.body_text().contains("calendar-multiget") =>
            {
                MockResponse::xml(
                    207,
                    "Multi-Status",
                    &format!(
                        r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:response>
    <d:href>/alice/calendars/home/evt-1.ics</d:href>
    <d:propstat>
      <d:prop>
        <d:getetag>"e1"</d:getetag>
        <c:calendar-data>{}</c:calendar-data>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#,
                        ics.replace('&', "&amp;").replace('<', "&lt;")
                    ),
                )
            }
            _ => MockResponse::empty(404, "Not Found"),
        }
    })
    .await;

    let client = server.client();
    let href = DavHref::parse("/alice/calendars/home/").unwrap();
    let page = client
        .sync_collection("alice", &href, Some(&SyncToken::parse("old").unwrap()))
        .await
        .unwrap();
    assert_eq!(page.upserts.len(), 1);
    assert_eq!(page.deletions[0].as_str(), "/alice/calendars/home/gone.ics");
    assert_eq!(
        page.sync_token.unwrap().as_str(),
        "http://radicale.org/ns/sync/2"
    );

    let fetches = client
        .fetch_resources(
            "alice",
            CollectionKind::Calendar,
            &[page.upserts[0].href.clone()],
        )
        .await
        .unwrap();
    let resource = fetches[0].result.as_ref().unwrap();
    assert!(resource.payload.raw().contains("X-UNKNOWN:keep"));
    let patched = resource
        .payload
        .patch(&[PropertyUpdate::set("SUMMARY", "Dinner")])
        .unwrap();
    assert!(patched.raw().contains("SUMMARY:Dinner"));
    assert!(patched.raw().contains("X-UNKNOWN:keep"));
}

#[tokio::test]
async fn invalid_sync_token_retries_without_token() {
    let tokens = Arc::new(Mutex::new(Vec::new()));
    let seen = tokens.clone();
    let server = MockServer::start(move |request| {
        if request.method == "REPORT" && request.body_text().contains("sync-collection") {
            let body = request.body_text();
            seen.lock().unwrap().push(body.clone());
            if body.contains("stale-token") {
                return MockResponse::xml(
                    403,
                    "Forbidden",
                    r#"<?xml version="1.0"?><d:error xmlns:d="DAV:"><d:valid-sync-token/></d:error>"#,
                );
            }
            return MockResponse::xml(
                207,
                "Multi-Status",
                r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:"><d:sync-token>fresh</d:sync-token></d:multistatus>"#,
            );
        }
        MockResponse::empty(404, "Not Found")
    })
    .await;

    let page = server
        .client()
        .sync_collection(
            "alice",
            &DavHref::parse("/alice/calendars/home/").unwrap(),
            Some(&SyncToken::parse("stale-token").unwrap()),
        )
        .await
        .unwrap();
    assert!(page.token_reset);
    assert_eq!(page.sync_token.unwrap().as_str(), "fresh");
    assert_eq!(tokens.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn conditional_put_and_delete_honor_etags() {
    let last_precondition = Arc::new(Mutex::new(String::new()));
    let seen = last_precondition.clone();
    let server = MockServer::start(move |request| {
        if request.method == "PUT" {
            *seen.lock().unwrap() = request
                .header("if-match")
                .or_else(|| request.header("if-none-match"))
                .unwrap_or("")
                .to_string();
            if request.header("if-match") == Some("\"stale\"") {
                return MockResponse::empty(412, "Precondition Failed");
            }
            return MockResponse::empty(201, "Created").with_etag("\"fresh\"");
        }
        if request.method == "DELETE" {
            assert_eq!(request.header("if-match"), Some("\"fresh\""));
            return MockResponse::empty(204, "No Content");
        }
        if request.method == "GET" {
            return MockResponse {
                status: 200,
                reason: "OK",
                headers: vec![
                    ("ETag".into(), "\"fresh\"".into()),
                    ("Content-Type".into(), "text/calendar".into()),
                    ("Content-Length".into(), event_ics().len().to_string()),
                ],
                body: event_ics().as_bytes().to_vec(),
            };
        }
        MockResponse::empty(404, "Not Found")
    })
    .await;

    let client = server.client();
    let href = DavHref::parse("/alice/calendars/home/evt-1.ics").unwrap();
    let payload = DavPayload::from_raw(DavMediaType::ICalendar, event_ics()).unwrap();
    let created = client
        .put_resource("alice", &href, &payload, PutPrecondition::IfNoneMatchStar)
        .await
        .unwrap();
    assert!(created.created);
    assert_eq!(created.etag.unwrap().as_str(), "\"fresh\"");

    let stale = client
        .put_resource(
            "alice",
            &href,
            &payload,
            PutPrecondition::IfMatch(ETag::parse("\"stale\"").unwrap()),
        )
        .await
        .unwrap_err();
    assert!(stale.is_stale());

    client
        .delete_resource("alice", &href, &ETag::parse("\"fresh\"").unwrap())
        .await
        .unwrap();
}

#[tokio::test]
async fn create_retry_recovers_identical_payload() {
    let server = MockServer::start(|request| {
        if request.method == "PUT" {
            return MockResponse::empty(412, "Precondition Failed");
        }
        if request.method == "GET" {
            return MockResponse {
                status: 200,
                reason: "OK",
                headers: vec![
                    ("ETag".into(), "\"exists\"".into()),
                    ("Content-Type".into(), "text/calendar".into()),
                    ("Content-Length".into(), event_ics().len().to_string()),
                ],
                body: event_ics().as_bytes().to_vec(),
            };
        }
        MockResponse::empty(404, "Not Found")
    })
    .await;

    let href = DavHref::parse("/alice/calendars/home/evt-1.ics").unwrap();
    let payload = DavPayload::from_raw(DavMediaType::ICalendar, event_ics()).unwrap();
    let result = server
        .client()
        .put_resource("alice", &href, &payload, PutPrecondition::IfNoneMatchStar)
        .await
        .unwrap();
    assert!(!result.created);
    assert_eq!(result.etag.unwrap().as_str(), "\"exists\"");
}

#[tokio::test]
async fn foreign_user_href_is_rejected_before_io() {
    let server = MockServer::start(|_| MockResponse::empty(500, "should-not-run")).await;
    let error = server
        .client()
        .delete_resource(
            "alice",
            &DavHref::parse("/bob/calendars/home/evt.ics").unwrap(),
            &ETag::parse("\"1\"").unwrap(),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, DavError::UnsafePath(_)));
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn href_escape_to_another_host_is_rejected() {
    let xml = r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:">
      <d:response><d:href>http://evil.example/steal</d:href>
      <d:propstat><d:prop/><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
    </d:multistatus>"#;
    let server = MockServer::start(move |request| {
        if request.method == "REPORT" {
            MockResponse::xml(207, "Multi-Status", xml)
        } else {
            MockResponse::empty(404, "Not Found")
        }
    })
    .await;
    let error = server
        .client()
        .sync_collection(
            "alice",
            &DavHref::parse("/alice/calendars/home/").unwrap(),
            None,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, DavError::UnsafeUrl(_)));
}

#[tokio::test]
async fn errors_never_include_service_password() {
    let config = DavConfig::new("http://127.0.0.1:1", "foyer", "super-secret-password").unwrap();
    let client = DavClient::new(config).unwrap();
    let error = client
        .discover("alice")
        .await
        .expect_err("connection to port 1 should fail");
    let rendered = error.to_string();
    assert!(!rendered.contains("super-secret-password"));
    assert!(!rendered.contains("Basic "));
}

#[tokio::test]
async fn xml_and_path_bounds_are_enforced() {
    assert!(DavHref::parse("/alice/../../etc/passwd").is_err());
    assert!(UserPaths::for_user("../alice").is_err());
    assert!(UserPaths::for_user("alice/calendars").is_err());
    let huge = format!(
        "BEGIN:VCALENDAR\r\n{}END:VCALENDAR\r\n",
        "A".repeat(300_000)
    );
    assert!(DavPayload::from_raw(DavMediaType::ICalendar, huge).is_err());
}

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

async fn test_pool() -> Option<PgPool> {
    let url = test_database_url().await?;
    for _ in 0..40 {
        if let Ok(pool) = PgPool::connect(&url).await {
            sqlx::migrate!("./migrations").run(&pool).await.ok()?;
            return Some(pool);
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    None
}

#[tokio::test]
async fn projector_advances_checkpoint_only_on_commit() {
    let Some(pool) = test_pool().await else {
        eprintln!(
            "skipping projector_advances_checkpoint_only_on_commit: PostgreSQL is unavailable"
        );
        return;
    };
    let projector = Projector::new(pool);
    let href = DavHref::parse("/alice/calendars/home/").unwrap();
    let collection = dav::DavCollection {
        href: href.clone(),
        kind: CollectionKind::Calendar,
        display_name: Some("Home".into()),
        etag: Some(ETag::parse("\"c1\"").unwrap()),
        sync_token: None,
        supported_components: vec!["VEVENT".into()],
    };
    projector
        .remember_collection("alice", "home", &collection)
        .await
        .unwrap();
    let loaded = projector
        .load_checkpoint("alice", &href)
        .await
        .unwrap()
        .unwrap();
    assert!(loaded.sync_token.is_none());

    let mut tx = projector.pool().begin().await.unwrap();
    projector
        .commit_checkpoint(
            &mut tx,
            "alice",
            &href,
            Some(&SyncToken::parse("token-2").unwrap()),
            Some(&ETag::parse("\"c2\"").unwrap()),
        )
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    let rolled = projector
        .load_checkpoint("alice", &href)
        .await
        .unwrap()
        .unwrap();
    assert!(rolled.sync_token.is_none());

    let mut tx = projector.pool().begin().await.unwrap();
    projector
        .commit_checkpoint(
            &mut tx,
            "alice",
            &href,
            Some(&SyncToken::parse("token-2").unwrap()),
            None,
        )
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let committed = projector
        .load_checkpoint("alice", &href)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(committed.sync_token.as_deref(), Some("token-2"));

    projector.reset_user_checkpoints("alice").await.unwrap();
    assert!(
        projector
            .load_checkpoint("alice", &href)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn operation_binding_replays_identical_requests() {
    let Some(pool) = test_pool().await else {
        eprintln!(
            "skipping operation_binding_replays_identical_requests: PostgreSQL is unavailable"
        );
        return;
    };
    let projector = Projector::new(pool);
    let operation_id = Uuid::new_v4().to_string();
    let binding = OperationBinding {
        user_id: "alice".into(),
        operation_id: operation_id.clone(),
        entity_type: "event".into(),
        entity_id: Uuid::new_v4().to_string(),
        operation: "create".into(),
        request_body: json!({"summary": "Lunch"}),
    };
    let write = BoundDavWrite {
        href: "/alice/calendars/home/evt.ics".into(),
        etag: Some("\"1\"".into()),
        uid: Some("evt-1".into()),
        created: true,
    };
    let first = projector
        .with_operation(binding.clone(), |_| {
            let write = write.clone();
            Box::pin(async move { Ok(write) })
        })
        .await
        .unwrap();
    let second = projector
        .with_operation(binding.clone(), |_| {
            Box::pin(async move {
                panic!("identical DAV operation must not run again");
            })
        })
        .await
        .unwrap();
    assert_eq!(first, second);

    let mut conflict = binding.clone();
    conflict.request_body = json!({"summary": "Dinner"});
    let error = projector
        .with_operation(conflict, |_| Box::pin(async move { Ok(write.clone()) }))
        .await
        .unwrap_err();
    assert_eq!(error, DavError::OperationConflict);
}
