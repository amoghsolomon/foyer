use foyer_shell_ui::tokens;
use gpui::{AnyElement, FontWeight, SharedString, div, prelude::*, px, rgb};
use gpui_component::{Icon, Sizable, scroll::ScrollableElement, v_flex};

pub(super) fn content_column() -> gpui_component::scroll::Scrollable<gpui::Div> {
    v_flex().size_full().overflow_y_scrollbar().p_5().gap_4()
}

pub(super) fn control_card() -> gpui::Div {
    v_flex()
        .gap_4()
        .p_4()
        .rounded_lg()
        .border_1()
        .border_color(rgb(tokens::BORDER))
        .bg(rgb(tokens::SURFACE))
}

pub(super) fn section_label(label: &'static str) -> gpui::Div {
    div()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(tokens::SUBTLE))
        .child(label)
}

pub(super) fn muted_text(text: impl Into<SharedString>) -> gpui::Div {
    div()
        .text_sm()
        .text_color(rgb(tokens::MUTED))
        .child(text.into())
}

pub(super) fn error_text(error: impl Into<SharedString>) -> gpui::Div {
    div()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(rgb(tokens::FOREGROUND))
        .text_xs()
        .child(error.into())
}

pub(super) fn empty_state(
    icon: Icon,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
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

pub(super) enum ReplicaBanner {
    Offline { pending: usize },
    Pending { pending: usize },
    Stale { message: String },
    Error { message: String },
}

pub(super) fn replica_status_elements(
    using_powersync: bool,
    fallback: &'static str,
    banner: Option<ReplicaBanner>,
) -> Vec<AnyElement> {
    let mut children = vec![
        section_label("STATUS").into_any_element(),
        muted_text(if using_powersync {
            "Reading the PowerSync replica."
        } else {
            fallback
        })
        .into_any_element(),
    ];
    match banner {
        Some(ReplicaBanner::Offline { pending }) => {
            children.push(
                error_text(if pending == 0 {
                    "Offline. Reading the local replica. Changes will upload when Foyer Server is reachable."
                        .to_string()
                } else {
                    format!(
                        "Offline. {pending} change(s) are queued and will upload when you are back online."
                    )
                })
                .into_any_element(),
            );
        }
        Some(ReplicaBanner::Pending { pending }) => {
            children.push(
                muted_text(format!(
                    "Pending sync. {pending} change(s) are waiting to upload to Foyer Server."
                ))
                .into_any_element(),
            );
        }
        Some(ReplicaBanner::Stale { message }) => {
            children.push(error_text(format!("Stale revision. {message}")).into_any_element());
        }
        Some(ReplicaBanner::Error { message }) => {
            children.push(error_text(format!("Couldn’t sync. {message}")).into_any_element());
        }
        None => {
            children.push(muted_text("Synced").into_any_element());
        }
    }
    children
}
