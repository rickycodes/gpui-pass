use super::*;
use crate::generator_view::CREATE_CONTENT_HEIGHT;

impl PassApp {
    pub(super) fn generator_content_area(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if self.state.generator_mode == GeneratorMode::PassDefault {
            div()
                .w_full()
                .h(px(CREATE_CONTENT_HEIGHT))
                .flex()
                .items_start()
                .justify_center()
                .child(
                    div()
                        .w_full()
                        .rounded(px(self.state.theme.radii.field))
                        .border_1()
                        .border_color(rgb(self.state.theme.colors.accent))
                        .bg(rgb(self.state.theme.colors.accent_surface))
                        .p(px(12.))
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .text_color(rgb(self.state.theme.colors.ink))
                        .child(i18n::text("pass-default-help")),
                )
                .into_any_element()
        } else {
            div()
                .w_full()
                .id("create-entry-content")
                .h(px(CREATE_CONTENT_HEIGHT))
                .overflow_y_scroll()
                .child(self.generator_name_field(window, cx))
                .into_any_element()
        }
    }

    pub(super) fn generator_mode_buttons(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                self.state
                    .theme
                    .button(
                        "mode-new",
                        i18n::text("new-entry"),
                        self.state.generator_mode == GeneratorMode::New,
                    )
                    .on_click(cx.listener(|app, _, _, cx| {
                        app.select_generator_mode(GeneratorMode::New, cx);
                    })),
            )
            .child(
                self.state
                    .theme
                    .button(
                        "mode-memorable",
                        i18n::text("memorable-phrase"),
                        self.state.generator_mode == GeneratorMode::Memorable,
                    )
                    .on_click(cx.listener(|app, _, _, cx| {
                        app.select_generator_mode(GeneratorMode::Memorable, cx);
                    })),
            )
            .child(
                self.state
                    .theme
                    .button(
                        "mode-pass-default",
                        i18n::text("pass-default"),
                        self.state.generator_mode == GeneratorMode::PassDefault,
                    )
                    .on_click(cx.listener(|app, _, _, cx| {
                        app.select_generator_mode(GeneratorMode::PassDefault, cx);
                    })),
            )
    }

    pub(super) fn generator_memorable_options(&self, cx: &mut Context<Self>) -> Div {
        let options = self.generator_options();
        let slider_left = ((self.state.generation_words - MIN_MEMORABLE_WORDS) as f32
            / (MAX_MEMORABLE_WORDS - MIN_MEMORABLE_WORDS) as f32)
            * 360.;
        let entity = cx.entity().clone();
        let slider_track = div()
            .id("generator-strength-slider")
            .relative()
            .w(px(360.))
            .h(px(26.))
            .cursor_pointer()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|app, event: &gpui::MouseDownEvent, _, cx| {
                    app.set_generator_strength_from_x(event.position.x, cx);
                }),
            )
            .on_mouse_move(cx.listener(|app, event: &gpui::MouseMoveEvent, _, cx| {
                if event.dragging() {
                    app.set_generator_strength_from_x(event.position.x, cx);
                }
            }))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(11.))
                    .h(px(3.))
                    .bg(rgb(self.state.theme.colors.border)),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(11.))
                    .w(px(slider_left))
                    .h(px(3.))
                    .bg(rgb(self.state.theme.colors.accent)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(slider_left - 7.))
                    .top(px(5.))
                    .size(px(15.))
                    .border_1()
                    .border_color(rgb(self.state.theme.colors.accent))
                    .bg(rgb(self.state.theme.colors.surface)),
            )
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, (), _, cx| {
                        entity.update(cx, |app, _| {
                            app.state.generator_slider_bounds = Some(bounds)
                        });
                    },
                )
                .absolute()
                .size_full(),
            );
        let slider = div()
            .w(px(360.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(slider_track)
            .child(
                div()
                    .flex()
                    .justify_between()
                    .font_family(self.state.theme.mono_font)
                    .text_size(px(9.))
                    .text_color(rgb(self.state.theme.colors.muted))
                    .child(format!("{MIN_MEMORABLE_WORDS} WORDS"))
                    .child(format!("{MAX_MEMORABLE_WORDS} WORDS")),
            );
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(16.))
                    .child(slider)
                    .child(
                        div()
                            .min_w(px(126.))
                            .font_family(self.state.theme.mono_font)
                            .text_size(px(11.))
                            .text_color(rgb(self.state.theme.colors.accent))
                            .child(format!(
                                "{} WORDS  ·  {:.0} BITS",
                                options.words,
                                options.entropy_bits()
                            )),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .child(self.generator_toggle(
                        "toggle-generator-digits",
                        self.state.generation_digits,
                        true,
                        "append-digit",
                        cx,
                    ))
                    .child(self.generator_toggle(
                        "toggle-generator-separators",
                        self.state.generation_separators,
                        false,
                        "separate-words",
                        cx,
                    )),
            )
    }

    pub(super) fn generator_toggle(
        &self,
        id: &'static str,
        checked: bool,
        toggle_digits: bool,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        div()
            .id(id)
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(px(12.))
            .text_color(rgb(self.state.theme.colors.muted))
            .cursor_pointer()
            .on_click(cx.listener(move |app, _, _, cx| {
                if toggle_digits {
                    app.state.generation_digits = !app.state.generation_digits;
                } else {
                    app.state.generation_separators = !app.state.generation_separators;
                }
                cx.notify();
            }))
            .child(
                div()
                    .size(px(22.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_2()
                    .border_color(rgb(if checked {
                        self.state.theme.colors.accent
                    } else {
                        self.state.theme.colors.border
                    }))
                    .bg(rgb(if checked {
                        self.state.theme.colors.accent_surface
                    } else {
                        self.state.theme.colors.background
                    }))
                    .text_size(px(16.))
                    .text_color(rgb(self.state.theme.colors.accent))
                    .child(if checked { "✓" } else { "" }),
            )
            .child(i18n::text(label))
            .into_any_element()
    }

    pub(super) fn generator_footer(&self, cx: &mut Context<Self>) -> Div {
        let mut footer = div()
            .mt_auto()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(8.));
        if self.state.generator_mode == GeneratorMode::Memorable {
            footer = footer.child(
                self.state
                    .theme
                    .button("generate-into-contents", i18n::text("generate"), false)
                    .on_click(cx.listener(|app, _, _, cx| app.generate_into_contents(cx))),
            );
        }
        let save_label = if self.state.saving.is_some() {
            if self.state.generator_mode == GeneratorMode::PassDefault {
                i18n::text("custom-entry-generating")
            } else {
                i18n::text("custom-entry-saving")
            }
        } else if self.state.generator_mode == GeneratorMode::PassDefault {
            i18n::text("custom-entry-generate-save")
        } else {
            i18n::text("custom-entry-save")
        };
        footer.child(
            self.state
                .theme
                .button("save-entry", save_label, true)
                .on_click(cx.listener(|app, _, _, cx| app.save_entry(cx))),
        )
    }
}
