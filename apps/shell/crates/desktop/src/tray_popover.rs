use foyer_shell_services::{TrayItem, TrayMenuItem};
use foyer_shell_ui::{Root, tokens};
use gpui::{
    AnyElement, App, Bounds, Context, DisplayId, FontWeight, KeyBinding, ObjectFit, Render, Window,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions, actions,
    div, img, layer_shell::*, point, prelude::*, px, rgb, size,
};
use gpui_component::{
    Disableable, Icon, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    scroll::ScrollableElement,
    v_flex,
};

use crate::{panel, state::FoyerShellState};

actions!(foyer_shell_tray, [Dismiss]);

const SURFACE_WIDTH: f32 = 280.0;
const MAX_SURFACE_HEIGHT: f32 = 560.0;
const GRID_WIDTH: f32 = 164.0;
const MENU_WIDTH: f32 = 264.0;
const CELL_SIZE: f32 = 44.0;
const GRID_GAP: f32 = 4.0;
const CARD_PADDING: f32 = 12.0;
const HEADER_HEIGHT: f32 = 44.0;
const MENU_ROW_HEIGHT: f32 = 34.0;
const EMPTY_HEIGHT: f32 = 72.0;

#[derive(Clone)]
pub struct TrayPopoverSurface {
    pub display_id: DisplayId,
    pub handle: WindowHandle<Root>,
}

struct TrayPopover {
    selected: Option<TrayItem>,
    submenu: Vec<i32>,
}

pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", Dismiss, Some("ShellTray"))]);
    cx.on_window_closed(|cx, window_id| {
        let closed = FoyerShellState::global(cx)
            .tray_popover_surface
            .as_ref()
            .is_some_and(|surface| surface.handle.window_id() == window_id);
        if closed {
            FoyerShellState::global_mut(cx).tray_popover_surface = None;
            FoyerShellState::global(cx)
                .service_controller
                .close_tray_menu();
            cx.refresh_windows();
        }
    })
    .detach();
}

pub fn is_open_on(display_id: DisplayId, cx: &App) -> bool {
    FoyerShellState::global(cx)
        .tray_popover_surface
        .as_ref()
        .is_some_and(|surface| surface.display_id == display_id)
}

pub fn toggle(
    requested_display: Option<DisplayId>,
    anchor_bottom: Option<gpui::Pixels>,
    cx: &mut App,
) {
    let display_id = requested_display
        .or_else(|| FoyerShellState::focused_display_id(cx))
        .or_else(|| cx.displays().first().map(|display| display.id()));
    let Some(display_id) = display_id else { return };

    if is_open_on(display_id, cx) {
        close(cx);
        return;
    }
    close(cx);
    panel::close(cx);
    open(display_id, anchor_bottom, cx);
}

pub fn close(cx: &mut App) {
    let Some(surface) = FoyerShellState::global_mut(cx).tray_popover_surface.take() else {
        return;
    };
    FoyerShellState::global(cx)
        .service_controller
        .close_tray_menu();
    let _ = surface
        .handle
        .update(cx, |_, window, _| window.remove_window());
    cx.refresh_windows();
}

pub fn close_on_display(display_id: DisplayId, cx: &mut App) {
    if is_open_on(display_id, cx) {
        close(cx);
    }
}

