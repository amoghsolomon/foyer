use std::time::{Duration, Instant};

use foyer_shell_ui::{FoyerShellIcon, Root, tokens};
use gpui::{
    Animation, AnimationExt, App, Bounds, Context, DisplayId, Entity, FontWeight, Pixels, Render,
    Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions, div,
    ease_out_quint, layer_shell::*, point, prelude::*, px, rgb, size,
};
use gpui_component::{Icon, IconName, Sizable, h_flex, progress::Progress, v_flex};

use crate::state::FoyerShellState;

const WIDTH: f32 = 340.0;
const HEIGHT: f32 = 82.0;
const HIDE_AFTER: Duration = Duration::from_millis(1_600);
const OPTIMISTIC_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub struct OsdSurface {
    view: Entity<Osd>,
    handle: WindowHandle<Root>,
    display_id: DisplayId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Content {
    Volume { percent: u16, muted: bool },
    Microphone { percent: u16, muted: bool },
    Brightness { percent: u8 },
}

struct Osd {
    content: Content,
    generation: u64,
    optimistic: Option<Optimistic>,
}

#[derive(Clone, Copy, Debug)]
struct Optimistic {
    target: Content,
    direction: i8,
    expires_at: Instant,
}

pub fn handle_snapshot_change(
    previous: &foyer_shell_services::Snapshot,
    current: &foyer_shell_services::Snapshot,
    cx: &mut App,
) {
    if previous.audio.availability.is_available()
        && current.audio.availability.is_available()
        && ((previous.audio.volume - current.audio.volume).abs() > 0.005
            || previous.audio.muted != current.audio.muted)
    {
        show_authoritative(
            Content::Volume {
                percent: (current.audio.volume * 100.0).round().clamp(0.0, 150.0) as u16,
                muted: current.audio.muted,
            },
            cx,
        );
    }

    if previous.audio.availability.is_available()
        && current.audio.availability.is_available()
        && ((previous.audio.input_volume - current.audio.input_volume).abs() > 0.005
            || previous.audio.input_muted != current.audio.input_muted)
    {
        show_authoritative(
            Content::Microphone {
                percent: (current.audio.input_volume * 100.0)
                    .round()
                    .clamp(0.0, 100.0) as u16,
                muted: current.audio.input_muted,
            },
            cx,
        );
    }

    if previous.brightness.availability.is_available()
        && current.brightness.availability.is_available()
        && previous.brightness.percent != current.brightness.percent
    {
        show_authoritative(
            Content::Brightness {
                percent: current.brightness.percent,
            },
            cx,
        );
    }
}

pub fn preview_volume(delta_percent: i16, cx: &mut App) -> f32 {
    let current = current_content(ContentKind::Volume, cx).unwrap_or_else(|| {
        let audio = &FoyerShellState::global(cx).services.audio;
        Content::Volume {
            percent: (audio.volume * 100.0).round().clamp(0.0, 150.0) as u16,
            muted: audio.muted,
        }
    });
    let Content::Volume { percent, muted } = current else {
        return 0.0;
    };
    let percent = stepped(percent, delta_percent, 0, 150);
    show_optimistic(
        Content::Volume { percent, muted },
        delta_percent.signum() as i8,
        cx,
    );
    f32::from(percent) / 100.0
}

pub fn preview_input_volume(delta_percent: i16, cx: &mut App) -> f32 {
    let current = current_content(ContentKind::Microphone, cx).unwrap_or_else(|| {
        let audio = &FoyerShellState::global(cx).services.audio;
        Content::Microphone {
            percent: (audio.input_volume * 100.0).round().clamp(0.0, 100.0) as u16,
            muted: audio.input_muted,
        }
    });
    let Content::Microphone { percent, muted } = current else {
        return 0.0;
    };
    let percent = stepped(percent, delta_percent, 0, 100);
    show_optimistic(
        Content::Microphone { percent, muted },
        delta_percent.signum() as i8,
        cx,
    );
    f32::from(percent) / 100.0
}

pub fn preview_brightness(delta_percent: i16, cx: &mut App) -> u8 {
    let current =
        current_content(ContentKind::Brightness, cx).unwrap_or_else(|| Content::Brightness {
            percent: FoyerShellState::global(cx).services.brightness.percent,
        });
    let Content::Brightness { percent } = current else {
        return 1;
    };
    let percent = stepped(u16::from(percent), delta_percent, 1, 100) as u8;
    show_optimistic(
        Content::Brightness { percent },
        delta_percent.signum() as i8,
        cx,
    );
    percent
}

pub fn preview_toggle_mute(input: bool, cx: &mut App) -> bool {
    let kind = if input {
        ContentKind::Microphone
    } else {
        ContentKind::Volume
    };
    let current = current_content(kind, cx).unwrap_or_else(|| {
        let audio = &FoyerShellState::global(cx).services.audio;
        if input {
            Content::Microphone {
                percent: (audio.input_volume * 100.0).round().clamp(0.0, 100.0) as u16,
                muted: audio.input_muted,
            }
        } else {
            Content::Volume {
                percent: (audio.volume * 100.0).round().clamp(0.0, 150.0) as u16,
                muted: audio.muted,
            }
        }
    });
    let toggled = match current {
        Content::Volume { percent, muted } => Content::Volume {
            percent,
            muted: !muted,
        },
        Content::Microphone { percent, muted } => Content::Microphone {
            percent,
            muted: !muted,
        },
        Content::Brightness { .. } => return false,
    };
    let muted = match toggled {
        Content::Volume { muted, .. } | Content::Microphone { muted, .. } => muted,
        Content::Brightness { .. } => false,
    };
    show_optimistic(toggled, 0, cx);
    muted
}

pub fn close_on_display(display_id: DisplayId, cx: &mut App) {
    let Some(surface) = FoyerShellState::global(cx).osd_surface.clone() else {
        return;
    };
    if surface.display_id != display_id {
        return;
    }
    let _ = surface
        .handle
        .update(cx, |_, window, _| window.remove_window());
    FoyerShellState::global_mut(cx).osd_surface = None;
    cx.refresh_windows();
}

fn show_authoritative(content: Content, cx: &mut App) {
    show(content, None, cx);
}

fn show_optimistic(content: Content, direction: i8, cx: &mut App) {
    show(content, Some(direction), cx);
}

fn show(content: Content, optimistic_direction: Option<i8>, cx: &mut App) {
    let generation = if let Some(surface) = FoyerShellState::global(cx).osd_surface.clone() {
        surface.view.update(cx, |osd, cx| {
            if optimistic_direction.is_none() && !osd.accept_authoritative(content) {
                return None;
            }
            osd.generation = osd.generation.wrapping_add(1).max(1);
            osd.content = content;
            if let Some(direction) = optimistic_direction {
                osd.optimistic = Some(Optimistic {
                    target: content,
                    direction,
                    expires_at: Instant::now() + OPTIMISTIC_TIMEOUT,
                });
            }
            cx.notify();
            Some(osd.generation)
        })
    } else {
        let Some(surface) = open(content, optimistic_direction, cx) else {
            return;
        };
        let generation = Some(surface.view.read(cx).generation);
        FoyerShellState::global_mut(cx).osd_surface = Some(surface);
        generation
    };
    let Some(generation) = generation else {
        return;
    };
    schedule_hide(generation, cx);
    cx.refresh_windows();
}

fn open(content: Content, optimistic_direction: Option<i8>, cx: &mut App) -> Option<OsdSurface> {
    let display_id = FoyerShellState::focused_display_id(cx)
        .or_else(|| cx.displays().first().map(|display| display.id()))?;
    let view = cx.new(|_| Osd {
        content,
        generation: 1,
        optimistic: optimistic_direction.map(|direction| Optimistic {
            target: content,
            direction,
            expires_at: Instant::now() + OPTIMISTIC_TIMEOUT,
        }),
    });
    open_surface(view, display_id, cx)
}

fn open_surface(view: Entity<Osd>, display_id: DisplayId, cx: &mut App) -> Option<OsdSurface> {
    let right_margin = crate::panel::ambient_right_margin(display_id, cx);
    let options = WindowOptions {
        display_id: Some(display_id),
        titlebar: None,
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.0), px(0.0)),
            size: size(px(WIDTH), px(HEIGHT)),
        })),
        app_id: Some("foyer-shell-osd".into()),
        window_background: WindowBackgroundAppearance::Transparent,
        kind: WindowKind::LayerShell(LayerShellOptions {
            namespace: "foyer-shell-osd".into(),
            layer: Layer::Overlay,
            anchor: Anchor::TOP | Anchor::RIGHT,
            margin: Some((px(12.0), px(right_margin), px(0.0), px(0.0))),
            keyboard_interactivity: KeyboardInteractivity::None,
            ..Default::default()
        }),
        focus: false,
        ..Default::default()
    };

    let root_view = view.clone();
    let handle = match cx.open_window(options, move |window, cx| {
        cx.new(|cx| {
            Root::new(root_view, window, cx)
                .bordered(false)
                .bg(gpui::transparent_black())
        })
    }) {
        Ok(handle) => handle,
        Err(error) => {
            tracing::error!(%error, "failed to open OSD surface");
            return None;
        }
    };
    Some(OsdSurface {
        view,
        handle,
        display_id,
    })
}

