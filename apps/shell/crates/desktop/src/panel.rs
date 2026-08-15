use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use chrono::{DateTime, Local, Utc};
use foyer_shell_ui::{FoyerShellIcon, Root, tokens};
use gpui::{
    AnyElement, App, Bounds, Context, DisplayId, Entity, FontWeight, KeyBinding, ObjectFit, Render,
    SharedString, Subscription, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle,
    WindowKind, WindowOptions, actions, div, img, layer_shell::*, point, prelude::*, px, rgb, size,
};
use gpui_component::{
    ActiveTheme, Disableable, Icon, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    scroll::ScrollableElement,
    slider::{Slider, SliderEvent, SliderState},
    switch::Switch,
    v_flex,
};

mod bookmarks;
mod chrome;
mod contacts;
mod hosted_agenda;
mod hosted_tasks;

use crate::{
    applications::{self, ApplicationEntry},
    notification, osd,
    state::{FoyerShellState, PanelSurface},
};

actions!(
    foyer_shell_panel,
    [Close, SelectNext, SelectPrevious, Launch]
);

const MAX_RESULTS: usize = 9;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Search,
    Agenda,
    Tasks,
    Notes,
    Contacts,
    Bookmarks,
    Activities,
    Notifications,
    Audio,
    Network,
    Bluetooth,
    Display,
    Power,
}

impl Section {
    fn title(self) -> &'static str {
        match self {
            Self::Search => "Search",
            Self::Agenda => "Agenda",
            Self::Tasks => "Tasks",
            Self::Notes => "Notes",
            Self::Contacts => "Contacts",
            Self::Bookmarks => "Bookmarks",
            Self::Activities => "Activities",
            Self::Notifications => "Notifications",
            Self::Audio => "Audio",
            Self::Network => "Network",
            Self::Bluetooth => "Bluetooth",
            Self::Display => "Display",
            Self::Power => "Power and session",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Search => "Applications on this system",
            Self::Agenda => "Calendars and upcoming events",
            Self::Tasks => "Open tasks and due dates",
            Self::Notes => "Folders and Markdown notes",
            Self::Contacts => "Address books and people",
            Self::Bookmarks => "Folders and saved links",
            Self::Activities => "Foyer Shell work and agent activity",
            Self::Notifications => "Recent application updates",
            Self::Audio => "Devices, microphone, and application volume",
            Self::Network => "Wi-Fi networks and saved connections",
            Self::Bluetooth => "Nearby and remembered devices",
            Self::Display => "Output and backlight controls",
            Self::Power => "Local actions for this Niri session",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::Search => Icon::new(IconName::Search),
            Self::Agenda => Icon::new(IconName::Calendar),
            Self::Tasks => Icon::new(FoyerShellIcon::Tasks),
            Self::Notes => Icon::new(FoyerShellIcon::Notes),
            Self::Contacts => Icon::new(FoyerShellIcon::Contacts),
            Self::Bookmarks => Icon::new(FoyerShellIcon::Bookmarks),
            Self::Activities => Icon::new(IconName::Bot),
            Self::Notifications => Icon::new(IconName::Bell),
            Self::Audio => Icon::new(FoyerShellIcon::Volume),
            Self::Network => Icon::new(FoyerShellIcon::Wifi),
            Self::Bluetooth => Icon::new(FoyerShellIcon::Bluetooth),
            Self::Display => Icon::new(FoyerShellIcon::Display),
            Self::Power => Icon::new(FoyerShellIcon::Power),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionAction {
    Suspend,
    LogOut,
    Restart,
    PowerOff,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AgendaMode {
    Browse,
    EditEvent,
    RenameCalendar,
    ConfirmDeleteCalendar,
    ConfirmDeleteEvent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TasksMode {
    Browse,
    EditTask,
    RenameList,
    MoveTask,
    ConfirmDeleteList,
    ConfirmDeleteTask,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ContactsMode {
    Browse,
    EditContact,
    RenameBook,
    MoveContact,
    ConfirmDeleteBook,
    ConfirmDeleteContact,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BookmarksMode {
    Browse,
    EditBookmark,
    RenameFolder,
    MoveFolder,
    MoveBookmark,
    ConfirmDeleteFolder,
    ConfirmDeleteBookmark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NotesMode {
    Browse,
    EditNote,
    RenameFolder,
    MoveFolder,
    MoveNote,
    ConfirmDeleteFolder,
    ConfirmDeleteNote,
}

impl SessionAction {
    fn label(self) -> &'static str {
        match self {
            Self::Suspend => "Suspend",
            Self::LogOut => "Log out",
            Self::Restart => "Restart",
            Self::PowerOff => "Power off",
        }
    }

    fn consequence(self) -> &'static str {
        match self {
            Self::Suspend => "The machine will sleep until you wake it.",
            Self::LogOut => "Niri and every application in this session will close.",
            Self::Restart => "All applications will close and the machine will restart.",
            Self::PowerOff => "All applications will close and the machine will shut down.",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::Suspend => Icon::new(IconName::Moon),
            Self::LogOut => Icon::new(FoyerShellIcon::LogOut),
            Self::Restart => Icon::new(FoyerShellIcon::Restart),
            Self::PowerOff => Icon::new(FoyerShellIcon::Power),
        }
    }
}

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", Close, Some("ShellPanel")),
        KeyBinding::new("down", SelectNext, Some("ShellPanel")),
        KeyBinding::new("up", SelectPrevious, Some("ShellPanel")),
        KeyBinding::new("enter", Launch, Some("ShellPanel")),
    ]);
    cx.on_window_closed(finish_close).detach();
}

pub fn selected_on(section: Section, display_id: DisplayId, cx: &App) -> bool {
    FoyerShellState::global(cx)
        .panel_surface
        .as_ref()
        .is_some_and(|panel| {
            !panel.closing && panel.section == section && panel.display_id == display_id
        })
}

pub fn is_open_on(display_id: DisplayId, cx: &App) -> bool {
    FoyerShellState::global(cx)
        .panel_surface
        .as_ref()
        .is_some_and(|panel| !panel.closing && panel.display_id == display_id)
}

pub fn ambient_right_margin(display_id: DisplayId, cx: &App) -> f32 {
    12.0 + if is_open_on(display_id, cx) {
        tokens::PANEL_WIDTH
    } else {
        0.0
    }
}

pub fn toggle(section: Section, requested_display: Option<DisplayId>, cx: &mut App) {
    show(section, requested_display, cx);
}

fn show(section: Section, requested_display: Option<DisplayId>, cx: &mut App) {
    crate::tray_popover::close(cx);
    let display_id = requested_display
        .or_else(|| FoyerShellState::focused_display_id(cx))
        .or_else(|| cx.displays().first().map(|display| display.id()));
    let Some(display_id) = display_id else {
        return;
    };

    let existing = FoyerShellState::global(cx)
        .panel_surface
        .as_ref()
        .map(|surface| {
            (
                surface.section,
                surface.display_id,
                surface.view.clone(),
                surface.handle,
                surface.closing,
            )
        });
    if let Some((current, current_display, view, handle, closing)) = existing {
        if closing {
            FoyerShellState::global_mut(cx).pending_panel = Some((section, display_id));
            return;
        }
        if current_display == display_id && current == section {
            close(cx);
            return;
        }
        if current_display == display_id {
            view.update(cx, |panel, cx| panel.set_section(section, cx));
            if let Some(surface) = FoyerShellState::global_mut(cx).panel_surface.as_mut() {
                surface.section = section;
            }
            focus_section(section, view, handle, cx);
            cx.refresh_windows();
            return;
        }
        replace_after_close(section, display_id, cx);
        return;
    }

    open(section, display_id, cx);
}

pub fn close(cx: &mut App) {
    FoyerShellState::global_mut(cx).pending_panel = None;
    begin_close(cx);
}

fn replace_after_close(section: Section, display_id: DisplayId, cx: &mut App) {
    FoyerShellState::global_mut(cx).pending_panel = Some((section, display_id));
    begin_close(cx);
}

fn begin_close(cx: &mut App) {
    let (handle, pending_without_surface) = {
        let state = FoyerShellState::global_mut(cx);
        match state.panel_surface.as_mut() {
            Some(surface) if !surface.closing => {
                surface.closing = true;
                (Some(surface.handle), None)
            }
            Some(_) => (None, None),
            None => (None, state.pending_panel.take()),
        }
    };
    if let Some((section, display_id)) = pending_without_surface {
        open(section, display_id, cx);
        return;
    }
    let Some(handle) = handle else {
        return;
    };
    let window_id = handle.window_id();
    if handle
        .update(cx, |_, window, _| window.remove_window())
        .is_err()
    {
        finish_close(cx, window_id);
    }
    cx.refresh_windows();
}

fn finish_close(cx: &mut App, window_id: gpui::WindowId) {
    let closed = FoyerShellState::global(cx)
        .panel_surface
        .as_ref()
        .filter(|surface| surface.handle.window_id() == window_id)
        .map(|surface| surface.display_id);
    let Some(display_id) = closed else {
        return;
    };
    let pending = {
        let state = FoyerShellState::global_mut(cx);
        state.panel_surface = None;
        state.pending_panel.take()
    };
    notification::reposition(display_id, cx);
    osd::reposition(display_id, cx);
    cx.refresh_windows();
    if let Some((section, display_id)) = pending {
        open(section, display_id, cx);
    }
}

pub fn close_on_display(display_id: DisplayId, cx: &mut App) {
    if FoyerShellState::global(cx)
        .panel_surface
        .as_ref()
        .is_some_and(|panel| panel.display_id == display_id)
    {
        close(cx);
    }
}

fn open(section: Section, display_id: DisplayId, cx: &mut App) {
    if FoyerShellState::global(cx).panel_surface.is_some() {
        tracing::error!(?section, ?display_id, "refused to open a second panel");
        FoyerShellState::global_mut(cx).pending_panel = Some((section, display_id));
        return;
    }
    let display_size = FoyerShellState::display_size(display_id, cx)
        .unwrap_or_else(|| size(px(1920.0), px(1080.0)));
    let panel_slot = Rc::new(RefCell::new(None));
    let open_slot = panel_slot.clone();
    let options = WindowOptions {
        display_id: Some(display_id),
        titlebar: None,
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.0), px(0.0)),
            size: size(px(tokens::PANEL_WIDTH), display_size.height),
        })),
        app_id: Some("foyer-shell-panel".into()),
        window_background: WindowBackgroundAppearance::Transparent,
        kind: WindowKind::LayerShell(LayerShellOptions {
            namespace: "foyer-shell-panel".into(),
            layer: Layer::Overlay,
            anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::RIGHT,
            margin: Some((px(0.0), px(0.0), px(0.0), px(0.0))),
            keyboard_interactivity: KeyboardInteractivity::OnDemand,
            ..Default::default()
        }),
        focus: true,
        ..Default::default()
    };

    let handle = match cx.open_window(options, move |window, cx| {
        let panel = cx.new(|cx| Panel::new(section, display_id, window, cx));
        *open_slot.borrow_mut() = Some(panel.clone());
        let input = match section {
            Section::Activities => panel.read(cx).activity_input.clone(),
            _ => panel.read(cx).input.clone(),
        };
        window.defer(cx, move |window, cx| {
            if matches!(section, Section::Search | Section::Activities) {
                input.update(cx, |input, cx| input.focus(window, cx));
            }
        });
        cx.new(|cx| {
            Root::new(panel, window, cx)
                .bordered(false)
                .bg(gpui::transparent_black())
        })
    }) {
        Ok(handle) => handle,
        Err(error) => {
            tracing::error!(%error, "failed to open Foyer Shell panel");
            return;
        }
    };
    let Some(view) = panel_slot.borrow_mut().take() else {
        tracing::error!("panel opened without creating its view");
        let _ = handle.update(cx, |_, window, _| window.remove_window());
        return;
    };
    FoyerShellState::global_mut(cx).panel_surface = Some(PanelSurface {
        section,
        display_id,
        view,
        handle,
        closing: false,
    });
    notification::reposition(display_id, cx);
    osd::reposition(display_id, cx);
    cx.refresh_windows();
}

