use super::*;

impl PassApp {
    pub(super) fn brand_mark(&self, size: f32, color: u32) -> Div {
        div()
            .size(px(size))
            .border_1()
            .border_color(rgb(color))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(size / 2.))
            .text_color(rgb(color))
            .font_family(self.state.theme.mono_font)
            .child("/")
    }

    pub(super) fn brand_logotype(&self) -> Div {
        div()
            .flex()
            .items_center()
            .gap(px(14.))
            .child(self.brand_mark(32., self.state.theme.colors.nav_foreground))
            .child(
                div()
                    .text_size(px(22.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(self.state.theme.colors.nav_foreground))
                    .child("pass"),
            )
    }
}