fn open(display_id: DisplayId, anchor_bottom: Option<gpui::Pixels>, cx: &mut App) {
    let display_size = FoyerShellState::display_size(display_id, cx)
        .unwrap_or_else(|| size(px(1920.0), px(1080.0)));
    let anchor_bottom = anchor_bottom
        .unwrap_or(display_size.height - px(106.0))
        .max(px(80.0))
        .min(display_size.height);
    let surface_height = anchor_bottom.min(px(MAX_SURFACE_HEIGHT));
    let bottom_margin = (display_size.height - anchor_bottom).max(px(0.0));
    let options = WindowOptions {
        display_id: Some(display_id),
        titlebar: None,
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.0), px(0.0)),
            size: size(px(SURFACE_WIDTH), surface_height),
        })),
        app_id: Some("foyer-shell-tray-popover".into()),
        window_background: WindowBackgroundAppearance::Transparent,
        kind: WindowKind::LayerShell(LayerShellOptions {
            namespace: "foyer-shell-tray-popover".into(),
            layer: Layer::Overlay,
            anchor: Anchor::BOTTOM | Anchor::RIGHT,
            margin: Some((px(0.0), px(tokens::TOOLBAR_WIDTH), bottom_margin, px(0.0))),
            keyboard_interactivity: KeyboardInteractivity::OnDemand,
            ..Default::default()
        }),
        focus: true,
        ..Default::default()
    };

    let handle = match cx.open_window(options, move |window, cx| {
        let view = cx.new(|_| TrayPopover {
            selected: None,
            submenu: Vec::new(),
        });
        cx.new(|cx| {
            Root::new(view, window, cx)
                .bordered(false)
                .bg(gpui::transparent_black())
        })
    }) {
        Ok(handle) => handle,
        Err(error) => {
            tracing::error!(%error, "failed to open tray popover");
            return;
        }
    };
    FoyerShellState::global_mut(cx).tray_popover_surface =
        Some(TrayPopoverSurface { display_id, handle });
    cx.refresh_windows();
}

impl TrayPopover {
    fn dismiss(&mut self, cx: &mut Context<Self>) {
        if self.submenu.pop().is_some() {
            cx.notify();
        } else if self.selected.take().is_some() {
            FoyerShellState::global(cx)
                .service_controller
                .close_tray_menu();
            cx.notify();
        } else {
            close(cx);
        }
    }

