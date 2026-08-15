use foyer_shell_protocol::{CodeFile, CodeStep, SlideCode};
use gpui::{
    App, AppContext, Entity, FocusHandle, InteractiveElement, IntoElement, Keystroke,
    ParentElement, Styled, Window, div, prelude::FluentBuilder, px, rgb,
};
use gpui_component::{
    input::{Editor, EditorState, Position},
    list::ListItem,
    tree::{Tree, TreeItem, TreeState},
};
use std::collections::BTreeMap;

pub struct CodeSurfaceState {
    pub editor: Entity<EditorState>,
    pub explorer: Entity<TreeState>,
    presentation_focus: FocusHandle,
    active_file_id: String,
    canonical_content: String,
    applied_step_id: Option<String>,
}

impl CodeSurfaceState {
    pub fn new(code: &SlideCode, window: &mut Window, cx: &mut App) -> Self {
        let files = files(code);
        let active = files
            .first()
            .cloned()
            .unwrap_or_else(|| fallback_file(code));
        let language = language_name(&active.language);
        let content = active.content.clone();
        let editor = cx.new(|cx| {
            EditorState::new(language, window, cx)
                .line_number(true)
                .default_value(content.clone())
        });
        let editor_base = editor.read(cx).base_state().clone();
        editor_base.update(cx, |editor, cx| {
            editor.set_soft_wrap(false, window, cx);
            editor.set_indent_guides(true, window, cx);
        });
        let explorer_items = file_tree(&files);
        let explorer = cx.new(|cx| TreeState::new(cx).items(explorer_items));
        Self {
            editor,
            explorer,
            presentation_focus: cx.focus_handle(),
            active_file_id: active.id,
            canonical_content: active.content,
            applied_step_id: None,
        }
    }

    pub fn sync(
        &mut self,
        code: &SlideCode,
        step: Option<&CodeStep>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let files = files(code);
        let selected_file = self
            .explorer
            .read(cx)
            .selected_entry()
            .and_then(|entry| {
                files
                    .iter()
                    .find(|file| file.id == entry.item().id.as_ref())
            })
            .cloned();
        let requested_file = step
            .and_then(|step| step.file_id.as_deref())
            .and_then(|id| files.iter().find(|file| file.id == id))
            .cloned();
        let file = requested_file
            .or(selected_file)
            .or_else(|| {
                files
                    .iter()
                    .find(|file| file.id == self.active_file_id)
                    .cloned()
            })
            .or_else(|| files.first().cloned())
            .unwrap_or_else(|| fallback_file(code));
        let step_id = step.map(|step| step.id.clone());
        let changed_file = file.id != self.active_file_id;
        let was_edited = self.editor.read(cx).value().as_ref() != self.canonical_content;
        if changed_file || was_edited {
            self.active_file_id = file.id.clone();
            self.canonical_content = file.content.clone();
            self.editor.update(cx, |editor, cx| {
                editor.set_value(file.content.clone(), window, cx);
            });
        }
        if changed_file || step_id != self.applied_step_id {
            if let Some(step) = step {
                let line_count = file.content.lines().count().max(1);
                let start_line = usize::from(step.start_line).max(1).min(line_count);
                let end_line = usize::from(step.end_line).max(start_line).min(line_count);
                let editor = self.editor.clone();
                let presentation_focus = self.presentation_focus.clone();

                // Input installs its action handlers while rendering. Apply the range on the
                // following frame so even the first narration beat gets a real selection rather
                // than just a caret parked on its first line.
                window.on_next_frame(move |window, cx| {
                    editor.update(cx, |editor, cx| {
                        let base = editor.base_state().clone();
                        base.update(cx, |editor, cx| {
                            editor.set_cursor_position(
                                Position::new(start_line.saturating_sub(1) as u32, 0),
                                window,
                                cx,
                            );
                        });
                    });
                    for _ in start_line..end_line {
                        dispatch_keystroke(window, cx, "shift-down");
                    }
                    dispatch_keystroke(window, cx, "shift-end");

                    // Preserve the selection paint but remove the blinking editor caret. This
                    // surface is a narrated reader, not an editing target.
                    presentation_focus.focus(window, cx);
                });
            }
            self.applied_step_id = step_id;
        }
    }

