use std::{
    thread,
    time::{Duration, Instant},
};

use foyer_shell_notes::{Availability, Runtime, Snapshot};
use uuid::Uuid;

fn wait_for(
    runtime: &Runtime,
    timeout: Duration,
    predicate: impl Fn(&Snapshot) -> bool,
) -> Snapshot {
    let deadline = Instant::now() + timeout;
    let mut latest = None;
    while Instant::now() < deadline {
        match runtime.updates.try_recv() {
            Ok(snapshot) => {
                if predicate(&snapshot) {
                    return snapshot;
                }
                latest = Some(snapshot);
            }
            Err(async_channel::TryRecvError::Empty) => thread::sleep(Duration::from_millis(25)),
            Err(async_channel::TryRecvError::Closed) => break,
        }
    }
    panic!("notes condition was not reached; latest snapshot: {latest:#?}");
}

/// Run explicitly against the development Compose stack:
///
/// FOYER_DEVELOPMENT_AUTH=1 \
/// FOYER_DEV_TOKEN=foyer-dev-token-do-not-use-outside-development \
/// FOYER_SHELL_NOTES_REPLICA_PATH=/tmp/foyer-notes-live.sqlite3 \
/// cargo test -p foyer-shell-notes --test live_sync -- --ignored --nocapture
#[test]
#[ignore = "requires the local Foyer Server and PowerSync Compose stack"]
fn native_replica_downloads_and_uploads_through_foyer_server() {
    let runtime = foyer_shell_notes::start();
    let initial = wait_for(&runtime, Duration::from_secs(30), |snapshot| {
        snapshot.availability == Availability::Available
            && !snapshot.offline
            && !snapshot.folders.is_empty()
    });
    assert!(initial.using_powersync);

    let folder = initial.folders[0].clone();
    let marker = Uuid::new_v4().to_string();
    let title = format!("Native PowerSync {marker}");
    let body = format!("# Native Rust\n\nLossless **Markdown** `{marker}`");
    runtime
        .controller
        .create_note(folder.id.clone(), title.clone(), body.clone());

    let local = wait_for(&runtime, Duration::from_secs(10), |snapshot| {
        snapshot.notes.iter().any(|note| note.title == title)
    });
    let note = local.notes.iter().find(|note| note.title == title).unwrap();
    assert_eq!(note.body, body);

    let uploaded = wait_for(&runtime, Duration::from_secs(30), |snapshot| {
        snapshot.pending_uploads == 0 && snapshot.notes.iter().any(|note| note.title == title)
    });
    assert!(!uploaded.offline);
    assert!(uploaded.last_error.is_none(), "{:#?}", uploaded.last_error);
}

#[test]
#[ignore = "requires the local Foyer Server and PowerSync Compose stack"]
fn native_replica_updates_an_existing_server_note() {
    let title = std::env::var("FOYER_NOTES_TEST_TITLE")
        .expect("FOYER_NOTES_TEST_TITLE must identify the note to update");
    let body = std::env::var("FOYER_NOTES_TEST_BODY")
        .unwrap_or_else(|_| "# Shell update\n\n**native** round trip\n".to_string());
    let runtime = foyer_shell_notes::start();
    let initial = wait_for(&runtime, Duration::from_secs(30), |snapshot| {
        snapshot.availability == Availability::Available
            && !snapshot.offline
            && snapshot.notes.iter().any(|note| note.title == title)
    });
    let note = initial
        .notes
        .iter()
        .find(|note| note.title == title)
        .unwrap()
        .clone();
    runtime.controller.update_note(
        note.id.clone(),
        note.revision,
        note.title.clone(),
        body.clone(),
    );

    let uploaded = wait_for(&runtime, Duration::from_secs(30), |snapshot| {
        snapshot.pending_uploads == 0
            && snapshot
                .notes
                .iter()
                .any(|candidate| candidate.id == note.id && candidate.body == body)
    });
    assert!(!uploaded.offline);
    assert!(uploaded.last_error.is_none(), "{:#?}", uploaded.last_error);
}

#[test]
#[ignore = "requires a previously synced replica while the local stack is stopped"]
fn offline_write_remains_queued_after_worker_restart() {
    let title = std::env::var("FOYER_NOTES_TEST_TITLE")
        .expect("FOYER_NOTES_TEST_TITLE must identify the offline test note");
    let body = "# Offline native Rust\n\nqueued across a worker restart".to_string();
    let runtime = foyer_shell_notes::start();
    let cached = wait_for(&runtime, Duration::from_secs(15), |snapshot| {
        snapshot.availability == Availability::Available
            && snapshot.offline
            && !snapshot.folders.is_empty()
    });
    runtime
        .controller
        .create_note(cached.folders[0].id.clone(), title.clone(), body.clone());
    let queued = wait_for(&runtime, Duration::from_secs(10), |snapshot| {
        snapshot.offline
            && snapshot.pending_uploads > 0
            && snapshot.notes.iter().any(|note| note.title == title)
    });
    assert_eq!(
        queued
            .notes
            .iter()
            .find(|note| note.title == title)
            .unwrap()
            .body,
        body
    );
    drop(runtime);
    thread::sleep(Duration::from_millis(250));

    let reopened = foyer_shell_notes::start();
    let durable = wait_for(&reopened, Duration::from_secs(15), |snapshot| {
        snapshot.offline
            && snapshot.pending_uploads > 0
            && snapshot.notes.iter().any(|note| note.title == title)
    });
    assert_eq!(
        durable
            .notes
            .iter()
            .find(|note| note.title == title)
            .unwrap()
            .body,
        body
    );
}

#[test]
#[ignore = "requires the local stack and a replica containing an offline test write"]
fn queued_offline_write_uploads_after_reconnect() {
    let title = std::env::var("FOYER_NOTES_TEST_TITLE")
        .expect("FOYER_NOTES_TEST_TITLE must identify the offline test note");
    let runtime = foyer_shell_notes::start();
    let recovered = wait_for(&runtime, Duration::from_secs(30), |snapshot| {
        snapshot.availability == Availability::Available
            && !snapshot.offline
            && snapshot.pending_uploads == 0
            && snapshot.notes.iter().any(|note| note.title == title)
    });
    assert!(
        recovered.last_error.is_none(),
        "{:#?}",
        recovered.last_error
    );
}

#[test]
#[ignore = "requires the local stack and a replica whose server row was tombstoned"]
fn server_tombstone_removes_the_native_replica_row() {
    let title = std::env::var("FOYER_NOTES_TEST_TITLE")
        .expect("FOYER_NOTES_TEST_TITLE must identify the tombstoned note");
    let runtime = foyer_shell_notes::start();
    let reconciled = wait_for(&runtime, Duration::from_secs(30), |snapshot| {
        snapshot.availability == Availability::Available
            && !snapshot.offline
            && snapshot.pending_uploads == 0
            && !snapshot.notes.iter().any(|note| note.title == title)
    });
    assert!(
        !reconciled.notes.iter().any(|note| note.title == title),
        "tombstoned note remained in the native replica"
    );
    assert!(
        reconciled.last_error.is_none(),
        "{:#?}",
        reconciled.last_error
    );
}
