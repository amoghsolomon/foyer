use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

const WORKSPACE_ONE: &str = "foyer-shell-workspace-1";
const RETRY_AFTER: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Correction {
    MoveWindow { window_id: u64, workspace_id: u64 },
    MoveHomeToFirst { workspace_id: u64 },
}

#[derive(Default)]
pub struct WorkspacePolicy {
    pending_windows: HashMap<u64, Instant>,
    pending_home_position: Option<Instant>,
}

impl WorkspacePolicy {
    pub fn reconcile(&mut self, snapshot: &foyer_shell_niri::Snapshot) {
        let corrections = self.corrections(snapshot, Instant::now());
        for correction in corrections {
            std::thread::spawn(move || {
                let result = match correction {
                    Correction::MoveWindow {
                        window_id,
                        workspace_id,
                    } => foyer_shell_niri::move_window_to_workspace(window_id, workspace_id),
                    Correction::MoveHomeToFirst { workspace_id } => {
                        foyer_shell_niri::move_workspace_to_index(workspace_id, 1)
                    }
                };
                if let Err(error) = result {
                    tracing::warn!(?correction, %error, "could not restore the Workspace 1 reservation");
                }
            });
        }
    }

    fn corrections(
        &mut self,
        snapshot: &foyer_shell_niri::Snapshot,
        now: Instant,
    ) -> Vec<Correction> {
        if !snapshot.connected {
            self.pending_windows.clear();
            self.pending_home_position = None;
            return Vec::new();
        }

        let Some(home) = snapshot
            .workspaces
            .iter()
            .find(|workspace| workspace.name.as_deref() == Some(WORKSPACE_ONE))
        else {
            return Vec::new();
        };

        let fallback = snapshot
            .workspaces
            .iter()
            .filter(|workspace| workspace.id != home.id && workspace.output == home.output)
            .min_by_key(|workspace| workspace.index)
            .map(|workspace| workspace.id);

        self.pending_windows.retain(|window_id, _| {
            snapshot
                .windows
                .iter()
                .find(|window| window.id == *window_id)
                .is_some_and(|window| {
                    if window.app_id == crate::workspace_view::APP_ID {
                        window.workspace_id != Some(home.id)
                    } else {
                        window.workspace_id == Some(home.id)
                    }
                })
        });

        let mut corrections = Vec::new();
        for window in &snapshot.windows {
            let target = if window.app_id == crate::workspace_view::APP_ID
                && window.workspace_id != Some(home.id)
            {
                Some(home.id)
            } else if window.app_id != crate::workspace_view::APP_ID
                && window.workspace_id == Some(home.id)
            {
                fallback
            } else {
                None
            };

            let Some(workspace_id) = target else {
                continue;
            };
            let due = self
                .pending_windows
                .get(&window.id)
                .is_none_or(|last| now.duration_since(*last) >= RETRY_AFTER);
            if due {
                self.pending_windows.insert(window.id, now);
                corrections.push(Correction::MoveWindow {
                    window_id: window.id,
                    workspace_id,
                });
            }
        }

        if home.index == 1 {
            self.pending_home_position = None;
        } else if self
            .pending_home_position
            .is_none_or(|last| now.duration_since(last) >= RETRY_AFTER)
        {
            self.pending_home_position = Some(now);
            corrections.push(Correction::MoveHomeToFirst {
                workspace_id: home.id,
            });
        }

        corrections
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> foyer_shell_niri::Snapshot {
        foyer_shell_niri::Snapshot {
            connected: true,
            workspaces: vec![
                foyer_shell_niri::Workspace {
                    id: 10,
                    index: 1,
                    name: Some(WORKSPACE_ONE.into()),
                    output: Some("eDP-1".into()),
                    active: true,
                    focused: true,
                },
                foyer_shell_niri::Workspace {
                    id: 11,
                    index: 2,
                    name: None,
                    output: Some("eDP-1".into()),
                    active: false,
                    focused: false,
                },
            ],
            windows: vec![
                foyer_shell_niri::Window {
                    id: 20,
                    title: "Overview".into(),
                    app_id: crate::workspace_view::APP_ID.into(),
                    workspace_id: Some(10),
                    focused: true,
                },
                foyer_shell_niri::Window {
                    id: 21,
                    title: "Terminal".into(),
                    app_id: "Alacritty".into(),
                    workspace_id: Some(10),
                    focused: false,
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn moves_ordinary_windows_out_of_home() {
        let now = Instant::now();
        let corrections = WorkspacePolicy::default().corrections(&snapshot(), now);
        assert_eq!(
            corrections,
            vec![Correction::MoveWindow {
                window_id: 21,
                workspace_id: 11,
            }]
        );
    }

    #[test]
    fn restores_workspace_and_workspace_position() {
        let now = Instant::now();
        let mut snapshot = snapshot();
        snapshot.workspaces[0].index = 2;
        snapshot.workspaces[1].index = 1;
        snapshot.windows[0].workspace_id = Some(11);
        snapshot.windows.pop();

        let corrections = WorkspacePolicy::default().corrections(&snapshot, now);
        assert_eq!(
            corrections,
            vec![
                Correction::MoveWindow {
                    window_id: 20,
                    workspace_id: 10,
                },
                Correction::MoveHomeToFirst { workspace_id: 10 },
            ]
        );
    }
}
