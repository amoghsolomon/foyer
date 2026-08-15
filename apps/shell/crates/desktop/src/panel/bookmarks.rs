use foyer_shell_ui::FoyerShellIcon;
use gpui::{AnyElement, Context, FontWeight, SharedString, Window, div, prelude::*};
use gpui_component::{
    Disableable, Icon, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, Textarea},
};

use super::{
    BookmarksMode, Panel,
    chrome::{
        ReplicaBanner, content_column, control_card, empty_state, error_text, muted_text,
        replica_status_elements, section_label,
    },
};
use crate::state::FoyerShellState;

impl Panel {
    pub(super) fn bookmarks_content(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let snapshot = FoyerShellState::global(cx).bookmarks.clone();
        let controller = FoyerShellState::global(cx).bookmarks_controller.clone();
        if let foyer_shell_bookmarks::Availability::Unavailable(error) = &snapshot.availability {
            let refresh = controller.clone();
            return content_column()
                .child(section_label("BOOKMARKS"))
                .child(error_text(error.clone()))
                .child(
                    Button::new("bookmarks-refresh-unavailable")
                        .label("Try again")
                        .outline()
                        .on_click(move |_, _, _| refresh.refresh()),
                )
                .into_any_element();
        }
        if matches!(
            snapshot.availability,
            foyer_shell_bookmarks::Availability::Loading
        ) {
            return empty_state(
                Icon::new(FoyerShellIcon::Bookmarks),
                "Loading bookmarks",
                "Connecting to Foyer Server…",
            );
        }

        if let Some(bookmark_id) = self.bookmarks_bookmark_id.clone()
            && let Some(bookmark) = snapshot.bookmark(&bookmark_id).cloned()
        {
            return self.bookmark_detail(bookmark, snapshot, controller, cx);
        }

        match self.bookmarks_mode {
            BookmarksMode::RenameFolder => self.bookmark_rename_folder(snapshot, controller, cx),
            BookmarksMode::MoveFolder => self.bookmark_move_folder(snapshot, controller, cx),
            BookmarksMode::ConfirmDeleteFolder => {
                self.bookmark_delete_folder(snapshot, controller, cx)
            }
            _ => self.bookmarks_browse(snapshot, controller, cx),
        }
    }

