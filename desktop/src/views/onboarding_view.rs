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
        let recipient_hint = concat!(
            "If no email is suggested, run `gpg --list-secret-keys --keyid-format LONG` and enter a key's email or ID. ",
            "If no key is listed, create or import one first; gpui-pass doesn't manage keys."
        );
        let choices_open = self.state.onboarding_recipient_picker_open;
        let choice_count = self.state.onboarding_recipient_choices.len();
        let picker_bounds = self.state.onboarding_recipient_picker_bounds;
        let picker_width = picker_bounds.map_or(px(596.), |bounds| bounds.size.width);
        let recipient_picker_label = if choices_open {
            i18n::text("gpg-recipient-hide-choices")
        } else {
            let mut args = FluentArgs::new();
            args.set("count", choice_count as i64);
            i18n::text_with_args("gpg-recipient-choices", &args)
        };
        let picker_entity = cx.entity().clone();
        let recipient_picker_button = self
            .state
            .theme
            .button("onboarding-recipient-picker", recipient_picker_label, false)
            .w(px(260.))
            .on_click(cx.listener(|app, _, _, cx| {
                app.state.onboarding_recipient_picker_open =
                    !app.state.onboarding_recipient_picker_open;
                cx.notify();
                cx.stop_propagation();
            }));
        let recipient_picker = div()
            .w_full()
            .relative()
            .flex()
            .items_center()
            .justify_start()
            .child(recipient_picker_button)
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, (), _, cx| {
                        picker_entity.update(cx, |app, cx| {
                            if app.state.onboarding_recipient_picker_bounds != Some(bounds) {
                                app.state.onboarding_recipient_picker_bounds = Some(bounds);
                                cx.notify();
                            }
                        });
                    },
                )
                .absolute()
                .size_full(),
            );
        let mut recipient_choices = div()
            .id("onboarding-recipient-choices")
            .w(picker_width)
            .max_h(px(240.))
            .overflow_y_scroll()
            .min_h_0()
            .flex()
            .flex_col()
            .rounded(px(self.state.theme.radii.control))
            .border_1()
            .border_color(rgb(self.state.theme.colors.border))
            .bg(rgb(self.state.theme.colors.surface));
        if choices_open {
            for (index, choice) in self.state.onboarding_recipient_choices.iter().enumerate() {
                let label = choice.label.clone();
                let fingerprint = choice.fingerprint.clone();
                let hover_color = self.state.theme.colors.hover;
                let surface_color = self.state.theme.colors.surface;
                let border_color = self.state.theme.colors.border;
                let muted_color = self.state.theme.colors.muted;
                let mono_font = self.state.theme.mono_font;
                let row =
                    div()
                        .id(("onboarding-recipient-choice", index))
                        .w_full()
                        .p(px(8.))
                        .bg(rgb(surface_color))
                        .cursor_pointer()
                        .hover(move |style| style.bg(rgb(hover_color)))
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.select_onboarding_recipient(index, cx)
                        }))
                        .child(
                            div().flex().flex_col().gap(px(2.)).child(label).child(
                                div()
                                    .font_family(mono_font)
                                    .text_size(px(10.))
                                    .text_color(rgb(muted_color))
                                    .child(fingerprint),
                            ),
                        )
                        .when(index + 1 < choice_count, |row| {
                            row.border_b_1().border_color(rgb(border_color))
                        });
                recipient_choices = recipient_choices.child(row);
            }
        }
        let panel = self
            .state
            .theme
            .panel()
            .pb(px(0.))
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
            .when(choice_count > 0, |panel| panel.child(recipient_picker))
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .text_color(rgb(self.state.theme.colors.muted))
                    .child(recipient_hint),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .justify_end()
                    .gap(px(10.))
                    .child(
                        self.state
                            .theme
                            .button(
                                "onboarding-open-store",
                                i18n::text("open-existing-store"),
                                false,
                            )
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
            )
            .child(
                div()
                    .h(px(self.state.theme.spacing.panel_padding))
                    .flex_shrink_0(),
            );

        div()
            .id("onboarding-window")
            .on_click(cx.listener(|app, _, _, cx| {
                if app.state.onboarding_recipient_picker_open {
                    app.state.onboarding_recipient_picker_open = false;
                    cx.notify();
                }
            }))
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
            .when(choices_open && picker_bounds.is_some(), |root| {
                let bounds = picker_bounds.expect("picker bounds checked");
                root.child(
                    gpui::anchored()
                        .anchor(gpui::Corner::TopLeft)
                        .position(gpui::point(bounds.origin.x, bounds.bottom() - px(2.)))
                        .position_mode(gpui::AnchoredPositionMode::Window)
                        .child(recipient_choices),
                )
            })
            .into_any_element()
    }
}