fn focus_section(section: Section, view: Entity<Panel>, handle: WindowHandle<Root>, cx: &mut App) {
    let input = match section {
        Section::Activities => view.read(cx).activity_input.clone(),
        _ => view.read(cx).input.clone(),
    };
    let _ = handle.update(cx, |_, window, cx| {
        window.activate_window();
        if matches!(section, Section::Search | Section::Activities) {
            input.update(cx, |input, cx| input.focus(window, cx));
        }
    });
}

fn defer_close(_: &mut Window, cx: &mut App) {
    cx.defer(close);
}

pub struct Panel {
    section: Section,
    display_id: DisplayId,
    applications: Arc<Vec<ApplicationEntry>>,
    input: Entity<InputState>,
    activity_input: Entity<InputState>,
    matches: Vec<usize>,
    selected: usize,
    volume_slider: Entity<SliderState>,
    input_volume_slider: Entity<SliderState>,
    brightness_slider: Entity<SliderState>,
    wifi_password: Entity<InputState>,
    bluetooth_code: Entity<InputState>,
    selected_wifi: Option<String>,
    pending_volume: Option<(f32, Instant)>,
    pending_input_volume: Option<(f32, Instant)>,
    pending_brightness: Option<(u8, Instant)>,
    pending_session_action: Option<SessionAction>,
    notes_folder_id: Option<String>,
    notes_note_id: Option<String>,
    notes_mode: NotesMode,
    notes_preview: bool,
    notes_title: Entity<InputState>,
    notes_body: Entity<TextareaState>,
    agenda_calendar_id: Option<String>,
    agenda_event_id: Option<String>,
    agenda_mode: AgendaMode,
    agenda_title: Entity<InputState>,
    agenda_start: Entity<InputState>,
    agenda_location: Entity<InputState>,
    agenda_description: Entity<TextareaState>,
    tasks_list_id: Option<String>,
    tasks_task_id: Option<String>,
    tasks_mode: TasksMode,
    tasks_title: Entity<InputState>,
    tasks_body: Entity<TextareaState>,
    tasks_due: Entity<InputState>,
    contacts_book_id: Option<String>,
    contacts_contact_id: Option<String>,
    contacts_mode: ContactsMode,
    contacts_query: Entity<InputState>,
    contacts_display_name: Entity<InputState>,
    contacts_email: Entity<InputState>,
    contacts_phone: Entity<InputState>,
    contacts_org: Entity<InputState>,
    contacts_job_title: Entity<InputState>,
    bookmarks_folder_id: Option<String>,
    bookmarks_bookmark_id: Option<String>,
    bookmarks_mode: BookmarksMode,
    bookmarks_filter: foyer_shell_bookmarks::Filter,
    bookmarks_query: Entity<InputState>,
    bookmarks_title: Entity<InputState>,
    bookmarks_url: Entity<InputState>,
    bookmarks_tags: Entity<InputState>,
    bookmarks_description: Entity<TextareaState>,
    _subscriptions: Vec<Subscription>,
}

