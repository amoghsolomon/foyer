//! Niri-owned compositor state and commands for Foyer Shell.

use std::{thread, time::Duration};

use anyhow::{Context as _, Result, anyhow};
use async_channel::{Receiver, Sender};
use niri_ipc::{
    Action, Event, Request, Response, WorkspaceReferenceArg,
    socket::Socket,
    state::{EventStreamState, EventStreamStatePart},
};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub connected: bool,
    pub outputs: Vec<Output>,
    pub workspaces: Vec<Workspace>,
    pub windows: Vec<Window>,
    pub focused_window: Option<FocusedWindow>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workspace {
    pub id: u64,
    pub index: u8,
    pub name: Option<String>,
    pub output: Option<String>,
    pub active: bool,
    pub focused: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    pub id: u64,
    pub title: String,
    pub app_id: String,
    pub workspace_id: Option<u64>,
    pub focused: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FocusedWindow {
    pub id: u64,
    pub title: String,
    pub app_id: String,
}

/// Starts a reconnecting Niri event-stream listener on a blocking worker thread.
pub fn subscribe() -> Receiver<Snapshot> {
    let (sender, receiver) = async_channel::unbounded();
    thread::Builder::new()
        .name("foyer-shell-niri-events".into())
        .spawn(move || listen_forever(sender))
        .expect("failed to start Niri event listener");
    receiver
}

/// Focuses a workspace using its stable Niri id.
pub fn focus_workspace(id: u64) -> Result<()> {
    send_action(Action::FocusWorkspace {
        reference: WorkspaceReferenceArg::Id(id),
    })
}

/// Moves a specific window without changing the user's focused workspace.
pub fn move_window_to_workspace(window_id: u64, workspace_id: u64) -> Result<()> {
    send_action(Action::MoveWindowToWorkspace {
        window_id: Some(window_id),
        reference: WorkspaceReferenceArg::Id(workspace_id),
        focus: false,
    })
}

/// Restores a workspace to a stable position on its current output.
pub fn move_workspace_to_index(workspace_id: u64, index: usize) -> Result<()> {
    send_action(Action::MoveWorkspaceToIndex {
        index,
        reference: Some(WorkspaceReferenceArg::Id(workspace_id)),
    })
}

/// Asks Niri to spawn a process without going through a shell.
pub fn spawn(command: Vec<String>) -> Result<()> {
    if command.is_empty() {
        return Err(anyhow!("cannot spawn an empty command"));
    }
    send_action(Action::Spawn { command })
}

/// Ends the current Niri session after confirmation has already been collected by Foyer Shell.
pub fn quit() -> Result<()> {
    send_action(Action::Quit {
        skip_confirmation: true,
    })
}

fn listen_forever(sender: Sender<Snapshot>) {
    loop {
        if let Err(error) = listen_once(&sender) {
            tracing::warn!(%error, "Niri event stream disconnected; retrying");
            let _ = sender.send_blocking(Snapshot::default());
            thread::sleep(Duration::from_millis(750));
        }
    }
}

fn listen_once(sender: &Sender<Snapshot>) -> Result<()> {
    let mut outputs = fetch_outputs().unwrap_or_default();
    let mut socket = Socket::connect().context("connect to Niri IPC")?;
    let reply = socket
        .send(Request::EventStream)
        .context("request Niri event stream")?
        .map_err(|message| anyhow!(message))?;
    if !matches!(reply, Response::Handled) {
        return Err(anyhow!("unexpected Niri event-stream response: {reply:?}"));
    }

    tracing::info!("connected to Niri event stream");
    let mut state = EventStreamState::default();
    let mut read_event = socket.read_events();
    loop {
        let event = read_event().context("read Niri event")?;
        let outputs_may_have_changed = matches!(&event, Event::WorkspacesChanged { .. });
        state.apply(event);
        if outputs_may_have_changed {
            outputs = fetch_outputs().unwrap_or(outputs);
        }
        sender
            .send_blocking(map_snapshot(&state, &outputs))
            .context("publish Niri state")?;
    }
}

fn fetch_outputs() -> Result<Vec<Output>> {
    let mut socket = Socket::connect().context("connect to Niri IPC for outputs")?;
    let response = socket
        .send(Request::Outputs)
        .context("request Niri outputs")?
        .map_err(|message| anyhow!(message))?;
    let Response::Outputs(outputs) = response else {
        return Err(anyhow!("unexpected Niri outputs response: {response:?}"));
    };

    let mut outputs = outputs
        .into_values()
        .filter_map(|output| {
            let logical = output.logical?;
            Some(Output {
                name: output.name,
                x: logical.x,
                y: logical.y,
                width: logical.width,
                height: logical.height,
                scale: logical.scale,
            })
        })
        .collect::<Vec<_>>();
    outputs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(outputs)
}

fn map_snapshot(state: &EventStreamState, outputs: &[Output]) -> Snapshot {
    let mut workspaces = state
        .workspaces
        .workspaces
        .values()
        .map(|workspace| Workspace {
            id: workspace.id,
            index: workspace.idx,
            name: workspace.name.clone(),
            output: workspace.output.clone(),
            active: workspace.is_active,
            focused: workspace.is_focused,
        })
        .collect::<Vec<_>>();
    workspaces.sort_by(|a, b| a.output.cmp(&b.output).then_with(|| a.index.cmp(&b.index)));

    let mut windows = state
        .windows
        .windows
        .values()
        .map(|window| Window {
            id: window.id,
            title: window.title.clone().unwrap_or_default(),
            app_id: window.app_id.clone().unwrap_or_default(),
            workspace_id: window.workspace_id,
            focused: window.is_focused,
        })
        .collect::<Vec<_>>();
    windows.sort_by_key(|window| window.id);

    let focused_window = state
        .windows
        .windows
        .values()
        .find(|window| window.is_focused)
        .map(|window| FocusedWindow {
            id: window.id,
            title: window.title.clone().unwrap_or_default(),
            app_id: window.app_id.clone().unwrap_or_default(),
        });

    Snapshot {
        connected: true,
        outputs: outputs.to_vec(),
        workspaces,
        windows,
        focused_window,
    }
}

fn send_action(action: Action) -> Result<()> {
    let mut socket = Socket::connect().context("connect to Niri IPC")?;
    let response = socket
        .send(Request::Action(action))
        .context("send Niri action")?
        .map_err(|message| anyhow!(message))?;
    if matches!(response, Response::Handled) {
        Ok(())
    } else {
        Err(anyhow!("unexpected Niri action response: {response:?}"))
    }
}
