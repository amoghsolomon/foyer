use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use foyer_shell_services::notifications::{CloseReason, Event, Notification, Urgency};
use foyer_shell_storage::NotificationUrgency;
use foyer_shell_ui::{Root, tokens};
use gpui::{
    Animation, AnimationExt, App, Bounds, Context, DisplayId, Entity, FontWeight, Render, Window,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions, div,
    ease_out_quint, layer_shell::*, point, prelude::*, px, rgb, size,
};
use gpui_component::{
    IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};

use crate::state::FoyerShellState;

const WIDTH: f32 = 380.0;
const HEIGHT: f32 = 580.0;
const BASE_CARD_HEIGHT: f32 = 112.0;
const ACTION_CARD_HEIGHT: f32 = 150.0;
const MANY_ACTIONS_CARD_HEIGHT: f32 = 188.0;
const GAP: f32 = 8.0;
const MAX_VISIBLE: usize = 3;
const LOCAL_NOTIFICATION_START: u32 = 0x8000_0000;
static NEXT_LOCAL_NOTIFICATION_ID: AtomicU32 = AtomicU32::new(LOCAL_NOTIFICATION_START);

#[derive(Clone)]
pub struct NotificationSurface {
    view: Entity<NotificationCenter>,
    handle: WindowHandle<Root>,
    display_id: DisplayId,
}

#[derive(Clone)]
struct Entry {
    notification: Notification,
    generation: u64,
}

#[derive(Default)]
struct NotificationCenter {
    entries: Vec<Entry>,
    next_generation: u64,
}

struct PushOutcome {
    generation: u64,
    evicted: Option<u32>,
}

pub fn handle(event: Event, cx: &mut App) {
    match event {
        Event::Status(status) => {
            let previous = FoyerShellState::global(cx)
                .notification_availability
                .clone();
            if previous != status {
                match &status {
                    foyer_shell_services::Availability::Available => {
                        tracing::info!("notification daemon acquired the session-bus name")
                    }
                    foyer_shell_services::Availability::Unavailable(error) => {
                        tracing::warn!(%error, "notification daemon unavailable")
                    }
                    foyer_shell_services::Availability::Loading => {}
                }
                FoyerShellState::global_mut(cx).notification_availability = status;
            }
        }
        Event::Show(notification) => show(notification, cx),
        Event::Close(id) => {
            FoyerShellState::global(cx)
                .storage_controller
                .mark_notification_read(id);
            dismiss(id, None, CloseReason::ClosedByCall, cx);
        }
    }
}

pub fn show_local(summary: &str, body: &str, urgency: Urgency, cx: &mut App) {
    let id = NEXT_LOCAL_NOTIFICATION_ID.fetch_add(1, Ordering::Relaxed);
    show(
        Notification {
            id,
            app_name: "Foyer Shell Transcription".into(),
            summary: foyer_shell_services::notifications::plain_text(summary, 160),
            body: foyer_shell_services::notifications::plain_text(body, 1_024),
            urgency,
            actions: Vec::new(),
            desktop_entry: None,
            resident: false,
            timeout: Some(Duration::from_secs(6)),
        },
        cx,
    );
}

pub fn close_on_display(display_id: DisplayId, cx: &mut App) {
    let Some(surface) = FoyerShellState::global(cx).notification_surface.clone() else {
        return;
    };
    if surface.display_id != display_id {
        return;
    }

    let ids = surface.view.update(cx, |center, cx| {
        let ids = center
            .entries
            .drain(..)
            .map(|entry| entry.notification.id)
            .collect::<Vec<_>>();
        cx.notify();
        ids
    });
    let controller = FoyerShellState::global(cx).notification_controller.clone();
    for id in ids {
        controller.closed(id, CloseReason::Undefined);
    }
    let _ = surface
        .handle
        .update(cx, |_, window, _| window.remove_window());
    FoyerShellState::global_mut(cx).notification_surface = None;
    cx.refresh_windows();
}

