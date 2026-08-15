use foyer_shell_protocol::{SlideTree, SlideTreeNode};
use gpui::{App, AppContext, Entity, IntoElement, ParentElement, SharedString, Styled, div, px};
use gpui_component::{
    list::ListItem,
    tree::{Tree, TreeItem, TreeState},
};

pub struct TreeSurfaceState {
    pub tree: Entity<TreeState>,
}

impl TreeSurfaceState {
    pub fn new(spec: &SlideTree, cx: &mut App) -> Self {
        let items = spec.nodes.iter().map(tree_item).collect::<Vec<_>>();
        Self {
            tree: cx.new(|cx| TreeState::new(cx).items(items)),
        }
    }
}

fn tree_item(node: &SlideTreeNode) -> TreeItem {
    TreeItem::new(node.id.clone(), node.label.clone())
        .expanded(node.expanded)
        .children(node.children.iter().map(tree_item))
}

pub struct TreeSurface;

impl TreeSurface {
    pub fn render(state: &TreeSurfaceState) -> impl IntoElement {
        Tree::new(&state.tree, |index, entry, selected, _, _| {
            let marker: SharedString = if entry.is_folder() {
                if entry.is_expanded() { "▾" } else { "▸" }
            } else {
                ""
            }
            .into();
            ListItem::new(index)
                .px_2()
                .py_1()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .pl(px(entry.depth() as f32 * 14.0))
                        .text_sm()
                        .child(div().w(px(12.0)).child(marker))
                        .child(entry.item().label.clone()),
                )
                .selected(selected)
        })
        .size_full()
    }
}
