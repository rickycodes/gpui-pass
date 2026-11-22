use super::*;

impl PassApp {
    pub(super) fn overwrite_confirmation_view(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(pending) = &self.state.confirming_overwrite else {
            return div().into_any_element();
        };
        div()
            .id("overwrite-confirmation")
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bottom_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(self.state.theme.colors.background))
            .child(
                self.state
                    .theme
                    .panel()
                    .w(px(520.))
                    .max_w(px(520.))
                    .p(px(32.))
                    .flex()
                    .flex_col()
                    .gap(px(20.))
                    .child(
                        div()
                            .text_size(px(18.))
                            .line_height(px(24.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(i18n::text("overwrite-entry-title")),
                    )
                    .child(
                        div()
                            .text_size(px(24.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(pending.entry.name().to_owned()),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .text_color(rgb(self.state.theme.colors.muted))
                            .child(i18n::text("overwrite-entry-help")),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(10.))
                            .child(
                                self.state
                                    .theme
                                    .button("cancel-overwrite", i18n::text("cancel"), false)
                                    .on_click(cx.listener(|app, _, _, cx| {
                                        app.state.confirming_overwrite = None;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                self.state
                                    .theme
                                    .danger_button(
                                        "confirm-overwrite",
                                        i18n::text("overwrite-entry-confirm"),
                                    )
                                    .on_click(
                                        cx.listener(|app, _, _, cx| app.confirm_overwrite(cx)),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }
}
