use std::time::Duration;

use axum::{Json, extract::State};
use foyer_server::auth::Principal;
use foyer_server::config::DevUser;
use foyer_server::tasks::{
    CreateTaskListRequest, CreateTaskRequest, DeleteRequest, Due, MoveTaskRequest, TodoPatch,
    UpdateTaskRequest, extract_todo, install_dav_backend, memory_backend, new_todo_calendar,
    parse_calendar, patch_todo, serialize_calendar, validate_due,
};
use foyer_server::{AppState, Config, app_state};
use sqlx::PgPool;
use uuid::Uuid;

const DEV_USER: &str = "dev-user";

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
    install_dav_backend(memory_backend());
    let mut config = Config::test_development(url);
    config.dev_users = vec![DevUser {
        user_id: DEV_USER.into(),
        token: "dev-token".into(),
    }];
    Some(app_state(config).await.expect("app state"))
}

fn principal() -> Principal {
    Principal {
        user_id: DEV_USER.into(),
        device_key_id: None,
    }
}

fn uuid() -> String {
    Uuid::new_v4().to_string()
}

#[test]
fn vtodo_round_trip_preserves_unknown_properties_and_markdown() {
    let markdown = "Line one\n\n<script>keep</script>\n\n- [ ] checkbox\n";
    let original = concat!(
        "BEGIN:VCALENDAR\r\n",
        "VERSION:2.0\r\n",
        "PRODID:-//External//CalDAV//EN\r\n",
        "BEGIN:VTODO\r\n",
        "UID:9f1b6a2e-6c5c-4d0a-9c1f-1f0d8a7b1234\r\n",
        "SUMMARY:Before\r\n",
        "X-VENDOR-FLAG:preserve\r\n",
        "CATEGORIES:Home,Later\r\n",
        "BEGIN:VALARM\r\n",
        "ACTION:DISPLAY\r\n",
        "DESCRIPTION:Soon\r\n",
        "TRIGGER:-PT10M\r\n",
        "END:VALARM\r\n",
        "END:VTODO\r\n",
        "END:VCALENDAR\r\n",
    );
    let mut calendar = parse_calendar(original).expect("parse");
    patch_todo(
        &mut calendar,
        &TodoPatch {
            title: Some("After".into()),
            description: Some(markdown.into()),
            due: Some(Some(Due {
                local: "2026-12-01".into(),
                time_zone: None,
                all_day: true,
                at: None,
            })),
            priority: Some(1),
            position: Some(4),
            ..TodoPatch::default()
        },
    )
    .expect("patch");
    let serialized = serialize_calendar(&calendar);
    assert!(serialized.contains("X-VENDOR-FLAG:preserve"));
    assert!(serialized.contains("CATEGORIES:Home,Later"));
    assert!(serialized.contains("BEGIN:VALARM"));
    assert!(serialized.contains("PRODID:-//External//CalDAV//EN"));
    let fields = extract_todo(&parse_calendar(&serialized).unwrap()).unwrap();
    assert_eq!(fields.title, "After");
    assert_eq!(fields.description, markdown);
    assert_eq!(fields.due.unwrap().local, "2026-12-01");
    assert_eq!(fields.priority, 1);
    assert_eq!(fields.position, 4);
}

#[test]
fn due_date_semantics_cover_all_day_floating_and_utc() {
    let all_day = validate_due(&Due {
        local: "2026-08-15".into(),
        time_zone: Some("America/Chicago".into()),
        all_day: true,
        at: None,
    })
    .unwrap();
    assert!(all_day.all_day);
    assert!(all_day.at.is_none());

    let floating = validate_due(&Due {
        local: "2026-08-15T09:30:00".into(),
        time_zone: None,
        all_day: false,
        at: None,
    })
    .unwrap();
    assert!(floating.time_zone.is_none());
    assert!(floating.at.is_none());

    let utc = validate_due(&Due {
        local: "2026-08-15T13:30:00".into(),
        time_zone: Some("UTC".into()),
        all_day: false,
        at: None,
    })
    .unwrap();
    assert_eq!(utc.at.unwrap().to_rfc3339(), "2026-08-15T13:30:00+00:00");

    let calendar = new_todo_calendar(&foyer_server::tasks::TodoFields {
        uid: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".into(),
        title: "Timed".into(),
        description: String::new(),
        due: Some(Due {
            local: "2026-08-15T18:00:00".into(),
            time_zone: Some("Europe/Paris".into()),
            all_day: false,
            at: None,
        }),
        priority: 0,
        completed: false,
        completed_at: None,
        position: 0,
        operation_id: None,
    });
    let serialized = serialize_calendar(&calendar);
    assert!(serialized.contains("TZID=Europe/Paris"));
    assert!(!serialized.contains("18:00:00Z"));
}