impl Panel {
    fn new(
        section: Section,
        display_id: DisplayId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (applications, controller, initial_volume, initial_input_volume, initial_brightness) = {
            let state = FoyerShellState::global(cx);
            (
                state.applications.clone(),
                state.service_controller.clone(),
                state.services.audio.volume * 100.0,
                state.services.audio.input_volume * 100.0,
                f32::from(state.services.brightness.percent.max(1)),
            )
        };
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search applications…"));
        let activity_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Ask Foyer Shell to explain something…")
        });
        let notes_title = cx.new(|cx| InputState::new(window, cx).placeholder("Note title"));
        let notes_body = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(6, 20)
                .placeholder("Write Markdown…")
        });
        let agenda_title = cx.new(|cx| InputState::new(window, cx).placeholder("Event title"));
        let agenda_start = cx.new(|cx| InputState::new(window, cx).placeholder("YYYY-MM-DD"));
        let agenda_location = cx.new(|cx| InputState::new(window, cx).placeholder("Location"));
        let agenda_description = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(3, 8)
                .placeholder("Description")
        });
        let tasks_title = cx.new(|cx| InputState::new(window, cx).placeholder("Task title"));
        let tasks_body = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(3, 8)
                .placeholder("Markdown description")
        });
        let tasks_due = cx.new(|cx| InputState::new(window, cx).placeholder("Due YYYY-MM-DD"));
        let contacts_query =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search contacts"));
        let contacts_display_name =
            cx.new(|cx| InputState::new(window, cx).placeholder("Display name"));
        let contacts_email = cx.new(|cx| InputState::new(window, cx).placeholder("Email"));
        let contacts_phone = cx.new(|cx| InputState::new(window, cx).placeholder("Phone"));
        let contacts_org = cx.new(|cx| InputState::new(window, cx).placeholder("Organization"));
        let contacts_job_title = cx.new(|cx| InputState::new(window, cx).placeholder("Title"));
        let bookmarks_query =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search bookmarks"));
        let bookmarks_title = cx.new(|cx| InputState::new(window, cx).placeholder("Title"));
        let bookmarks_url = cx.new(|cx| InputState::new(window, cx).placeholder("https://"));
        let bookmarks_tags = cx.new(|cx| InputState::new(window, cx).placeholder("tag, tag"));
        let bookmarks_description = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(3, 8)
                .placeholder("Description")
        });
        let matches = applications::matches(&applications, "", MAX_RESULTS);
        let volume_slider = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(150.0)
                .step(1.0)
                .default_value(initial_volume)
        });
        let input_volume_slider = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(100.0)
                .step(1.0)
                .default_value(initial_input_volume)
        });
        let brightness_slider = cx.new(|_| {
            SliderState::new()
                .min(1.0)
                .max(100.0)
                .step(1.0)
                .default_value(initial_brightness)
        });
        let wifi_password = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder("Wi-Fi password")
        });
        let bluetooth_code = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder("PIN or passkey")
        });

        let input_subscription = cx.subscribe_in(&input, window, {
            let input = input.clone();
            move |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let query = input.read(cx).value().to_string();
                    this.matches = applications::matches(&this.applications, &query, MAX_RESULTS);
                    this.selected = 0;
                    cx.notify();
                }
            }
        });
        let volume_subscription = cx.subscribe(&volume_slider, {
            let controller = controller.clone();
            move |this, _, event: &SliderEvent, cx| {
                let volume = match event {
                    SliderEvent::Change(value) | SliderEvent::Release(value) => value.end() / 100.0,
                };
                this.pending_volume = Some((volume, Instant::now()));
                if matches!(event, SliderEvent::Release(_)) {
                    controller.set_volume(volume);
                }
                cx.notify();
            }
        });
        let input_volume_subscription = cx.subscribe(&input_volume_slider, {
            let controller = controller.clone();
            move |this, _, event: &SliderEvent, cx| {
                let volume = match event {
                    SliderEvent::Change(value) | SliderEvent::Release(value) => value.end() / 100.0,
                };
                this.pending_input_volume = Some((volume, Instant::now()));
                if matches!(event, SliderEvent::Release(_)) {
                    controller.set_input_volume(volume);
                }
                cx.notify();
            }
        });
        let contacts_query_subscription = cx.subscribe_in(&contacts_query, window, {
            move |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.contacts_mode = ContactsMode::Browse;
                    cx.notify();
                }
            }
        });
        let bookmarks_query_subscription = cx.subscribe_in(&bookmarks_query, window, {
            move |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.bookmarks_mode = BookmarksMode::Browse;
                    cx.notify();
                }
            }
        });
        let brightness_subscription = cx.subscribe(
            &brightness_slider,
            move |this, _, event: &SliderEvent, cx| {
                let percent = match event {
                    SliderEvent::Change(value) | SliderEvent::Release(value) => {
                        value.end().round().clamp(1.0, 100.0) as u8
                    }
                };
                this.pending_brightness = Some((percent, Instant::now()));
                if matches!(event, SliderEvent::Release(_)) {
                    controller.set_brightness(percent);
                }
                cx.notify();
            },
        );
        if section == Section::Notifications {
            FoyerShellState::global(cx)
                .storage_controller
                .mark_all_notifications_read();
        } else if section == Section::Activities {
            FoyerShellState::global(cx)
                .storage_controller
                .refresh_presentations();
        }
        Self {
            section,
            display_id,
            applications,
            input,
            activity_input,
            matches,
            selected: 0,
            volume_slider,
            input_volume_slider,
            brightness_slider,
            wifi_password,
            bluetooth_code,
            selected_wifi: None,
            pending_volume: None,
            pending_input_volume: None,
            pending_brightness: None,
            pending_session_action: None,
            notes_folder_id: None,
            notes_note_id: None,
            notes_mode: NotesMode::Browse,
            notes_preview: true,
            notes_title,
            notes_body,
            agenda_calendar_id: None,
            agenda_event_id: None,
            agenda_mode: AgendaMode::Browse,
            agenda_title,
            agenda_start,
            agenda_location,
            agenda_description,
            tasks_list_id: None,
            tasks_task_id: None,
            tasks_mode: TasksMode::Browse,
            tasks_title,
            tasks_body,
            tasks_due,
            contacts_book_id: None,
            contacts_contact_id: None,
            contacts_mode: ContactsMode::Browse,
            contacts_query,
            contacts_display_name,
            contacts_email,
            contacts_phone,
            contacts_org,
            contacts_job_title,
            bookmarks_folder_id: None,
            bookmarks_bookmark_id: None,
            bookmarks_mode: BookmarksMode::Browse,
            bookmarks_filter: foyer_shell_bookmarks::Filter::All,
            bookmarks_query,
            bookmarks_title,
            bookmarks_url,
            bookmarks_tags,
            bookmarks_description,
            _subscriptions: vec![
                input_subscription,
                volume_subscription,
                input_volume_subscription,
                brightness_subscription,
                contacts_query_subscription,
                bookmarks_query_subscription,
            ],
        }
    }

    fn set_section(&mut self, section: Section, cx: &mut Context<Self>) {
        if self.section == section {
            return;
        }
        self.section = section;
        self.selected_wifi = None;
        self.pending_session_action = None;
        if section == Section::Notifications {
            FoyerShellState::global(cx)
                .storage_controller
                .mark_all_notifications_read();
        } else if section == Section::Activities {
            FoyerShellState::global(cx)
                .storage_controller
                .refresh_presentations();
        }
        cx.notify();
    }

    fn sync_controls(
        &mut self,
        services: &foyer_shell_services::Snapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let volume = services.audio.volume;
        let wait_for_volume = self.pending_volume.is_some_and(|(expected, sent_at)| {
            (expected - volume).abs() > 0.011 && sent_at.elapsed() < Duration::from_secs(2)
        });
        if !wait_for_volume {
            self.pending_volume = None;
            let percent = volume * 100.0;
            if (self.volume_slider.read(cx).value().end() - percent).abs() > 0.5 {
                self.volume_slider
                    .update(cx, |slider, cx| slider.set_value(percent, window, cx));
            }
        }

        let brightness = services.brightness.percent;
        let wait_for_brightness = self.pending_brightness.is_some_and(|(expected, sent_at)| {
            expected != brightness && sent_at.elapsed() < Duration::from_secs(2)
        });
        if !wait_for_brightness {
            self.pending_brightness = None;
            let percent = f32::from(brightness.max(1));
            if (self.brightness_slider.read(cx).value().end() - percent).abs() > 0.5 {
                self.brightness_slider
                    .update(cx, |slider, cx| slider.set_value(percent, window, cx));
            }
        }

        let input_volume = services.audio.input_volume;
        let wait_for_input_volume = self
            .pending_input_volume
            .is_some_and(|(expected, sent_at)| {
                (expected - input_volume).abs() > 0.011
                    && sent_at.elapsed() < Duration::from_secs(2)
            });
        if !wait_for_input_volume {
            self.pending_input_volume = None;
            let percent = input_volume * 100.0;
            if (self.input_volume_slider.read(cx).value().end() - percent).abs() > 0.5 {
                self.input_volume_slider
                    .update(cx, |slider, cx| slider.set_value(percent, window, cx));
            }
        }
    }

    fn select_next(&mut self, cx: &mut Context<Self>) {
        if self.section == Section::Search && !self.matches.is_empty() {
            self.selected = (self.selected + 1).min(self.matches.len() - 1);
            cx.notify();
        }
    }

    fn select_previous(&mut self, cx: &mut Context<Self>) {
        if self.section == Section::Search {
            self.selected = self.selected.saturating_sub(1);
            cx.notify();
        }
    }

    fn launch_selected(&self, window: &mut Window, cx: &mut App) {
        if self.section == Section::Activities {
            let prompt = self.activity_input.read(cx).value().trim().to_string();
            if !prompt.is_empty() {
                crate::workspace_view::present(prompt, cx);
                defer_close(window, cx);
            }
            return;
        }
        if self.section != Section::Search {
            return;
        }
        let Some(index) = self.matches.get(self.selected) else {
            return;
        };
        applications::launch(self.applications[*index].clone());
        defer_close(window, cx);
    }

    fn content_for(
        &mut self,
        section: Section,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match section {
            Section::Search => self.search_content(cx),
            Section::Agenda => self.hosted_agenda_content(window, cx),
            Section::Tasks => self.hosted_tasks_content(window, cx),
            Section::Notes => self.notes_content(window, cx),
            Section::Contacts => self.contacts_content(window, cx),
            Section::Bookmarks => self.bookmarks_content(window, cx),
            Section::Activities => self.activities_content(window, cx),
            Section::Notifications => self.notification_history_content(cx),
            Section::Audio => self.audio_content(cx),
            Section::Network => self.network_content(window, cx),
            Section::Bluetooth => self.bluetooth_content(cx),
            Section::Display => self.display_content(cx),
            Section::Power => self.power_content(window, cx),
        }
    }

    fn notes_content(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let snapshot = FoyerShellState::global(cx).notes.clone();
        let controller = FoyerShellState::global(cx).notes_controller.clone();
        if let foyer_shell_notes::Availability::Unavailable(error) = &snapshot.availability {
            let detail = error.clone();
            let refresh = controller.clone();
            return content_column()
                .child(section_label("NOTES"))
                .child(error_text(detail))
                .child(muted_text(foyer_shell_notes::powersync_status()))
                .child(
                    Button::new("notes-refresh-unavailable")
                        .label("Try again")
                        .outline()
                        .on_click(move |_, _, _| refresh.refresh()),
                )
                .into_any_element();
        }
        if matches!(
            snapshot.availability,
            foyer_shell_notes::Availability::Loading
        ) {
            return empty_state(
                Icon::new(FoyerShellIcon::Notes),
                "Loading notes",
                "Connecting to Foyer Server…",
            );
        }

        if let Some(note_id) = self.notes_note_id.clone()
            && let Some(note) = snapshot.note(&note_id).cloned()
        {
            return self.note_detail_content(note, snapshot, controller, window, cx);
        }

        match self.notes_mode {
            NotesMode::RenameFolder => self.folder_rename_content(snapshot, controller, cx),
            NotesMode::MoveFolder => self.folder_move_content(snapshot, controller, cx),
            NotesMode::ConfirmDeleteFolder => self.folder_delete_content(snapshot, controller, cx),
            _ => self.notes_browse_content(snapshot, controller, cx),
        }
    }

    fn notes_browse_content(
        &mut self,
        snapshot: foyer_shell_notes::Snapshot,
        controller: foyer_shell_notes::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let parent = self.notes_folder_id.clone();
        let current = parent
            .as_deref()
            .and_then(|id| snapshot.folder(id))
            .cloned();
        let folders = snapshot.child_folders(parent.as_deref());
        let notes = parent
            .as_deref()
            .map(|id| snapshot.notes_in(id))
            .unwrap_or_default();
        let title_input = self.notes_title.clone();
        let body_input = self.notes_body.clone();
        let create_controller = controller.clone();
        let folder_for_create = parent.clone();
        let heading = current
            .as_ref()
            .map(|folder| folder.name.clone())
            .unwrap_or_else(|| "Vault".into());
        let path_label = current
            .as_ref()
            .map(|folder| snapshot.folder_path_label(&folder.id))
            .unwrap_or_else(|| "Root".into());
        let folder_empty = current
            .as_ref()
            .is_some_and(|folder| snapshot.folder_is_empty(&folder.id));
        content_column()
            .children(notes_status_elements(&snapshot))
            .when(parent.is_some(), |column| {
                column.child(
                    Button::new("notes-up")
                        .label("Parent folder")
                        .outline()
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(current) = this.notes_folder_id.clone() {
                                this.notes_folder_id = FoyerShellState::global(cx)
                                    .notes
                                    .folder(&current)
                                    .and_then(|folder| folder.parent_id.clone());
                                this.notes_mode = NotesMode::Browse;
                            }
                            cx.notify();
                        })),
                )
            })
            .child(section_label("FOLDER"))
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::MEDIUM)
                    .child(heading),
            )
            .child(muted_text(path_label))
            .when_some(current.clone(), |column, folder| {
                let rename_folder = folder.clone();
                column.child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("notes-rename-folder")
                                .label("Rename")
                                .outline()
                                .small()
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.notes_mode = NotesMode::RenameFolder;
                                    let name = rename_folder.name.clone();
                                    this.notes_title
                                        .update(cx, |input, cx| input.set_value(name, window, cx));
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("notes-move-folder")
                                .label("Move")
                                .outline()
                                .small()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.notes_mode = NotesMode::MoveFolder;
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("notes-delete-folder")
                                .label("Delete")
                                .outline()
                                .small()
                                .disabled(!folder_empty)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.notes_mode = NotesMode::ConfirmDeleteFolder;
                                    cx.notify();
                                })),
                        ),
                )
            })
            .child(section_label("FOLDERS"))
            .children(folders.into_iter().map(|folder| {
                let folder_id = folder.id.clone();
                control_card()
                    .id(SharedString::from(format!("notes-folder-{}", folder.id)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.notes_folder_id = Some(folder_id.clone());
                        this.notes_note_id = None;
                        this.notes_mode = NotesMode::Browse;
                        cx.notify();
                    }))
                    .child(div().text_sm().child(folder.name.clone()))
            }))
            .child(section_label("NOTES"))
            .children(notes.into_iter().map(|note| {
                let note_id = note.id.clone();
                control_card()
                    .id(SharedString::from(format!("notes-row-{}", note.id)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.notes_note_id = Some(note_id.clone());
                        this.notes_mode = NotesMode::Browse;
                        this.notes_preview = true;
                        cx.notify();
                    }))
                    .child(div().text_sm().child(note.title.clone()))
            }))
            .child(section_label("CREATE"))
            .child(Input::new(&self.notes_title).cleanable(true))
            .child(Textarea::new(&self.notes_body))
            .child(
                Button::new("notes-create-folder")
                    .label("New folder")
                    .outline()
                    .on_click({
                        let controller = create_controller.clone();
                        let input = title_input.clone();
                        let parent = folder_for_create.clone();
                        move |_, window, cx| {
                            let name = input.read(cx).value().trim().to_string();
                            if !name.is_empty() {
                                controller.create_folder(name, parent.clone());
                                input.update(cx, |input, cx| input.set_value("", window, cx));
                            }
                        }
                    }),
            )
            .child(
                Button::new("notes-create-note")
                    .label("New note")
                    .primary()
                    .disabled(folder_for_create.is_none())
                    .on_click({
                        let controller = create_controller;
                        let title = title_input;
                        let body = body_input;
                        let folder = folder_for_create;
                        move |_, window, cx| {
                            let title_value = title.read(cx).value().trim().to_string();
                            let body_value = body.read(cx).value().to_string();
                            if let Some(folder_id) = folder.clone()
                                && !title_value.is_empty()
                            {
                                controller.create_note(folder_id, title_value, body_value);
                                title.update(cx, |input, cx| input.set_value("", window, cx));
                                body.update(cx, |input, cx| input.set_value("", window, cx));
                            }
                        }
                    }),
            )
            .when(parent.is_none(), |column| {
                column.child(muted_text("Open a folder to write a note."))
            })
            .into_any_element()
    }

    fn folder_rename_content(
        &mut self,
        snapshot: foyer_shell_notes::Snapshot,
        controller: foyer_shell_notes::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(folder) = self
            .notes_folder_id
            .as_deref()
            .and_then(|id| snapshot.folder(id))
            .cloned()
        else {
            self.notes_mode = NotesMode::Browse;
            return self.notes_browse_content(snapshot, controller, cx);
        };
        let title_input = self.notes_title.clone();
        let rename_controller = controller.clone();
        content_column()
            .children(notes_status_elements(&snapshot))
            .child(section_label("RENAME FOLDER"))
            .child(muted_text(snapshot.folder_path_label(&folder.id)))
            .child(Input::new(&self.notes_title).cleanable(true))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("notes-rename-cancel")
                            .label("Cancel")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.notes_mode = NotesMode::Browse;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("notes-rename-save")
                            .label("Save")
                            .primary()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let name = title_input.read(cx).value().trim().to_string();
                                if !name.is_empty() {
                                    rename_controller.rename_folder(
                                        folder.id.clone(),
                                        folder.revision,
                                        name,
                                    );
                                    title_input
                                        .update(cx, |input, cx| input.set_value("", window, cx));
                                    this.notes_mode = NotesMode::Browse;
                                    cx.notify();
                                }
                            })),
                    ),
            )
            .into_any_element()
    }

    fn folder_move_content(
        &mut self,
        snapshot: foyer_shell_notes::Snapshot,
        controller: foyer_shell_notes::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(folder) = self
            .notes_folder_id
            .as_deref()
            .and_then(|id| snapshot.folder(id))
            .cloned()
        else {
            self.notes_mode = NotesMode::Browse;
            return self.notes_browse_content(snapshot, controller, cx);
        };
        let targets = snapshot.valid_folder_move_targets(&folder.id);
        let current_parent = folder.parent_id.clone();
        content_column()
            .children(notes_status_elements(&snapshot))
            .child(section_label("MOVE FOLDER"))
            .child(muted_text(format!("Moving {}", folder.name)))
            .child(
                Button::new("notes-move-cancel")
                    .label("Cancel")
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.notes_mode = NotesMode::Browse;
                        cx.notify();
                    })),
            )
            .child(
                control_card()
                    .id("notes-move-root")
                    .cursor_pointer()
                    .on_click({
                        let move_controller = controller.clone();
                        let folder = folder.clone();
                        cx.listener(move |this, _, _, cx| {
                            move_controller.move_folder(folder.id.clone(), folder.revision, None);
                            this.notes_mode = NotesMode::Browse;
                            cx.notify();
                        })
                    })
                    .child(div().text_sm().child("Vault root"))
                    .when(current_parent.is_none(), |card| {
                        card.child(muted_text("Current location"))
                    }),
            )
            .children(targets.into_iter().map(|target| {
                let selected = current_parent.as_deref() == Some(target.id.as_str());
                let move_controller = controller.clone();
                let folder = folder.clone();
                let parent_id = target.id.clone();
                control_card()
                    .id(SharedString::from(format!("notes-move-{}", target.id)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        move_controller.move_folder(
                            folder.id.clone(),
                            folder.revision,
                            Some(parent_id.clone()),
                        );
                        this.notes_mode = NotesMode::Browse;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .text_sm()
                            .child(snapshot.folder_path_label(&target.id)),
                    )
                    .when(selected, |card| card.child(muted_text("Current location")))
            }))
            .into_any_element()
    }

    fn folder_delete_content(
        &mut self,
        snapshot: foyer_shell_notes::Snapshot,
        controller: foyer_shell_notes::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(folder) = self
            .notes_folder_id
            .as_deref()
            .and_then(|id| snapshot.folder(id))
            .cloned()
        else {
            self.notes_mode = NotesMode::Browse;
            return self.notes_browse_content(snapshot, controller, cx);
        };
        let delete_controller = controller;
        content_column()
            .children(notes_status_elements(&snapshot))
            .child(section_label("DELETE FOLDER"))
            .child(error_text(format!(
                "Delete “{}”? This cannot be undone after it syncs.",
                folder.name
            )))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("notes-delete-folder-cancel")
                            .label("Cancel")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.notes_mode = NotesMode::Browse;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("notes-delete-folder-confirm")
                            .label("Delete folder")
                            .primary()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                delete_controller.delete_folder(folder.id.clone(), folder.revision);
                                this.notes_folder_id = folder.parent_id.clone();
                                this.notes_mode = NotesMode::Browse;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    fn note_detail_content(
        &mut self,
        note: foyer_shell_notes::Note,
        snapshot: foyer_shell_notes::Snapshot,
        controller: foyer_shell_notes::Controller,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let folder_label = snapshot.folder_path_label(&note.folder_id);
        let back_id = note.folder_id.clone();
        if self.notes_mode == NotesMode::MoveNote {
            return self.note_move_content(note, snapshot, controller, cx);
        }
        if self.notes_mode == NotesMode::EditNote {
            return self.note_edit_content(note, snapshot, controller, cx);
        }
        if self.notes_mode == NotesMode::ConfirmDeleteNote {
            return self.note_delete_content(note, snapshot, controller, cx);
        }
        let edit_note = note.clone();
        content_column()
            .children(notes_status_elements(&snapshot))
            .child(
                Button::new("notes-back-note")
                    .label("Back")
                    .outline()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.notes_note_id = None;
                        this.notes_folder_id = Some(back_id.clone());
                        this.notes_mode = NotesMode::Browse;
                        cx.notify();
                    })),
            )
            .child(section_label("NOTE"))
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::MEDIUM)
                    .child(note.title.clone()),
            )
            .child(muted_text(format!("Folder · {folder_label}")))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("notes-preview-mode")
                            .label("Preview")
                            .when(self.notes_preview, |button| button.primary())
                            .when(!self.notes_preview, |button| button.outline())
                            .small()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.notes_preview = true;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("notes-source-mode")
                            .label("Source")
                            .when(!self.notes_preview, |button| button.primary())
                            .when(self.notes_preview, |button| button.outline())
                            .small()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.notes_preview = false;
                                cx.notify();
                            })),
                    ),
            )
            .children(if self.notes_preview {
                markdown_preview_elements(&note.body)
            } else {
                vec![muted_text(note.body.clone()).into_any_element()]
            })
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("notes-edit-note")
                            .label("Edit")
                            .primary()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.notes_mode = NotesMode::EditNote;
                                this.notes_preview = false;
                                let title = edit_note.title.clone();
                                let body = edit_note.body.clone();
                                this.notes_title
                                    .update(cx, |input, cx| input.set_value(title, window, cx));
                                this.notes_body
                                    .update(cx, |input, cx| input.set_value(body, window, cx));
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("notes-move-note")
                            .label("Move")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.notes_mode = NotesMode::MoveNote;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("notes-delete-note")
                            .label("Delete")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.notes_mode = NotesMode::ConfirmDeleteNote;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    fn note_delete_content(
        &mut self,
        note: foyer_shell_notes::Note,
        snapshot: foyer_shell_notes::Snapshot,
        controller: foyer_shell_notes::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        content_column()
            .children(notes_status_elements(&snapshot))
            .child(section_label("DELETE NOTE"))
            .child(error_text(format!(
                "Delete “{}”? This cannot be undone after it syncs.",
                note.title
            )))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("notes-delete-note-cancel")
                            .label("Cancel")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.notes_mode = NotesMode::Browse;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("notes-delete-note-confirm")
                            .label("Delete note")
                            .primary()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                controller.delete_note(note.id.clone(), note.revision);
                                this.notes_note_id = None;
                                this.notes_folder_id = Some(note.folder_id.clone());
                                this.notes_mode = NotesMode::Browse;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    fn note_edit_content(
        &mut self,
        note: foyer_shell_notes::Note,
        snapshot: foyer_shell_notes::Snapshot,
        controller: foyer_shell_notes::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let title_input = self.notes_title.clone();
        let body_input = self.notes_body.clone();
        let save_controller = controller;
        let preview_source = if self.notes_preview {
            body_input.read(cx).value().to_string()
        } else {
            String::new()
        };
        content_column()
            .children(notes_status_elements(&snapshot))
            .child(section_label("EDIT NOTE"))
            .child(muted_text(snapshot.folder_path_label(&note.folder_id)))
            .child(Input::new(&self.notes_title).cleanable(true))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("notes-edit-source")
                            .label("Source")
                            .when(!self.notes_preview, |button| button.primary())
                            .when(self.notes_preview, |button| button.outline())
                            .small()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.notes_preview = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("notes-edit-preview")
                            .label("Preview")
                            .when(self.notes_preview, |button| button.primary())
                            .when(!self.notes_preview, |button| button.outline())
                            .small()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.notes_preview = true;
                                cx.notify();
                            })),
                    ),
            )
            .when(!self.notes_preview, |column| {
                column.child(Textarea::new(&self.notes_body))
            })
            .when(self.notes_preview, |column| {
                column.children(markdown_preview_elements(&preview_source))
            })
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("notes-edit-cancel")
                            .label("Cancel")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.notes_mode = NotesMode::Browse;
                                this.notes_preview = true;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("notes-edit-save")
                            .label("Save")
                            .primary()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let title = title_input.read(cx).value().trim().to_string();
                                let body = body_input.read(cx).value().to_string();
                                if !title.is_empty() {
                                    save_controller.update_note(
                                        note.id.clone(),
                                        note.revision,
                                        title,
                                        body,
                                    );
                                    this.notes_mode = NotesMode::Browse;
                                    this.notes_preview = true;
                                    cx.notify();
                                }
                            })),
                    ),
            )
            .into_any_element()
    }

    fn note_move_content(
        &mut self,
        note: foyer_shell_notes::Note,
        snapshot: foyer_shell_notes::Snapshot,
        controller: foyer_shell_notes::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        content_column()
            .children(notes_status_elements(&snapshot))
            .child(section_label("MOVE NOTE"))
            .child(muted_text(note.title.clone()))
            .child(
                Button::new("notes-move-note-cancel")
                    .label("Cancel")
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.notes_mode = NotesMode::Browse;
                        cx.notify();
                    })),
            )
            .children(snapshot.folders.iter().map(|folder| {
                let selected = folder.id == note.folder_id;
                let move_controller = controller.clone();
                let note = note.clone();
                let folder_id = folder.id.clone();
                control_card()
                    .id(SharedString::from(format!("notes-move-note-{}", folder.id)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        move_controller.move_note(
                            note.id.clone(),
                            note.revision,
                            folder_id.clone(),
                        );
                        this.notes_folder_id = Some(folder_id.clone());
                        this.notes_mode = NotesMode::Browse;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .text_sm()
                            .child(snapshot.folder_path_label(&folder.id)),
                    )
                    .when(selected, |card| card.child(muted_text("Current folder")))
            }))
            .into_any_element()
    }

    fn search_content(&self, cx: &mut Context<Self>) -> AnyElement {
        let result_count = self.matches.len();
        let rows = self
            .matches
            .iter()
            .enumerate()
            .map(|(position, application_index)| {
                let application = self.applications[*application_index].clone();
                let clicked_application = application.clone();
                let selected = position == self.selected;
                let initials = application
                    .name
                    .split_whitespace()
                    .filter_map(|part| part.chars().next())
                    .take(2)
                    .collect::<String>()
                    .to_uppercase();
                div()
                    .id(("panel-application", *application_index))
                    .min_h(px(56.0))
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_3()
                    .rounded_lg()
                    .cursor_pointer()
                    .when(selected, |row| row.bg(cx.theme().accent))
                    .hover(|row| row.bg(cx.theme().accent.opacity(0.72)))
                    .on_click(move |_, window, cx| {
                        applications::launch(clicked_application.clone());
                        defer_close(window, cx);
                    })
                    .child(
                        div()
                            .size(px(32.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_lg()
                            .bg(rgb(tokens::SURFACE_RAISED))
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(initials),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .overflow_hidden()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(application.name),
                            )
                            .when(!application.comment.is_empty(), |column| {
                                column.child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(tokens::MUTED))
                                        .text_ellipsis()
                                        .child(application.comment),
                                )
                            }),
                    )
            })
            .collect::<Vec<_>>();

        v_flex()
            .size_full()
            .child(
                div()
                    .px_5()
                    .pt_4()
                    .pb_3()
                    .child(Input::new(&self.input).large()),
            )
            .child(
                v_flex()
                    .flex_1()
                    .overflow_y_scrollbar()
                    .px_3()
                    .gap_1()
                    .when(result_count == 0, |list| {
                        list.child(
                            div()
                                .flex_1()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_sm()
                                .text_color(rgb(tokens::MUTED))
                                .child("No applications found"),
                        )
                    })
                    .children(rows),
            )
            .child(
                h_flex()
                    .h_10()
                    .px_5()
                    .justify_between()
                    .border_t_1()
                    .border_color(rgb(tokens::BORDER))
                    .text_xs()
                    .text_color(rgb(tokens::SUBTLE))
                    .child(format!("{result_count} results"))
                    .child("Arrow keys navigate · Enter opens"),
            )
            .into_any_element()
    }

    fn notification_history_content(&self, cx: &mut Context<Self>) -> AnyElement {
        let snapshot = FoyerShellState::global(cx).storage.clone();
        if !matches!(
            snapshot.availability,
            foyer_shell_storage::Availability::Available
        ) {
            return empty_state(
                Icon::new(IconName::TriangleAlert),
                "Notification history unavailable",
                snapshot.availability.detail().to_string(),
            );
        }
        let controller = FoyerShellState::global(cx).storage_controller.clone();
        if snapshot.notifications.is_empty() {
            let dnd_controller = controller.clone();
            return content_column()
                .child(section_label("DELIVERY"))
                .child(
                    control_card().child(
                        h_flex()
                            .justify_between()
                            .child(muted_text("Do Not Disturb"))
                            .child(
                                Switch::new("notification-dnd-empty")
                                    .small()
                                    .checked(snapshot.do_not_disturb)
                                    .on_click(move |enabled, _, _| {
                                        dnd_controller.set_do_not_disturb(*enabled)
                                    }),
                            ),
                    ),
                )
                .child(section_label("HISTORY"))
                .child(control_card().child(muted_text(
                    "No notifications yet. New application notifications will be kept here for up to 30 days.",
                )))
                .into_any_element();
        }

        let count = snapshot.notifications.len();
        let rows = snapshot
            .notifications
            .iter()
            .cloned()
            .map(|record| {
                let id = record.id;
                let app_name = if record.app_name.is_empty() {
                    "Application".to_string()
                } else {
                    record.app_name.clone()
                };
                let timestamp = notification_timestamp(record.received_at_ms);
                let border = match record.urgency {
                    foyer_shell_storage::NotificationUrgency::Critical => tokens::FOREGROUND,
                    foyer_shell_storage::NotificationUrgency::Low
                    | foyer_shell_storage::NotificationUrgency::Normal => tokens::BORDER,
                };
                let delete_controller = controller.clone();
                v_flex()
                    .id(("notification-history", id as u64))
                    .min_h(px(94.0))
                    .p_3()
                    .gap_1()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(border))
                    .bg(rgb(if record.is_read {
                        tokens::SURFACE_RECESSED
                    } else {
                        tokens::SURFACE
                    }))
                    .child(
                        h_flex()
                            .h_5()
                            .justify_between()
                            .gap_2()
                            .child(
                                h_flex()
                                    .flex_1()
                                    .overflow_hidden()
                                    .gap_2()
                                    .when(!record.is_read, |row| {
                                        row.child(
                                            div()
                                                .size(px(5.0))
                                                .rounded_full()
                                                .bg(rgb(tokens::FOREGROUND)),
                                        )
                                    })
                                    .child(
                                        div()
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .text_xs()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(tokens::MUTED))
                                            .child(app_name),
                                    )
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .text_xs()
                                            .text_color(rgb(tokens::SUBTLE))
                                            .child(timestamp),
                                    ),
                            )
                            .child(
                                Button::new(("delete-notification-history", id as u64))
                                    .icon(IconName::Close)
                                    .tooltip("Delete from history")
                                    .accessibility_id("Delete from notification history")
                                    .ghost()
                                    .xsmall()
                                    .on_click(move |_, _, _| {
                                        delete_controller.delete_notification(id)
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(record.summary),
                    )
                    .when(!record.body.is_empty(), |row| {
                        row.child(
                            div()
                                .overflow_hidden()
                                .text_ellipsis()
                                .text_xs()
                                .text_color(rgb(tokens::MUTED))
                                .child(notification_history_body(&record.body)),
                        )
                    })
            })
            .collect::<Vec<_>>();
        let clear_controller = controller.clone();
        let dnd_controller = controller.clone();

        v_flex()
            .size_full()
            .child(
                v_flex()
                    .border_b_1()
                    .border_color(rgb(tokens::BORDER))
                    .child(
                        h_flex()
                            .h_12()
                            .px_5()
                            .justify_between()
                            .child(muted_text("Do Not Disturb"))
                            .child(
                                Switch::new("notification-dnd")
                                    .small()
                                    .checked(snapshot.do_not_disturb)
                                    .on_click(move |enabled, _, _| {
                                        dnd_controller.set_do_not_disturb(*enabled)
                                    }),
                            ),
                    )
                    .child(
                        h_flex()
                            .h_10()
                            .px_5()
                            .justify_between()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(tokens::MUTED))
                                    .child(format!("{count} saved")),
                            )
                            .child(
                                Button::new("clear-notification-history")
                                    .label("Clear all")
                                    .ghost()
                                    .small()
                                    .on_click(move |_, _, _| {
                                        clear_controller.clear_notifications()
                                    }),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .overflow_y_scrollbar()
                    .p_3()
                    .gap_2()
                    .children(rows),
            )
            .into_any_element()
    }

    fn activities_content(&self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let snapshot = FoyerShellState::global(cx).storage.clone();
        let prompt_input = self.activity_input.clone();
        let composer = h_flex()
            .gap_2()
            .child(Input::new(&self.activity_input).large().flex_1())
            .child(
                Button::new("activity-explain")
                    .icon(IconName::Bot)
                    .label("Explain")
                    .on_click(move |_, window, cx| {
                        let prompt = prompt_input.read(cx).value().trim().to_string();
                        if !prompt.is_empty() {
                            crate::workspace_view::present(prompt, cx);
                            defer_close(window, cx);
                        }
                    }),
            );
        if let foyer_shell_storage::Availability::Unavailable(error) = snapshot.availability {
            return content_column()
                .child(composer)
                .child(section_label("PRESENTATIONS"))
                .child(error_text(error))
                .into_any_element();
        }
        if matches!(
            snapshot.availability,
            foyer_shell_storage::Availability::Loading
        ) {
            return empty_state(
                Icon::new(IconName::Bot),
                "Loading Activities",
                "Opening the durable presentation catalog…",
            );
        }
        if snapshot.presentations.is_empty() {
            return content_column()
                .child(composer)
                .child(empty_state(
                    Icon::new(IconName::Bot),
                    "No saved presentations",
                    "Completed visual presentations will appear here and replay without another model call.",
                ))
                .into_any_element();
        }

        let rows = snapshot
            .presentations
            .iter()
            .enumerate()
            .map(|(index, presentation)| {
                let path = presentation.bundle_path.clone();
                let created = activity_timestamp(presentation.created_at_ms);
                let duration = format_duration(presentation.duration_ms);
                let detail = format!(
                    "{} · {} slides · {} · {}",
                    created,
                    presentation.slide_count,
                    duration,
                    presentation.status.replace('_', " ")
                );
                control_card()
                    .gap_3()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .line_clamp(2)
                                    .child(presentation.title.clone()),
                            )
                            .child(div().text_xs().text_color(rgb(tokens::MUTED)).child(detail)),
                    )
                    .child(
                        Button::new(("activity-replay", index))
                            .icon(FoyerShellIcon::Play)
                            .label("Replay presentation")
                            .outline()
                            .disabled(presentation.slide_count == 0)
                            .on_click(move |_, window, cx| {
                                crate::workspace_view::replay(path.clone(), cx);
                                defer_close(window, cx);
                            }),
                    )
            })
            .collect::<Vec<_>>();

        content_column()
            .child(composer)
            .child(section_label("PRESENTATIONS"))
            .children(rows)
            .child(div().text_xs().text_color(rgb(tokens::SUBTLE)).child(
                "Saved presentations replay from retained slides, cues, assets, and narration.",
            ))
            .into_any_element()
    }

    fn audio_content(&self, cx: &mut Context<Self>) -> AnyElement {
        let services = FoyerShellState::global(cx).services.clone();
        let controller = FoyerShellState::global(cx).service_controller.clone();
        let available = services.audio.availability.is_available();
        let output_percent = self
            .pending_volume
            .map(|(value, _)| (value * 100.0).round() as u16)
            .unwrap_or_else(|| (services.audio.volume * 100.0).round() as u16);
        let input_percent = self
            .pending_input_volume
            .map(|(value, _)| (value * 100.0).round() as u16)
            .unwrap_or_else(|| (services.audio.input_volume * 100.0).round() as u16);
        let value = if available {
            if services.audio.muted {
                "Muted".into()
            } else {
                format!("{output_percent}%")
            }
        } else {
            services.audio.availability.detail().into()
        };
        let output_devices = services.audio.outputs.iter().map(|device| {
            let id = device.id.clone();
            let selected = device.is_default;
            let controller = controller.clone();
            selectable_row(
                format!("audio-output-{}", device.id),
                FoyerShellIcon::Headphones,
                device.description.clone(),
                if selected { "" } else { "Use this output" },
                selected,
            )
            .on_click(move |_, _, _| controller.set_default_output(id.clone()))
        });
        let input_devices = services.audio.inputs.iter().map(|device| {
            let id = device.id.clone();
            let selected = device.is_default;
            let controller = controller.clone();
            selectable_row(
                format!("audio-input-{}", device.id),
                FoyerShellIcon::Microphone,
                device.description.clone(),
                if selected { "" } else { "Use this input" },
                selected,
            )
            .on_click(move |_, _, _| controller.set_default_input(id.clone()))
        });
        let streams = services.audio.streams.iter().map(|stream| {
            let lower_controller = controller.clone();
            let raise_controller = controller.clone();
            let mute_controller = controller.clone();
            let id = stream.id;
            let volume = stream.volume;
            h_flex()
                .gap_2()
                .child(
                    v_flex()
                        .flex_1()
                        .overflow_hidden()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .text_ellipsis()
                                .child(stream.name.clone()),
                        )
                        .child(muted_text(if stream.muted {
                            "Muted".into()
                        } else {
                            format!("{}%", (volume * 100.0).round() as u16)
                        })),
                )
                .child(
                    Button::new(("stream-lower", id as u64))
                        .icon(IconName::Minus)
                        .tooltip("Lower application volume")
                        .ghost()
                        .xsmall()
                        .on_click(move |_, _, _| {
                            lower_controller.set_stream_volume(id, (volume - 0.05).max(0.0))
                        }),
                )
                .child(
                    Button::new(("stream-raise", id as u64))
                        .icon(IconName::Plus)
                        .tooltip("Raise application volume")
                        .ghost()
                        .xsmall()
                        .on_click(move |_, _, _| {
                            raise_controller.set_stream_volume(id, (volume + 0.05).min(1.5))
                        }),
                )
                .child(
                    Switch::new(("stream-mute", id as u64))
                        .small()
                        .checked(stream.muted)
                        .on_click(move |muted, _, _| mute_controller.set_stream_muted(id, *muted)),
                )
        });
        let output_mute = controller.clone();
        let input_mute = controller.clone();
        let media_players = services.media.players.iter().map(|player| {
            let previous_controller = controller.clone();
            let play_controller = controller.clone();
            let next_controller = controller.clone();
            let raise_controller = controller.clone();
            let previous_bus = player.bus_name.clone();
            let play_bus = player.bus_name.clone();
            let next_bus = player.bus_name.clone();
            let raise_bus = player.bus_name.clone();
            let playing = player.playback_status == "Playing";
            control_card()
                .gap_2()
                .when_some(player.art_url.clone(), |card, art_url| {
                    card.child(media_artwork(art_url))
                })
                .child(control_heading(
                    player.title.clone(),
                    player.identity.clone(),
                ))
                .when(!player.artist.is_empty(), |card| {
                    card.child(muted_text(player.artist.clone()))
                })
                .when(!player.album.is_empty(), |card| {
                    card.child(
                        div()
                            .text_xs()
                            .text_color(rgb(tokens::SUBTLE))
                            .child(player.album.clone()),
                    )
                })
                .child(
                    h_flex()
                        .justify_between()
                        .child(
                            Button::new(format!("media-raise-{}", player.bus_name))
                                .label("Open player")
                                .ghost()
                                .small()
                                .on_click(move |_, _, _| {
                                    raise_controller.media_raise(raise_bus.clone())
                                }),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Button::new(format!("media-previous-{}", player.bus_name))
                                        .icon(FoyerShellIcon::SkipBack)
                                        .tooltip("Previous track")
                                        .ghost()
                                        .small()
                                        .disabled(!player.can_go_previous)
                                        .on_click(move |_, _, _| {
                                            previous_controller.media_previous(previous_bus.clone())
                                        }),
                                )
                                .child(
                                    Button::new(format!("media-play-{}", player.bus_name))
                                        .icon(if playing {
                                            FoyerShellIcon::Pause
                                        } else {
                                            FoyerShellIcon::Play
                                        })
                                        .tooltip(if playing { "Pause" } else { "Play" })
                                        .outline()
                                        .small()
                                        .disabled(if playing {
                                            !player.can_pause
                                        } else {
                                            !player.can_play
                                        })
                                        .on_click(move |_, _, _| {
                                            play_controller.media_play_pause(play_bus.clone())
                                        }),
                                )
                                .child(
                                    Button::new(format!("media-next-{}", player.bus_name))
                                        .icon(FoyerShellIcon::SkipForward)
                                        .tooltip("Next track")
                                        .ghost()
                                        .small()
                                        .disabled(!player.can_go_next)
                                        .on_click(move |_, _, _| {
                                            next_controller.media_next(next_bus.clone())
                                        }),
                                ),
                        ),
                )
        });
        content_column()
            .when(!services.media.players.is_empty(), |column| {
                column
                    .child(section_label("NOW PLAYING"))
                    .children(media_players)
            })
            .child(section_label("OUTPUT"))
            .child(
                control_card()
                    .child(control_heading(services.audio.device.clone(), value))
                    .child(Slider::new(&self.volume_slider).disabled(!available))
                    .child(
                        h_flex()
                            .justify_between()
                            .child(muted_text("Mute output"))
                            .child(
                                Switch::new("panel-audio-mute")
                                    .small()
                                    .checked(services.audio.muted)
                                    .disabled(!available)
                                    .on_click(move |muted, _, _| output_mute.set_muted(*muted)),
                            ),
                    ),
            )
            .children(output_devices)
            .child(section_label("MICROPHONE"))
            .child(
                control_card()
                    .child(control_heading(
                        services.audio.input_device.clone(),
                        if services.audio.input_muted {
                            "Muted".into()
                        } else {
                            format!("{input_percent}%")
                        },
                    ))
                    .child(Slider::new(&self.input_volume_slider).disabled(!available))
                    .child(
                        h_flex()
                            .justify_between()
                            .child(muted_text("Mute microphone"))
                            .child(
                                Switch::new("panel-input-mute")
                                    .small()
                                    .checked(services.audio.input_muted)
                                    .disabled(!available)
                                    .on_click(move |muted, _, _| {
                                        input_mute.set_input_muted(*muted)
                                    }),
                            ),
                    )
                    .when(!services.audio.recording_apps.is_empty(), |card| {
                        card.child(
                            div()
                                .text_xs()
                                .text_color(rgb(tokens::SUBTLE))
                                .child(format!(
                                    "Microphone in use by {}",
                                    services
                                        .audio
                                        .recording_apps
                                        .iter()
                                        .map(|app| app.name.as_str())
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                )),
                        )
                    }),
            )
            .children(input_devices)
            .child(section_label("APPLICATIONS"))
            .child(
                control_card()
                    .when(services.audio.streams.is_empty(), |card| {
                        card.child(muted_text("No applications are playing audio."))
                    })
                    .children(streams),
            )
            .when_some(services.audio.last_error.clone(), |column, error| {
                column.child(error_text(error))
            })
            .into_any_element()
    }

    fn network_content(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let services = FoyerShellState::global(cx).services.clone();
        let controller = FoyerShellState::global(cx).service_controller.clone();
        let available = services.network.availability.is_available();
        let connection = services
            .network
            .connection
            .clone()
            .unwrap_or_else(|| "No active Wi-Fi connection".into());
        let detail = match (
            services.network.signal,
            services.network.security.as_deref(),
        ) {
            (Some(signal), Some(security)) => format!("{signal}% signal · {security}"),
            (Some(signal), None) => format!("{signal}% signal"),
            _ if available => services.network.connectivity.label().into(),
            _ => services.network.availability.detail().into(),
        };
        if let Some(ssid) = self.selected_wifi.clone()
            && let Some(network) = services
                .network
                .networks
                .iter()
                .find(|item| item.ssid == ssid)
        {
            let connect_controller = controller.clone();
            let password = self.wifi_password.clone();
            let target_ssid = network.ssid.clone();
            let saved_uuid = network.saved_uuid.clone();
            return content_column()
                .child(section_label("CONNECT TO WI-FI"))
                .child(
                    control_card()
                        .child(control_heading(
                            network.ssid.clone(),
                            wifi_detail(network.signal, network.security.as_deref()),
                        ))
                        .child(Input::new(&self.wifi_password))
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(tokens::SUBTLE))
                                .child("The password is sent directly to NetworkManager and is never stored in Foyer Shell's database."),
                        ),
                )
                .child(
                    h_flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("wifi-password-back")
                                .label("Back")
                                .outline()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.selected_wifi = None;
                                    this.wifi_password.update(cx, |input, cx| {
                                        input.set_value("", window, cx)
                                    });
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("wifi-password-connect")
                                .label("Connect")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    let value = password.read(cx).value().to_string();
                                    connect_controller.connect_wifi(
                                        target_ssid.clone(),
                                        (!value.is_empty()).then_some(value),
                                        saved_uuid.clone(),
                                    );
                                    password.update(cx, |input, cx| {
                                        input.set_value("", window, cx)
                                    });
                                    this.selected_wifi = None;
                                    cx.notify();
                                })),
                        ),
                )
                .into_any_element();
        }

        let refresh_controller = controller.clone();
        let disconnect_controller = controller.clone();
        let networks = services
            .network
            .networks
            .iter()
            .map(|network| {
                let connect_controller = controller.clone();
                let forget_controller = controller.clone();
                let ssid = network.ssid.clone();
                let connect_ssid = ssid.clone();
                let saved_uuid = network.saved_uuid.clone();
                let forget_uuid = network.saved_uuid.clone();
                let secure = network.security.is_some();
                let active = network.active;
                control_card()
                    .gap_2()
                    .child(control_heading(
                        ssid.clone(),
                        wifi_detail(network.signal, network.security.as_deref()),
                    ))
                    .child(
                        h_flex()
                            .justify_end()
                            .gap_2()
                            .when_some(forget_uuid, |row, uuid| {
                                row.child(
                                    Button::new(format!("wifi-forget-{uuid}"))
                                        .label("Forget")
                                        .ghost()
                                        .small()
                                        .on_click(move |_, _, _| {
                                            forget_controller.forget_wifi(uuid.clone())
                                        }),
                                )
                            })
                            .child(if active {
                                Button::new(format!("wifi-disconnect-{ssid}"))
                                    .label("Disconnect")
                                    .outline()
                                    .small()
                                    .on_click({
                                        let controller = disconnect_controller.clone();
                                        move |_, _, _| controller.disconnect_wifi()
                                    })
                            } else {
                                Button::new(format!("wifi-connect-{ssid}"))
                                    .label("Connect")
                                    .small()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if saved_uuid.is_some() || !secure {
                                            connect_controller.connect_wifi(
                                                connect_ssid.clone(),
                                                None,
                                                saved_uuid.clone(),
                                            );
                                        } else {
                                            this.selected_wifi = Some(connect_ssid.clone());
                                            cx.notify();
                                        }
                                    }))
                            }),
                    )
            })
            .collect::<Vec<_>>();
        let _ = window;
        content_column()
            .child(section_label("WIRELESS"))
            .child(
                control_card()
                    .child(control_heading(connection, detail))
                    .child(
                        h_flex()
                            .justify_between()
                            .child(muted_text("Wi-Fi radio"))
                            .child(
                                Switch::new("panel-wifi")
                                    .small()
                                    .checked(services.network.wifi_enabled)
                                    .disabled(!available)
                                    .on_click(move |enabled, _, _| {
                                        controller.set_wifi_enabled(*enabled)
                                    }),
                            ),
                    )
                    .when(services.network.connection.is_some(), |card| {
                        card.child(
                            Button::new("wifi-disconnect")
                                .label("Disconnect")
                                .outline()
                                .small()
                                .on_click(move |_, _, _| disconnect_controller.disconnect_wifi()),
                        )
                    }),
            )
            .child(
                h_flex()
                    .justify_between()
                    .child(section_label("AVAILABLE NETWORKS"))
                    .child(
                        Button::new("refresh-wifi")
                            .label("Scan")
                            .ghost()
                            .small()
                            .disabled(
                                !services.network.wifi_enabled || services.network.busy.is_some(),
                            )
                            .on_click(move |_, _, _| refresh_controller.refresh_wifi()),
                    ),
            )
            .when_some(services.network.busy.clone(), |column, busy| {
                column.child(muted_text(busy))
            })
            .children(networks)
            .when(
                services.network.wifi_enabled && services.network.networks.is_empty(),
                |column| column.child(muted_text("No Wi-Fi networks found. Try Scan.")),
            )
            .when_some(services.network.last_error.clone(), |column, error| {
                column.child(error_text(error))
            })
            .into_any_element()
    }

    fn bluetooth_content(&self, cx: &mut Context<Self>) -> AnyElement {
        let bluetooth = FoyerShellState::global(cx).services.bluetooth.clone();
        let controller = FoyerShellState::global(cx).service_controller.clone();
        if let Some(request) = bluetooth.pairing.clone() {
            let cancel_controller = controller.clone();
            let cancel_input = self.bluetooth_code.clone();
            let request_id = request.id;
            let content = match request.kind {
                foyer_shell_services::BluetoothPairingKind::PinCode => {
                    let submit_controller = controller.clone();
                    let input = self.bluetooth_code.clone();
                    control_card()
                        .child(muted_text(
                            "Enter the PIN shown by the device or its documentation.",
                        ))
                        .child(Input::new(&self.bluetooth_code))
                        .child(
                            Button::new("bluetooth-submit-pin")
                                .label("Continue")
                                .on_click(move |_, window, cx| {
                                    let value = input.read(cx).value().to_string();
                                    submit_controller.answer_bluetooth_pairing(request_id, value);
                                    input.update(cx, |input, cx| input.set_value("", window, cx));
                                }),
                        )
                        .into_any_element()
                }
                foyer_shell_services::BluetoothPairingKind::Passkey => {
                    let submit_controller = controller.clone();
                    let input = self.bluetooth_code.clone();
                    control_card()
                        .child(muted_text("Enter the six-digit passkey for this device."))
                        .child(Input::new(&self.bluetooth_code))
                        .child(
                            Button::new("bluetooth-submit-passkey")
                                .label("Continue")
                                .on_click(move |_, window, cx| {
                                    let value = input.read(cx).value().to_string();
                                    submit_controller.answer_bluetooth_pairing(request_id, value);
                                    input.update(cx, |input, cx| input.set_value("", window, cx));
                                }),
                        )
                        .into_any_element()
                }
                foyer_shell_services::BluetoothPairingKind::ConfirmPasskey(passkey) => {
                    let approve_controller = controller.clone();
                    let reject_controller = controller.clone();
                    control_card()
                        .child(control_heading(
                            format!("{passkey:06}"),
                            "Confirm on both devices",
                        ))
                        .child(muted_text(
                            "Make sure this number matches the one shown on the device.",
                        ))
                        .child(
                            h_flex()
                                .justify_end()
                                .gap_2()
                                .child(
                                    Button::new("bluetooth-reject-passkey")
                                        .label("Does not match")
                                        .outline()
                                        .on_click(move |_, _, _| {
                                            reject_controller
                                                .confirm_bluetooth_pairing(request_id, false)
                                        }),
                                )
                                .child(
                                    Button::new("bluetooth-confirm-passkey")
                                        .label("Matches")
                                        .on_click(move |_, _, _| {
                                            approve_controller
                                                .confirm_bluetooth_pairing(request_id, true)
                                        }),
                                ),
                        )
                        .into_any_element()
                }
                foyer_shell_services::BluetoothPairingKind::Authorize
                | foyer_shell_services::BluetoothPairingKind::AuthorizeService(_) => {
                    let approve_controller = controller.clone();
                    let reject_controller = controller.clone();
                    let detail = match &request.kind {
                        foyer_shell_services::BluetoothPairingKind::AuthorizeService(uuid) => {
                            format!("The device requested Bluetooth service {uuid}.")
                        }
                        _ => "The device requested permission to complete pairing.".into(),
                    };
                    control_card()
                        .child(muted_text(detail))
                        .child(
                            h_flex()
                                .justify_end()
                                .gap_2()
                                .child(
                                    Button::new("bluetooth-reject-authorization")
                                        .label("Reject")
                                        .outline()
                                        .on_click(move |_, _, _| {
                                            reject_controller
                                                .confirm_bluetooth_pairing(request_id, false)
                                        }),
                                )
                                .child(
                                    Button::new("bluetooth-approve-authorization")
                                        .label("Allow")
                                        .on_click(move |_, _, _| {
                                            approve_controller
                                                .confirm_bluetooth_pairing(request_id, true)
                                        }),
                                ),
                        )
                        .into_any_element()
                }
                foyer_shell_services::BluetoothPairingKind::DisplayPinCode(pin) => control_card()
                    .child(control_heading(pin, "Enter this PIN on the device"))
                    .into_any_element(),
                foyer_shell_services::BluetoothPairingKind::DisplayPasskey { passkey, entered } => {
                    control_card()
                        .child(control_heading(
                            format!("{passkey:06}"),
                            format!("{entered} digits entered"),
                        ))
                        .into_any_element()
                }
            };
            return content_column()
                .child(section_label("PAIRING REQUEST"))
                .child(control_heading(request.name, request.address))
                .child(content)
                .child(
                    Button::new("bluetooth-cancel-pairing")
                        .label("Cancel pairing")
                        .outline()
                        .on_click(move |_, window, cx| {
                            cancel_controller.cancel_bluetooth_pairing(request_id);
                            cancel_input.update(cx, |input, cx| input.set_value("", window, cx));
                        }),
                )
                .into_any_element();
        }
        let available = bluetooth.availability.is_available();
        let radio_controller = controller.clone();
        let refresh_controller = controller.clone();
        let devices = bluetooth.devices.iter().map(|device| {
            let action_controller = controller.clone();
            let remove_controller = controller.clone();
            let address = device.address.clone();
            let remove_address = address.clone();
            let connected = device.connected;
            let paired = device.paired;
            let detail = match (device.battery_percent, connected, paired) {
                (Some(percent), true, _) => format!("Connected · {percent}% battery"),
                (_, true, _) => "Connected".into(),
                (_, false, true) => "Paired".into(),
                _ => "Nearby".into(),
            };
            control_card()
                .gap_2()
                .child(control_heading(device.name.clone(), detail))
                .child(
                    h_flex()
                        .justify_end()
                        .gap_2()
                        .when(paired, |row| {
                            row.child(
                                Button::new(format!("bluetooth-remove-{remove_address}"))
                                    .label("Remove")
                                    .ghost()
                                    .small()
                                    .on_click(move |_, _, _| {
                                        remove_controller.remove_bluetooth(remove_address.clone())
                                    }),
                            )
                        })
                        .child(
                            Button::new(format!("bluetooth-action-{address}"))
                                .label(if connected {
                                    "Disconnect"
                                } else if paired {
                                    "Connect"
                                } else {
                                    "Pair"
                                })
                                .outline()
                                .small()
                                .on_click(move |_, _, _| {
                                    if connected {
                                        action_controller.disconnect_bluetooth(address.clone());
                                    } else if paired {
                                        action_controller.connect_bluetooth(address.clone());
                                    } else {
                                        action_controller.pair_bluetooth(address.clone());
                                    }
                                }),
                        ),
                )
        });
        content_column()
            .child(section_label("BLUETOOTH"))
            .child(
                control_card()
                    .child(
                        h_flex()
                            .justify_between()
                            .child(muted_text(if available {
                                if bluetooth.powered {
                                    "Bluetooth is on"
                                } else {
                                    "Bluetooth is off"
                                }
                            } else {
                                bluetooth.availability.detail()
                            }))
                            .child(
                                Switch::new("panel-bluetooth-radio")
                                    .small()
                                    .checked(bluetooth.powered)
                                    .disabled(!available)
                                    .on_click(move |enabled, _, _| {
                                        radio_controller.set_bluetooth_powered(*enabled)
                                    }),
                            ),
                    )
                    .child(
                        Button::new("refresh-bluetooth")
                            .label("Scan for devices")
                            .outline()
                            .small()
                            .disabled(!bluetooth.powered || bluetooth.busy.is_some())
                            .on_click(move |_, _, _| refresh_controller.refresh_bluetooth()),
                    ),
            )
            .when_some(bluetooth.busy.clone(), |column, busy| {
                column.child(muted_text(busy))
            })
            .child(section_label("DEVICES"))
            .children(devices)
            .when(
                bluetooth.powered && bluetooth.devices.is_empty(),
                |column| column.child(muted_text("No Bluetooth devices found. Start a scan.")),
            )
            .when_some(bluetooth.last_error.clone(), |column, error| {
                column.child(error_text(error))
            })
            .into_any_element()
    }

    fn display_content(&self, cx: &mut Context<Self>) -> AnyElement {
        let services = &FoyerShellState::global(cx).services;
        let available = services.brightness.availability.is_available();
        let device = services
            .brightness
            .device
            .clone()
            .unwrap_or_else(|| "Display backlight".into());
        let value = if available {
            format!(
                "{}%",
                self.pending_brightness
                    .map(|(percent, _)| percent)
                    .unwrap_or(services.brightness.percent)
            )
        } else {
            services.brightness.availability.detail().into()
        };
        let output_name = FoyerShellState::output_name(self.display_id, cx)
            .unwrap_or_else(|| "Focused output".into());
        let output = FoyerShellState::global(cx)
            .niri
            .outputs
            .iter()
            .find(|output| output.name == output_name)
            .cloned();
        content_column()
            .child(section_label("OUTPUT"))
            .child(
                control_card()
                    .child(control_heading(output_name, "Managed by Niri"))
                    .when_some(output, |card, output| {
                        card.child(status_row(
                            "Logical resolution",
                            format!("{} × {}", output.width, output.height),
                        ))
                        .child(status_row("Scale", format!("{}×", output.scale)))
                    }),
            )
            .child(section_label("BACKLIGHT"))
            .child(
                control_card().child(control_heading(device, value)).child(
                    Slider::new(&self.brightness_slider)
                        .disabled(!available || !services.brightness.can_set),
                ),
            )
            .when_some(services.brightness.last_error.clone(), |column, error| {
                column.child(error_text(error))
            })
            .into_any_element()
    }

    fn power_content(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if let Some(action) = self.pending_session_action {
            return self.confirmation(action, window, cx);
        }
        let state = FoyerShellState::global(cx);
        let session = state.services.session.clone();
        let battery = state.services.battery.clone();
        let niri_connected = state.niri.connected;
        let controller = state.service_controller.clone();
        content_column()
            .when(battery.present, |column| {
                column.child(section_label("BATTERY")).child(
                    control_card()
                        .child(control_heading(
                            format!("{}%", battery.percentage),
                            battery_state_label(&battery.state),
                        ))
                        .when_some(battery.time_remaining.clone(), |card, time| {
                            card.child(status_row("Remaining", time))
                        })
                        .when_some(battery.health_percent, |card, health| {
                            card.child(status_row("Battery health", format!("{health}%")))
                        })
                        .when_some(battery.energy_rate_watts, |card, watts| {
                            card.child(status_row("Power draw", format!("{watts:.1} W")))
                        }),
                )
            })
            .when(!battery.power_profiles.is_empty(), |column| {
                column
                    .child(section_label("POWER MODE"))
                    .child(v_flex().gap_2().children(battery.power_profiles.iter().map(
                        |profile| {
                            let controller = controller.clone();
                            let target = profile.clone();
                            selectable_row(
                                format!("power-profile-{profile}"),
                                IconName::Cpu,
                                power_profile_label(profile),
                                if battery.active_power_profile.as_deref() == Some(profile.as_str())
                                {
                                    "Current mode"
                                } else {
                                    "Switch power mode"
                                },
                                battery.active_power_profile.as_deref() == Some(profile.as_str()),
                            )
                            .on_click(move |_, _, _| controller.set_power_profile(target.clone()))
                        },
                    )))
            })
            .child(section_label("THIS SESSION"))
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        session_button(
                            "panel-lock",
                            Icon::new(FoyerShellIcon::Lock),
                            "Lock",
                            "Lock the session immediately",
                            session.lock_available,
                        )
                        .on_click(move |_, window, cx| {
                            let controller = controller.clone();
                            window.defer(cx, move |_, cx| {
                                close(cx);
                                controller.lock();
                            });
                        }),
                    )
                    .child(
                        session_button(
                            "panel-suspend",
                            Icon::new(IconName::Moon),
                            "Suspend",
                            "Sleep after confirmation",
                            session.suspend_available,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.pending_session_action = Some(SessionAction::Suspend);
                            cx.notify();
                        })),
                    )
                    .child(
                        session_button(
                            "panel-logout",
                            Icon::new(FoyerShellIcon::LogOut),
                            "Log out",
                            "End the current Niri session",
                            niri_connected,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.pending_session_action = Some(SessionAction::LogOut);
                            cx.notify();
                        })),
                    )
                    .child(
                        session_button(
                            "panel-restart",
                            Icon::new(FoyerShellIcon::Restart),
                            "Restart",
                            "Restart the machine",
                            session.restart_available,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.pending_session_action = Some(SessionAction::Restart);
                            cx.notify();
                        })),
                    )
                    .child(
                        session_button(
                            "panel-power-off",
                            Icon::new(FoyerShellIcon::Power),
                            "Power off",
                            "Shut down the machine",
                            session.power_off_available,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.pending_session_action = Some(SessionAction::PowerOff);
                            cx.notify();
                        })),
                    ),
            )
            .into_any_element()
    }

    fn confirmation(
        &mut self,
        action: SessionAction,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        content_column()
            .child(section_label("CONFIRM SESSION ACTION"))
            .child(
                control_card()
                    .child(
                        h_flex().gap_3().child(action.icon().large()).child(
                            v_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .text_lg()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(action.label()),
                                )
                                .child(muted_text(action.consequence())),
                        ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(tokens::SUBTLE))
                            .child("This local action cannot be undone from Foyer Shell."),
                    ),
            )
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("panel-power-back")
                            .icon(IconName::ArrowLeft)
                            .label("Back")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.pending_session_action = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("panel-power-confirm")
                            .icon(action.icon())
                            .label(format!("Confirm {}", action.label()))
                            .danger()
                            .on_click(move |_, window, cx| {
                                window.defer(cx, move |_, cx| {
                                    close(cx);
                                    execute_session_action(action, cx);
                                });
                            }),
                    ),
            )
            .into_any_element()
    }
}

