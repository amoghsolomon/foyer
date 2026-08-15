use std::{
    cell::Cell,
    f32::consts::TAU,
    rc::Rc,
    time::{Duration, Instant},
};

use chrono::Local;
use foyer_shell_ui::{FoyerShellIcon, Root, tokens};
use gpui::{
    App, Bounds, ColorSpace, Context, DisplayId, ElementId, Entity, FontWeight, Hsla, MouseButton,
    Render, SharedString, Size, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle,
    WindowKind, WindowOptions, canvas, div, fill, layer_shell::*, linear_color_stop,
    linear_gradient, point, prelude::*, px, rgb, size,
};
use gpui_component::{
    Disableable, ElementExt, Icon, IconName,
    button::{Button, ButtonVariants},
    h_flex,
    tooltip::Tooltip,
    v_flex,
};

use crate::{
    notification, osd, panel, state::FoyerShellState, state::ToolbarSurface, tray_popover,
};

pub struct Toolbar {
    display_id: DisplayId,
    ambient_started: Instant,
    ambient_last_frame: Instant,
    ambient_visibility: f32,
    ambient_rms: f32,
    ambient_waveform: [f32; foyer_shell_transcription::WAVEFORM_SAMPLES],
}

const TOOLTIP_WIDTH: f32 = 240.0;
const TOOLTIP_HEIGHT: f32 = 48.0;
const TOOLTIP_SHOW_DELAY: Duration = Duration::from_millis(500);
const AMBIENT_PERIOD_SECONDS: f32 = 5.5;
const AMBIENT_ATTACK_SECONDS: f32 = 0.38;
const AMBIENT_RELEASE_SECONDS: f32 = 0.72;
const AMBIENT_RMS_ATTACK_SECONDS: f32 = 0.08;
const AMBIENT_RMS_RELEASE_SECONDS: f32 = 0.22;
const AMBIENT_WAVEFORM_ATTACK_SECONDS: f32 = 0.05;
const AMBIENT_WAVEFORM_RELEASE_SECONDS: f32 = 0.14;
const AMBIENT_STRIPS: usize = 64;
const AMBIENT_MIN_HEIGHT: f32 = 28.0;
const AMBIENT_CYAN: u32 = 0x48e4ff;
const AMBIENT_BLUE: u32 = 0x3d91ff;
const AMBIENT_VIOLET: u32 = 0xb18cff;
const AMBIENT_MAGENTA: u32 = 0xff72dc;
const AMBIENT_LAYERS: [(u32, f32); 4] = [
    (AMBIENT_CYAN, 0.0),
    (AMBIENT_BLUE, TAU * 0.25),
    (AMBIENT_VIOLET, TAU * 0.50),
    (AMBIENT_MAGENTA, TAU * 0.75),
];

pub struct ToolbarTooltipSurface {
    handle: WindowHandle<Root>,
}

struct ToolbarTooltip {
    tooltip: Entity<Tooltip>,
}

impl Render for ToolbarTooltip {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let no_input: [Bounds<gpui::Pixels>; 0] = [];
        window.set_input_region(Some(&no_input));
        h_flex()
            .size_full()
            .items_center()
            .justify_end()
            .child(self.tooltip.clone())
    }
}

impl Toolbar {
    fn new(display_id: DisplayId, cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(30))
                    .await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();
        Self {
            display_id,
            ambient_started: Instant::now(),
            ambient_last_frame: Instant::now(),
            ambient_visibility: 0.0,
            ambient_rms: 0.0,
            ambient_waveform: [0.0; foyer_shell_transcription::WAVEFORM_SAMPLES],
        }
    }
}

fn ambient_band(
    phase: f32,
    visibility: f32,
    rms: f32,
    waveform: [f32; foyer_shell_transcription::WAVEFORM_SAMPLES],
) -> impl gpui::IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            paint_ambient_band(bounds, phase, visibility, rms, &waveform, window)
        },
    )
    .w_full()
    .flex_1()
}