#[tokio::test]
async fn create_update_complete_move_and_stale_etag_conflict() {
    let Some(state) = test_state().await else {
        eprintln!("skipping tasks DAV slice: PostgreSQL is unavailable");
        return;
    };
    let list_id = uuid();
    let Json(list) = foyer_server::tasks::create_task_list(
        State(state.clone()),
        principal(),
        Json(CreateTaskListRequest {
            operation_id: uuid(),
            id: list_id.clone(),
            name: "Inbox".into(),
            position: None,
        }),
    )
    .await
    .expect("create list");
    assert_eq!(list.name, "Inbox");
    assert_eq!(list.revision, 1);
    assert!(list.href.ends_with(&format!("/{list_id}/")));

    let replay = foyer_server::tasks::create_task_list(
        State(state.clone()),
        principal(),
        Json(CreateTaskListRequest {
            operation_id: list_id.clone(),
            id: list_id.clone(),
            name: "Other".into(),
            position: None,
        }),
    )
    .await;
    // A reused identifier that is not the original operation id is a conflict;
    // exact operation replay is covered below with a stable operation id.
    assert!(replay.is_err());

    let create_op = uuid();
    let task_id = uuid();
    let markdown = "# Body\n\nKeep trailing newline.\n";
    let Json(created) = foyer_server::tasks::create_task(
        State(state.clone()),
        principal(),
        Json(CreateTaskRequest {
            operation_id: create_op.clone(),
            id: task_id.clone(),
            list_id: list.id.clone(),
            title: "Write ADR".into(),
            description: markdown.into(),
            due: Some(Due {
                local: "2026-08-18T16:00:00".into(),
                time_zone: Some("UTC".into()),
                all_day: false,
                at: None,
            }),
            priority: Some(1),
            position: Some(2),
        }),
    )
    .await
    .expect("create task");
    assert_eq!(created.description, markdown);
    assert_eq!(created.priority, 1);
    assert!(!created.completed);
    assert_eq!(created.due.as_ref().unwrap().local, "2026-08-18T16:00:00");

    let Json(replayed) = foyer_server::tasks::create_task(
        State(state.clone()),
        principal(),
        Json(CreateTaskRequest {
            operation_id: create_op,
            id: task_id.clone(),
            list_id: list.id.clone(),
            title: "Write ADR".into(),
            description: markdown.into(),
            due: Some(Due {
                local: "2026-08-18T16:00:00".into(),
                time_zone: Some("UTC".into()),
                all_day: false,
                at: None,
            }),
            priority: Some(1),
            position: Some(2),
        }),
    )
    .await
    .expect("replay create");
    assert_eq!(replayed.etag, created.etag);
    assert_eq!(replayed.revision, created.revision);

    let Json(updated) = foyer_server::tasks::update_task(
        State(state.clone()),
        principal(),
        axum::extract::Path(task_id.clone()),
        Json(UpdateTaskRequest {
            operation_id: uuid(),
            expected_revision: created.revision,
            title: "Write accepted ADR".into(),
            description: markdown.into(),
            due: created.due.clone(),
            priority: 1,
            position: 2,
        }),
    )
    .await
    .expect("update task");
    assert_eq!(updated.title, "Write accepted ADR");
    assert!(updated.revision > created.revision);

    let conflict = foyer_server::tasks::update_task(
        State(state.clone()),
        principal(),
        axum::extract::Path(task_id.clone()),
        Json(UpdateTaskRequest {
            operation_id: uuid(),
            expected_revision: created.revision,
            title: "Should lose".into(),
            description: markdown.into(),
            due: created.due.clone(),
            priority: 9,
            position: 2,
        }),
    )
    .await
    .expect_err("stale revision");
    let response = axum::response::IntoResponse::into_response(conflict);
    assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);

    let Json(completed) = foyer_server::tasks::complete_task(
        State(state.clone()),
        principal(),
        axum::extract::Path(task_id.clone()),
        Json(DeleteRequest {
            operation_id: uuid(),
            expected_revision: updated.revision,
        }),
    )
    .await
    .expect("complete");
    assert!(completed.completed);
    assert!(completed.completed_at.is_some());

    let Json(reopened) = foyer_server::tasks::reopen_task(
        State(state.clone()),
        principal(),
        axum::extract::Path(task_id.clone()),
        Json(DeleteRequest {
            operation_id: uuid(),
            expected_revision: completed.revision,
        }),
    )
    .await
    .expect("reopen");
    assert!(!reopened.completed);

    let other_list = uuid();
    let Json(later) = foyer_server::tasks::create_task_list(
        State(state.clone()),
        principal(),
        Json(CreateTaskListRequest {
            operation_id: uuid(),
            id: other_list.clone(),
            name: "Later".into(),
            position: None,
        }),
    )
    .await
    .expect("second list");
    let Json(moved) = foyer_server::tasks::move_task(
        State(state.clone()),
        principal(),
        axum::extract::Path(task_id.clone()),
        Json(MoveTaskRequest {
            operation_id: uuid(),
            expected_revision: reopened.revision,
            list_id: later.id.clone(),
            position: Some(0),
        }),
    )
    .await
    .expect("move");
    assert_eq!(moved.list_id, later.id);
    assert!(moved.href.contains(&later.id));
    assert_eq!(moved.id, task_id);

    let Json(deleted) = foyer_server::tasks::delete_task(
        State(state.clone()),
        principal(),
        axum::extract::Path(task_id),
        Json(DeleteRequest {
            operation_id: uuid(),
            expected_revision: moved.revision,
        }),
    )
    .await
    .expect("delete");
    assert!(deleted.deleted_at.is_some());
}