impl Render for Panel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let services = FoyerShellState::global(cx).services.clone();
        self.sync_controls(&services, window, cx);
        let section = self.section;
        let content = self.content_for(section, window, cx);
        let content_stack = div().relative().flex_1().overflow_hidden().child(content);
        let header = h_flex()
            .h(px(72.0))
            .px_5()
            .gap_3()
            .border_b_1()
            .border_color(rgb(tokens::BORDER))
            .child(section.icon().large())
            .child(
                v_flex()
                    .gap_0p5()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(section.title()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(tokens::MUTED))
                            .child(section.description()),
                    ),
            );

        div()
            .id("foyer-shell-panel")
            .key_context("ShellPanel")
            .on_action(cx.listener(|_, _: &Close, window, cx| defer_close(window, cx)))
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.select_next(cx)))
            .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| this.select_previous(cx)))
            .on_action(cx.listener(|this, _: &Launch, window, cx| this.launch_selected(window, cx)))
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .overflow_hidden()
            .border_l_1()
            .border_color(rgb(tokens::BORDER))
            .bg(rgb(tokens::BACKGROUND))
            .text_color(rgb(tokens::FOREGROUND))
            .child(header)
            .child(content_stack)
    }
}

fn execute_session_action(action: SessionAction, cx: &mut App) {
    let controller = FoyerShellState::global(cx).service_controller.clone();
    match action {
        SessionAction::Suspend => controller.suspend(),
        SessionAction::LogOut => {
            std::thread::spawn(|| {
                if let Err(error) = foyer_shell_niri::quit() {
                    tracing::error!(%error, "failed to end the Niri session");
                }
            });
        }
        SessionAction::Restart => controller.restart(),
        SessionAction::PowerOff => controller.power_off(),
    }
}