fn paint_ambient_band(
    bounds: Bounds<gpui::Pixels>,
    phase: f32,
    visibility: f32,
    rms: f32,
    waveform: &[f32; foyer_shell_transcription::WAVEFORM_SAMPLES],
    window: &mut Window,
) {
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    if height < AMBIENT_MIN_HEIGHT || width <= 0.0 || visibility <= 0.001 {
        return;
    }

    let center_y = height * 0.5;
    let strip_height = height / AMBIENT_STRIPS as f32;
    let energy = ambient_energy(rms);

    for index in 0..AMBIENT_STRIPS {
        let local_y = (index as f32 + 0.5) * strip_height;
        let normalized = (local_y - center_y) / center_y;
        let envelope = ambient_envelope(normalized);
        if envelope <= 0.0 {
            continue;
        }

        let live_wave = ambient_smoothed_waveform_sample(waveform, normalized);
        let wave = (live_wave * (1.12 + energy * 0.72)
            + ambient_wave(normalized, phase) * (0.055 + energy * 0.035))
            .clamp(-1.0, 1.0);
        let baseline = width * (0.43 - energy * 0.07);
        let displacement = wave * width * (0.18 + energy * 0.30) * envelope;
        let inset = (baseline + displacement).clamp(width * 0.04, width * 0.82);
        let strip_bounds = Bounds {
            origin: point(
                bounds.origin.x + px(inset),
                bounds.origin.y + px(local_y - strip_height * 0.5),
            ),
            size: size(px(width - inset), px(strip_height + 1.0)),
        };

        for (color, phase_offset) in AMBIENT_LAYERS {
            let color_flow = 0.72
                + (normalized * 4.4 - phase * 0.8 + phase_offset).sin() * 0.18
                + (normalized * 8.8 + phase * 0.45 - phase_offset).sin() * 0.10;
            let opacity = envelope
                * color_flow
                * (0.075 + live_wave.abs() * 0.28 + energy * 0.22)
                * visibility;
            let color = Hsla::from(rgb(color));
            let gradient = linear_gradient(
                90.0,
                linear_color_stop(color.opacity(0.0), 0.0),
                linear_color_stop(color.opacity(opacity), 1.0),
            )
            .color_space(ColorSpace::Oklab);
            window.paint_quad(fill(strip_bounds, gradient));
        }
    }
}

fn ambient_waveform_sample(
    waveform: &[f32; foyer_shell_transcription::WAVEFORM_SAMPLES],
    normalized: f32,
) -> f32 {
    let position =
        ((normalized + 1.0) * 0.5 * (foyer_shell_transcription::WAVEFORM_SAMPLES - 1) as f32)
            .clamp(
                0.0,
                (foyer_shell_transcription::WAVEFORM_SAMPLES - 1) as f32,
            );
    let left = position.floor() as usize;
    let right = (left + 1).min(foyer_shell_transcription::WAVEFORM_SAMPLES - 1);
    let fraction = position - left as f32;
    waveform[left] * (1.0 - fraction) + waveform[right] * fraction
}

fn ambient_smoothed_waveform_sample(
    waveform: &[f32; foyer_shell_transcription::WAVEFORM_SAMPLES],
    normalized: f32,
) -> f32 {
    let sample_step = 2.0 / (foyer_shell_transcription::WAVEFORM_SAMPLES - 1) as f32;
    ambient_waveform_sample(waveform, normalized) * 0.50
        + ambient_waveform_sample(waveform, normalized - sample_step) * 0.25
        + ambient_waveform_sample(waveform, normalized + sample_step) * 0.25
}

fn smooth_toward(current: f32, target: f32, elapsed: f32, seconds: f32) -> f32 {
    if seconds <= f32::EPSILON {
        return target;
    }
    let blend = 1.0 - (-elapsed.max(0.0) / seconds).exp();
    current + (target - current) * blend
}

fn ambient_wave(normalized: f32, phase: f32) -> f32 {
    (normalized * 7.4 - phase).sin() * 0.58
        + (normalized * 14.8 + phase * 2.0).sin() * 0.27
        + (normalized * 3.2 - phase * 3.0).sin() * 0.15
}

