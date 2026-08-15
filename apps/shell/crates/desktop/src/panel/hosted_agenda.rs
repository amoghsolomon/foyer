use gpui::{AnyElement, Context, FontWeight, SharedString, Window, div, prelude::*};
use gpui_component::{
    Disableable, Icon, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, Textarea},
    switch::Switch,
};

use super::{
    AgendaMode, Panel,
    chrome::{
        ReplicaBanner, content_column, control_card, empty_state, error_text, muted_text,
        replica_status_elements, section_label,
    },
};
use crate::state::FoyerShellState;

impl Panel {
    pub(super) fn hosted_agenda_content(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let snapshot = FoyerShellState::global(cx).calendar.clone();
        let controller = FoyerShellState::global(cx).calendar_controller.clone();
        if let foyer_shell_calendar::Availability::Unavailable(error) = &snapshot.availability {
            let refresh = controller.clone();
            return content_column()
                .child(section_label("AGENDA"))
                .child(error_text(error.clone()))
                .child(muted_text(
                    foyer_shell_calendar::Availability::Loading.detail(),
                ))
                .child(
                    Button::new("agenda-refresh-unavailable")
                        .label("Try again")
                        .outline()
                        .on_click(move |_, _, _| refresh.refresh()),
                )
                .into_any_element();
        }
        if matches!(
            snapshot.availability,
            foyer_shell_calendar::Availability::Loading
        ) {
            return empty_state(
                Icon::new(IconName::Calendar),
                "Loading your agenda",
                "Connecting to Foyer Server…",
            );
        }

        if let Some(event_id) = self.agenda_event_id.clone()
            && let Some(event) = snapshot.event(&event_id).cloned()
        {
            return self.agenda_event_content(event, snapshot, controller, cx);
        }

        match self.agenda_mode {
            AgendaMode::RenameCalendar => self.agenda_rename_calendar(snapshot, controller, cx),
            AgendaMode::ConfirmDeleteCalendar => {
                self.agenda_delete_calendar(snapshot, controller, cx)
            }
            _ => self.agenda_browse(snapshot, controller, cx),
        }
    }