fn notes_status_elements(snapshot: &foyer_shell_notes::Snapshot) -> Vec<AnyElement> {
    let mut children = vec![
        section_label("STATUS").into_any_element(),
        muted_text(if snapshot.using_powersync {
            "Reading the PowerSync replica."
        } else {
            foyer_shell_notes::powersync_status()
        })
        .into_any_element(),
    ];
    match snapshot.sync_banner() {
        Some(foyer_shell_notes::SyncBanner::Offline { pending }) => {
            children.push(error_text(if pending == 0 {
                "Offline. Reading the local replica. Changes will upload when Foyer Server is reachable."
                    .to_string()
            } else {
                format!(
                    "Offline. {pending} change(s) are queued and will upload when you are back online."
                )
            }).into_any_element());
        }
        Some(foyer_shell_notes::SyncBanner::Pending { pending }) => {
            children.push(
                muted_text(format!(
                    "Pending sync. {pending} change(s) are waiting to upload to Foyer Server."
                ))
                .into_any_element(),
            );
        }
        Some(foyer_shell_notes::SyncBanner::StaleRevision { message }) => {
            children.push(error_text(format!("Stale revision. {message}")).into_any_element());
        }
        Some(foyer_shell_notes::SyncBanner::Error { message }) => {
            children.push(error_text(format!("Couldn’t sync. {message}")).into_any_element());
        }
        None => {
            children.push(muted_text("Synced").into_any_element());
        }
    }
    children
}