fn show(notification: Notification, cx: &mut App) {
    FoyerShellState::global(cx)
        .storage_controller
        .upsert_notification(
            notification.id,
            notification.app_name.clone(),
            notification.summary.clone(),
            notification.body.clone(),
            storage_urgency(notification.urgency),
        );
    if FoyerShellState::global(cx).storage.do_not_disturb
        && notification.urgency != Urgency::Critical
    {
        FoyerShellState::global(cx)
            .notification_controller
            .closed(notification.id, CloseReason::Expired);
        return;
    }
    let Some(surface) = ensure_surface(cx) else {
        FoyerShellState::global(cx)
            .notification_controller
            .closed(notification.id, CloseReason::Undefined);
        return;
    };
    let outcome = surface
        .view
        .update(cx, |center, cx| center.push(notification.clone(), cx));

    if let Some(id) = outcome.evicted {
        FoyerShellState::global(cx)
            .notification_controller
            .closed(id, CloseReason::Expired);
    }
    if let Some(timeout) = notification.timeout {
        schedule_expiration(notification.id, outcome.generation, timeout, cx);
    }
    cx.refresh_windows();
}

fn ensure_surface(cx: &mut App) -> Option<NotificationSurface> {
    if let Some(surface) = FoyerShellState::global(cx).notification_surface.clone() {
        return Some(surface);
    }

    let display_id = FoyerShellState::focused_display_id(cx)
        .or_else(|| cx.displays().first().map(|display| display.id()))?;
    let view = cx.new(|_| NotificationCenter::default());
    open_surface(view, display_id, cx)
}

fn open_surface(
    view: Entity<NotificationCenter>,
    display_id: DisplayId,
    cx: &mut App,
) -> Option<NotificationSurface> {
    let right_margin = crate::panel::ambient_right_margin(display_id, cx);
    let options = WindowOptions {
        display_id: Some(display_id),
        titlebar: None,
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.0), px(0.0)),
            size: size(px(WIDTH), px(HEIGHT)),
        })),
        app_id: Some("foyer-shell-notifications".into()),
        window_background: WindowBackgroundAppearance::Transparent,
        kind: WindowKind::LayerShell(LayerShellOptions {
            namespace: "foyer-shell-notifications".into(),
            layer: Layer::Overlay,
            anchor: Anchor::BOTTOM | Anchor::RIGHT,
            margin: Some((px(0.0), px(right_margin), px(12.0), px(0.0))),
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
            tracing::error!(%error, "failed to open notification surface");
            return None;
        }
    };
    let surface = NotificationSurface {
        view,
        handle,
        display_id,
    };
    FoyerShellState::global_mut(cx).notification_surface = Some(surface.clone());
    Some(surface)
}

pub fn reposition(display_id: DisplayId, cx: &mut App) {
    let should_reposition = FoyerShellState::global(cx)
        .notification_surface
        .as_ref()
        .is_some_and(|surface| surface.display_id == display_id);
    if !should_reposition {
        return;
    }
    let Some(surface) = FoyerShellState::global_mut(cx).notification_surface.take() else {
        return;
    };
    let _ = surface
        .handle
        .update(cx, |_, window, _| window.remove_window());
    FoyerShellState::global_mut(cx).notification_surface =
        open_surface(surface.view, display_id, cx);
}

fn schedule_expiration(id: u32, generation: u64, timeout: Duration, cx: &mut App) {
    cx.spawn(async move |cx| {
        cx.background_executor().timer(timeout).await;
        cx.update(|cx| dismiss(id, Some(generation), CloseReason::Expired, cx));
    })
    .detach();
}

fn dismiss(id: u32, generation: Option<u64>, reason: CloseReason, cx: &mut App) {
    let Some(surface) = FoyerShellState::global(cx).notification_surface.clone() else {
        return;
    };
    let (removed, empty) = surface
        .view
        .update(cx, |center, cx| center.remove(id, generation, cx));
    if !removed {
        return;
    }

    if reason == CloseReason::DismissedByUser {
        FoyerShellState::global(cx)
            .storage_controller
            .mark_notification_read(id);
    }

    FoyerShellState::global(cx)
        .notification_controller
        .closed(id, reason);
    if empty {
        let _ = surface
            .handle
            .update(cx, |_, window, _| window.remove_window());
        FoyerShellState::global_mut(cx).notification_surface = None;
    }
    cx.refresh_windows();
}