    fn grid(&self, cx: &mut Context<Self>) -> (AnyElement, f32, f32) {
        let items = FoyerShellState::global(cx)
            .services
            .tray
            .items
            .iter()
            .filter(|item| item.status != "Passive")
            .cloned()
            .collect::<Vec<_>>();
        let rows = items.len().div_ceil(3).max(1);
        let height = if items.is_empty() {
            EMPTY_HEIGHT
        } else {
            CARD_PADDING * 2.0 + rows as f32 * CELL_SIZE + rows.saturating_sub(1) as f32 * GRID_GAP
        };
        let controller = FoyerShellState::global(cx).service_controller.clone();
        let content = v_flex()
            .w(px(GRID_WIDTH))
            .p_3()
            .gap_1()
            .when(items.is_empty(), |column| {
                column
                    .h(px(EMPTY_HEIGHT - CARD_PADDING * 2.0))
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(tokens::MUTED))
                            .child("No tray applications"),
                    )
            })
            .children(items.chunks(3).enumerate().map(|(row, items)| {
                h_flex()
                    .gap_1()
                    .children(items.iter().enumerate().map(|(column, item)| {
                        let selected = item.clone();
                        let menu_controller = controller.clone();
                        Button::new(format!("tray-item-{row}-{column}"))
                            .ghost()
                            .compact()
                            .w(px(CELL_SIZE))
                            .h(px(CELL_SIZE))
                            .child(tray_icon(item))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.selected = Some(selected.clone());
                                this.submenu.clear();
                                menu_controller.open_tray_menu(selected.clone());
                                cx.notify();
                            }))
                    }))
            }));
        (content.into_any_element(), GRID_WIDTH, height)
    }

    fn menu(&self, selected: &TrayItem, cx: &mut Context<Self>) -> (AnyElement, f32, f32) {
        let snapshot = FoyerShellState::global(cx).services.tray.clone();
        let active = snapshot
            .active_menu
            .as_ref()
            .filter(|menu| menu.service == selected.service && menu.item_path == selected.path);
        let items = active
            .map(|menu| menu_level(&menu.items, &self.submenu))
            .unwrap_or_default();
        let fallback =
            selected.menu_path.is_none() || (active.is_none() && snapshot.last_error.is_some());
        let row_count = if fallback {
            2
        } else {
            items.iter().filter(|item| !item.separator).count().max(1)
        };
        let separators = if fallback {
            0
        } else {
            items.iter().filter(|item| item.separator).count()
        };
        let height = (HEADER_HEIGHT
            + CARD_PADDING * 2.0
            + row_count as f32 * MENU_ROW_HEIGHT
            + separators as f32 * 9.0)
            .min(MAX_SURFACE_HEIGHT);
        let header_title = self
            .submenu
            .last()
            .and_then(|id| {
                find_menu_item(
                    active.map(|menu| menu.items.as_slice()).unwrap_or_default(),
                    *id,
                )
            })
            .map(|item| item.label.clone())
            .filter(|label| !label.is_empty())
            .unwrap_or_else(|| selected.title.clone());

        let body = if fallback {
            self.fallback_menu(selected, cx)
        } else if active.is_none() {
            v_flex()
                .h(px(MENU_ROW_HEIGHT))
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(tokens::MUTED))
                        .child("Loading menu…"),
                )
                .into_any_element()
        } else if items.is_empty() {
            v_flex()
                .h(px(MENU_ROW_HEIGHT))
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(tokens::MUTED))
                        .child("No actions available"),
                )
                .into_any_element()
        } else {
            v_flex()
                .children(items.into_iter().enumerate().map(|(index, item)| {
                    self.menu_row(index, item, active.expect("checked above"), cx)
                }))
                .into_any_element()
        };

        let content = v_flex()
            .w(px(MENU_WIDTH))
            .max_h(px(MAX_SURFACE_HEIGHT))
            .child(
                h_flex()
                    .h(px(HEADER_HEIGHT))
                    .px_2()
                    .gap_1()
                    .border_b_1()
                    .border_color(rgb(tokens::BORDER))
                    .child(
                        Button::new("tray-menu-back")
                            .icon(IconName::ArrowLeft)
                            .ghost()
                            .compact()
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.submenu.pop().is_none() {
                                    this.selected = None;
                                    FoyerShellState::global(cx)
                                        .service_controller
                                        .close_tray_menu();
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(header_title),
                    ),
            )
            .child(
                v_flex()
                    .max_h(px(MAX_SURFACE_HEIGHT - HEADER_HEIGHT))
                    .overflow_y_scrollbar()
                    .p_3()
                    .child(body),
            );
        (content.into_any_element(), MENU_WIDTH, height)
    }

    fn menu_row(
        &self,
        index: usize,
        item: TrayMenuItem,
        menu: &foyer_shell_services::TrayMenu,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if item.separator {
            return div()
                .my_1()
                .h(px(1.0))
                .w_full()
                .bg(rgb(tokens::BORDER))
                .into_any_element();
        }
        let has_children = !item.children.is_empty();
        let enabled = item.enabled;
        let service = menu.service.clone();
        let menu_path = menu.menu_path.clone();
        let id = item.id;
        Button::new(format!("tray-menu-item-{index}-{id}"))
            .ghost()
            .small()
            .w_full()
            .disabled(!enabled)
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .child(div().w_4().when(item.toggle_state > 0, |slot| {
                        slot.child(Icon::new(IconName::Check).xsmall())
                    }))
                    .child(div().flex_1().text_left().child(if item.label.is_empty() {
                        "Untitled action".to_string()
                    } else {
                        item.label
                    }))
                    .when(has_children, |row| {
                        row.child(Icon::new(IconName::ChevronRight).xsmall())
                    }),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if has_children {
                    this.submenu.push(id);
                    cx.notify();
                } else {
                    FoyerShellState::global(cx)
                        .service_controller
                        .activate_tray_menu_item(service.clone(), menu_path.clone(), id);
                    close(cx);
                }
            }))
            .into_any_element()
    }

    fn fallback_menu(&self, selected: &TrayItem, cx: &mut Context<Self>) -> AnyElement {
        let open_controller = FoyerShellState::global(cx).service_controller.clone();
        let secondary_controller = open_controller.clone();
        let open_service = selected.service.clone();
        let open_path = selected.path.clone();
        let secondary_service = selected.service.clone();
        let secondary_path = selected.path.clone();
        v_flex()
            .child(
                Button::new("tray-fallback-open")
                    .label(if selected.item_is_menu {
                        "Open menu"
                    } else {
                        "Open"
                    })
                    .ghost()
                    .small()
                    .w_full()
                    .on_click(move |_, _, cx| {
                        open_controller.tray_activate(open_service.clone(), open_path.clone());
                        close(cx);
                    }),
            )
            .child(
                Button::new("tray-fallback-secondary")
                    .label("Secondary action")
                    .ghost()
                    .small()
                    .w_full()
                    .on_click(move |_, _, cx| {
                        secondary_controller.tray_secondary_activate(
                            secondary_service.clone(),
                            secondary_path.clone(),
                        );
                        close(cx);
                    }),
            )
            .into_any_element()
    }
}

