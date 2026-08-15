use std::sync::Arc;

use foyer_shell_ui::Root;
use gpui::{App, DisplayId, Entity, Global, Size, WindowHandle};

use crate::{
    applications::ApplicationEntry,
    notification::NotificationSurface,
    osd::OsdSurface,
    panel::{Panel, Section},
    toolbar::ToolbarTooltipSurface,
    tray_popover::TrayPopoverSurface,
    workspace::WorkspacePolicy,
};

pub struct ToolbarSurface {
    pub output: foyer_shell_niri::Output,
    pub display_id: DisplayId,
    pub handle: WindowHandle<Root>,
}

pub struct PanelSurface {
    pub section: Section,
    pub display_id: DisplayId,
    pub view: Entity<Panel>,
    pub handle: WindowHandle<Root>,
    pub closing: bool,
}

pub struct FoyerShellState {
    pub niri: foyer_shell_niri::Snapshot,
    pub workspace_policy: WorkspacePolicy,
    pub services: foyer_shell_services::Snapshot,
    pub services_initialized: bool,
    pub service_controller: foyer_shell_services::Controller,
    pub notification_controller: foyer_shell_services::notifications::Controller,
    pub notification_availability: foyer_shell_services::Availability,
    pub transcription_controller: foyer_shell_transcription::Controller,
    pub transcription: foyer_shell_transcription::Snapshot,
    pub last_handled_transcription_generation: u64,
    pub storage_controller: foyer_shell_storage::Controller,
    pub storage: foyer_shell_storage::Snapshot,
    pub agenda: foyer_shell_agenda::Snapshot,
    pub notes_controller: foyer_shell_notes::Controller,
    pub notes: foyer_shell_notes::Snapshot,
    pub tasks_controller: foyer_shell_tasks::Controller,
    pub tasks: foyer_shell_tasks::Snapshot,
    pub calendar_controller: foyer_shell_calendar::Controller,
    pub calendar: foyer_shell_calendar::Snapshot,
    pub contacts_controller: foyer_shell_contacts::Controller,
    pub contacts: foyer_shell_contacts::Snapshot,
    pub bookmarks_controller: foyer_shell_bookmarks::Controller,
    pub bookmarks: foyer_shell_bookmarks::Snapshot,
    pub applications: Arc<Vec<ApplicationEntry>>,
    pub toolbars: Vec<ToolbarSurface>,
    pub toolbar_tooltip_generation: u64,
    pub toolbar_tooltip_surface: Option<ToolbarTooltipSurface>,
    pub tray_popover_surface: Option<TrayPopoverSurface>,
    pub panel_surface: Option<PanelSurface>,
    pub pending_panel: Option<(Section, DisplayId)>,
    pub notification_surface: Option<NotificationSurface>,
    pub osd_surface: Option<OsdSurface>,
}

impl Global for FoyerShellState {}

impl FoyerShellState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        applications: Arc<Vec<ApplicationEntry>>,
        service_controller: foyer_shell_services::Controller,
        notification_controller: foyer_shell_services::notifications::Controller,
        transcription_controller: foyer_shell_transcription::Controller,
        storage_controller: foyer_shell_storage::Controller,
        notes_controller: foyer_shell_notes::Controller,
        tasks_controller: foyer_shell_tasks::Controller,
        calendar_controller: foyer_shell_calendar::Controller,
        contacts_controller: foyer_shell_contacts::Controller,
        bookmarks_controller: foyer_shell_bookmarks::Controller,
    ) -> Self {
        Self {
            niri: foyer_shell_niri::Snapshot::default(),
            workspace_policy: WorkspacePolicy::default(),
            services: foyer_shell_services::Snapshot::default(),
            services_initialized: false,
            service_controller,
            notification_controller,
            notification_availability: foyer_shell_services::Availability::Loading,
            transcription_controller,
            transcription: foyer_shell_transcription::Snapshot::default(),
            last_handled_transcription_generation: 0,
            storage_controller,
            storage: foyer_shell_storage::Snapshot::default(),
            agenda: foyer_shell_agenda::Snapshot::default(),
            notes_controller,
            notes: foyer_shell_notes::Snapshot::default(),
            tasks_controller,
            tasks: foyer_shell_tasks::Snapshot::default(),
            calendar_controller,
            calendar: foyer_shell_calendar::Snapshot::default(),
            contacts_controller,
            contacts: foyer_shell_contacts::Snapshot::default(),
            bookmarks_controller,
            bookmarks: foyer_shell_bookmarks::Snapshot::default(),
            applications,
            toolbars: Vec::new(),
            toolbar_tooltip_generation: 0,
            toolbar_tooltip_surface: None,
            tray_popover_surface: None,
            panel_surface: None,
            pending_panel: None,
            notification_surface: None,
            osd_surface: None,
        }
    }

    pub fn global(cx: &App) -> &Self {
        cx.global::<Self>()
    }

    pub fn global_mut(cx: &mut App) -> &mut Self {
        cx.global_mut::<Self>()
    }

    pub fn output_name(display_id: DisplayId, cx: &App) -> Option<String> {
        let display_uuid = cx.find_display(display_id)?.uuid().ok()?;
        Self::global(cx)
            .niri
            .outputs
            .iter()
            .find(|output| {
                uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_DNS, output.name.as_bytes())
                    == display_uuid
            })
            .map(|output| output.name.clone())
    }

    pub fn display_id_for_output(output_name: &str, cx: &App) -> Option<DisplayId> {
        let output_uuid = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_DNS, output_name.as_bytes());
        cx.displays()
            .into_iter()
            .find(|display| display.uuid().ok() == Some(output_uuid))
            .map(|display| display.id())
    }

    pub fn focused_display_id(cx: &App) -> Option<DisplayId> {
        let output_name = Self::global(cx)
            .niri
            .workspaces
            .iter()
            .find(|workspace| workspace.focused)
            .and_then(|workspace| workspace.output.as_deref());

        output_name
            .and_then(|output_name| Self::display_id_for_output(output_name, cx))
            .or_else(|| cx.displays().first().map(|display| display.id()))
    }

    pub fn display_size(display_id: DisplayId, cx: &App) -> Option<Size<gpui::Pixels>> {
        Self::output_name(display_id, cx)
            .and_then(|name| {
                Self::global(cx)
                    .niri
                    .outputs
                    .iter()
                    .find(|output| output.name == name)
                    .map(|output| {
                        Size::new(
                            gpui::px(output.width as f32),
                            gpui::px(output.height as f32),
                        )
                    })
            })
            .or_else(|| {
                cx.find_display(display_id)
                    .map(|display| display.bounds().size)
            })
    }
}