pub fn reposition(display_id: DisplayId, cx: &mut App) {
    let should_reposition = FoyerShellState::global(cx)
        .osd_surface
        .as_ref()
        .is_some_and(|surface| surface.display_id == display_id);
    if !should_reposition {
        return;
    }
    let Some(surface) = FoyerShellState::global_mut(cx).osd_surface.take() else {
        return;
    };
    let _ = surface
        .handle
        .update(cx, |_, window, _| window.remove_window());
    FoyerShellState::global_mut(cx).osd_surface = open_surface(surface.view, display_id, cx);
}

fn schedule_hide(generation: u64, cx: &mut App) {
    cx.spawn(async move |cx| {
        cx.background_executor().timer(HIDE_AFTER).await;
        cx.update(|cx| {
            let Some(surface) = FoyerShellState::global(cx).osd_surface.clone() else {
                return;
            };
            if surface.view.read(cx).generation != generation {
                return;
            }
            let _ = surface
                .handle
                .update(cx, |_, window, _| window.remove_window());
            FoyerShellState::global_mut(cx).osd_surface = None;
            cx.refresh_windows();
        });
    })
    .detach();
}

impl Render for Osd {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let no_input: [Bounds<Pixels>; 0] = [];
        window.set_input_region(Some(&no_input));
        let (icon, label, value) = match self.content {
            Content::Volume { percent, muted } => (
                Icon::new(FoyerShellIcon::Volume),
                if muted {
                    "Muted".into()
                } else {
                    format!("{percent}%")
                },
                if muted { 0.0 } else { f32::from(percent) },
            ),
            Content::Brightness { percent } => (
                Icon::new(IconName::Sun),
                format!("{percent}%"),
                f32::from(percent),
            ),
            Content::Microphone { percent, muted } => (
                Icon::new(FoyerShellIcon::Microphone),
                if muted {
                    "Muted".into()
                } else {
                    format!("{percent}%")
                },
                if muted { 0.0 } else { f32::from(percent) },
            ),
        };