impl Render for TrayPopover {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(selected) = self.selected.clone() {
            let still_present = FoyerShellState::global(cx)
                .services
                .tray
                .items
                .iter()
                .any(|item| item.service == selected.service && item.path == selected.path);
            if !still_present {
                self.selected = None;
                self.submenu.clear();
            }
        }
        let (content, width, height) = if let Some(selected) = self.selected.clone() {
            self.menu(&selected, cx)
        } else {
            self.grid(cx)
        };
        let surface_height = window.viewport_size().height;
        let input_region = [Bounds {
            origin: point(
                px(SURFACE_WIDTH - width),
                (surface_height - px(height)).max(px(0.0)),
            ),
            size: size(px(width), px(height).min(surface_height)),
        }];
        window.set_input_region(Some(&input_region));

        div()
            .id("foyer-shell-tray-popover")
            .key_context("ShellTray")
            .on_action(cx.listener(|this, _: &Dismiss, _, cx| this.dismiss(cx)))
            .size_full()
            .flex()
            .items_end()
            .justify_end()
            .text_color(rgb(tokens::FOREGROUND))
            .child(
                div()
                    .overflow_hidden()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(tokens::BORDER))
                    .bg(rgb(tokens::BACKGROUND))
                    .shadow_lg()
                    .child(content),
            )
    }
}

fn tray_icon(item: &TrayItem) -> AnyElement {
    if let Some(path) = item.icon_path.as_ref() {
        img(path.clone())
            .size_6()
            .object_fit(ObjectFit::Contain)
            .into_any_element()
    } else {
        Icon::new(IconName::Ellipsis).size_5().into_any_element()
    }
}

fn menu_level(items: &[TrayMenuItem], path: &[i32]) -> Vec<TrayMenuItem> {
    let mut level = items;
    for id in path {
        let Some(item) = level.iter().find(|item| item.id == *id) else {
            return Vec::new();
        };
        level = &item.children;
    }
    level.to_vec()
}

fn find_menu_item(items: &[TrayMenuItem], id: i32) -> Option<&TrayMenuItem> {
    for item in items {
        if item.id == id {
            return Some(item);
        }
        if let Some(found) = find_menu_item(&item.children, id) {
            return Some(found);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_nested_menu_path() {
        let child = TrayMenuItem {
            id: 2,
            label: "Child".into(),
            enabled: true,
            separator: false,
            toggle_type: None,
            toggle_state: -1,
            icon_name: None,
            children: Vec::new(),
        };
        let root = TrayMenuItem {
            id: 1,
            label: "Root".into(),
            enabled: true,
            separator: false,
            toggle_type: None,
            toggle_state: -1,
            icon_name: None,
            children: vec![child.clone()],
        };
        assert_eq!(menu_level(&[root], &[1]), vec![child]);
    }
}