impl NotificationCenter {
    fn push(&mut self, notification: Notification, cx: &mut Context<Self>) -> PushOutcome {
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        let generation = self.next_generation;
        let mut evicted = None;
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.notification.id == notification.id)
        {
            entry.notification = notification;
            entry.generation = generation;
        } else {
            if self.entries.len() == MAX_VISIBLE {
                evicted = Some(self.entries.remove(0).notification.id);
            }
            self.entries.push(Entry {
                notification,
                generation,
            });
        }
        cx.notify();
        PushOutcome {
            generation,
            evicted,
        }
    }

    fn remove(&mut self, id: u32, generation: Option<u64>, cx: &mut Context<Self>) -> (bool, bool) {
        let Some(index) = self.entries.iter().position(|entry| {
            entry.notification.id == id
                && generation.is_none_or(|generation| entry.generation == generation)
        }) else {
            return (false, self.entries.is_empty());
        };
        self.entries.remove(index);
        cx.notify();
        (true, self.entries.is_empty())
    }
}

impl Render for NotificationCenter {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.entries.len();
        let input_height = self
            .entries
            .iter()
            .map(|entry| notification_card_height(&entry.notification))
            .sum::<f32>()
            + count.saturating_sub(1) as f32 * GAP;
        let input_region = [Bounds {
            origin: point(px(0.0), px(HEIGHT - input_height)),
            size: size(px(WIDTH), px(input_height)),
        }];
        window.set_input_region(Some(&input_region));

        v_flex()
            .size_full()
            .justify_end()
            .gap_2()
            .children(self.entries.iter().cloned().map(|entry| {
                let notification = entry.notification;
                let id = notification.id;
                let app_name = if notification.app_name.is_empty() {
                    "Application".to_string()
                } else {
                    notification.app_name.clone()
                };
                let body = display_body(&notification.body);
                let actions = notification.actions.clone();
                let has_default_action = actions.iter().any(|action| action.key == "default");
                let fallback_application = (!has_default_action)
                    .then_some(notification.desktop_entry.as_deref())
                    .flatten()
                    .and_then(|desktop_id| {
                        crate::applications::find_by_desktop_id(
                            &FoyerShellState::global(cx).applications,
                            desktop_id,
                        )
                    });
                let resident = notification.resident;
                let card_height = notification_card_height(&notification);
                let border = match notification.urgency {
                    Urgency::Critical => tokens::FOREGROUND,
                    Urgency::Low | Urgency::Normal => tokens::BORDER,
                };
                v_flex()
                    .id(("notification-card", id as u64))
                    .h(px(card_height))
                    .p_3()
                    .gap_1()
                    .overflow_hidden()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(border))
                    .bg(rgb(tokens::SURFACE))
                    .text_color(rgb(tokens::FOREGROUND))
                    .child(
                        h_flex()
                            .h_5()
                            .justify_between()
                            .child(
                                div()
                                    .flex_1()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(tokens::MUTED))
                                    .child(app_name),
                            )
                            .child(
                                Button::new(("dismiss-notification", id as u64))
                                    .icon(IconName::Close)
                                    .tooltip("Dismiss notification")
                                    .accessibility_id("Dismiss notification")
                                    .ghost()
                                    .xsmall()
                                    .tab_stop(false)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        let (removed, empty) = this.remove(id, None, cx);
                                        if removed {
                                            FoyerShellState::global(cx)
                                                .notification_controller
                                                .closed(id, CloseReason::DismissedByUser);
                                            FoyerShellState::global(cx)
                                                .storage_controller
                                                .mark_notification_read(id);
                                        }
                                        if empty {
                                            FoyerShellState::global_mut(cx).notification_surface =
                                                None;
                                            window.remove_window();
                                            cx.refresh_windows();
                                        }
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(notification.summary),
                    )
                    .when(!body.is_empty(), |card| {
                        card.child(
                            div()
                                .flex_1()
                                .overflow_hidden()
                                .text_xs()
                                .text_color(rgb(tokens::MUTED))
                                .child(body),
                        )
                    })
                    .when(!actions.is_empty(), |card| {
                        card.child(h_flex().flex_wrap().mt_2().gap_2().children(
                            actions.into_iter().map(|action| {
                                let key = action.key.clone();
                                let label = if action.label.is_empty() {
                                    if action.key == "default" {
                                        "Open".to_string()
                                    } else {
                                        action.key.clone()
                                    }
                                } else {
                                    action.label
                                };
                                Button::new(format!("notification-action-{id}-{key}"))
                                    .label(label)
                                    .outline()
                                    .small()
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        FoyerShellState::global(cx)
                                            .notification_controller
                                            .invoke(id, key.clone());
                                        FoyerShellState::global(cx)
                                            .storage_controller
                                            .mark_notification_read(id);
                                        if !resident {
                                            let (_, empty) = this.remove(id, None, cx);
                                            FoyerShellState::global(cx)
                                                .notification_controller
                                                .closed(id, CloseReason::DismissedByUser);
                                            if empty {
                                                FoyerShellState::global_mut(cx)
                                                    .notification_surface = None;
                                                window.remove_window();
                                            }
                                            cx.refresh_windows();
                                        }
                                    }))
                            }),
                        ))
                    })
                    .when_some(fallback_application, |card, application| {
                        card.child(
                            Button::new(("notification-open-application", id as u64))
                                .label("Open application")
                                .outline()
                                .small()
                                .mt_2()
                                .w_full()
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    crate::applications::launch(application.clone());
                                    FoyerShellState::global(cx)
                                        .storage_controller
                                        .mark_notification_read(id);
                                    let (_, empty) = this.remove(id, None, cx);
                                    FoyerShellState::global(cx)
                                        .notification_controller
                                        .closed(id, CloseReason::DismissedByUser);
                                    if empty {
                                        FoyerShellState::global_mut(cx).notification_surface = None;
                                        window.remove_window();
                                    }
                                    cx.refresh_windows();
                                })),
                        )
                    })
                    .with_animation(
                        format!("notification-in-{id}-{}", entry.generation),
                        Animation::new(Duration::from_millis(160)).with_easing(ease_out_quint()),
                        |card, delta| {
                            card.left(px((1.0 - delta) * 18.0))
                                .opacity(0.55 + delta * 0.45)
                        },
                    )
            }))
    }
}

