use std::{
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};

use async_channel::{Receiver, Sender};
use zbus::blocking::{Connection, Proxy};

use crate::{BUS_NAME, INTERFACE, OBJECT_PATH, Snapshot, SnapshotWire, State};

const ACTIVE_POLL: Duration = Duration::from_millis(32);
const IDLE_POLL: Duration = Duration::from_millis(250);
const RETRY_POLL: Duration = Duration::from_secs(1);

#[derive(Debug)]
enum Command {
    Toggle,
    Cancel,
}

#[derive(Clone, Debug)]
pub struct Controller {
    commands: mpsc::Sender<Command>,
}

impl Controller {
    pub fn toggle_dictation(&self) {
        if self.commands.send(Command::Toggle).is_err() {
            tracing::warn!("transcription client is not running");
        }
    }

    pub fn cancel(&self) {
        if self.commands.send(Command::Cancel).is_err() {
            tracing::warn!("transcription client is not running");
        }
    }
}

pub struct Runtime {
    pub updates: Receiver<Snapshot>,
    pub controller: Controller,
}

pub fn start() -> Runtime {
    let (updates_tx, updates) = async_channel::unbounded();
    let (commands, command_rx) = mpsc::channel();
    thread::Builder::new()
        .name("foyer-shell-transcription-client".into())
        .spawn(move || run(updates_tx, command_rx))
        .expect("failed to start transcription client");
    Runtime {
        updates,
        controller: Controller { commands },
    }
}

fn run(updates: Sender<Snapshot>, commands: mpsc::Receiver<Command>) {
    let current = Arc::new(Mutex::new(Snapshot::default()));
    loop {
        match Connection::session().and_then(|connection| {
            Proxy::new(&connection, BUS_NAME, OBJECT_PATH, INTERFACE)
                .map(|proxy| (connection, proxy))
        }) {
            Ok((_connection, proxy)) => {
                if serve(&proxy, &updates, &commands, &current) {
                    return;
                }
                if !wait_before_reconnect(&commands) {
                    return;
                }
            }
            Err(error) => {
                let unavailable = Snapshot {
                    error: Arc::from(error.to_string()),
                    ..Snapshot::default()
                };
                publish_if_changed(&updates, &current, unavailable);
                if !wait_before_reconnect(&commands) {
                    return;
                }
            }
        }
    }
}

fn wait_before_reconnect(commands: &mpsc::Receiver<Command>) -> bool {
    !matches!(
        commands.recv_timeout(RETRY_POLL),
        Err(mpsc::RecvTimeoutError::Disconnected)
    )
}

fn serve(
    proxy: &Proxy<'_>,
    updates: &Sender<Snapshot>,
    commands: &mpsc::Receiver<Command>,
    current: &Arc<Mutex<Snapshot>>,
) -> bool {
    loop {
        if let Ok(command) = commands.try_recv() {
            handle_command(proxy, command, current);
        }

        let wire: SnapshotWire = match proxy.call("GetSnapshot", &()) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                tracing::warn!(%error, "lost transcription service");
                return false;
            }
        };
        let snapshot = Snapshot::from_wire(wire);
        let poll = if snapshot.state.is_active() {
            ACTIVE_POLL
        } else {
            IDLE_POLL
        };
        publish_if_changed(updates, current, snapshot);

        match commands.recv_timeout(poll) {
            Ok(command) => handle_command(proxy, command, current),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return true,
        }
    }
}

fn handle_command(proxy: &Proxy<'_>, command: Command, current: &Arc<Mutex<Snapshot>>) {
    let snapshot = current
        .lock()
        .map(|snapshot| snapshot.clone())
        .unwrap_or_default();
    let result = match command {
        Command::Toggle => match snapshot.state {
            State::Recording => proxy
                .call::<_, _, bool>("Stop", &snapshot.session_id)
                .map(|_| ()),
            State::LoadingModel | State::Transcribing => Ok(()),
            _ => proxy
                .call::<_, _, u64>("Start", &("dictation".to_string()))
                .map(|_| ()),
        },
        Command::Cancel if snapshot.session_id != 0 => proxy
            .call::<_, _, bool>("Cancel", &snapshot.session_id)
            .map(|_| ()),
        Command::Cancel => Ok(()),
    };
    if let Err(error) = result {
        tracing::warn!(%error, "transcription command failed");
    }
}

fn publish_if_changed(
    updates: &Sender<Snapshot>,
    current: &Arc<Mutex<Snapshot>>,
    snapshot: Snapshot,
) {
    let changed = current
        .lock()
        .map(|mut current| {
            if *current == snapshot {
                false
            } else {
                *current = snapshot.clone();
                true
            }
        })
        .unwrap_or(true);
    if changed {
        let _ = updates.send_blocking(snapshot);
    }
}
