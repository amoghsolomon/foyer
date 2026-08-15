use foyer_shell_ui::FoyerShellIcon;
use gpui::{AnyElement, Context, FontWeight, SharedString, Window, div, prelude::*};
use gpui_component::{
    Disableable, Icon, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::Input,
};

use super::{
    ContactsMode, Panel,
    chrome::{
        ReplicaBanner, content_column, control_card, empty_state, error_text, muted_text,
        replica_status_elements, section_label,
    },
};
use crate::state::FoyerShellState;

impl Panel {
    pub(super) fn contacts_content(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let snapshot = FoyerShellState::global(cx).contacts.clone();
        let controller = FoyerShellState::global(cx).contacts_controller.clone();
        if let foyer_shell_contacts::Availability::Unavailable(error) = &snapshot.availability {
            let refresh = controller.clone();
            return content_column()
                .child(section_label("CONTACTS"))
                .child(error_text(error.clone()))
                .child(
                    Button::new("contacts-refresh-unavailable")
                        .label("Try again")
                        .outline()
                        .on_click(move |_, _, _| refresh.refresh()),
                )
                .into_any_element();
        }
        if matches!(
            snapshot.availability,
            foyer_shell_contacts::Availability::Loading
        ) {
            return empty_state(
                Icon::new(FoyerShellIcon::Contacts),
                "Loading contacts",
                "Connecting to Foyer Server…",
            );
        }

        if let Some(contact_id) = self.contacts_contact_id.clone()
            && let Some(contact) = snapshot.contact(&contact_id).cloned()
        {
            return self.contact_detail(contact, snapshot, controller, cx);
        }

        match self.contacts_mode {
            ContactsMode::RenameBook => self.contact_rename_book(snapshot, controller, cx),
            ContactsMode::ConfirmDeleteBook => self.contact_delete_book(snapshot, controller, cx),
            _ => self.contacts_browse(snapshot, controller, cx),
        }
    }