fn display_body(body: &str) -> String {
    let mut text = body.chars().take(180).collect::<String>();
    if body.chars().count() > 180 {
        text.push('…');
    }
    text
}

fn notification_card_height(notification: &Notification) -> f32 {
    let action_count = notification.actions.len()
        + usize::from(
            notification.desktop_entry.is_some()
                && !notification
                    .actions
                    .iter()
                    .any(|action| action.key == "default"),
        );
    match action_count {
        0 => BASE_CARD_HEIGHT,
        1..=3 => ACTION_CARD_HEIGHT,
        _ => MANY_ACTIONS_CARD_HEIGHT,
    }
}

fn storage_urgency(urgency: Urgency) -> NotificationUrgency {
    match urgency {
        Urgency::Low => NotificationUrgency::Low,
        Urgency::Normal => NotificationUrgency::Normal,
        Urgency::Critical => NotificationUrgency::Critical,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_notification_body_for_the_fixed_card() {
        assert_eq!(display_body("short"), "short");
        let long = "a".repeat(200);
        assert_eq!(display_body(&long).chars().count(), 181);
        assert!(display_body(&long).ends_with('…'));
    }

    #[test]
    fn reserves_more_space_for_complete_action_sets() {
        let mut notification = Notification {
            id: 1,
            app_name: "App".into(),
            summary: "Summary".into(),
            body: "Body".into(),
            urgency: Urgency::Normal,
            actions: Vec::new(),
            desktop_entry: None,
            resident: false,
            timeout: None,
        };
        assert_eq!(notification_card_height(&notification), BASE_CARD_HEIGHT);
        notification.actions = (0..6)
            .map(
                |index| foyer_shell_services::notifications::NotificationAction {
                    key: format!("action-{index}"),
                    label: format!("Action {index}"),
                },
            )
            .collect();
        assert_eq!(
            notification_card_height(&notification),
            MANY_ACTIONS_CARD_HEIGHT
        );
    }
}