fn ambient_energy(rms: f32) -> f32 {
    let level = ((rms - 0.08) / 0.92).clamp(0.0, 1.0);
    level * level * (3.0 - 2.0 * level)
}

fn ambient_envelope(normalized: f32) -> f32 {
    let distance = normalized.abs();
    if distance >= 1.0 {
        return 0.0;
    }
    let remaining = 1.0 - distance;
    remaining * remaining * (3.0 - 2.0 * remaining)
}

pub(crate) fn icon_button(
    id: impl Into<ElementId>,
    icon: impl Into<Icon>,
    tooltip: impl Into<SharedString>,
) -> Button {
    let tooltip = tooltip.into();
    Button::new(id)
        .icon(icon.into())
        .accessibility_id(tooltip)
        .ghost()
        .compact()
        .tab_stop(false)
}

fn toolbar_item(
    id: &'static str,
    selected: bool,
    attention: bool,
    tooltip: Option<&'static str>,
    button: Button,
    display_id: DisplayId,
) -> impl IntoElement {
    toolbar_item_with_bounds(
        id,
        selected,
        attention,
        tooltip,
        button,
        display_id,
        Rc::new(Cell::new(Bounds::default())),
    )
}

fn toolbar_item_with_bounds(
    id: &'static str,
    selected: bool,
    attention: bool,
    tooltip: Option<&'static str>,
    button: Button,
    display_id: DisplayId,
    item_bounds: Rc<Cell<Bounds<gpui::Pixels>>>,
) -> impl IntoElement {
    let bounds_writer = item_bounds.clone();

    div()
        .id(id)
        .relative()
        .h_8()
        .w_full()
        .flex()
        .items_center()
        .justify_center()
        .on_prepaint(move |bounds, _, _| bounds_writer.set(bounds))
        .on_mouse_down(MouseButton::Left, |_, _, cx| hide_tooltip(cx))
        .when_some(tooltip, |item, tooltip| {
            item.on_hover(move |hovered, _, cx| {
                if *hovered {
                    show_tooltip(display_id, tooltip.into(), item_bounds.get().center().y, cx);
                } else {
                    hide_tooltip(cx);
                }
            })
        })
        .when(selected, |item| {
            item.child(
                div()
                    .absolute()
                    .left(px(3.0))
                    .h(px(14.0))
                    .w(px(2.0))
                    .rounded_full()
                    .bg(rgb(tokens::FOREGROUND)),
            )
        })
        .when(attention, |item| {
            item.child(
                div()
                    .absolute()
                    .top(px(5.0))
                    .right(px(7.0))
                    .size(px(5.0))
                    .rounded_full()
                    .bg(rgb(tokens::FOREGROUND)),
            )
        })
        .child(button)
}

fn clock_readout(hour: String, minute: String) -> gpui::Div {
    v_flex()
        .h_8()
        .w_full()
        .items_center()
        .justify_center()
        .text_center()
        .text_xs()
        .line_height(px(11.0))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(tokens::MUTED))
        .child(hour)
        .child(minute)
}

fn show_tooltip(display_id: DisplayId, label: SharedString, anchor_y: gpui::Pixels, cx: &mut App) {
    let (generation, replace_visible) = {
        let state = FoyerShellState::global_mut(cx);
        state.toolbar_tooltip_generation = state.toolbar_tooltip_generation.wrapping_add(1);
        (
            state.toolbar_tooltip_generation,
            state.toolbar_tooltip_surface.is_some(),
        )
    };
    close_tooltip_surface(cx);

    if replace_visible {
        open_tooltip(display_id, label, anchor_y, generation, cx);
        return;
    }

    cx.spawn(async move |cx| {
        cx.background_executor().timer(TOOLTIP_SHOW_DELAY).await;
        cx.update(|cx| {
            if FoyerShellState::global(cx).toolbar_tooltip_generation == generation {
                open_tooltip(display_id, label, anchor_y, generation, cx);
            }
        });
    })
    .detach();
}

