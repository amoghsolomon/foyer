use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use chrono::Local;
use foyer_shell_ui::{Root, tokens};
use gpui::{
    App, Bounds, Context, Entity, FontWeight, Global, Render, Subscription, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div, prelude::*, px,
    relative, rgb, size,
};
use gpui_component::v_flex;

pub const APP_ID: &str = "foyer-shell-workspace";
const WORKSPACE_TRANSITION_MS: u64 = 460;

const UPCOMING: &str = "Up next, we’re building the quiet center of Foyer Shell: persistent agent work, clear approvals, useful memory, and explainable presentations that appear only when they have something worth showing.";
const THOUGHT: &str = "The best interface knows when to remain still.";

pub struct Overview;

impl Overview {
    fn new(cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(15))
                    .await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();
        Self
    }
}

#[allow(dead_code)]
enum WorkspaceCommand {
    Present(String),
    Replay(PathBuf),
}

#[derive(Clone)]
struct WorkspaceController {
    commands: async_channel::Sender<WorkspaceCommand>,
}

impl Global for WorkspaceController {}

#[allow(dead_code)]
pub fn present(prompt: String, cx: &mut App) {
    let _ = cx
        .global::<WorkspaceController>()
        .commands
        .try_send(WorkspaceCommand::Present(prompt));
}

pub fn replay(path: PathBuf, cx: &mut App) {
    let _ = cx
        .global::<WorkspaceController>()
        .commands
        .try_send(WorkspaceCommand::Replay(path));
}

struct WorkspaceTransition {
    entering: bool,
    started: Instant,
}

struct WorkspaceHost {
    overview: Entity<Overview>,
    player: Option<Entity<foyer_shell_presentation_player::PresentationView>>,
    transition: Option<WorkspaceTransition>,
    player_subscription: Option<Subscription>,
}

