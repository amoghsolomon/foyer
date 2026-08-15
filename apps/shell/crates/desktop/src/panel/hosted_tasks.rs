use foyer_shell_ui::FoyerShellIcon;
use gpui::{AnyElement, Context, FontWeight, SharedString, Window, div, prelude::*};
use gpui_component::{
    Disableable, Icon, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, Textarea},
};

use super::{
    Panel, TasksMode,
    chrome::{
        ReplicaBanner, content_column, control_card, empty_state, error_text, muted_text,
        replica_status_elements, section_label,
    },
};
use crate::state::FoyerShellState;

impl Panel {
    pub(super) fn hosted_tasks_content(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let snapshot = FoyerShellState::global(cx).tasks.clone();
        let controller = FoyerShellState::global(cx).tasks_controller.clone();
        if let foyer_shell_tasks::Availability::Unavailable(error) = &snapshot.availability {
            let refresh = controller.clone();
            return content_column()
                .child(section_label("TASKS"))
                .child(error_text(error.clone()))
                .child(
                    Button::new("tasks-refresh-unavailable")
                        .label("Try again")
                        .outline()
                        .on_click(move |_, _, _| refresh.refresh()),
                )
                .into_any_element();
        }
        if matches!(
            snapshot.availability,
            foyer_shell_tasks::Availability::Loading
        ) {
            return empty_state(
                Icon::new(FoyerShellIcon::Tasks),
                "Loading your tasks",
                "Connecting to Foyer Server…",
            );
        }

        if let Some(task_id) = self.tasks_task_id.clone()
            && let Some(task) = snapshot.task(&task_id).cloned()
        {
            return self.task_detail(task, snapshot, controller, cx);
        }

        match self.tasks_mode {
            TasksMode::RenameList => self.task_rename_list(snapshot, controller, cx),
            TasksMode::ConfirmDeleteList => self.task_delete_list(snapshot, controller, cx),
            _ => self.tasks_browse(snapshot, controller, cx),
        }
    }