fn hide_tooltip(cx: &mut App) {
    let state = FoyerShellState::global_mut(cx);
    state.toolbar_tooltip_generation = state.toolbar_tooltip_generation.wrapping_add(1);
    close_tooltip_surface(cx);
}

fn close_tooltip_surface(cx: &mut App) {
    let Some(surface) = FoyerShellState::global_mut(cx)
        .toolbar_tooltip_surface
        .take()
    else {
        return;
    };
    let _ = surface
        .handle
        .update(cx, |_, window, _| window.remove_window());
    cx.refresh_windows();
}

fn close_tooltip_on_display(_: DisplayId, cx: &mut App) {
    // Also invalidates a tooltip whose delayed open has not created a surface yet.
    hide_tooltip(cx);
}

fn open_tooltip(
    display_id: DisplayId,
    label: SharedString,
    anchor_y: gpui::Pixels,
    generation: u64,
    cx: &mut App,
) {
    if FoyerShellState::global(cx).toolbar_tooltip_generation != generation {
        return;
    }

    let display_height = FoyerShellState::display_size(display_id, cx)
        .map(|display| display.height)
        .unwrap_or(px(1080.0));
    let max_top = (display_height - px(TOOLTIP_HEIGHT)).max(px(0.0));
    let top = (anchor_y - px(TOOLTIP_HEIGHT / 2.0))
        .max(px(0.0))
        .min(max_top);
    let options = WindowOptions {
        display_id: Some(display_id),
        titlebar: None,
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.0), px(0.0)),
            size: size(px(TOOLTIP_WIDTH), px(TOOLTIP_HEIGHT)),
        })),
        app_id: Some("foyer-shell-toolbar-tooltip".into()),
        window_background: WindowBackgroundAppearance::Transparent,
        kind: WindowKind::LayerShell(LayerShellOptions {
            namespace: "foyer-shell-toolbar-tooltip".into(),
            layer: Layer::Overlay,
            anchor: Anchor::TOP | Anchor::RIGHT,
            margin: Some((top, px(0.0), px(0.0), px(0.0))),
            keyboard_interactivity: KeyboardInteractivity::None,
            ..Default::default()
        }),
        focus: false,
        ..Default::default()
    };

    match cx.open_window(options, move |window, cx| {
        let tooltip = cx.new(|_| Tooltip::new(label));
        let view = cx.new(|_| ToolbarTooltip { tooltip });
        cx.new(|cx| {
            Root::new(view, window, cx)
                .bordered(false)
                .bg(gpui::transparent_black())
        })
    }) {
        Ok(handle) => {
            FoyerShellState::global_mut(cx).toolbar_tooltip_surface =
                Some(ToolbarTooltipSurface { handle });
        }
        Err(error) => tracing::error!(?display_id, %error, "failed to open toolbar tooltip"),
    }
}