    pub fn active_path<'a>(&self, code: &'a SlideCode) -> &'a str {
        code.files
            .iter()
            .find(|file| file.id == self.active_file_id)
            .map(|file| file.path.as_str())
            .unwrap_or("PRESENTATION")
    }
}

pub struct CodeSurface;

impl CodeSurface {
    pub fn render(
        state: &CodeSurfaceState,
        code: &SlideCode,
        step: Option<&CodeStep>,
    ) -> impl IntoElement {
        let show_explorer = code.show_explorer && code.files.len() > 1;
        let label = step
            .and_then(|step| step.label.clone())
            .unwrap_or_else(|| "Code walkthrough".into());
        div()
            .track_focus(&state.presentation_focus)
            .size_full()
            .flex()
            .gap_3()
            .when(show_explorer, |surface| {
                surface.child(
                    div()
                        .w(px(250.0))
                        .h_full()
                        .overflow_hidden()
                        .rounded(px(18.0))
                        .border_1()
                        .border_color(rgb(0x303036))
                        .bg(rgb(0x141416))
                        .p_2()
                        .child(
                            Tree::new(&state.explorer, |index, entry, selected, _, _| {
                                let marker = if entry.is_folder() {
                                    if entry.is_expanded() { "▾" } else { "▸" }
                                } else {
                                    ""
                                };
                                ListItem::new(index)
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .pl(px(entry.depth() as f32 * 12.0))
                                            .text_sm()
                                            .child(div().w(px(12.0)).child(marker))
                                            .child(entry.item().label.clone()),
                                    )
                                    .selected(selected)
                            })
                            .size_full(),
                        ),
                )
            })
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w(px(0.0))
                    .h_full()
                    .overflow_hidden()
                    .rounded(px(18.0))
                    .border_1()
                    .border_color(rgb(0x303036))
                    .bg(rgb(0x111113))
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .h(px(42.0))
                            .px_4()
                            .flex()
                            .items_center()
                            .justify_between()
                            .border_b_1()
                            .border_color(rgb(0x27272a))
                            .text_xs()
                            .text_color(rgb(0xa1a1aa))
                            .child(state.active_path(code).to_string())
                            .child(label),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px(43.0))
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .child(
                                Editor::new(&state.editor)
                                    .appearance(false)
                                    .bordered(false)
                                    .disabled(true)
                                    .h_full(),
                            ),
                    ),
            )
    }
}

fn dispatch_keystroke(window: &mut Window, cx: &mut App, source: &str) {
    if let Ok(keystroke) = Keystroke::parse(source) {
        window.dispatch_keystroke(keystroke, cx);
    }
}

fn fallback_file(code: &SlideCode) -> CodeFile {
    CodeFile {
        id: "primary".into(),
        path: "presentation".into(),
        language: code.language.clone(),
        content: code.content.clone(),
    }
}

fn files(code: &SlideCode) -> Vec<CodeFile> {
    if code.files.is_empty() {
        vec![fallback_file(code)]
    } else {
        code.files.clone()
    }
}

fn language_name(language: &str) -> String {
    match language.to_ascii_lowercase().as_str() {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "py" => "python",
        other if other.is_empty() => "text",
        other => other,
    }
    .to_string()
}

fn file_tree(files: &[CodeFile]) -> Vec<TreeItem> {
    #[derive(Default)]
    struct Folder {
        directories: BTreeMap<String, Folder>,
        files: Vec<(String, String)>,
    }

    fn items(folder: Folder, prefix: &str) -> Vec<TreeItem> {
        let mut result = folder
            .directories
            .into_iter()
            .map(|(name, child)| {
                let path = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}/{name}")
                };
                TreeItem::new(format!("folder:{path}"), name)
                    .expanded(true)
                    .children(items(child, &path))
            })
            .collect::<Vec<_>>();
        result.extend(
            folder
                .files
                .into_iter()
                .map(|(id, label)| TreeItem::new(id, label)),
        );
        result
    }

    let mut root = Folder::default();
    for file in files {
        let mut segments = file
            .path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .peekable();
        let mut folder = &mut root;
        while let Some(segment) = segments.next() {
            if segments.peek().is_none() {
                folder.files.push((file.id.clone(), segment.to_string()));
            } else {
                folder = folder.directories.entry(segment.to_string()).or_default();
            }
        }
    }
    items(root, "")
}