impl WorkspaceHost {
    fn new(
        commands: async_channel::Receiver<WorkspaceCommand>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let overview = cx.new(Overview::new);
        cx.spawn_in(window, async move |this, cx| {
            while let Ok(command) = commands.recv().await {
                let source = match command {
                    WorkspaceCommand::Present(prompt) => PlayerSource::Live(prompt),
                    WorkspaceCommand::Replay(path) => {
                        let (sender, receiver) = async_channel::bounded(1);
                        std::thread::spawn(move || {
                            let _ = sender.send_blocking(
                                foyer_shell_presentation::PresentationBundle::open(path),
                            );
                        });
                        match receiver.recv().await {
                            Ok(Ok(bundle)) => PlayerSource::Replay(bundle),
                            Ok(Err(error)) => {
                                tracing::error!(%error, "failed to load presentation bundle");
                                continue;
                            }
                            Err(_) => continue,
                        }
                    }
                };
                if this
                    .update_in(cx, |host, window, cx| {
                        let player = cx.new(|cx| match source {
                            PlayerSource::Live(prompt) => {
                                foyer_shell_presentation_player::PresentationView::live_embedded(
                                    prompt, window, cx,
                                )
                            }
                            PlayerSource::Replay(bundle) => {
                                foyer_shell_presentation_player::PresentationView::replay(
                                    bundle, window, cx,
                                )
                            }
                        });
                        let subscription = cx.subscribe(
                            &player,
                            |host, _, event: &foyer_shell_presentation_player::PlayerEvent, cx| {
                                if matches!(
                                    event,
                                    foyer_shell_presentation_player::PlayerEvent::Exit
                                ) {
                                    host.transition = Some(WorkspaceTransition {
                                        entering: false,
                                        started: Instant::now(),
                                    });
                                    cx.notify();
                                }
                            },
                        );
                        host.player = Some(player);
                        host.player_subscription = Some(subscription);
                        host.transition = Some(WorkspaceTransition {
                            entering: true,
                            started: Instant::now(),
                        });
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        Self {
            overview,
            player: None,
            transition: None,
            player_subscription: None,
        }
    }
}

#[allow(clippy::large_enum_variant)]
enum PlayerSource {
    Live(String),
    Replay(foyer_shell_presentation::PresentationBundle),
}

impl Render for WorkspaceHost {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let width = f32::from(window.viewport_size().width);
        let mut track_x = 0.0;
        if self.player.is_some() {
            if let Some(transition) = self.transition.as_ref() {
                let raw = (transition.started.elapsed().as_millis() as f32
                    / WORKSPACE_TRANSITION_MS as f32)
                    .clamp(0.0, 1.0);
                let progress = raw * raw * (3.0 - 2.0 * raw);
                if transition.entering {
                    track_x = -width * progress;
                } else {
                    track_x = -width * (1.0 - progress);
                }
                if raw < 1.0 {
                    window.request_animation_frame();
                } else if transition.entering {
                    self.transition = None;
                    track_x = -width;
                    if let Some(player) = self.player.clone() {
                        window.defer(cx, move |_, cx| {
                            player.update(cx, |player, cx| player.resume_playback(cx));
                        });
                    }
                } else {
                    self.transition = None;
                    self.player = None;
                    self.player_subscription = None;
                    track_x = 0.0;
                }
            } else {
                track_x = -width;
            }
        }

        let root = div()
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(rgb(tokens::BACKGROUND));
        if let Some(player) = self.player.clone() {
            root.child(
                div()
                    .absolute()
                    .left(px(track_x))
                    .top_0()
                    .h_full()
                    .w(px(width * 2.0))
                    .flex()
                    .child(
                        div()
                            .w(px(width))
                            .h_full()
                            .flex_none()
                            .child(self.overview.clone()),
                    )
                    .child(div().w(px(width)).h_full().flex_none().child(player)),
            )
        } else {
            root.child(self.overview.clone())
        }
    }
}

impl Render for Overview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let now = Local::now();
        let time = now.format("%H:%M").to_string();
        let date = now.format("%A, %B %-d").to_string();

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(tokens::BACKGROUND))
            .text_color(rgb(tokens::FOREGROUND))
            .px(px(72.0))
            .py(px(60.0))
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(54.0))
                            .line_height(relative(1.0))
                            .font_weight(FontWeight::MEDIUM)
                            .child(time),
                    )
                    .child(div().text_size(px(16.0)).child(date)),
            )
            .child(
                div().flex_1().flex().items_center().child(
                    div()
                        .max_w(px(860.0))
                        .text_size(px(32.0))
                        .line_height(relative(1.28))
                        .font_weight(FontWeight::MEDIUM)
                        .child(UPCOMING),
                ),
            )
            .child(
                div()
                    .text_size(px(16.0))
                    .line_height(relative(1.4))
                    .child(THOUGHT),
            )
    }
}

pub fn open(cx: &mut App) {
    let (commands, command_receiver) = async_channel::unbounded();
    cx.set_global(WorkspaceController { commands });
    let restore_bounds = Bounds::centered(None, size(px(1440.0), px(900.0)), cx);
    let options = WindowOptions {
        titlebar: None,
        window_bounds: Some(WindowBounds::Maximized(restore_bounds)),
        app_id: Some(APP_ID.into()),
        window_background: WindowBackgroundAppearance::Opaque,
        kind: WindowKind::Normal,
        focus: false,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        ..Default::default()
    };

    match cx.open_window(options, |window, cx| {
        window.on_window_should_close(cx, |_, _| false);
        let workspace_view = cx.new(|cx| WorkspaceHost::new(command_receiver, window, cx));
        cx.new(|cx| Root::new(workspace_view, window, cx).bordered(false))
    }) {
        Ok(_) => tracing::info!("opened Foyer Shell Workspace 1"),
        Err(error) => tracing::error!(%error, "failed to open Foyer Shell Workspace 1"),
    }
}