impl Render for Toolbar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = FoyerShellState::global(cx);
        let connected = state.niri.connected;
        let has_unread_notifications = state.storage.unread_count > 0;
        let microphone_active = !state.services.audio.recording_apps.is_empty();
        let tray_attention = state
            .services
            .tray
            .items
            .iter()
            .any(|item| item.status == "NeedsAttention");
        let low_battery = state.services.battery.present
            && state.services.battery.percentage <= 15
            && state.services.battery.state == "discharging";
        let transcription_active = state.transcription.state.is_active();
        let transcription_rms = state.transcription.rms;
        let transcription_waveform = *state.transcription.waveform;
        let display_id = self.display_id;
        let hour = Local::now().format("%H").to_string();
        let minute = Local::now().format("%M").to_string();
        let panel_open = panel::is_open_on(display_id, cx);
        let tray_bounds = Rc::new(Cell::new(Bounds::default()));
        let clicked_tray_bounds = tray_bounds.clone();
        let now = Instant::now();
        let elapsed = now
            .duration_since(self.ambient_last_frame)
            .as_secs_f32()
            .min(0.1);
        self.ambient_last_frame = now;
        let visibility_target = if transcription_active { 1.0 } else { 0.0 };
        let visibility_seconds = if transcription_active {
            AMBIENT_ATTACK_SECONDS
        } else {
            AMBIENT_RELEASE_SECONDS
        };
        self.ambient_visibility = smooth_toward(
            self.ambient_visibility,
            visibility_target,
            elapsed,
            visibility_seconds,
        );
        self.ambient_rms = smooth_toward(
            self.ambient_rms,
            if transcription_active {
                transcription_rms
            } else {
                0.0
            },
            elapsed,
            if transcription_rms > self.ambient_rms {
                AMBIENT_RMS_ATTACK_SECONDS
            } else {
                AMBIENT_RMS_RELEASE_SECONDS
            },
        );
        for (smoothed, sample) in self.ambient_waveform.iter_mut().zip(transcription_waveform) {
            let target = if transcription_active { sample } else { 0.0 };
            *smoothed = smooth_toward(
                *smoothed,
                target,
                elapsed,
                if target.signum() != smoothed.signum() || target.abs() > smoothed.abs() {
                    AMBIENT_WAVEFORM_ATTACK_SECONDS
                } else {
                    AMBIENT_WAVEFORM_RELEASE_SECONDS
                },
            );
        }
        if !transcription_active && self.ambient_visibility < 0.001 {
            self.ambient_visibility = 0.0;
            self.ambient_rms = 0.0;
            self.ambient_waveform.fill(0.0);
        }
        let should_animate = transcription_active || self.ambient_visibility > 0.001;
        let phase = if cx.reduce_motion() {
            0.0
        } else {
            if should_animate {
                window.request_animation_frame();
            }
            (self.ambient_started.elapsed().as_secs_f32() / AMBIENT_PERIOD_SECONDS * TAU) % TAU
        };
        let ambient_visibility = self.ambient_visibility;
        let ambient_rms = self.ambient_rms;
        let ambient_waveform = self.ambient_waveform;
        div().size_full().flex().child(
            div()
                .relative()
                .h_full()
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_between()
                .border_l_1()
                .border_color(rgb(tokens::BORDER))
                .bg(rgb(tokens::BACKGROUND))
                .text_color(rgb(tokens::FOREGROUND))
                .child(
                    v_flex()
                        .w_full()
                        .items_center()
                        .gap_1()
                        .pt_2()
                        .child(toolbar_item(
                            "search",
                            panel::selected_on(panel::Section::Search, display_id, cx),
                            false,
                            Some("Search applications"),
                            icon_button("toolbar-search", IconName::Search, "Search applications")
                                .on_click(move |_, _, cx| {
                                    panel::toggle(panel::Section::Search, Some(display_id), cx)
                                }),
                            display_id,
                        ))
                        .child(toolbar_item(
                            "agenda",
                            panel::selected_on(panel::Section::Agenda, display_id, cx),
                            false,
                            Some("Agenda"),
                            icon_button("toolbar-agenda", IconName::Calendar, "Agenda").on_click(
                                move |_, _, cx| {
                                    panel::toggle(panel::Section::Agenda, Some(display_id), cx)
                                },
                            ),
                            display_id,
                        ))
                        .child(toolbar_item(
                            "tasks",
                            panel::selected_on(panel::Section::Tasks, display_id, cx),
                            false,
                            Some("Tasks"),
                            icon_button("toolbar-tasks", FoyerShellIcon::Tasks, "Tasks").on_click(
                                move |_, _, cx| {
                                    panel::toggle(panel::Section::Tasks, Some(display_id), cx)
                                },
                            ),
                            display_id,
                        ))
                        .child(toolbar_item(
                            "notes",
                            panel::selected_on(panel::Section::Notes, display_id, cx),
                            false,
                            Some("Notes"),
                            icon_button("toolbar-notes", FoyerShellIcon::Notes, "Notes").on_click(
                                move |_, _, cx| {
                                    panel::toggle(panel::Section::Notes, Some(display_id), cx)
                                },
                            ),
                            display_id,
                        ))
                        .child(toolbar_item(
                            "contacts",
                            panel::selected_on(panel::Section::Contacts, display_id, cx),
                            false,
                            Some("Contacts"),
                            icon_button("toolbar-contacts", FoyerShellIcon::Contacts, "Contacts")
                                .on_click(move |_, _, cx| {
                                    panel::toggle(panel::Section::Contacts, Some(display_id), cx)
                                }),
                            display_id,
                        ))
                        .child(toolbar_item(
                            "bookmarks",
                            panel::selected_on(panel::Section::Bookmarks, display_id, cx),
                            false,
                            Some("Bookmarks"),
                            icon_button(
                                "toolbar-bookmarks",
                                FoyerShellIcon::Bookmarks,
                                "Bookmarks",
                            )
                            .on_click(move |_, _, cx| {
                                panel::toggle(panel::Section::Bookmarks, Some(display_id), cx)
                            }),
                            display_id,
                        ))
                        .child(toolbar_item(
                            "activities",
                            panel::selected_on(panel::Section::Activities, display_id, cx),
                            false,
                            Some("Activities and agents"),
                            icon_button(
                                "toolbar-activities",
                                IconName::Bot,
                                "Activities and agents",
                            )
                            .on_click(move |_, _, cx| {
                                panel::toggle(panel::Section::Activities, Some(display_id), cx)
                            }),
                            display_id,
                        ))
                        .when(!connected, |column| {
                            column.child(toolbar_item(
                                "niri-status",
                                false,
                                false,
                                Some("Niri is reconnecting"),
                                icon_button(
                                    "toolbar-niri-status",
                                    IconName::TriangleAlert,
                                    "Niri is reconnecting",
                                )
                                .disabled(true),
                                display_id,
                            ))
                        }),
                )
                .child(ambient_band(
                    phase,
                    ambient_visibility,
                    ambient_rms,
                    ambient_waveform,
                ))
                .child(
                    v_flex()
                        .w_full()
                        .items_center()
                        .gap_1()
                        .pb_2()
                        .child(toolbar_item(
                            "notifications",
                            panel::selected_on(panel::Section::Notifications, display_id, cx),
                            has_unread_notifications,
                            Some("Notifications"),
                            icon_button("toolbar-notifications", IconName::Bell, "Notifications")
                                .on_click(move |_, _, cx| {
                                    panel::toggle(
                                        panel::Section::Notifications,
                                        Some(display_id),
                                        cx,
                                    )
                                }),
                            display_id,
                        ))
                        .child(toolbar_item(
                            "audio",
                            panel::selected_on(panel::Section::Audio, display_id, cx),
                            microphone_active,
                            Some("Audio"),
                            icon_button("toolbar-audio", FoyerShellIcon::Volume, "Audio").on_click(
                                move |_, _, cx| {
                                    panel::toggle(panel::Section::Audio, Some(display_id), cx)
                                },
                            ),
                            display_id,
                        ))
                        .child(toolbar_item(
                            "network",
                            panel::selected_on(panel::Section::Network, display_id, cx),
                            false,
                            Some("Network"),
                            icon_button("toolbar-network", FoyerShellIcon::Wifi, "Network")
                                .on_click(move |_, _, cx| {
                                    panel::toggle(panel::Section::Network, Some(display_id), cx)
                                }),
                            display_id,
                        ))
                        .child(toolbar_item(
                            "bluetooth",
                            panel::selected_on(panel::Section::Bluetooth, display_id, cx),
                            state.services.bluetooth.pairing.is_some(),
                            Some("Bluetooth"),
                            icon_button(
                                "toolbar-bluetooth",
                                FoyerShellIcon::Bluetooth,
                                "Bluetooth",
                            )
                            .on_click(move |_, _, cx| {
                                panel::toggle(panel::Section::Bluetooth, Some(display_id), cx)
                            }),
                            display_id,
                        ))
                        .child(toolbar_item(
                            "display",
                            panel::selected_on(panel::Section::Display, display_id, cx),
                            false,
                            Some("Display and brightness"),
                            icon_button(
                                "toolbar-display",
                                FoyerShellIcon::Display,
                                "Display and brightness",
                            )
                            .on_click(move |_, _, cx| {
                                panel::toggle(panel::Section::Display, Some(display_id), cx)
                            }),
                            display_id,
                        ))
                        .child(toolbar_item_with_bounds(
                            "tray",
                            tray_popover::is_open_on(display_id, cx),
                            tray_attention,
                            Some("Application status"),
                            icon_button("toolbar-tray", IconName::Ellipsis, "Application status")
                                .on_click(move |_, _, cx| {
                                    tray_popover::toggle(
                                        Some(display_id),
                                        Some(clicked_tray_bounds.get().bottom()),
                                        cx,
                                    )
                                }),
                            display_id,
                            tray_bounds,
                        ))
                        .child(div().my_1().h(px(1.0)).w_5().bg(rgb(tokens::BORDER)))
                        .child(toolbar_item(
                            "power",
                            panel::selected_on(panel::Section::Power, display_id, cx),
                            low_battery,
                            Some("Power and session"),
                            icon_button(
                                "toolbar-power",
                                FoyerShellIcon::Power,
                                "Power and session",
                            )
                            .on_click(move |_, _, cx| {
                                panel::toggle(panel::Section::Power, Some(display_id), cx)
                            }),
                            display_id,
                        ))
                        .child(if panel_open {
                            toolbar_item(
                                "close",
                                false,
                                false,
                                Some("Close panel"),
                                icon_button("toolbar-close", IconName::Close, "Close panel")
                                    .on_click(|_, _, cx| panel::close(cx)),
                                display_id,
                            )
                            .into_any_element()
                        } else {
                            clock_readout(hour, minute).into_any_element()
                        }),
                ),
        )
    }
}

