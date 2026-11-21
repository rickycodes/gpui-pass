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
            "Initializing…"
        } else {
            "Initialize store"
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
            .gap(px(18.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        self.state
                            .theme
                            .panel_title("Set up your password store"),
                    )
                    .child(self.state.theme.status_badge(if preview {
                        "DRY RUN"
                    } else {
                        "LOCAL ONLY"
                    })),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(22.))
                    .text_color(rgb(self.state.theme.colors.muted))
                    .child("This creates a standard pass store using existing GPG keys. No keys are generated, imported, or changed."),
            )
            .child(self.state.theme.label("STORE LOCATION"))
            .child(
                div()
                    .w_full()
                    .p(px(12.))
                    .rounded(px(self.state.theme.radii.field))
                    .border_1()
                    .border_color(rgb(self.state.theme.colors.border))
                    .bg(rgb(self.state.theme.colors.background))
                    .font_family(self.state.theme.mono_font)
                    .text_size(px(12.))
                    .text_color(rgb(self.state.theme.colors.ink))
                    .child(onboarding.path.display().to_string()),
            )
            .child(self.state.theme.label("GPG RECIPIENTS"))
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
                            .button("onboarding-open-store", "Open existing store", false)
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
                    .px(px(32.))
                    .pt(px(48.))
                    .pb(px(48.))
                    .child(panel),
            )
            .into_any_element()
    }
}
