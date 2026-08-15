//! Live Radicale + PostgreSQL projection rebuild. Skips unless both a DAV origin
//! and a test database are available.

use std::time::Duration;

use foyer_server::config::DavSettings;
use foyer_server::dav::{DavHref, DavMediaType, DavPayload, NewCalendar, PutPrecondition};
use foyer_server::{AppState, Config, app_state, calendar};
use sqlx::PgPool;
use uuid::Uuid;

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

fn dav_settings() -> Option<DavSettings> {
    let base_url = std::env::var("FOYER_DAV_URL")
        .ok()
        .filter(|value| !value.is_empty())?;
    Some(DavSettings {
        base_url,
        username: std::env::var("FOYER_DAV_USERNAME").unwrap_or_else(|_| "foyer".into()),
        password: std::env::var("FOYER_DAV_PASSWORD")
            .unwrap_or_else(|_| "foyer-dev-dav-password-do-not-use-outside-development".into()),
    })
}

async fn live_state() -> Option<AppState> {
    let dav = dav_settings()?;
    let url = test_database_url().await?;
    for _ in 0..40 {
        if PgPool::connect(&url).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let mut config = Config::test_development(url);
    config.dav = Some(dav);
    Some(app_state(config).await.expect("app state"))
}

#[tokio::test]
async fn radicale_round_trip_rebuilds_calendar_projection() {
    let Some(state) = live_state().await else {
        eprintln!("skipping live Radicale test: FOYER_DAV_URL or PostgreSQL unavailable");
        return;
    };
    let user_id = "dev-user";
    let calendar_id = Uuid::new_v4().to_string();
    let event_id = Uuid::new_v4().to_string();
    let client = state.dav_client().expect("dav client").clone();
    let created = client
        .create_calendar(
            user_id,
            &NewCalendar {
                collection_id: calendar_id,
                display_name: "Live".into(),
            },
        )
        .await
        .expect("create calendar");
    let href = DavHref::parse(&format!("{}{}.ics", created.href.as_str(), event_id)).expect("href");
    let ical = format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Foyer//Live//EN\r\nBEGIN:VEVENT\r\nUID:{event_id}\r\nDTSTAMP:20260301T120000Z\r\nDTSTART;TZID=America/New_York:20260302T100000\r\nDTEND;TZID=America/New_York:20260302T103000\r\nSUMMARY:Live event\r\nX-UNKNOWN:keep\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
    );
    let payload = DavPayload::from_raw(DavMediaType::ICalendar, ical).expect("payload");
    client
        .put_resource(user_id, &href, &payload, PutPrecondition::IfNoneMatchStar)
        .await
        .expect("put event");

    sqlx::query("DELETE FROM calendar_event_payloads WHERE event_id IN (SELECT id FROM calendar_events WHERE user_id = $1)")
        .bind(user_id)
        .execute(&state.pool)
        .await
        .ok();
    sqlx::query("DELETE FROM calendar_events WHERE user_id = $1")
        .bind(user_id)
        .execute(&state.pool)
        .await
        .expect("clear events");
    sqlx::query("DELETE FROM calendar_calendars WHERE user_id = $1")
        .bind(user_id)
        .execute(&state.pool)
        .await
        .expect("clear calendars");
    state
        .projector
        .reset_user_checkpoints(user_id)
        .await
        .expect("reset checkpoints");

    calendar::reconcile_user(&state, user_id)
        .await
        .expect("rebuild");
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM calendar_events WHERE user_id = $1 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await
    .expect("count");
    assert!(
        count >= 1,
        "projection rebuild should restore the live VEVENT"
    );
}