pub fn reconcile(cx: &mut App) {
    if !FoyerShellState::global(cx).niri.connected {
        return;
    }

    let outputs = FoyerShellState::global(cx).niri.outputs.clone();
    let mapped_outputs = outputs
        .iter()
        .filter_map(|output| {
            FoyerShellState::display_id_for_output(&output.name, cx)
                .map(|display_id| (output.clone(), display_id))
        })
        .collect::<Vec<_>>();
    let existing = std::mem::take(&mut FoyerShellState::global_mut(cx).toolbars);
    let mut retained = Vec::new();
    let mut removed = Vec::new();
    let mut lost_displays = Vec::new();

    for toolbar in existing {
        let output = outputs
            .iter()
            .find(|output| output.name == toolbar.output.name);
        let mapped = mapped_outputs
            .iter()
            .find(|(output, _)| output.name == toolbar.output.name);
        match (output, mapped) {
            (Some(_), None) => retained.push(toolbar),
            (Some(output), Some((_, display_id)))
                if output == &toolbar.output && display_id == &toolbar.display_id =>
            {
                retained.push(toolbar);
            }
            (Some(_), Some((_, display_id))) => {
                if display_id != &toolbar.display_id {
                    lost_displays.push(toolbar.display_id);
                }
                removed.push(toolbar.handle);
            }
            (None, _) => {
                lost_displays.push(toolbar.display_id);
                removed.push(toolbar.handle);
            }
        }
    }
    FoyerShellState::global_mut(cx).toolbars = retained;

    for handle in removed {
        let _ = handle.update(cx, |_, window, _| window.remove_window());
    }
    lost_displays.sort_unstable_by_key(|display_id| u64::from(*display_id));
    lost_displays.dedup();
    for display_id in lost_displays {
        close_tooltip_on_display(display_id, cx);
        tray_popover::close_on_display(display_id, cx);
        panel::close_on_display(display_id, cx);
        notification::close_on_display(display_id, cx);
        osd::close_on_display(display_id, cx);
    }

    for (output, display_id) in mapped_outputs {
        if FoyerShellState::global(cx)
            .toolbars
            .iter()
            .any(|toolbar| toolbar.output.name == output.name)
        {
            continue;
        }
        open(output, display_id, cx);
    }
}