    fn agenda_browse(
        &mut self,
        snapshot: foyer_shell_calendar::Snapshot,
        controller: foyer_shell_calendar::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let agenda = FoyerShellState::global(cx).agenda.clone();
        let storage = FoyerShellState::global(cx).storage_controller.clone();
        let selected = self.agenda_calendar_id.clone().or_else(|| {
            snapshot
                .selected_calendar()
                .map(|calendar| calendar.id.clone())
        });
        let create_controller = controller.clone();
        let title_input = self.agenda_title.clone();
        let start_input = self.agenda_start.clone();
        let location_input = self.agenda_location.clone();
        let description_input = self.agenda_description.clone();
        let calendar_for_create = selected.clone();

        content_column()
            .children(calendar_status(&snapshot))
            .child(
                h_flex()
                    .justify_between()
                    .child(section_label("CALENDARS"))
                    .child(
                        Button::new("agenda-refresh")
                            .label("Refresh")
                            .ghost()
                            .small()
                            .on_click({
                                let refresh = controller.clone();
                                move |_, _, _| refresh.refresh()
                            }),
                    ),
            )
            .children(
                snapshot
                    .calendars
                    .iter()
                    .enumerate()
                    .map(|(index, calendar)| {
                        let calendar = calendar.clone();
                        let selected_here = selected.as_deref() == Some(calendar.id.as_str());
                        let visible = agenda
                            .sources
                            .iter()
                            .find(|source| source.id == calendar.id)
                            .map(|source| source.visible)
                            .unwrap_or(true);
                        let select_controller = controller.clone();
                        let visibility = storage.clone();
                        let calendar_id = calendar.id.clone();
                        let visible_id = calendar.id.clone();
                        let rename_calendar = calendar.clone();
                        let delete_empty = snapshot.events_in(Some(&calendar.id)).is_empty();
                        control_card()
                            .child(
                                h_flex()
                                    .justify_between()
                                    .gap_3()
                                    .child(
                                        Button::new(SharedString::from(format!(
                                            "agenda-cal-{}",
                                            calendar.id
                                        )))
                                        .ghost()
                                        .label(calendar.display_name.clone())
                                        .when(selected_here, |button| button.primary())
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.agenda_calendar_id = Some(calendar_id.clone());
                                                this.agenda_mode = AgendaMode::Browse;
                                                select_controller
                                                    .select_calendar(Some(calendar_id.clone()));
                                                cx.notify();
                                            }),
                                        ),
                                    )
                                    .child(
                                        Switch::new(("agenda-calendar-visible", index))
                                            .small()
                                            .checked(visible)
                                            .on_click(move |checked, _, _| {
                                                visibility.set_agenda_source_visible(
                                                    visible_id.clone(),
                                                    *checked,
                                                )
                                            }),
                                    ),
                            )
                            .when(selected_here, |card| {
                                card.child(
                                    h_flex()
                                        .gap_2()
                                        .child(
                                            Button::new("agenda-rename-calendar")
                                                .label("Rename")
                                                .outline()
                                                .small()
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.agenda_mode =
                                                            AgendaMode::RenameCalendar;
                                                        let name =
                                                            rename_calendar.display_name.clone();
                                                        this.agenda_title.update(
                                                            cx,
                                                            |input, cx| {
                                                                input.set_value(name, window, cx)
                                                            },
                                                        );
                                                        cx.notify();
                                                    },
                                                )),
                                        )
                                        .child(
                                            Button::new("agenda-delete-calendar")
                                                .label("Delete")
                                                .outline()
                                                .small()
                                                .disabled(!delete_empty)
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.agenda_mode =
                                                        AgendaMode::ConfirmDeleteCalendar;
                                                    cx.notify();
                                                })),
                                        ),
                                )
                            })
                    }),
            )
            .child(section_label("UPCOMING"))
            .children(
                agenda
                    .items
                    .iter()
                    .filter(|item| item.kind == foyer_shell_agenda::ItemKind::Event)
                    .take(80)
                    .map(|item| {
                        let event_id = item.id.split(':').next().unwrap_or(&item.id).to_string();
                        control_card()
                            .id(SharedString::from(format!("agenda-event-{}", item.id)))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.agenda_event_id = Some(event_id.clone());
                                this.agenda_mode = AgendaMode::Browse;
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(gpui::rgb(foyer_shell_ui::tokens::SUBTLE))
                                    .child(super::agenda_item_schedule(item)),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(item.title.clone()),
                            )
                    }),
            )
            .child(section_label("CREATE"))
            .child(Input::new(&self.agenda_title).cleanable(true))
            .child(Input::new(&self.agenda_start).cleanable(true))
            .child(Input::new(&self.agenda_location).cleanable(true))
            .child(Textarea::new(&self.agenda_description))
            .child(
                Button::new("agenda-create-calendar")
                    .label("New calendar")
                    .outline()
                    .on_click({
                        let controller = create_controller.clone();
                        let input = title_input.clone();
                        move |_, window, cx| {
                            let name = input.read(cx).value().trim().to_string();
                            if !name.is_empty() {
                                controller.create_calendar(name, String::new(), None);
                                input.update(cx, |input, cx| input.set_value("", window, cx));
                            }
                        }
                    }),
            )
            .child(
                Button::new("agenda-create-event")
                    .label("New event")
                    .primary()
                    .disabled(calendar_for_create.is_none())
                    .on_click({
                        let controller = create_controller;
                        move |_, window, cx| {
                            let Some(calendar_id) = calendar_for_create.clone() else {
                                return;
                            };
                            let summary = title_input.read(cx).value().trim().to_string();
                            let start = start_input.read(cx).value().trim().replace('-', "");
                            if summary.is_empty() || start.len() != 8 {
                                return;
                            }
                            controller.create_event(foyer_shell_calendar::EventDraft {
                                summary,
                                description: description_input.read(cx).value().to_string(),
                                location: location_input.read(cx).value().trim().to_string(),
                                all_day: true,
                                dtstart: start,
                                dtend: None,
                                tzid: None,
                                rrule: None,
                                exdates: Vec::new(),
                                calendar_id,
                            });
                            title_input.update(cx, |input, cx| input.set_value("", window, cx));
                            start_input.update(cx, |input, cx| input.set_value("", window, cx));
                            location_input.update(cx, |input, cx| input.set_value("", window, cx));
                            description_input
                                .update(cx, |input, cx| input.set_value("", window, cx));
                        }
                    }),
            )
            .into_any_element()
    }

    fn agenda_rename_calendar(
        &mut self,
        snapshot: foyer_shell_calendar::Snapshot,
        controller: foyer_shell_calendar::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(calendar) = self
            .agenda_calendar_id
            .as_deref()
            .and_then(|id| snapshot.calendar(id))
            .cloned()
        else {
            self.agenda_mode = AgendaMode::Browse;
            return self.agenda_browse(snapshot, controller, cx);
        };
        let title_input = self.agenda_title.clone();
        content_column()
            .children(calendar_status(&snapshot))
            .child(section_label("RENAME CALENDAR"))
            .child(Input::new(&self.agenda_title).cleanable(true))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("agenda-rename-cancel")
                            .label("Cancel")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.agenda_mode = AgendaMode::Browse;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("agenda-rename-save")
                            .label("Save")
                            .primary()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let name = title_input.read(cx).value().trim().to_string();
                                if !name.is_empty() {
                                    controller.rename_calendar(
                                        calendar.id.clone(),
                                        calendar.revision,
                                        calendar.etag.clone(),
                                        name,
                                    );
                                    this.agenda_mode = AgendaMode::Browse;
                                    cx.notify();
                                }
                            })),
                    ),
            )
            .into_any_element()
    }

    fn agenda_delete_calendar(
        &mut self,
        snapshot: foyer_shell_calendar::Snapshot,
        controller: foyer_shell_calendar::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(calendar) = self
            .agenda_calendar_id
            .as_deref()
            .and_then(|id| snapshot.calendar(id))
            .cloned()
        else {
            self.agenda_mode = AgendaMode::Browse;
            return self.agenda_browse(snapshot, controller, cx);
        };
        content_column()
            .children(calendar_status(&snapshot))
            .child(section_label("DELETE CALENDAR"))
            .child(error_text(format!(
                "Delete “{}”? This cannot be undone after it syncs.",
                calendar.display_name
            )))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("agenda-delete-calendar-cancel")
                            .label("Cancel")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.agenda_mode = AgendaMode::Browse;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("agenda-delete-calendar-confirm")
                            .label("Delete calendar")
                            .primary()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                controller.delete_calendar(
                                    calendar.id.clone(),
                                    calendar.revision,
                                    calendar.etag.clone(),
                                );
                                this.agenda_calendar_id = None;
                                this.agenda_mode = AgendaMode::Browse;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    fn agenda_event_content(
        &mut self,
        event: foyer_shell_calendar::Event,
        snapshot: foyer_shell_calendar::Snapshot,
        controller: foyer_shell_calendar::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.agenda_mode == AgendaMode::EditEvent {
            return self.agenda_edit_event(event, snapshot, controller, cx);
        }
        if self.agenda_mode == AgendaMode::ConfirmDeleteEvent {
            return self.agenda_delete_event(event, snapshot, controller, cx);
        }
        let calendar_name = snapshot
            .calendar(&event.calendar_id)
            .map(|calendar| calendar.display_name.clone())
            .unwrap_or_else(|| "Calendar".into());
        content_column()
            .children(calendar_status(&snapshot))
            .child(
                Button::new("agenda-back-event")
                    .label("Back")
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.agenda_event_id = None;
                        this.agenda_mode = AgendaMode::Browse;
                        cx.notify();
                    })),
            )
            .child(section_label("EVENT"))
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::MEDIUM)
                    .child(event.summary.clone()),
            )
            .child(muted_text(foyer_shell_calendar::format_when(&event)))
            .child(muted_text(format!("Calendar · {calendar_name}")))
            .when(!event.location.is_empty(), |column| {
                column.child(muted_text(event.location.clone()))
            })
            .when(!event.description.is_empty(), |column| {
                column.child(muted_text(event.description.clone()))
            })
            .when(event.is_recurring(), |column| {
                column.child(muted_text(foyer_shell_calendar::recurrence_summary(
                    event.rrule.as_deref(),
                )))
            })
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("agenda-edit-event")
                            .label("Edit")
                            .primary()
                            .on_click(cx.listener({
                                let event = event.clone();
                                move |this, _, window, cx| {
                                    this.agenda_mode = AgendaMode::EditEvent;
                                    this.agenda_title.update(cx, |input, cx| {
                                        input.set_value(event.summary.clone(), window, cx)
                                    });
                                    this.agenda_start.update(cx, |input, cx| {
                                        input.set_value(event.dtstart.clone(), window, cx)
                                    });
                                    this.agenda_location.update(cx, |input, cx| {
                                        input.set_value(event.location.clone(), window, cx)
                                    });
                                    this.agenda_description.update(cx, |input, cx| {
                                        input.set_value(event.description.clone(), window, cx)
                                    });
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        Button::new("agenda-delete-event")
                            .label("Delete")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.agenda_mode = AgendaMode::ConfirmDeleteEvent;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    fn agenda_edit_event(
        &mut self,
        event: foyer_shell_calendar::Event,
        snapshot: foyer_shell_calendar::Snapshot,
        controller: foyer_shell_calendar::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let title_input = self.agenda_title.clone();
        let start_input = self.agenda_start.clone();
        let location_input = self.agenda_location.clone();
        let description_input = self.agenda_description.clone();
        content_column()
            .children(calendar_status(&snapshot))
            .child(section_label("EDIT EVENT"))
            .child(Input::new(&self.agenda_title).cleanable(true))
            .child(Input::new(&self.agenda_start).cleanable(true))
            .child(Input::new(&self.agenda_location).cleanable(true))
            .child(Textarea::new(&self.agenda_description))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("agenda-edit-cancel")
                            .label("Cancel")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.agenda_mode = AgendaMode::Browse;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("agenda-edit-save")
                            .label("Save")
                            .primary()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let summary = title_input.read(cx).value().trim().to_string();
                                let mut dtstart = start_input.read(cx).value().trim().to_string();
                                if dtstart.contains('-') && dtstart.len() == 10 {
                                    dtstart = dtstart.replace('-', "");
                                }
                                if !summary.is_empty() && !dtstart.is_empty() {
                                    controller.update_event(
                                        event.id.clone(),
                                        event.revision,
                                        event.etag.clone(),
                                        foyer_shell_calendar::EventDraft {
                                            summary,
                                            description: description_input
                                                .read(cx)
                                                .value()
                                                .to_string(),
                                            location: location_input
                                                .read(cx)
                                                .value()
                                                .trim()
                                                .to_string(),
                                            all_day: event.all_day || dtstart.len() == 8,
                                            dtstart,
                                            dtend: event.dtend.clone(),
                                            tzid: event.tzid.clone(),
                                            rrule: event.rrule.clone(),
                                            exdates: foyer_shell_calendar::parse_exdates(
                                                &event.exdates,
                                            ),
                                            calendar_id: event.calendar_id.clone(),
                                        },
                                    );
                                    this.agenda_mode = AgendaMode::Browse;
                                    cx.notify();
                                }
                            })),
                    ),
            )
            .into_any_element()
    }

    fn agenda_delete_event(
        &mut self,
        event: foyer_shell_calendar::Event,
        snapshot: foyer_shell_calendar::Snapshot,
        controller: foyer_shell_calendar::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        content_column()
            .children(calendar_status(&snapshot))
            .child(section_label("DELETE EVENT"))
            .child(error_text(format!(
                "Delete “{}”? This cannot be undone after it syncs.",
                event.summary
            )))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("agenda-delete-event-cancel")
                            .label("Cancel")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.agenda_mode = AgendaMode::Browse;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("agenda-delete-event-confirm")
                            .label("Delete event")
                            .primary()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                controller.delete_event(
                                    event.id.clone(),
                                    event.revision,
                                    event.etag.clone(),
                                );
                                this.agenda_event_id = None;
                                this.agenda_mode = AgendaMode::Browse;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }
}

fn calendar_status(snapshot: &foyer_shell_calendar::Snapshot) -> Vec<AnyElement> {
    replica_status_elements(
        snapshot.using_powersync,
        foyer_shell_notes::powersync_status(),
        snapshot.sync_banner().map(|banner| match banner {
            foyer_shell_calendar::SyncBanner::Offline { pending } => {
                ReplicaBanner::Offline { pending }
            }
            foyer_shell_calendar::SyncBanner::Pending { pending } => {
                ReplicaBanner::Pending { pending }
            }
            foyer_shell_calendar::SyncBanner::StaleEtag { message } => {
                ReplicaBanner::Stale { message }
            }
            foyer_shell_calendar::SyncBanner::Error { message } => ReplicaBanner::Error { message },
        }),
    )
}