fn markdown_preview_elements(source: &str) -> Vec<AnyElement> {
    foyer_shell_notes::markdown::markdown_blocks(source)
        .into_iter()
        .map(|block| match block {
            foyer_shell_notes::markdown::MarkdownBlock::Heading { text, .. } => div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .child(text)
                .into_any_element(),
            foyer_shell_notes::markdown::MarkdownBlock::ListItem(text) => {
                muted_text(format!("• {text}")).into_any_element()
            }
            foyer_shell_notes::markdown::MarkdownBlock::Code(text) => {
                muted_text(text).into_any_element()
            }
            foyer_shell_notes::markdown::MarkdownBlock::Paragraph(text) => {
                muted_text(text).into_any_element()
            }
        })
        .collect()
}

fn content_column() -> gpui_component::scroll::Scrollable<gpui::Div> {
    v_flex().size_full().overflow_y_scrollbar().p_5().gap_4()
}

fn control_card() -> gpui::Div {
    v_flex()
        .gap_4()
        .p_4()
        .rounded_lg()
        .border_1()
        .border_color(rgb(tokens::BORDER))
        .bg(rgb(tokens::SURFACE))
}

fn section_label(label: &'static str) -> gpui::Div {
    div()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(tokens::SUBTLE))
        .child(label)
}

