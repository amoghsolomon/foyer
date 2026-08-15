mod applications;
mod control;
mod notification;
mod osd;
mod panel;
mod state;
mod toolbar;
mod tray_popover;
mod workspace;
mod workspace_view;

use std::{sync::Arc, thread, time::Duration};

use gpui::App;
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

use state::FoyerShellState;

fn main() -> anyhow::Result<()> {
    let _ = foyer_shell_paths::config_root();
    if control::handle_client_invocation()? {
        return Ok(());
    }

    let _ = tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().without_time())
        .with(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("foyer_shell=info".parse().expect("valid tracing directive")),
        )
        .try_init();

    let control_commands = control::listen()?;
    let applications = Arc::new(applications::index());
    tracing::info!(count = applications.len(), "indexed desktop applications");
    let niri_updates = foyer_shell_niri::subscribe();
    let services = foyer_shell_services::start();
    let service_updates = services.updates;
    let service_controller = services.controller;
    let notifications = foyer_shell_services::notifications::start();
    let notification_events = notifications.events;
    let notification_controller = notifications.controller;
    let transcription = foyer_shell_transcription::start();
    let transcription_updates = transcription.updates;
    let transcription_controller = transcription.controller;
    let (paste_events_tx, paste_events) = async_channel::unbounded();
    let storage = foyer_shell_storage::start();
    let storage_updates = storage.updates;
    let storage_controller = storage.controller;
    let personal = foyer_shell_personal::start();
    let notes_updates = personal.notes_updates;
    let notes_controller = personal.notes;
    let tasks_updates = personal.tasks_updates;
    let tasks_controller = personal.tasks;
    let calendar_updates = personal.calendar_updates;
    let calendar_controller = personal.calendar;
    let contacts_updates = personal.contacts_updates;
    let contacts_controller = personal.contacts;
    let bookmarks_updates = personal.bookmarks_updates;
    let bookmarks_controller = personal.bookmarks;

    gpui_platform::application()
        .with_assets(foyer_shell_ui::Assets)
        .run(move |cx: &mut App| {
            foyer_shell_presentation_player::init(cx);
            panel::init(cx);
            tray_popover::init(cx);
            cx.set_global(FoyerShellState::new(
                applications.clone(),
                service_controller.clone(),
                notification_controller.clone(),
                transcription_controller.clone(),
                storage_controller.clone(),
                notes_controller.clone(),
                tasks_controller.clone(),
                calendar_controller.clone(),
                contacts_controller.clone(),
                bookmarks_controller.clone(),
            ));
            workspace_view::open(cx);

            cx.spawn(async move |cx| {
                while let Ok(snapshot) = niri_updates.recv().await {
                    cx.update(|cx| {
                        FoyerShellState::global_mut(cx)
                            .workspace_policy
                            .reconcile(&snapshot);
                        FoyerShellState::global_mut(cx).niri = snapshot;
                        toolbar::reconcile(cx);
                        cx.refresh_windows();
                    });
                }
            })
            .detach();

            cx.spawn(async move |cx| {
                while let Ok(snapshot) = storage_updates.recv().await {
                    cx.update(|cx| {
                        FoyerShellState::global_mut(cx).storage = snapshot;
                        refresh_agenda(cx);
                        cx.refresh_windows();
                    });
                }
            })
            .detach();

            cx.spawn(async move |cx| {
                while let Ok(snapshot) = notes_updates.recv().await {
                    cx.update(|cx| {
                        FoyerShellState::global_mut(cx).notes = snapshot;
                        cx.refresh_windows();
                    });
                }
            })
            .detach();

            cx.spawn(async move |cx| {
                while let Ok(snapshot) = tasks_updates.recv().await {
                    cx.update(|cx| {
                        FoyerShellState::global_mut(cx).tasks = snapshot;
                        refresh_agenda(cx);
                        cx.refresh_windows();
                    });
                }
            })
            .detach();

            cx.spawn(async move |cx| {
                while let Ok(snapshot) = calendar_updates.recv().await {
                    cx.update(|cx| {
                        FoyerShellState::global_mut(cx).calendar = snapshot;
                        refresh_agenda(cx);
                        cx.refresh_windows();
                    });
                }
            })
            .detach();

            cx.spawn(async move |cx| {
                while let Ok(snapshot) = contacts_updates.recv().await {
                    cx.update(|cx| {
                        FoyerShellState::global_mut(cx).contacts = snapshot;
                        cx.refresh_windows();
                    });
                }
            })
            .detach();

            cx.spawn(async move |cx| {
                while let Ok(snapshot) = bookmarks_updates.recv().await {
                    cx.update(|cx| {
                        FoyerShellState::global_mut(cx).bookmarks = snapshot;
                        cx.refresh_windows();
                    });
                }
            })
            .detach();

            cx.spawn(async move |cx| {
                while let Ok(snapshot) = service_updates.recv().await {
                    cx.update(|cx| {
                        let (previous, was_initialized) = {
                            let state = FoyerShellState::global_mut(cx);
                            let previous = state.services.clone();
                            let was_initialized = state.services_initialized;
                            state.services = snapshot.clone();
                            state.services_initialized = true;
                            (previous, was_initialized)
                        };
                        if was_initialized {
                            osd::handle_snapshot_change(&previous, &snapshot, cx);
                        }
                        cx.refresh_windows();
                    });
                }
            })
            .detach();

            cx.spawn(async move |cx| {
                while let Ok(event) = notification_events.recv().await {
                    cx.update(|cx| notification::handle(event, cx));
                }
            })
            .detach();

            let paste_sender = paste_events_tx.clone();
            cx.spawn(async move |cx| {
                while let Ok(snapshot) = transcription_updates.recv().await {
                    cx.update(|cx| {
                        let should_paste = snapshot.state
                            == foyer_shell_transcription::State::Ready
                            && !snapshot.transcript.is_empty()
                            && snapshot.generation
                                != FoyerShellState::global(cx)
                                    .last_handled_transcription_generation;
                        let should_report_error = snapshot.state
                            == foyer_shell_transcription::State::Error
                            && snapshot.generation
                                != FoyerShellState::global(cx)
                                    .last_handled_transcription_generation;
                        {
                            let state = FoyerShellState::global_mut(cx);
                            state.transcription = snapshot.clone();
                            if should_paste || should_report_error {
                                state.last_handled_transcription_generation = snapshot.generation;
                            }
                        }
                        cx.refresh_windows();
                        if should_paste {
                            let text = snapshot.transcript.to_string();
                            let sender = paste_sender.clone();
                            thread::Builder::new()
                                .name("foyer-shell-transcription-paste".into())
                                .spawn(move || {
                                    let result = foyer_shell_transcription::copy_and_paste(&text)
                                        .map_err(|error| error.to_string());
                                    let _ = sender.send_blocking((text, result));
                                })
                                .ok();
                        } else if should_report_error {
                            notification::show_local(
                                "Transcription failed",
                                &snapshot.error,
                                foyer_shell_services::notifications::Urgency::Critical,
                                cx,
                            );
                        }
                    });
                }
            })
            .detach();

            cx.spawn(async move |cx| {
                while let Ok((text, result)) = paste_events.recv().await {
                    cx.update(|cx| match result {
                        Ok(()) => notification::show_local(
                            "Pasted transcription",
                            &text,
                            foyer_shell_services::notifications::Urgency::Normal,
                            cx,
                        ),
                        Err(error) => notification::show_local(
                            "Transcription paste failed",
                            &error,
                            foyer_shell_services::notifications::Urgency::Critical,
                            cx,
                        ),
                    });
                }
            })
            .detach();

            cx.spawn(async move |cx| {
                while let Ok(command) = control_commands.recv().await {
                    cx.update(|cx| match command {
                        control::Command::Search => panel::toggle(panel::Section::Search, None, cx),
                        control::Command::Agenda => panel::toggle(panel::Section::Agenda, None, cx),
                        control::Command::Tasks => panel::toggle(panel::Section::Tasks, None, cx),
                        control::Command::Notes => panel::toggle(panel::Section::Notes, None, cx),
                        control::Command::Contacts => {
                            panel::toggle(panel::Section::Contacts, None, cx)
                        }
                        control::Command::Bookmarks => {
                            panel::toggle(panel::Section::Bookmarks, None, cx)
                        }
                        control::Command::Notifications => {
                            panel::toggle(panel::Section::Notifications, None, cx)
                        }
                        control::Command::Transcription => FoyerShellState::global(cx)
                            .transcription_controller
                            .toggle_dictation(),
                        control::Command::Audio => panel::toggle(panel::Section::Audio, None, cx),
                        control::Command::Network => {
                            panel::toggle(panel::Section::Network, None, cx)
                        }
                        control::Command::Bluetooth => {
                            panel::toggle(panel::Section::Bluetooth, None, cx)
                        }
                        control::Command::Display => {
                            panel::toggle(panel::Section::Display, None, cx)
                        }
                        control::Command::Tray => tray_popover::toggle(None, None, cx),
                        control::Command::Power => panel::toggle(panel::Section::Power, None, cx),
                        control::Command::Lock => {
                            let controller = FoyerShellState::global(cx).service_controller.clone();
                            tray_popover::close(cx);
                            panel::close(cx);
                            controller.lock();
                        }
                        control::Command::RaiseVolume => {
                            let volume = osd::preview_volume(5, cx);
                            FoyerShellState::global(cx)
                                .service_controller
                                .set_volume(volume)
                        }
                        control::Command::LowerVolume => {
                            let volume = osd::preview_volume(-5, cx);
                            FoyerShellState::global(cx)
                                .service_controller
                                .set_volume(volume)
                        }
                        control::Command::ToggleMute => {
                            let muted = osd::preview_toggle_mute(false, cx);
                            FoyerShellState::global(cx)
                                .service_controller
                                .set_muted(muted)
                        }
                        control::Command::RaiseInputVolume => {
                            let volume = osd::preview_input_volume(5, cx);
                            FoyerShellState::global(cx)
                                .service_controller
                                .set_input_volume(volume)
                        }
                        control::Command::LowerInputVolume => {
                            let volume = osd::preview_input_volume(-5, cx);
                            FoyerShellState::global(cx)
                                .service_controller
                                .set_input_volume(volume)
                        }
                        control::Command::ToggleInputMute => {
                            let muted = osd::preview_toggle_mute(true, cx);
                            FoyerShellState::global(cx)
                                .service_controller
                                .set_input_muted(muted)
                        }
                        control::Command::RaiseBrightness => {
                            let percent = osd::preview_brightness(5, cx);
                            FoyerShellState::global(cx)
                                .service_controller
                                .set_brightness(percent)
                        }
                        control::Command::LowerBrightness => {
                            let percent = osd::preview_brightness(-5, cx);
                            FoyerShellState::global(cx)
                                .service_controller
                                .set_brightness(percent)
                        }
                    });
                }
            })
            .detach();

            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(150))
                    .await;
                loop {
                    cx.update(toolbar::reconcile);
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                }
            })
            .detach();
        });
    Ok(())
}

fn refresh_agenda(cx: &mut App) {
    let state = FoyerShellState::global_mut(cx);
    state.agenda = foyer_shell_personal::agenda_snapshot(
        &state.calendar,
        &state.tasks,
        state.storage.hidden_agenda_sources.as_ref(),
    );
}