    fn contacts_browse(
        &mut self,
        snapshot: foyer_shell_contacts::Snapshot,
        controller: foyer_shell_contacts::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let query = self.contacts_query.read(cx).value().to_string();
        let selected = self.contacts_book_id.clone();
        let current = selected
            .as_deref()
            .and_then(|id| snapshot.address_book(id))
            .cloned();
        let people = snapshot.search(&query, selected.as_deref());
        let name_input = self.contacts_display_name.clone();
        let email_input = self.contacts_email.clone();
        let phone_input = self.contacts_phone.clone();
        let org_input = self.contacts_org.clone();
        let title_input = self.contacts_job_title.clone();
        let create_controller = controller.clone();
        let book_for_create = selected.clone();

        content_column()
            .children(contacts_status(&snapshot))
            .child(section_label("SEARCH"))
            .child(Input::new(&self.contacts_query).cleanable(true))
            .child(
                h_flex()
                    .justify_between()
                    .child(section_label("ADDRESS BOOKS"))
                    .child(
                        Button::new("contacts-refresh")
                            .label("Refresh")
                            .ghost()
                            .small()
                            .on_click({
                                let refresh = controller.clone();
                                move |_, _, _| refresh.refresh()
                            }),
                    ),
            )
            .child(
                Button::new("contacts-all-books")
                    .label("All contacts")
                    .when(selected.is_none(), |button| button.primary())
                    .when(selected.is_some(), |button| button.outline())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.contacts_book_id = None;
                        this.contacts_mode = ContactsMode::Browse;
                        cx.notify();
                    })),
            )
            .children(snapshot.address_books.iter().map(|book| {
                let book_id = book.id.clone();
                let selected_here = selected.as_deref() == Some(book.id.as_str());
                control_card()
                    .id(SharedString::from(format!("contacts-book-{}", book.id)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.contacts_book_id = Some(book_id.clone());
                        this.contacts_contact_id = None;
                        this.contacts_mode = ContactsMode::Browse;
                        cx.notify();
                    }))
                    .child(div().text_sm().child(book.display_name.clone()))
                    .when(selected_here, |card| card.child(muted_text("Selected")))
            }))
            .when_some(current.clone(), |column, book| {
                let empty = snapshot.validate_address_book_delete(&book.id).is_ok();
                column.child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("contacts-rename-book")
                                .label("Rename")
                                .outline()
                                .small()
                                .on_click(cx.listener({
                                    let name = book.display_name.clone();
                                    move |this, _, window, cx| {
                                        this.contacts_mode = ContactsMode::RenameBook;
                                        this.contacts_display_name.update(cx, |input, cx| {
                                            input.set_value(name.clone(), window, cx)
                                        });
                                        cx.notify();
                                    }
                                })),
                        )
                        .child(
                            Button::new("contacts-delete-book")
                                .label("Delete")
                                .outline()
                                .small()
                                .disabled(!empty)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.contacts_mode = ContactsMode::ConfirmDeleteBook;
                                    cx.notify();
                                })),
                        ),
                )
            })
            .child(section_label("PEOPLE"))
            .children(people.into_iter().map(|contact| {
                let contact_id = contact.id.clone();
                control_card()
                    .id(SharedString::from(format!("contacts-row-{}", contact.id)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.contacts_contact_id = Some(contact_id.clone());
                        this.contacts_mode = ContactsMode::Browse;
                        cx.notify();
                    }))
                    .child(div().text_sm().child(contact.display_name.clone()))
                    .when_some(contact.emails.first(), |card, email| {
                        card.child(muted_text(email.value.clone()))
                    })
            }))
            .child(section_label("CREATE"))
            .child(Input::new(&self.contacts_display_name).cleanable(true))
            .child(Input::new(&self.contacts_email).cleanable(true))
            .child(Input::new(&self.contacts_phone).cleanable(true))
            .child(Input::new(&self.contacts_org).cleanable(true))
            .child(Input::new(&self.contacts_job_title).cleanable(true))
            .child(
                Button::new("contacts-create-book")
                    .label("New address book")
                    .outline()
                    .on_click({
                        let controller = create_controller.clone();
                        let input = name_input.clone();
                        move |_, window, cx| {
                            let name = input.read(cx).value().trim().to_string();
                            if !name.is_empty() {
                                controller.create_address_book(name);
                                input.update(cx, |input, cx| input.set_value("", window, cx));
                            }
                        }
                    }),
            )
            .child(
                Button::new("contacts-create-contact")
                    .label("New contact")
                    .primary()
                    .disabled(book_for_create.is_none())
                    .on_click({
                        let controller = create_controller;
                        move |_, window, cx| {
                            let Some(address_book_id) = book_for_create.clone() else {
                                return;
                            };
                            let display_name = name_input.read(cx).value().trim().to_string();
                            if display_name.is_empty() {
                                return;
                            }
                            let email = email_input.read(cx).value().trim().to_string();
                            let phone = phone_input.read(cx).value().trim().to_string();
                            controller.create_contact(foyer_shell_contacts::ContactDraft {
                                display_name,
                                emails: typed_values(email),
                                phones: typed_values(phone),
                                organization: org_input.read(cx).value().trim().to_string(),
                                job_title: title_input.read(cx).value().trim().to_string(),
                                address_book_id,
                                ..foyer_shell_contacts::ContactDraft::default()
                            });
                            name_input.update(cx, |input, cx| input.set_value("", window, cx));
                            email_input.update(cx, |input, cx| input.set_value("", window, cx));
                            phone_input.update(cx, |input, cx| input.set_value("", window, cx));
                            org_input.update(cx, |input, cx| input.set_value("", window, cx));
                            title_input.update(cx, |input, cx| input.set_value("", window, cx));
                        }
                    }),
            )
            .into_any_element()
    }

    fn contact_rename_book(
        &mut self,
        snapshot: foyer_shell_contacts::Snapshot,
        controller: foyer_shell_contacts::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(book) = self
            .contacts_book_id
            .as_deref()
            .and_then(|id| snapshot.address_book(id))
            .cloned()
        else {
            self.contacts_mode = ContactsMode::Browse;
            return self.contacts_browse(snapshot, controller, cx);
        };
        let name_input = self.contacts_display_name.clone();
        content_column()
            .children(contacts_status(&snapshot))
            .child(section_label("RENAME ADDRESS BOOK"))
            .child(Input::new(&self.contacts_display_name).cleanable(true))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("contacts-rename-cancel")
                            .label("Cancel")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.contacts_mode = ContactsMode::Browse;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("contacts-rename-save")
                            .label("Save")
                            .primary()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let name = name_input.read(cx).value().trim().to_string();
                                if !name.is_empty() {
                                    controller.rename_address_book(
                                        book.id.clone(),
                                        book.revision,
                                        book.etag.clone(),
                                        name,
                                    );
                                    this.contacts_mode = ContactsMode::Browse;
                                    cx.notify();
                                }
                            })),
                    ),
            )
            .into_any_element()
    }

    fn contact_delete_book(
        &mut self,
        snapshot: foyer_shell_contacts::Snapshot,
        controller: foyer_shell_contacts::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(book) = self
            .contacts_book_id
            .as_deref()
            .and_then(|id| snapshot.address_book(id))
            .cloned()
        else {
            self.contacts_mode = ContactsMode::Browse;
            return self.contacts_browse(snapshot, controller, cx);
        };
        content_column()
            .children(contacts_status(&snapshot))
            .child(section_label("DELETE ADDRESS BOOK"))
            .child(error_text(format!(
                "Delete “{}”? This cannot be undone after it syncs.",
                book.display_name
            )))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("contacts-delete-book-cancel")
                            .label("Cancel")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.contacts_mode = ContactsMode::Browse;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("contacts-delete-book-confirm")
                            .label("Delete address book")
                            .primary()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                controller.delete_address_book(
                                    book.id.clone(),
                                    book.revision,
                                    book.etag.clone(),
                                );
                                this.contacts_book_id = None;
                                this.contacts_mode = ContactsMode::Browse;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    fn contact_detail(
        &mut self,
        contact: foyer_shell_contacts::Contact,
        snapshot: foyer_shell_contacts::Snapshot,
        controller: foyer_shell_contacts::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.contacts_mode == ContactsMode::EditContact {
            return self.contact_edit(contact, snapshot, controller, cx);
        }
        if self.contacts_mode == ContactsMode::MoveContact {
            return self.contact_move(contact, snapshot, controller, cx);
        }
        if self.contacts_mode == ContactsMode::ConfirmDeleteContact {
            return self.contact_delete(contact, snapshot, controller, cx);
        }
        let book_name = snapshot
            .address_book(&contact.address_book_id)
            .map(|book| book.display_name.clone())
            .unwrap_or_else(|| "Address book".into());
        content_column()
            .children(contacts_status(&snapshot))
            .child(
                Button::new("contacts-back")
                    .label("Back")
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.contacts_contact_id = None;
                        this.contacts_mode = ContactsMode::Browse;
                        cx.notify();
                    })),
            )
            .child(section_label("CONTACT"))
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::MEDIUM)
                    .child(contact.display_name.clone()),
            )
            .child(muted_text(format!("Address book · {book_name}")))
            .when(!contact.name.formatted().is_empty(), |column| {
                column.child(muted_text(contact.name.formatted()))
            })
            .when(
                !contact.organization.is_empty() || !contact.job_title.is_empty(),
                |column| {
                    column.child(muted_text(
                        [contact.job_title.as_str(), contact.organization.as_str()]
                            .into_iter()
                            .filter(|part| !part.is_empty())
                            .collect::<Vec<_>>()
                            .join(" · "),
                    ))
                },
            )
            .children(
                contact
                    .emails
                    .iter()
                    .map(|email| muted_text(email.value.clone())),
            )
            .children(
                contact
                    .phones
                    .iter()
                    .map(|phone| muted_text(phone.value.clone())),
            )
            .children(
                contact
                    .addresses
                    .iter()
                    .filter(|address| !address.one_line().is_empty())
                    .map(|address| muted_text(address.one_line())),
            )
            .when_some(contact.birthday.clone(), |column, birthday| {
                column.child(muted_text(format!("Birthday {birthday}")))
            })
            .when(!contact.notes.is_empty(), |column| {
                column.child(muted_text(contact.notes.clone()))
            })
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("contacts-edit")
                            .label("Edit")
                            .primary()
                            .on_click(cx.listener({
                                let contact = contact.clone();
                                move |this, _, window, cx| {
                                    this.contacts_mode = ContactsMode::EditContact;
                                    this.contacts_display_name.update(cx, |input, cx| {
                                        input.set_value(contact.display_name.clone(), window, cx)
                                    });
                                    this.contacts_email.update(cx, |input, cx| {
                                        input.set_value(
                                            contact
                                                .emails
                                                .first()
                                                .map(|email| email.value.clone())
                                                .unwrap_or_default(),
                                            window,
                                            cx,
                                        )
                                    });
                                    this.contacts_phone.update(cx, |input, cx| {
                                        input.set_value(
                                            contact
                                                .phones
                                                .first()
                                                .map(|phone| phone.value.clone())
                                                .unwrap_or_default(),
                                            window,
                                            cx,
                                        )
                                    });
                                    this.contacts_org.update(cx, |input, cx| {
                                        input.set_value(contact.organization.clone(), window, cx)
                                    });
                                    this.contacts_job_title.update(cx, |input, cx| {
                                        input.set_value(contact.job_title.clone(), window, cx)
                                    });
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        Button::new("contacts-move")
                            .label("Move")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.contacts_mode = ContactsMode::MoveContact;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("contacts-delete")
                            .label("Delete")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.contacts_mode = ContactsMode::ConfirmDeleteContact;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    fn contact_edit(
        &mut self,
        contact: foyer_shell_contacts::Contact,
        snapshot: foyer_shell_contacts::Snapshot,
        controller: foyer_shell_contacts::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name_input = self.contacts_display_name.clone();
        let email_input = self.contacts_email.clone();
        let phone_input = self.contacts_phone.clone();
        let org_input = self.contacts_org.clone();
        let title_input = self.contacts_job_title.clone();
        content_column()
            .children(contacts_status(&snapshot))
            .child(section_label("EDIT CONTACT"))
            .child(Input::new(&self.contacts_display_name).cleanable(true))
            .child(Input::new(&self.contacts_email).cleanable(true))
            .child(Input::new(&self.contacts_phone).cleanable(true))
            .child(Input::new(&self.contacts_org).cleanable(true))
            .child(Input::new(&self.contacts_job_title).cleanable(true))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("contacts-edit-cancel")
                            .label("Cancel")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.contacts_mode = ContactsMode::Browse;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("contacts-edit-save")
                            .label("Save")
                            .primary()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let display_name = name_input.read(cx).value().trim().to_string();
                                if display_name.is_empty() {
                                    return;
                                }
                                let mut draft =
                                    foyer_shell_contacts::ContactDraft::from_contact(&contact);
                                draft.display_name = display_name;
                                draft.emails =
                                    typed_values(email_input.read(cx).value().to_string());
                                draft.phones =
                                    typed_values(phone_input.read(cx).value().to_string());
                                draft.organization = org_input.read(cx).value().trim().to_string();
                                draft.job_title = title_input.read(cx).value().trim().to_string();
                                controller.update_contact(
                                    contact.id.clone(),
                                    contact.revision,
                                    contact.etag.clone(),
                                    draft,
                                );
                                this.contacts_mode = ContactsMode::Browse;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    fn contact_move(
        &mut self,
        contact: foyer_shell_contacts::Contact,
        snapshot: foyer_shell_contacts::Snapshot,
        controller: foyer_shell_contacts::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        content_column()
            .children(contacts_status(&snapshot))
            .child(section_label("MOVE CONTACT"))
            .child(muted_text(contact.display_name.clone()))
            .child(
                Button::new("contacts-move-cancel")
                    .label("Cancel")
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.contacts_mode = ContactsMode::Browse;
                        cx.notify();
                    })),
            )
            .children(snapshot.address_books.iter().map(|book| {
                let selected = book.id == contact.address_book_id;
                let move_controller = controller.clone();
                let contact = contact.clone();
                let book_id = book.id.clone();
                control_card()
                    .id(SharedString::from(format!("contacts-move-{}", book.id)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        move_controller.move_contact(
                            contact.id.clone(),
                            contact.revision,
                            contact.etag.clone(),
                            book_id.clone(),
                        );
                        this.contacts_book_id = Some(book_id.clone());
                        this.contacts_mode = ContactsMode::Browse;
                        cx.notify();
                    }))
                    .child(div().text_sm().child(book.display_name.clone()))
                    .when(selected, |card| {
                        card.child(muted_text("Current address book"))
                    })
            }))
            .into_any_element()
    }

    fn contact_delete(
        &mut self,
        contact: foyer_shell_contacts::Contact,
        snapshot: foyer_shell_contacts::Snapshot,
        controller: foyer_shell_contacts::Controller,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        content_column()
            .children(contacts_status(&snapshot))
            .child(section_label("DELETE CONTACT"))
            .child(error_text(format!(
                "Delete “{}”? This cannot be undone after it syncs.",
                contact.display_name
            )))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("contacts-delete-cancel")
                            .label("Cancel")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.contacts_mode = ContactsMode::Browse;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("contacts-delete-confirm")
                            .label("Delete contact")
                            .primary()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                controller.delete_contact(
                                    contact.id.clone(),
                                    contact.revision,
                                    contact.etag.clone(),
                                );
                                this.contacts_contact_id = None;
                                this.contacts_mode = ContactsMode::Browse;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }
}

fn typed_values(value: String) -> Vec<foyer_shell_contacts::TypedValue> {
    let value = value.trim().to_string();
    if value.is_empty() {
        Vec::new()
    } else {
        vec![foyer_shell_contacts::TypedValue {
            value,
            r#type: "other".into(),
            pref: true,
        }]
    }
}

fn contacts_status(snapshot: &foyer_shell_contacts::Snapshot) -> Vec<AnyElement> {
    replica_status_elements(
        snapshot.using_powersync,
        foyer_shell_contacts::powersync_status(),
        snapshot.sync_banner().map(|banner| match banner {
            foyer_shell_contacts::SyncBanner::Offline { pending } => {
                ReplicaBanner::Offline { pending }
            }
            foyer_shell_contacts::SyncBanner::Pending { pending } => {
                ReplicaBanner::Pending { pending }
            }
            foyer_shell_contacts::SyncBanner::StaleEtag { message } => {
                ReplicaBanner::Stale { message }
            }
            foyer_shell_contacts::SyncBanner::Error { message } => ReplicaBanner::Error { message },
        }),
    )
}