    fn bookmarks_browse(
        &mut self,
        snapshot: foyer_shell_bookmarks::Snapshot,
        controller: foyer_shell_bookmarks::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let parent = self.bookmarks_folder_id.clone();
        let current = parent
            .as_deref()
            .and_then(|id| snapshot.folder(id))
            .cloned();
        let folders = snapshot.child_folders(parent.as_deref());
        let query = self.bookmarks_query.read(cx).value().to_string();
        let visible =
            snapshot.visible_bookmarks(&query, self.bookmarks_filter, None, parent.as_deref());
        let title_input = self.bookmarks_title.clone();
        let url_input = self.bookmarks_url.clone();
        let tags_input = self.bookmarks_tags.clone();
        let description_input = self.bookmarks_description.clone();
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
            .children(bookmarks_status(&snapshot))
            .when(parent.is_some(), |column| {
                column.child(
                    Button::new("bookmarks-up")
                        .label("Parent folder")
                        .outline()
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(current) = this.bookmarks_folder_id.clone() {
                                this.bookmarks_folder_id = FoyerShellState::global(cx)
                                    .bookmarks
                                    .folder(&current)
                                    .and_then(|folder| folder.parent_id.clone());
                                this.bookmarks_mode = BookmarksMode::Browse;
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
                column.child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("bookmarks-rename-folder")
                                .label("Rename")
                                .outline()
                                .small()
                                .on_click(cx.listener({
                                    let name = folder.name.clone();
                                    move |this, _, window, cx| {
                                        this.bookmarks_mode = BookmarksMode::RenameFolder;
                                        this.bookmarks_title.update(cx, |input, cx| {
                                            input.set_value(name.clone(), window, cx)
                                        });
                                        cx.notify();
                                    }
                                })),
                        )
                        .child(
                            Button::new("bookmarks-move-folder")
                                .label("Move")
                                .outline()
                                .small()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.bookmarks_mode = BookmarksMode::MoveFolder;
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("bookmarks-delete-folder")
                                .label("Delete")
                                .outline()
                                .small()
                                .disabled(!folder_empty)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.bookmarks_mode = BookmarksMode::ConfirmDeleteFolder;
                                    cx.notify();
                                })),
                        ),
                )
            })
            .child(section_label("FILTER"))
            .child(
                h_flex()
                    .gap_2()
                    .child(filter_button(
                        "bookmarks-filter-all",
                        "All",
                        self.bookmarks_filter == foyer_shell_bookmarks::Filter::All,
                        foyer_shell_bookmarks::Filter::All,
                        cx,
                    ))
                    .child(filter_button(
                        "bookmarks-filter-favorites",
                        "Favorites",
                        self.bookmarks_filter == foyer_shell_bookmarks::Filter::Favorites,
                        foyer_shell_bookmarks::Filter::Favorites,
                        cx,
                    ))
                    .child(filter_button(
                        "bookmarks-filter-archived",
                        "Archived",
                        self.bookmarks_filter == foyer_shell_bookmarks::Filter::Archived,
                        foyer_shell_bookmarks::Filter::Archived,
                        cx,
                    )),
            )
            .child(Input::new(&self.bookmarks_query).cleanable(true))
            .child(section_label("FOLDERS"))
            .children(folders.into_iter().map(|folder| {
                let folder_id = folder.id.clone();
                control_card()
                    .id(SharedString::from(format!(
                        "bookmarks-folder-{}",
                        folder.id
                    )))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.bookmarks_folder_id = Some(folder_id.clone());
                        this.bookmarks_bookmark_id = None;
                        this.bookmarks_mode = BookmarksMode::Browse;
                        cx.notify();
                    }))
                    .child(div().text_sm().child(folder.name.clone()))
            }))
            .child(section_label("BOOKMARKS"))
            .children(visible.into_iter().map(|bookmark| {
                let bookmark_id = bookmark.id.clone();
                control_card()
                    .id(SharedString::from(format!("bookmarks-row-{}", bookmark.id)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.bookmarks_bookmark_id = Some(bookmark_id.clone());
                        this.bookmarks_mode = BookmarksMode::Browse;
                        cx.notify();
                    }))
                    .child(div().text_sm().child(bookmark.title.clone()))
                    .child(muted_text(bookmark.url.clone()))
            }))
            .child(section_label("CREATE"))
            .child(Input::new(&self.bookmarks_title).cleanable(true))
            .child(Input::new(&self.bookmarks_url).cleanable(true))
            .child(Input::new(&self.bookmarks_tags).cleanable(true))
            .child(Textarea::new(&self.bookmarks_description))
            .child(
                Button::new("bookmarks-create-folder")
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
                Button::new("bookmarks-create-bookmark")
                    .label("New bookmark")
                    .primary()
                    .disabled(folder_for_create.is_none())
                    .on_click({
                        let controller = create_controller;
                        move |_, window, cx| {
                            let Some(folder_id) = folder_for_create.clone() else {
                                return;
                            };
                            let title = title_input.read(cx).value().trim().to_string();
                            let url = url_input.read(cx).value().to_string();
                            let Ok(url) =
                                foyer_shell_bookmarks::validation::validate_bookmark_url(&url)
                            else {
                                return;
                            };
                            if title.is_empty() {
                                return;
                            }
                            let tags = parse_tags(&tags_input.read(cx).value());
                            controller.create_bookmark(
                                folder_id,
                                url,
                                title,
                                description_input.read(cx).value().to_string(),
                                tags,
                            );
                            title_input.update(cx, |input, cx| input.set_value("", window, cx));
                            url_input.update(cx, |input, cx| input.set_value("", window, cx));
                            tags_input.update(cx, |input, cx| input.set_value("", window, cx));
                            description_input
                                .update(cx, |input, cx| input.set_value("", window, cx));
                        }
                    }),
            )
            .when(parent.is_none(), |column| {
                column.child(muted_text("Open a folder to save a bookmark."))
            })
            .into_any_element()
    }

    fn bookmark_rename_folder(
        &mut self,
        snapshot: foyer_shell_bookmarks::Snapshot,
        controller: foyer_shell_bookmarks::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(folder) = self
            .bookmarks_folder_id
            .as_deref()
            .and_then(|id| snapshot.folder(id))
            .cloned()
        else {
            self.bookmarks_mode = BookmarksMode::Browse;
            return self.bookmarks_browse(snapshot, controller, cx);
        };
        let title_input = self.bookmarks_title.clone();
        content_column()
            .children(bookmarks_status(&snapshot))
            .child(section_label("RENAME FOLDER"))
            .child(Input::new(&self.bookmarks_title).cleanable(true))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("bookmarks-rename-cancel")
                            .label("Cancel")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.bookmarks_mode = BookmarksMode::Browse;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("bookmarks-rename-save")
                            .label("Save")
                            .primary()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let name = title_input.read(cx).value().trim().to_string();
                                if !name.is_empty() {
                                    controller.rename_folder(
                                        folder.id.clone(),
                                        folder.revision,
                                        name,
                                    );
                                    this.bookmarks_mode = BookmarksMode::Browse;
                                    cx.notify();
                                }
                            })),
                    ),
            )
            .into_any_element()
    }

    fn bookmark_move_folder(
        &mut self,
        snapshot: foyer_shell_bookmarks::Snapshot,
        controller: foyer_shell_bookmarks::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(folder) = self
            .bookmarks_folder_id
            .as_deref()
            .and_then(|id| snapshot.folder(id))
            .cloned()
        else {
            self.bookmarks_mode = BookmarksMode::Browse;
            return self.bookmarks_browse(snapshot, controller, cx);
        };
        let targets = snapshot.valid_folder_move_targets(&folder.id);
        let current_parent = folder.parent_id.clone();
        content_column()
            .children(bookmarks_status(&snapshot))
            .child(section_label("MOVE FOLDER"))
            .child(muted_text(format!("Moving {}", folder.name)))
            .child(
                Button::new("bookmarks-move-cancel")
                    .label("Cancel")
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.bookmarks_mode = BookmarksMode::Browse;
                        cx.notify();
                    })),
            )
            .child(
                control_card()
                    .id("bookmarks-move-root")
                    .cursor_pointer()
                    .on_click({
                        let move_controller = controller.clone();
                        let folder = folder.clone();
                        cx.listener(move |this, _, _, cx| {
                            move_controller.move_folder(folder.id.clone(), folder.revision, None);
                            this.bookmarks_mode = BookmarksMode::Browse;
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
                    .id(SharedString::from(format!("bookmarks-move-{}", target.id)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        move_controller.move_folder(
                            folder.id.clone(),
                            folder.revision,
                            Some(parent_id.clone()),
                        );
                        this.bookmarks_mode = BookmarksMode::Browse;
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

    fn bookmark_delete_folder(
        &mut self,
        snapshot: foyer_shell_bookmarks::Snapshot,
        controller: foyer_shell_bookmarks::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(folder) = self
            .bookmarks_folder_id
            .as_deref()
            .and_then(|id| snapshot.folder(id))
            .cloned()
        else {
            self.bookmarks_mode = BookmarksMode::Browse;
            return self.bookmarks_browse(snapshot, controller, cx);
        };
        content_column()
            .children(bookmarks_status(&snapshot))
            .child(section_label("DELETE FOLDER"))
            .child(error_text(format!(
                "Delete “{}”? This cannot be undone after it syncs.",
                folder.name
            )))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("bookmarks-delete-folder-cancel")
                            .label("Cancel")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.bookmarks_mode = BookmarksMode::Browse;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("bookmarks-delete-folder-confirm")
                            .label("Delete folder")
                            .primary()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                controller.delete_folder(folder.id.clone(), folder.revision);
                                this.bookmarks_folder_id = folder.parent_id.clone();
                                this.bookmarks_mode = BookmarksMode::Browse;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    fn bookmark_detail(
        &mut self,
        bookmark: foyer_shell_bookmarks::Bookmark,
        snapshot: foyer_shell_bookmarks::Snapshot,
        controller: foyer_shell_bookmarks::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.bookmarks_mode == BookmarksMode::EditBookmark {
            return self.bookmark_edit(bookmark, snapshot, controller, cx);
        }
        if self.bookmarks_mode == BookmarksMode::MoveBookmark {
            return self.bookmark_move(bookmark, snapshot, controller, cx);
        }
        if self.bookmarks_mode == BookmarksMode::ConfirmDeleteBookmark {
            return self.bookmark_delete(bookmark, snapshot, controller, cx);
        }
        let folder_label = snapshot.folder_path_label(&bookmark.folder_id);
        let favorite_controller = controller.clone();
        let archive_controller = controller.clone();
        let favorite_bookmark = bookmark.clone();
        let archive_bookmark = bookmark.clone();
        content_column()
            .children(bookmarks_status(&snapshot))
            .child(
                Button::new("bookmarks-back")
                    .label("Back")
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.bookmarks_bookmark_id = None;
                        this.bookmarks_mode = BookmarksMode::Browse;
                        cx.notify();
                    })),
            )
            .child(section_label("BOOKMARK"))
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::MEDIUM)
                    .child(bookmark.title.clone()),
            )
            .child(muted_text(bookmark.url.clone()))
            .child(muted_text(format!("Folder · {folder_label}")))
            .when(!bookmark.tags.is_empty(), |column| {
                column.child(muted_text(bookmark.tags.join(", ")))
            })
            .when(!bookmark.description.is_empty(), |column| {
                column.child(muted_text(bookmark.description.clone()))
            })
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("bookmarks-favorite")
                            .label(if bookmark.favorite {
                                "Unfavorite"
                            } else {
                                "Favorite"
                            })
                            .outline()
                            .on_click(move |_, _, _| {
                                favorite_controller.favorite_bookmark(
                                    favorite_bookmark.id.clone(),
                                    favorite_bookmark.revision,
                                    !favorite_bookmark.favorite,
                                );
                            }),
                    )
                    .child(
                        Button::new("bookmarks-archive")
                            .label(if bookmark.archived {
                                "Unarchive"
                            } else {
                                "Archive"
                            })
                            .outline()
                            .on_click(move |_, _, _| {
                                archive_controller.archive_bookmark(
                                    archive_bookmark.id.clone(),
                                    archive_bookmark.revision,
                                    !archive_bookmark.archived,
                                );
                            }),
                    )
                    .child(
                        Button::new("bookmarks-edit")
                            .label("Edit")
                            .primary()
                            .on_click(cx.listener({
                                let bookmark = bookmark.clone();
                                move |this, _, window, cx| {
                                    this.bookmarks_mode = BookmarksMode::EditBookmark;
                                    this.bookmarks_title.update(cx, |input, cx| {
                                        input.set_value(bookmark.title.clone(), window, cx)
                                    });
                                    this.bookmarks_url.update(cx, |input, cx| {
                                        input.set_value(bookmark.url.clone(), window, cx)
                                    });
                                    this.bookmarks_tags.update(cx, |input, cx| {
                                        input.set_value(bookmark.tags.join(", "), window, cx)
                                    });
                                    this.bookmarks_description.update(cx, |input, cx| {
                                        input.set_value(bookmark.description.clone(), window, cx)
                                    });
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        Button::new("bookmarks-move")
                            .label("Move")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.bookmarks_mode = BookmarksMode::MoveBookmark;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("bookmarks-delete")
                            .label("Delete")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.bookmarks_mode = BookmarksMode::ConfirmDeleteBookmark;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    fn bookmark_edit(
        &mut self,
        bookmark: foyer_shell_bookmarks::Bookmark,
        snapshot: foyer_shell_bookmarks::Snapshot,
        controller: foyer_shell_bookmarks::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let title_input = self.bookmarks_title.clone();
        let url_input = self.bookmarks_url.clone();
        let tags_input = self.bookmarks_tags.clone();
        let description_input = self.bookmarks_description.clone();
        content_column()
            .children(bookmarks_status(&snapshot))
            .child(section_label("EDIT BOOKMARK"))
            .child(Input::new(&self.bookmarks_title).cleanable(true))
            .child(Input::new(&self.bookmarks_url).cleanable(true))
            .child(Input::new(&self.bookmarks_tags).cleanable(true))
            .child(Textarea::new(&self.bookmarks_description))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("bookmarks-edit-cancel")
                            .label("Cancel")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.bookmarks_mode = BookmarksMode::Browse;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("bookmarks-edit-save")
                            .label("Save")
                            .primary()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let title = title_input.read(cx).value().trim().to_string();
                                let Ok(url) =
                                    foyer_shell_bookmarks::validation::validate_bookmark_url(
                                        &url_input.read(cx).value(),
                                    )
                                else {
                                    return;
                                };
                                if title.is_empty() {
                                    return;
                                }
                                controller.update_bookmark(
                                    bookmark.id.clone(),
                                    bookmark.revision,
                                    url,
                                    title,
                                    description_input.read(cx).value().to_string(),
                                    parse_tags(&tags_input.read(cx).value()),
                                );
                                this.bookmarks_mode = BookmarksMode::Browse;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    fn bookmark_move(
        &mut self,
        bookmark: foyer_shell_bookmarks::Bookmark,
        snapshot: foyer_shell_bookmarks::Snapshot,
        controller: foyer_shell_bookmarks::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        content_column()
            .children(bookmarks_status(&snapshot))
            .child(section_label("MOVE BOOKMARK"))
            .child(muted_text(bookmark.title.clone()))
            .child(
                Button::new("bookmarks-move-bookmark-cancel")
                    .label("Cancel")
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.bookmarks_mode = BookmarksMode::Browse;
                        cx.notify();
                    })),
            )
            .children(snapshot.folders.iter().map(|folder| {
                let selected = folder.id == bookmark.folder_id;
                let move_controller = controller.clone();
                let bookmark = bookmark.clone();
                let folder_id = folder.id.clone();
                control_card()
                    .id(SharedString::from(format!(
                        "bookmarks-move-bookmark-{}",
                        folder.id
                    )))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        move_controller.move_bookmark(
                            bookmark.id.clone(),
                            bookmark.revision,
                            folder_id.clone(),
                        );
                        this.bookmarks_folder_id = Some(folder_id.clone());
                        this.bookmarks_mode = BookmarksMode::Browse;
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

    fn bookmark_delete(
        &mut self,
        bookmark: foyer_shell_bookmarks::Bookmark,
        snapshot: foyer_shell_bookmarks::Snapshot,
        controller: foyer_shell_bookmarks::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        content_column()
            .children(bookmarks_status(&snapshot))
            .child(section_label("DELETE BOOKMARK"))
            .child(error_text(format!(
                "Delete “{}”? This cannot be undone after it syncs.",
                bookmark.title
            )))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("bookmarks-delete-cancel")
                            .label("Cancel")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.bookmarks_mode = BookmarksMode::Browse;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("bookmarks-delete-confirm")
                            .label("Delete bookmark")
                            .primary()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                controller.delete_bookmark(bookmark.id.clone(), bookmark.revision);
                                this.bookmarks_bookmark_id = None;
                                this.bookmarks_mode = BookmarksMode::Browse;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }
}

fn filter_button(
    id: &'static str,
    label: &'static str,
    selected: bool,
    filter: foyer_shell_bookmarks::Filter,
    cx: &mut Context<Panel>,
) -> Button {
    Button::new(id)
        .label(label)
        .small()
        .when(selected, |button| button.primary())
        .when(!selected, |button| button.outline())
        .on_click(cx.listener(move |this, _, _, cx| {
            this.bookmarks_filter = filter;
            cx.notify();
        }))
}

fn parse_tags(raw: &str) -> Vec<String> {
    let values = raw
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    foyer_shell_bookmarks::validation::normalize_tags(&values).unwrap_or_default()
}

fn bookmarks_status(snapshot: &foyer_shell_bookmarks::Snapshot) -> Vec<AnyElement> {
    replica_status_elements(
        snapshot.using_powersync,
        foyer_shell_bookmarks::powersync_status(),
        snapshot.sync_banner().map(|banner| match banner {
            foyer_shell_bookmarks::SyncBanner::Offline { pending } => {
                ReplicaBanner::Offline { pending }
            }
            foyer_shell_bookmarks::SyncBanner::Pending { pending } => {
                ReplicaBanner::Pending { pending }
            }
            foyer_shell_bookmarks::SyncBanner::StaleRevision { message } => {
                ReplicaBanner::Stale { message }
            }
            foyer_shell_bookmarks::SyncBanner::Error { message } => {
                ReplicaBanner::Error { message }
            }
        }),
    )
}