        h_flex()
            .id("osd")
            .size_full()
            .gap_3()
            .px_4()
            .rounded_lg()
            .border_1()
            .border_color(rgb(tokens::BORDER))
            .bg(rgb(tokens::SURFACE))
            .text_color(rgb(tokens::FOREGROUND))
            .child(
                div()
                    .w_10()
                    .text_color(rgb(tokens::MUTED))
                    .child(icon.large()),
            )
            .child(
                v_flex()
                    .flex_1()
                    .gap_2()
                    .child(
                        h_flex()
                            .justify_between()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(
                                match self.content {
                                    Content::Volume { .. } => "Volume",
                                    Content::Microphone { .. } => "Microphone",
                                    Content::Brightness { .. } => "Brightness",
                                },
                            ))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(label),
                            ),
                    )
                    .child(Progress::new("osd-level").value(value.min(100.0)).small()),
            )
            .with_animation(
                "osd-in",
                Animation::new(Duration::from_millis(140)).with_easing(ease_out_quint()),
                |osd, delta| {
                    osd.top(px((1.0 - delta) * 10.0))
                        .opacity(0.55 + delta * 0.45)
                },
            )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ContentKind {
    Volume,
    Microphone,
    Brightness,
}

impl Content {
    fn kind(self) -> ContentKind {
        match self {
            Self::Volume { .. } => ContentKind::Volume,
            Self::Microphone { .. } => ContentKind::Microphone,
            Self::Brightness { .. } => ContentKind::Brightness,
        }
    }

