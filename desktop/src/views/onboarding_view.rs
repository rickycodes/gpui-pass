use super::*;

impl PassApp {
    pub(super) fn onboarding_view(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(onboarding) = self.state.onboarding.as_ref() else {
            return div().into_any_element();
        };
        let pending = self.state.onboarding_pending.is_some();
        let preview = onboarding.preview;
        let initialize_label = if pending {
            i18n::text("initializing")
        } else {
            i18n::text("initialize-store")
        };
        let recipient_hint = "Use one or more existing GPG key IDs, separated by spaces.";
        let panel = self
            .state
            .theme
            .panel()
            .w(px(640.))
            .max_w(px(640.))
            .flex()
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
                            .panel_title(i18n::text("setup-store-title")),
                    )
                    .child(self.state.theme.status_badge(if preview {
                        i18n::text("dry-run")
                    } else {
                        i18n::text("local-only")
                    })),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(22.))
                    .text_color(rgb(self.state.theme.colors.muted))
                    .child("This creates a standard pass store using existing GPG keys. No keys are generated, imported, or changed."),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(self.state.theme.label(i18n::text("store-location")))
                    .child(
                        self.state
                            .theme
                            .button(
                                "onboarding-choose-location",
                                i18n::text("choose-store-location"),
                                false,
                            )
                            .on_click(cx.listener(|app, _, window, cx| {
                                app.choose_store_location(window, cx)
                            })),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .p(px(self.state.theme.spacing.field_padding))
                    .rounded(px(self.state.theme.radii.field))
                    .border_1()
                    .border_color(rgb(self.state.theme.colors.border))
                    .bg(rgb(self.state.theme.colors.background))
                    .font_family(self.state.theme.mono_font)
                    .text_size(px(12.))
                    .text_color(rgb(self.state.theme.colors.ink))
                    .child(onboarding.path.display().to_string()),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(self.state.theme.colors.muted))
                    .child(i18n::text("store-location-help")),
            )
            .child(self.state.theme.label(i18n::text("gpg-recipients")))
            .child(self.custom_entry_name_field(window, cx))
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .text_color(rgb(self.state.theme.colors.muted))
                    .child(recipient_hint),
            )
            .child(
                div()
                    .mt_auto()
                    .flex()
                    .justify_end()
                    .gap(px(10.))
                    .child(
                        self.state
                            .theme
                            .button("onboarding-open-store", i18n::text("open-existing-store"), false)
                            .on_click(cx.listener(|app, _, window, cx| {
                                app.open_store(window, cx)
                            })),
                    )
                    .child(
                        self.state
                            .theme
                            .button("onboarding-initialize", initialize_label, true)
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.initialize_onboarding(cx)
                            })),
                    ),
            );

        div()
            .id("onboarding-window")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(rgb(self.state.theme.colors.background))
            .child(self.titlebar("onboarding-titlebar", None, cx))
            .child(
                div()
                    .id("onboarding-content-scroll")
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
