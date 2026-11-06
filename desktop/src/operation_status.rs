//! Reusable presentation for progress and user-action hints from any background operation.
use crate::theme::{GREEN, LINE, MUTED, PAPER, TINT};
use gpui::{Div, FontWeight, div, prelude::*, px, rgb};
use gpui_pass_core::{OperationStatus, StatusKind};

pub fn panel(status: OperationStatus) -> Div {
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
        .border_color(rgb(LINE))
        .border_l_2()
        .bg(rgb(if attention { TINT } else { PAPER }))
        .child(
            div()
                .text_size(px(12.))
                .line_height(px(17.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(if attention { GREEN } else { MUTED }))
                .child(status.title),
        )
        .child(
            div()
                .text_size(px(11.))
                .line_height(px(17.))
                .text_color(rgb(MUTED))
                .child(status.detail),
        )
}
