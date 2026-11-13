//! Reusable presentation for progress and user-action hints from any background operation.
use crate::theme::Theme;
use gpui::{Div, FontWeight, div, prelude::*, px, rgb};
use gpui_pass_core::{OperationStatus, StatusKind};

pub fn panel(status: OperationStatus, theme: &Theme) -> Div {
    let attention = status.kind == StatusKind::Attention;
    div()
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .gap(px(3.))
        .px(px(14.))
        .py(px(10.))
        .border_1()
        .border_color(rgb(theme.colors.border))
        .border_l_2()
        .bg(rgb(if attention { theme.colors.accent_surface } else { theme.colors.background }))
        .child(
            div()
                .text_size(px(12.))
                .line_height(px(17.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(if attention { theme.colors.accent } else { theme.colors.muted }))
                .child(status.title),
        )
        .child(
            div()
                .text_size(px(11.))
                .line_height(px(17.))
                .text_color(rgb(theme.colors.muted))
                .child(status.detail),
        )
}
