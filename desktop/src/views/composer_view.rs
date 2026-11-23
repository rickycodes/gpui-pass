use super::*;
use crate::composer_mode::{
    CREATE_CONTENT_LABEL_HEIGHT, CREATE_OPTIONS_HEIGHT, CREATE_PANEL_MIN_HEIGHT,
};

impl PassApp {
    pub(super) fn composer_titlebar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        self.titlebar("composer-titlebar", None, cx)
    }

    pub(super) fn composer_view(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let panel = self
            .state
            .theme
            .panel()
            .w(px(640.))
            .max_w(px(640.))
            .p(px(36.))
            .min_h(px(CREATE_PANEL_MIN_HEIGHT))
            .flex()
            .flex_shrink_0()
            .flex_col()
            .gap(px(self.state.theme.spacing.section_gap))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        self.state
                            .theme
                            .panel_title(i18n::text("custom-entry-title")),
                    )
                    .child(
                        self.state
                            .theme
                            .button("close-composer", i18n::text("back"), false)
                            .on_click(cx.listener(|app, _, _, cx| app.close_composer(cx))),
                    ),
            )
            .child(self.state.theme.label(i18n::text("entry-name")))
            .child(self.custom_entry_name_field(window, cx))
            .when(
                self.state.composer_mode != ComposerMode::PassDefault,
                |panel| panel.child(self.state.theme.label(i18n::text("custom-entry-contents"))),
            )
            .when(
                self.state.composer_mode == ComposerMode::PassDefault,
                |panel| panel.child(div().h(px(CREATE_CONTENT_LABEL_HEIGHT))),
            )
            .child(self.composer_content_area(window, cx))
            .child(self.composer_mode_buttons(cx))
            .when(
                self.state.composer_mode == ComposerMode::Memorable,
                |panel| panel.child(self.composer_memorable_options(cx)),
            )
            .when(
                self.state.composer_mode != ComposerMode::Memorable,
                |panel| panel.child(div().h(px(CREATE_OPTIONS_HEIGHT))),
            )
            .child(self.composer_footer(cx));

        div()
            .id("composer-window")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(rgb(self.state.theme.colors.background))
            .child(self.composer_titlebar(cx))
            .child(
                div()
                    .id("composer-content-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .items_start()
                    .justify_center()
                    .px(px(self.state.theme.spacing.composer_horizontal))
                    .pt(px(self.state.theme.spacing.composer_vertical))
                    .pb(px(self.state.theme.spacing.composer_vertical))
                    .child(panel),
            )
            .into_any_element()
    }
}
