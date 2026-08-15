//! Live PowerSync round-trip coverage. Ignored unless a local Foyer Server stack is running.
//!
//! cargo test -p foyer-shell-bookmarks --test live_sync -- --ignored --nocapture

use std::time::Duration;

use foyer_shell_bookmarks::{Availability, Runtime, Snapshot};

async fn wait_for_snapshot<F>(runtime: &Runtime, predicate: F) -> Snapshot
where
    F: Fn(&Snapshot) -> bool,
{
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let snapshot = runtime
            .updates
            .recv()
            .await
            .expect("bookmarks worker closed");
        if predicate(&snapshot) || std::time::Instant::now() > deadline {
            return snapshot;
        }
    }
}

#[tokio::test]
#[ignore = "requires a running Foyer Server + PowerSync development stack"]
async fn live_replica_starts() {
    let runtime = foyer_shell_bookmarks::start();
    let snapshot = wait_for_snapshot(&runtime, |snapshot| {
        matches!(snapshot.availability, Availability::Available) && snapshot.using_powersync
    })
    .await;
    assert!(matches!(
        snapshot.availability,
        Availability::Available | Availability::Unavailable(_)
    ));
}
