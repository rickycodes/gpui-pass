use super::*;

impl PassApp {
    pub(super) fn titlebar(
        &self,
        id: &'static str,
        center: Option<gpui::AnyElement>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let generator_open = self.state.navigation.generator_open();
        let right = match center {
            Some(center) => div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(center)
                .child(self.window_controls(cx))
                .into_any_element(),
            None => self.window_controls(cx),
        };
        div()
            .id(id)
            .w_full()
            .h(px(self.state.theme.spacing.chrome_height))
            .flex_shrink_0()
            .px(px(9.))
            .flex()
            .items_center()
            .justify_between()
            .bg(rgb(self.state.theme.colors.nav_background))
            .on_mouse_down(gpui::MouseButton::Left, |event, window, cx| {
                if event.click_count == 2 {
                    window.toggle_fullscreen();
                } else {
                    window.start_window_move();
                }
                cx.stop_propagation();
            })
            .child(
                div()
                    .id("home-logo")
                    .flex()
                    .items_center()
                    .ml(px(17.))
                    .cursor_pointer()
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |app, _, window, cx| {
                        app.state.device_verification.cancel();
                        app.state.navigation.go_to(Page::Entries);
                        if generator_open {
                            app.close_generator(cx);
                        } else {
                            cx.notify();
                        }
                        app.state.search_focus.focus(window);
                    }))
                    .child(self.brand_logotype()),
            )
            .child(right)
            .into_any_element()
    }

    pub(super) fn navigation_links(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .when(self.state.demo, |header| {
                header.child(
                    div()
                        .text_size(px(10.))
                        .font_family(self.state.theme.mono_font)
                        .text_color(rgb(self.state.theme.colors.nav_muted))
                        .child(i18n::text("demo")),
                )
            })
            .when(!self.state.demo && self.state.store.is_some(), |header| {
                header.child(
                    self.state
                        .theme
                        .nav_link("open-create-entry", i18n::text("new-entry"), false)
                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(
                            cx.listener(|app, _, window, cx| app.open_create_entry(window, cx)),
                        ),
                )
            })
            .when(
                self.state.yubikey_connection == YubiConnection::Connected,
                |header| {
                    header.child(
                        self.state
                            .theme
                            .nav_link(
                                "open-yubikey-status",
                                i18n::text("card-status"),
                                self.state.navigation.page() == Page::CardDetails,
                            )
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.state.navigation.go_to(Page::CardDetails);
                                cx.notify();
                            })),
                    )
                },
            )
            .child(
                self.state
                    .theme
                    .nav_link("refresh", i18n::text("refresh"), false)
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .on_click(cx.listener(|app, _, _, cx| app.refresh(cx))),
            )
            .into_any_element()
    }

    fn window_controls(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .flex()
            .items_center()
            .gap(px(9.))
            .child(self.window_control_button("minimize-window", false, cx))
            .child(self.window_control_button("close-window", true, cx))
            .into_any_element()
    }

    fn window_control_button(
        &self,
        id: &'static str,
        close: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let foreground = self.state.theme.colors.nav_foreground;
        let border = self.state.theme.colors.nav_border;
        let hover = if close {
            self.state.theme.colors.danger_hover
        } else {
            self.state.theme.colors.nav_hover
        };
        let button = div()
            .id(id)
            .size(px(30.))
            .border_1()
            .border_color(rgb(border))
            .flex()
            .items_center()
            .justify_center()
            .text_color(rgb(foreground))
            .cursor_pointer()
            .hover(move |style| style.bg(rgb(hover)))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.stop_propagation()),
            )
            .child(window_control_glyph(close, foreground));
        if close {
            button
                .on_click(cx.listener(|_, _, _, cx| cx.quit()))
                .into_any_element()
        } else {
            button
                .on_click(cx.listener(|_, _, window, _| window.minimize_window()))
                .into_any_element()
        }
    }
}