fn muted_text(text: impl Into<gpui::SharedString>) -> gpui::Div {
    div()
        .text_sm()
        .text_color(rgb(tokens::MUTED))
        .child(text.into())
}

fn control_heading(
    title: impl Into<gpui::SharedString>,
    value: impl Into<gpui::SharedString>,
) -> gpui::Div {
    h_flex()
        .justify_between()
        .gap_3()
        .child(
            div()
                .flex_1()
                .overflow_hidden()
                .text_ellipsis()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .child(title.into()),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(tokens::SUBTLE))
                .child(value.into()),
        )
}

fn status_row(label: &'static str, value: impl Into<gpui::SharedString>) -> gpui::Div {
    h_flex()
        .justify_between()
        .gap_3()
        .child(muted_text(label))
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .child(value.into()),
        )
}

fn selectable_row(
    id: impl Into<gpui::ElementId>,
    icon: impl Into<Icon>,
    title: impl Into<gpui::SharedString>,
    description: impl Into<gpui::SharedString>,
    selected: bool,
) -> Button {
    let description = description.into();
    Button::new(id).ghost().w_full().child(
        h_flex()
            .w_full()
            .gap_3()
            .child(icon.into())
            .child(
                v_flex()
                    .flex_1()
                    .overflow_hidden()
                    .items_start()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_ellipsis()
                            .child(title.into()),
                    )
                    .when(!description.is_empty(), |column| {
                        column.child(
                            div()
                                .text_xs()
                                .text_color(rgb(tokens::SUBTLE))
                                .child(description),
                        )
                    }),
            )
            .when(selected, |row| row.child(Icon::new(IconName::Check))),
    )
}

