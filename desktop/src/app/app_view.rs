use super::*;

impl Render for PassApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render_sample = diagnostics::measure_render();
        self.advance_sidebar_scroll(window);
        let main_content = if self.state.navigation.generator_open() {
            self.generator_view(window, cx)
        } else if self.state.navigation.page() == Page::CardDetails {
            self.yubikey_view(cx)
        } else {
            div()
                .flex()
                .flex_1()
                .min_h_0()
                .child(self.sidebar(window, cx))
                .child(self.details(window, cx))
                .into_any_element()
        };
        let status = self
            .state
            .session
            .pending_status()
            .or_else(|| {
                self.state
                    .copy_pending
                    .as_ref()
                    .map(|pending| pending.progress.snapshot())
            })
            .or_else(|| {
                self.state
                    .saving
                    .as_ref()
                    .map(|(_, progress, _)| progress.snapshot())
            });
        let footer = if let Some(message) = &self.state.message {
            notification_bar(&self.state.theme, message.clone())
        } else if let Some(status) = status {
            notification_bar(
                &self.state.theme,
                format!("{} · {}", status.title, status.detail),
            )
        } else if self.state.device_verification.running()
            || self.state.verification_notification_until.is_some()
        {
            notification_bar(
                &self.state.theme,
                self.state.device_verification.state().message(),
            )
        } else if let Some(seconds) = self.state.clipboard.remaining_seconds(Instant::now()) {
            let entry_name = self.state.clipboard.source().unwrap_or("entry");
            div()
                .h(px(48.))
                .flex_shrink_0()
                .px(px(24.))
                .flex()
                .items_center()
                .justify_between()
                .border_t_1()
                .border_color(rgb(self.state.theme.colors.border))
                .bg(rgb(self.state.theme.colors.surface))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .text_size(px(11.))
                        .text_color(rgb(self.state.theme.colors.ink))
                        .child({
                            let mut args = FluentArgs::new();
                            args.set("entry", entry_name);
                            i18n::text_with_args("copied-entry", &args)
                        })
                        .child(div().text_color(rgb(self.state.theme.colors.muted)).child({
                            let mut args = FluentArgs::new();
                            args.set("seconds", seconds.min(i64::MAX as u64) as i64);
                            i18n::text_with_args("clipboard-restores-in", &args)
                        })),
                )
                .child(
                    self.state
                        .theme
                        .button("restore-clipboard-now", "Restore now", false)
                        .h(px(28.))
                        .on_click(cx.listener(|app, _, _, cx| app.restore_pass_clipboard(cx))),
                )
        } else {
            div()
                .h(px(48.))
                .flex_shrink_0()
                .px(px(24.))
                .flex()
                .items_center()
                .border_t_1()
                .border_color(rgb(self.state.theme.colors.border))
                .gap(px(18.))
                .overflow_hidden()
                .child(
                    self.state
                        .theme
                        .shortcut("Ctrl F", i18n::text("shortcut-search")),
                )
                .child(
                    self.state
                        .theme
                        .shortcut("↑ ↓", i18n::text("shortcut-browse")),
                )
                .child(
                    self.state
                        .theme
                        .shortcut("Enter", i18n::text("shortcut-reveal")),
                )
                .child(
                    self.state
                        .theme
                        .shortcut("Ctrl Shift C", i18n::text("shortcut-copy")),
                )
                .child(
                    self.state
                        .theme
                        .shortcut("Ctrl G", i18n::text("shortcut-generate")),
                )
                .child(
                    self.state
                        .theme
                        .shortcut("Esc", i18n::text("shortcut-hide")),
                )
        };
        app_actions::bind_actions(
            div()
                .size_full()
                .flex()
                .flex_col()
                .bg(rgb(self.state.theme.colors.background))
                .text_color(rgb(self.state.theme.colors.ink))
                .font_family("Inter")
                .text_size(px(15.))
                .key_context("Pass")
                .track_focus(&self.state.focus),
            cx,
        )
        .when(!self.state.navigation.generator_open(), |view| {
            view.child(
                div()
                    .id("titlebar")
                    .w_full()
                    .h(px(48.))
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
                            .flex()
                            .items_center()
                            .id("home-logo")
                            .ml(px(17.))
                            .cursor_pointer()
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation()
                            })
                            .on_click(cx.listener(|app, _, window, cx| {
                                app.state.device_verification.cancel();
                                app.state.navigation.go_to(Page::Entries);
                                app.state.search_focus.focus(window);
                                cx.notify();
                            }))
                            .child(self.brand_logotype()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .font_family(self.state.theme.mono_font)
                                    .text_color(rgb(self.state.theme.colors.nav_muted))
                                    .child(if self.state.demo { "DEMO" } else { "LOCAL" }),
                            )
                            .when(!self.state.demo && self.state.store.is_some(), |header| {
                                header.child(
                                    self.state
                                        .theme
                                        .nav_link(
                                            "open-custom-entry",
                                            i18n::text("new-entry"),
                                            false,
                                        )
                                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation()
                                        })
                                        .on_click(cx.listener(|app, _, window, cx| {
                                            app.open_custom_entry(window, cx)
                                        })),
                                )
                            })
                            .child(
                                self.state
                                    .theme
                                    .nav_link("open-generator", i18n::text("generate"), false)
                                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation();
                                    })
                                    .on_click(cx.listener(|app, _, window, cx| {
                                        app.open_generator(window, cx)
                                    })),
                            )
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
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(9.))
                                    .child(
                                        div()
                                            .id("minimize-window")
                                            .size(px(30.))
                                            .border_1()
                                            .border_color(rgb(self.state.theme.colors.nav_border))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(17.))
                                            .text_color(rgb(self.state.theme.colors.nav_foreground))
                                            .cursor_pointer()
                                            .hover(|style| {
                                                style.bg(rgb(self.state.theme.colors.nav_hover))
                                            })
                                            .on_mouse_down(
                                                gpui::MouseButton::Left,
                                                cx.listener(|_, _, _, cx| cx.stop_propagation()),
                                            )
                                            .on_click(cx.listener(|_, _, window, _| {
                                                window.minimize_window()
                                            }))
                                            .child("−"),
                                    )
                                    .child(
                                        div()
                                            .id("close-window")
                                            .size(px(30.))
                                            .border_1()
                                            .border_color(rgb(self.state.theme.colors.nav_border))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(20.))
                                            .text_color(rgb(self.state.theme.colors.nav_foreground))
                                            .cursor_pointer()
                                            .hover(|style| {
                                                style.bg(rgb(self.state.theme.colors.danger_hover))
                                            })
                                            .on_mouse_down(
                                                gpui::MouseButton::Left,
                                                cx.listener(|_, _, _, cx| cx.stop_propagation()),
                                            )
                                            .on_click(cx.listener(|_, _, _, cx| cx.quit()))
                                            .child("×"),
                                    ),
                            ),
                    ),
            )
        })
        .child(main_content)
        .child(footer)
        .when(self.state.confirming_delete.is_some(), |view| {
            view.child(self.delete_confirmation_view(cx))
        })
        .when(self.state.confirming_overwrite.is_some(), |view| {
            view.child(self.overwrite_confirmation_view(cx))
        })
        .child(resize_handle(
            "resize-top",
            ResizeEdge::Top,
            CursorStyle::ResizeUpDown,
            ResizeHandleBounds {
                top: Some(0.),
                left: Some(8.),
                right: Some(8.),
                height: Some(8.),
                ..Default::default()
            },
        ))
        .child(resize_handle(
            "resize-bottom",
            ResizeEdge::Bottom,
            CursorStyle::ResizeUpDown,
            ResizeHandleBounds {
                left: Some(8.),
                right: Some(8.),
                bottom: Some(0.),
                height: Some(8.),
                ..Default::default()
            },
        ))
        .child(resize_handle(
            "resize-left",
            ResizeEdge::Left,
            CursorStyle::ResizeLeftRight,
            ResizeHandleBounds {
                top: Some(8.),
                left: Some(0.),
                bottom: Some(8.),
                width: Some(8.),
                ..Default::default()
            },
        ))
        .child(resize_handle(
            "resize-right",
            ResizeEdge::Right,
            CursorStyle::ResizeLeftRight,
            ResizeHandleBounds {
                top: Some(8.),
                right: Some(0.),
                bottom: Some(8.),
                width: Some(8.),
                ..Default::default()
            },
        ))
        .child(resize_handle(
            "resize-top-left",
            ResizeEdge::TopLeft,
            CursorStyle::ResizeUpLeftDownRight,
            ResizeHandleBounds {
                top: Some(0.),
                left: Some(0.),
                width: Some(14.),
                height: Some(14.),
                ..Default::default()
            },
        ))
        .child(resize_handle(
            "resize-top-right",
            ResizeEdge::TopRight,
            CursorStyle::ResizeUpRightDownLeft,
            ResizeHandleBounds {
                top: Some(0.),
                right: Some(0.),
                width: Some(14.),
                height: Some(14.),
                ..Default::default()
            },
        ))
        .child(resize_handle(
            "resize-bottom-left",
            ResizeEdge::BottomLeft,
            CursorStyle::ResizeUpRightDownLeft,
            ResizeHandleBounds {
                left: Some(0.),
                bottom: Some(0.),
                width: Some(14.),
                height: Some(14.),
                ..Default::default()
            },
        ))
        .child(resize_handle(
            "resize-bottom-right",
            ResizeEdge::BottomRight,
            CursorStyle::ResizeUpLeftDownRight,
            ResizeHandleBounds {
                right: Some(0.),
                bottom: Some(0.),
                width: Some(14.),
                height: Some(14.),
                ..Default::default()
            },
        ))
    }
}

