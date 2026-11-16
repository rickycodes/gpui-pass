use super::*;

impl PassApp {
    pub(super) fn brand_logotype(&self) -> Div {
        div()
            .flex()
            .items_center()
            .gap(px(14.))
            .child(
                div()
                    .w(px(32.))
                    .h(px(32.))
                    .border_1()
                    .border_color(rgb(self.state.theme.colors.nav_foreground))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(16.))
                    .text_color(rgb(self.state.theme.colors.nav_foreground))
                    .font_family(self.state.theme.mono_font)
                    .child("/"),
            )
            .child(
                div()
                    .text_size(px(22.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(self.state.theme.colors.nav_foreground))
                    .child("pass"),
            )
    }
}