fn open(output: foyer_shell_niri::Output, display_id: DisplayId, cx: &mut App) {
    let options = WindowOptions {
        display_id: Some(display_id),
        titlebar: None,
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.0), px(0.0)),
            size: Size::new(px(tokens::TOOLBAR_WIDTH), px(output.height as f32)),
        })),
        app_id: Some("foyer-shell-toolbar".into()),
        window_background: WindowBackgroundAppearance::Transparent,
        kind: WindowKind::LayerShell(LayerShellOptions {
            namespace: "foyer-shell-toolbar".into(),
            layer: Layer::Top,
            anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::RIGHT,
            exclusive_zone: Some(px(tokens::TOOLBAR_WIDTH)),
            exclusive_edge: Some(Anchor::RIGHT),
            keyboard_interactivity: KeyboardInteractivity::None,
            ..Default::default()
        }),
        ..Default::default()
    };
    match cx.open_window(options, move |window, cx| {
        let toolbar = cx.new(|cx| Toolbar::new(display_id, cx));
        cx.new(|cx| Root::new(toolbar, window, cx).bordered(false))
    }) {
        Ok(handle) => FoyerShellState::global_mut(cx)
            .toolbars
            .push(ToolbarSurface {
                output,
                display_id,
                handle,
            }),
        Err(error) => tracing::error!(?display_id, %error, "failed to open Foyer Shell toolbar"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ambient_band_fades_fully_at_both_edges_of_the_control_gap() {
        assert_eq!(ambient_envelope(-1.0), 0.0);
        assert_eq!(ambient_envelope(1.0), 0.0);
        assert_eq!(ambient_envelope(0.0), 1.0);
        assert!(ambient_envelope(-0.75) < ambient_envelope(-0.25));
        assert!(ambient_envelope(0.75) < ambient_envelope(0.25));
    }

    #[test]
    fn ambient_motion_has_an_exact_loop_seam() {
        for normalized in [-0.8, -0.3, 0.0, 0.4, 0.9] {
            assert!((ambient_wave(normalized, 0.0) - ambient_wave(normalized, TAU)).abs() < 1e-5);
        }
    }

    #[test]
    fn ambient_energy_keeps_voice_levels_distinct() {
        let quiet = ambient_energy(0.25);
        let normal = ambient_energy(0.55);
        let loud = ambient_energy(0.85);
        assert!(quiet < normal && normal < loud);
        assert!(normal < 1.0);
    }

    #[test]
    fn ambient_visibility_builds_and_releases_gradually() {
        let built = smooth_toward(0.0, 1.0, 0.1, AMBIENT_ATTACK_SECONDS);
        assert!(built > 0.0 && built < 1.0);
        let released = smooth_toward(built, 0.0, 0.1, AMBIENT_RELEASE_SECONDS);
        assert!(released > 0.0 && released < built);
    }

    #[test]
    fn ambient_waveform_interpolates_service_samples() {
        let mut waveform = [0.0; foyer_shell_transcription::WAVEFORM_SAMPLES];
        waveform[15] = -1.0;
        waveform[16] = 1.0;
        assert!(ambient_waveform_sample(&waveform, 0.0).abs() < 1e-6);
    }

    #[test]
    fn ambient_waveform_smoothing_spreads_an_isolated_peak() {
        let mut waveform = [0.0; foyer_shell_transcription::WAVEFORM_SAMPLES];
        waveform[16] = 1.0;
        let sample_step = 2.0 / (foyer_shell_transcription::WAVEFORM_SAMPLES - 1) as f32;

        let center = ambient_smoothed_waveform_sample(&waveform, 1.0 / 31.0);
        let neighbor = ambient_smoothed_waveform_sample(&waveform, 1.0 / 31.0 - sample_step);

        assert!(center > neighbor);
        assert!(neighbor > 0.0);
    }
}