fn notification_bar(theme: &Theme, message: impl Into<SharedString>) -> Div {
    div()
        .h(px(48.))
        .flex_shrink_0()
        .px(px(24.))
        .flex()
        .items_center()
        .border_t_1()
        .border_color(rgb(theme.colors.border))
        .bg(rgb(theme.colors.accent_surface))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(px(13.))
                .line_height(px(18.))
                .text_color(rgb(theme.colors.ink))
                .child(message.into()),
        )
}

#[derive(Default)]
struct ResizeHandleBounds {
    top: Option<f32>,
    left: Option<f32>,
    right: Option<f32>,
    bottom: Option<f32>,
    width: Option<f32>,
    height: Option<f32>,
}

fn resize_handle(
    id: &'static str,
    edge: ResizeEdge,
    cursor: CursorStyle,
    bounds: ResizeHandleBounds,
) -> gpui::Stateful<gpui::Div> {
    let mut handle = div().id(id).absolute().cursor(cursor).on_mouse_down(
        gpui::MouseButton::Left,
        move |_, window, cx| {
            window.start_window_resize(edge);
            cx.stop_propagation();
        },
    );
    if let Some(top) = bounds.top {
        handle = handle.top(px(top));
    }
    if let Some(left) = bounds.left {
        handle = handle.left(px(left));
    }
    if let Some(right) = bounds.right {
        handle = handle.right(px(right));
    }
    if let Some(bottom) = bounds.bottom {
        handle = handle.bottom(px(bottom));
    }
    if let Some(width) = bounds.width {
        handle = handle.w(px(width));
    }
    if let Some(height) = bounds.height {
        handle = handle.h(px(height));
    }
    handle
}