    fn tasks_browse(
        &mut self,
        snapshot: foyer_shell_tasks::Snapshot,
        controller: foyer_shell_tasks::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self
            .tasks_list_id
            .clone()
            .or_else(|| snapshot.lists.first().map(|list| list.id.clone()));
        let current = selected
            .as_deref()
            .and_then(|id| snapshot.list(id))
            .cloned();
        let open = selected
            .as_deref()
            .map(|id| snapshot.open_tasks_in(id))
            .unwrap_or_else(|| snapshot.open_tasks());
        let completed = selected
            .as_deref()
            .map(|id| snapshot.completed_tasks_in(id))
            .unwrap_or_default();
        let title_input = self.tasks_title.clone();
        let body_input = self.tasks_body.clone();
        let due_input = self.tasks_due.clone();
        let create_controller = controller.clone();
        let list_for_create = selected.clone();

        content_column()
            .children(tasks_status(&snapshot))
            .child(
                h_flex()
                    .justify_between()
                    .child(section_label("LISTS"))
                    .child(
                        Button::new("tasks-refresh")
                            .label("Refresh")
                            .ghost()
                            .small()
                            .on_click({
                                let refresh = controller.clone();
                                move |_, _, _| refresh.refresh()
                            }),
                    ),
            )
            .children(snapshot.lists.iter().map(|list| {
                let list_id = list.id.clone();
                let selected_here = selected.as_deref() == Some(list.id.as_str());
                control_card()
                    .id(SharedString::from(format!("tasks-list-{}", list.id)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.tasks_list_id = Some(list_id.clone());
                        this.tasks_task_id = None;
                        this.tasks_mode = TasksMode::Browse;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(list.name.clone()),
                    )
                    .when(selected_here, |card| card.child(muted_text("Selected")))
            }))
            .when_some(current.clone(), |column, list| {
                let empty = snapshot.list_is_empty(&list.id);
                column.child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("tasks-rename-list")
                                .label("Rename")
                                .outline()
                                .small()
                                .on_click(cx.listener({
                                    let name = list.name.clone();
                                    move |this, _, window, cx| {
                                        this.tasks_mode = TasksMode::RenameList;
                                        this.tasks_title.update(cx, |input, cx| {
                                            input.set_value(name.clone(), window, cx)
                                        });
                                        cx.notify();
                                    }
                                })),
                        )
                        .child(
                            Button::new("tasks-delete-list")
                                .label("Delete")
                                .outline()
                                .small()
                                .disabled(!empty)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.tasks_mode = TasksMode::ConfirmDeleteList;
                                    cx.notify();
                                })),
                        ),
                )
            })
            .child(section_label("OPEN TASKS"))
            .children(open.into_iter().map(|task| {
                let task_id = task.id.clone();
                control_card()
                    .id(SharedString::from(format!("tasks-row-{}", task.id)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.tasks_task_id = Some(task_id.clone());
                        this.tasks_mode = TasksMode::Browse;
                        cx.notify();
                    }))
                    .child(div().text_sm().child(task.title.clone()))
                    .when_some(task.due.as_ref(), |card, due| {
                        card.child(muted_text(due.display_label()))
                    })
            }))
            .child(section_label("COMPLETED"))
            .children(completed.into_iter().map(|task| {
                let task_id = task.id.clone();
                control_card()
                    .id(SharedString::from(format!("tasks-done-{}", task.id)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.tasks_task_id = Some(task_id.clone());
                        this.tasks_mode = TasksMode::Browse;
                        cx.notify();
                    }))
                    .child(div().text_sm().child(task.title.clone()))
            }))
            .child(section_label("CREATE"))
            .child(Input::new(&self.tasks_title).cleanable(true))
            .child(Input::new(&self.tasks_due).cleanable(true))
            .child(Textarea::new(&self.tasks_body))
            .child(
                Button::new("tasks-create-list")
                    .label("New list")
                    .outline()
                    .on_click({
                        let controller = create_controller.clone();
                        let input = title_input.clone();
                        move |_, window, cx| {
                            let name = input.read(cx).value().trim().to_string();
                            if !name.is_empty() {
                                controller.create_list(name);
                                input.update(cx, |input, cx| input.set_value("", window, cx));
                            }
                        }
                    }),
            )
            .child(
                Button::new("tasks-create-task")
                    .label("New task")
                    .primary()
                    .disabled(list_for_create.is_none())
                    .on_click({
                        let controller = create_controller;
                        move |_, window, cx| {
                            let Some(list_id) = list_for_create.clone() else {
                                return;
                            };
                            let title = title_input.read(cx).value().trim().to_string();
                            if title.is_empty() {
                                return;
                            }
                            let due = foyer_shell_tasks::Due::parse(
                                due_input.read(cx).value().trim(),
                                None,
                                true,
                            );
                            controller.create_task(
                                list_id,
                                title,
                                body_input.read(cx).value().to_string(),
                                due,
                                0,
                            );
                            title_input.update(cx, |input, cx| input.set_value("", window, cx));
                            due_input.update(cx, |input, cx| input.set_value("", window, cx));
                            body_input.update(cx, |input, cx| input.set_value("", window, cx));
                        }
                    }),
            )
            .into_any_element()
    }

    fn task_rename_list(
        &mut self,
        snapshot: foyer_shell_tasks::Snapshot,
        controller: foyer_shell_tasks::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(list) = self
            .tasks_list_id
            .as_deref()
            .and_then(|id| snapshot.list(id))
            .cloned()
        else {
            self.tasks_mode = TasksMode::Browse;
            return self.tasks_browse(snapshot, controller, cx);
        };
        let title_input = self.tasks_title.clone();
        content_column()
            .children(tasks_status(&snapshot))
            .child(section_label("RENAME LIST"))
            .child(Input::new(&self.tasks_title).cleanable(true))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("tasks-rename-cancel")
                            .label("Cancel")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.tasks_mode = TasksMode::Browse;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("tasks-rename-save")
                            .label("Save")
                            .primary()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let name = title_input.read(cx).value().trim().to_string();
                                if !name.is_empty() {
                                    controller.rename_list(list.id.clone(), list.revision, name);
                                    this.tasks_mode = TasksMode::Browse;
                                    cx.notify();
                                }
                            })),
                    ),
            )
            .into_any_element()
    }

    fn task_delete_list(
        &mut self,
        snapshot: foyer_shell_tasks::Snapshot,
        controller: foyer_shell_tasks::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(list) = self
            .tasks_list_id
            .as_deref()
            .and_then(|id| snapshot.list(id))
            .cloned()
        else {
            self.tasks_mode = TasksMode::Browse;
            return self.tasks_browse(snapshot, controller, cx);
        };
        content_column()
            .children(tasks_status(&snapshot))
            .child(section_label("DELETE LIST"))
            .child(error_text(format!(
                "Delete “{}”? This cannot be undone after it syncs.",
                list.name
            )))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("tasks-delete-list-cancel")
                            .label("Cancel")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.tasks_mode = TasksMode::Browse;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("tasks-delete-list-confirm")
                            .label("Delete list")
                            .primary()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                controller.delete_list(list.id.clone(), list.revision);
                                this.tasks_list_id = None;
                                this.tasks_mode = TasksMode::Browse;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    fn task_detail(
        &mut self,
        task: foyer_shell_tasks::Task,
        snapshot: foyer_shell_tasks::Snapshot,
        controller: foyer_shell_tasks::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.tasks_mode == TasksMode::EditTask {
            return self.task_edit(task, snapshot, controller, cx);
        }
        if self.tasks_mode == TasksMode::MoveTask {
            return self.task_move(task, snapshot, controller, cx);
        }
        if self.tasks_mode == TasksMode::ConfirmDeleteTask {
            return self.task_delete(task, snapshot, controller, cx);
        }
        let list_name = snapshot
            .list(&task.list_id)
            .map(|list| list.name.clone())
            .unwrap_or_else(|| "List".into());
        let complete_controller = controller.clone();
        let complete_task = task.clone();
        content_column()
            .children(tasks_status(&snapshot))
            .child(
                Button::new("tasks-back")
                    .label("Back")
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tasks_task_id = None;
                        this.tasks_mode = TasksMode::Browse;
                        cx.notify();
                    })),
            )
            .child(section_label("TASK"))
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::MEDIUM)
                    .child(task.title.clone()),
            )
            .child(muted_text(format!(
                "{list_name} · {}",
                task.priority_label()
            )))
            .when_some(task.due.clone(), |column, due| {
                column.child(muted_text(format!("Due {}", due.display_label())))
            })
            .when(!task.description.is_empty(), |column| {
                column.child(muted_text(task.summary()))
            })
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("tasks-toggle-complete")
                            .label(if task.completed { "Reopen" } else { "Complete" })
                            .primary()
                            .on_click(move |_, _, _| {
                                if complete_task.completed {
                                    complete_controller.reopen_task(
                                        complete_task.id.clone(),
                                        complete_task.revision,
                                    );
                                } else {
                                    complete_controller.complete_task(
                                        complete_task.id.clone(),
                                        complete_task.revision,
                                    );
                                }
                            }),
                    )
                    .child(
                        Button::new("tasks-edit")
                            .label("Edit")
                            .outline()
                            .on_click(cx.listener({
                                let task = task.clone();
                                move |this, _, window, cx| {
                                    this.tasks_mode = TasksMode::EditTask;
                                    this.tasks_title.update(cx, |input, cx| {
                                        input.set_value(task.title.clone(), window, cx)
                                    });
                                    this.tasks_body.update(cx, |input, cx| {
                                        input.set_value(task.description.clone(), window, cx)
                                    });
                                    this.tasks_due.update(cx, |input, cx| {
                                        input.set_value(
                                            task.due
                                                .as_ref()
                                                .map(|due| due.local.clone())
                                                .unwrap_or_default(),
                                            window,
                                            cx,
                                        )
                                    });
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        Button::new("tasks-move")
                            .label("Move")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.tasks_mode = TasksMode::MoveTask;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("tasks-delete")
                            .label("Delete")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.tasks_mode = TasksMode::ConfirmDeleteTask;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    fn task_edit(
        &mut self,
        task: foyer_shell_tasks::Task,
        snapshot: foyer_shell_tasks::Snapshot,
        controller: foyer_shell_tasks::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let title_input = self.tasks_title.clone();
        let body_input = self.tasks_body.clone();
        let due_input = self.tasks_due.clone();
        content_column()
            .children(tasks_status(&snapshot))
            .child(section_label("EDIT TASK"))
            .child(Input::new(&self.tasks_title).cleanable(true))
            .child(Input::new(&self.tasks_due).cleanable(true))
            .child(Textarea::new(&self.tasks_body))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("tasks-edit-cancel")
                            .label("Cancel")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.tasks_mode = TasksMode::Browse;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("tasks-edit-save")
                            .label("Save")
                            .primary()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let title = title_input.read(cx).value().trim().to_string();
                                if title.is_empty() {
                                    return;
                                }
                                controller.update_task(
                                    task.id.clone(),
                                    task.revision,
                                    title,
                                    body_input.read(cx).value().to_string(),
                                    foyer_shell_tasks::Due::parse(
                                        due_input.read(cx).value().trim(),
                                        None,
                                        true,
                                    ),
                                    task.priority,
                                    task.position,
                                );
                                this.tasks_mode = TasksMode::Browse;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    fn task_move(
        &mut self,
        task: foyer_shell_tasks::Task,
        snapshot: foyer_shell_tasks::Snapshot,
        controller: foyer_shell_tasks::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        content_column()
            .children(tasks_status(&snapshot))
            .child(section_label("MOVE TASK"))
            .child(muted_text(task.title.clone()))
            .child(
                Button::new("tasks-move-cancel")
                    .label("Cancel")
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tasks_mode = TasksMode::Browse;
                        cx.notify();
                    })),
            )
            .children(snapshot.valid_move_targets().into_iter().map(|list| {
                let selected = list.id == task.list_id;
                let move_controller = controller.clone();
                let task = task.clone();
                let list_id = list.id.clone();
                control_card()
                    .id(SharedString::from(format!("tasks-move-{}", list.id)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        move_controller.move_task(task.id.clone(), task.revision, list_id.clone());
                        this.tasks_list_id = Some(list_id.clone());
                        this.tasks_mode = TasksMode::Browse;
                        cx.notify();
                    }))
                    .child(div().text_sm().child(list.name))
                    .when(selected, |card| card.child(muted_text("Current list")))
            }))
            .into_any_element()
    }

    fn task_delete(
        &mut self,
        task: foyer_shell_tasks::Task,
        snapshot: foyer_shell_tasks::Snapshot,
        controller: foyer_shell_tasks::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        content_column()
            .children(tasks_status(&snapshot))
            .child(section_label("DELETE TASK"))
            .child(error_text(format!(
                "Delete “{}”? This cannot be undone after it syncs.",
                task.title
            )))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("tasks-delete-cancel")
                            .label("Cancel")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.tasks_mode = TasksMode::Browse;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("tasks-delete-confirm")
                            .label("Delete task")
                            .primary()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                controller.delete_task(task.id.clone(), task.revision);
                                this.tasks_task_id = None;
                                this.tasks_mode = TasksMode::Browse;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }
}

fn tasks_status(snapshot: &foyer_shell_tasks::Snapshot) -> Vec<AnyElement> {
    replica_status_elements(
        snapshot.using_powersync,
        foyer_shell_tasks::powersync_status(),
        snapshot.sync_banner().map(|banner| match banner {
            foyer_shell_tasks::SyncBanner::Offline { pending } => {
                ReplicaBanner::Offline { pending }
            }
            foyer_shell_tasks::SyncBanner::Pending { pending } => {
                ReplicaBanner::Pending { pending }
            }
            foyer_shell_tasks::SyncBanner::StaleRevision { message } => {
                ReplicaBanner::Stale { message }
            }
            foyer_shell_tasks::SyncBanner::Error { message } => ReplicaBanner::Error { message },
        }),
    )
}