fn error_text(error: impl Into<gpui::SharedString>) -> gpui::Div {
    div()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(rgb(tokens::FOREGROUND))
        .text_xs()
        .child(error.into())
}

fn wifi_detail(signal: u8, security: Option<&str>) -> String {
    match security {
        Some(security) => format!("{signal}% signal · {security}"),
        None => format!("{signal}% signal · Open network"),
    }
}

fn battery_state_label(state: &str) -> &'static str {
    match state {
        "charging" => "Charging",
        "discharging" => "On battery",
        "fully-charged" => "Fully charged",
        "pending-charge" => "Waiting to charge",
        "pending-discharge" => "Connected to power",
        _ => "Battery status",
    }
}

fn power_profile_label(profile: &str) -> &'static str {
    match profile {
        "power-saver" => "Power saver",
        "performance" => "Performance",
        _ => "Balanced",
    }
}

fn media_artwork(url: String) -> AnyElement {
    let image = if let Some(path) = url.strip_prefix("file://") {
        img(PathBuf::from(path.replace("%20", " ")))
    } else {
        img(url)
    };
    image
        .h(px(120.0))
        .w_full()
        .rounded_lg()
        .object_fit(ObjectFit::Cover)
        .into_any_element()
}

fn empty_state(
    icon: Icon,
    title: impl Into<gpui::SharedString>,
    description: impl Into<gpui::SharedString>,
) -> AnyElement {
    let title = title.into();
    let description = description.into();
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .px_8()
        .gap_3()
        .text_center()
        .child(
            div()
                .size(px(52.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded_lg()
                .bg(rgb(tokens::SURFACE_RAISED))
                .child(icon.large()),
        )
        .child(
            div()
                .text_base()
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(
            div()
                .max_w(px(320.0))
                .text_sm()
                .text_color(rgb(tokens::MUTED))
                .child(description),
        )
        .into_any_element()
}

fn notification_timestamp(received_at_ms: i64) -> String {
    DateTime::<Utc>::from_timestamp_millis(received_at_ms)
        .map(|timestamp| {
            timestamp
                .with_timezone(&Local)
                .format("%b %-d · %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "Unknown time".into())
}

fn activity_timestamp(created_at_ms: i64) -> String {
    DateTime::<Utc>::from_timestamp_millis(created_at_ms)
        .map(|timestamp| timestamp.with_timezone(&Local))
        .map(|timestamp| timestamp.format("%b %-d, %-H:%M").to_string())
        .unwrap_or_else(|| "Unknown time".into())
}

fn format_duration(duration_ms: u64) -> String {
    let total_seconds = duration_ms / 1_000;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    if minutes == 0 {
        format!("{seconds}s")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

pub(super) fn agenda_item_schedule(item: &foyer_shell_agenda::AgendaItem) -> String {
    let timestamp_ms = match item.kind {
        foyer_shell_agenda::ItemKind::Event => item.start_ms,
        foyer_shell_agenda::ItemKind::Task => item.due_ms,
    };
    let Some(timestamp) = timestamp_ms
        .and_then(DateTime::<Utc>::from_timestamp_millis)
        .map(|timestamp| timestamp.with_timezone(&Local))
    else {
        return match item.kind {
            foyer_shell_agenda::ItemKind::Event => "Time unavailable".into(),
            foyer_shell_agenda::ItemKind::Task => "No due date".into(),
        };
    };
    let day = if timestamp.date_naive() == Local::now().date_naive() {
        "Today".into()
    } else {
        timestamp.format("%a, %b %-d").to_string()
    };
    match item.kind {
        foyer_shell_agenda::ItemKind::Task => format!("Due {day}"),
        foyer_shell_agenda::ItemKind::Event if item.all_day => format!("{day} · All day"),
        foyer_shell_agenda::ItemKind::Event => {
            let start = timestamp.format("%-H:%M");
            let end = item
                .end_ms
                .and_then(DateTime::<Utc>::from_timestamp_millis)
                .map(|end| end.with_timezone(&Local).format("%-H:%M").to_string());
            match end {
                Some(end) => format!("{day} · {start}–{end}"),
                None => format!("{day} · {start}"),
            }
        }
    }
}

fn notification_history_body(body: &str) -> String {
    let mut text = body.chars().take(240).collect::<String>();
    if body.chars().count() > 240 {
        text.push('…');
    }
    text
}

fn session_button(
    id: &'static str,
    icon: Icon,
    label: &'static str,
    description: &'static str,
    available: bool,
) -> Button {
    Button::new(id)
        .icon(icon)
        .label(label)
        .tooltip(description)
        .outline()
        .disabled(!available)
}
