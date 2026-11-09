use gpui::{Div, FontWeight, div, prelude::*, px, rgb};

pub const PAPER: u32 = 0xf5f4ef;
pub const WHITE: u32 = 0xfbfaf7;
pub const INK: u32 = 0x232d29;
pub const MUTED: u32 = 0x667069;
pub const LINE: u32 = 0xd9dcd4;
pub const GREEN: u32 = 0x23694c;
pub const TINT: u32 = 0xe4ece2;
pub const ERROR: u32 = 0x9d4035;
pub const MONO: &str = "DejaVu Sans Mono";

pub fn label(text: impl Into<gpui::SharedString>) -> Div {
    div()
        .text_size(px(10.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(MUTED))
        .child(text.into())
}

pub fn button(id: &'static str, text: &'static str, primary: bool) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .h(px(40.))
        .px(px(18.))
        .border_1()
        .border_color(rgb(if primary { GREEN } else { LINE }))
        .bg(rgb(if primary { GREEN } else { WHITE }))
        .text_color(rgb(if primary { WHITE } else { INK }))
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .hover(move |style| style.bg(rgb(if primary { 0x19543c } else { TINT })))
        .child(text)
}

pub fn danger_button(id: &'static str, text: &'static str) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .h(px(40.))
        .px(px(18.))
        .border_1()
        .border_color(rgb(0xe7c6c1))
        .bg(rgb(0xf8e9e6))
        .text_color(rgb(ERROR))
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .hover(|style| style.bg(rgb(0xf1d9d5)))
        .child(text)
}