    fn level(self) -> u16 {
        match self {
            Self::Volume { percent, .. } | Self::Microphone { percent, .. } => percent,
            Self::Brightness { percent } => u16::from(percent),
        }
    }
}

impl Osd {
    fn accept_authoritative(&mut self, content: Content) -> bool {
        let Some(optimistic) = self.optimistic else {
            return true;
        };
        if optimistic.target.kind() != content.kind() || Instant::now() >= optimistic.expires_at {
            self.optimistic = None;
            return true;
        }
        let matches_state = match (optimistic.target, content) {
            (
                Content::Volume {
                    muted: expected, ..
                },
                Content::Volume { muted: actual, .. },
            )
            | (
                Content::Microphone {
                    muted: expected, ..
                },
                Content::Microphone { muted: actual, .. },
            ) => expected == actual,
            (Content::Brightness { .. }, Content::Brightness { .. }) => true,
            _ => false,
        };
        let reached = matches_state
            && match optimistic.direction.cmp(&0) {
                std::cmp::Ordering::Greater => content.level() >= optimistic.target.level(),
                std::cmp::Ordering::Less => content.level() <= optimistic.target.level(),
                std::cmp::Ordering::Equal => content == optimistic.target,
            };
        if reached {
            self.optimistic = None;
        }
        reached
    }
}

fn current_content(kind: ContentKind, cx: &App) -> Option<Content> {
    let surface = FoyerShellState::global(cx).osd_surface.as_ref()?;
    let content = surface.view.read(cx).content;
    (content.kind() == kind).then_some(content)
}

fn stepped(value: u16, delta: i16, minimum: u16, maximum: u16) -> u16 {
    i32::from(value)
        .saturating_add(i32::from(delta))
        .clamp(i32::from(minimum), i32::from(maximum)) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optimistic_value_rejects_lagging_snapshots() {
        let mut osd = Osd {
            content: Content::Volume {
                percent: 60,
                muted: false,
            },
            generation: 1,
            optimistic: Some(Optimistic {
                target: Content::Volume {
                    percent: 60,
                    muted: false,
                },
                direction: 1,
                expires_at: Instant::now() + Duration::from_secs(1),
            }),
        };
        assert!(!osd.accept_authoritative(Content::Volume {
            percent: 55,
            muted: false,
        }));
        assert!(osd.accept_authoritative(Content::Volume {
            percent: 60,
            muted: false,
        }));
    }

    #[test]
    fn stepping_clamps_without_wrapping() {
        assert_eq!(stepped(2, -5, 1, 100), 1);
        assert_eq!(stepped(148, 5, 0, 150), 150);
    }
}
