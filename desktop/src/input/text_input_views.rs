use crate::{PassApp, search::input_byte_at_x};
use gpui::{Context, ElementInputHandler, Window, canvas, div, prelude::*, px, rgb};

impl PassApp {
    pub(crate) fn composer_content_field(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if self.text_editor_active() {
            let entity = cx.entity().clone();
            let focused = self.state.composer_content_focus.is_focused(window);
            return div()
                .id("entry-editor-field")
                .track_focus(&self.state.composer_content_focus)
                .rounded(px(self.state.theme.radii.field))
                .w_full()
                .min_w_0()
                .flex()
                .items_start()
                .border_1()
                .border_color(rgb(if focused {
                    self.state.theme.colors.accent
                } else {
                    self.state.theme.colors.border
                }))
                .bg(rgb(self.state.theme.colors.background))
                .cursor_text()
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|app, event: &gpui::MouseDownEvent, window, cx| {
                        app.begin_edit_mouse_selection(
                            event.position,
                            event.click_count,
                            window,
                            cx,
                        );
                    }),
                )
                .on_mouse_move(cx.listener(|app, event: &gpui::MouseMoveEvent, _, cx| {
                    if event.dragging() {
                        app.drag_edit_mouse_selection(event.position, cx);
                    }
                }))
                .on_mouse_up(
                    gpui::MouseButton::Left,
                    cx.listener(|app, _, _, _| app.end_edit_mouse_selection()),
                )
                .on_mouse_up_out(
                    gpui::MouseButton::Left,
                    cx.listener(|app, _, _, _| app.end_edit_mouse_selection()),
                )
                .child(
                    div()
                        .id("entry-editor-scroll")
                        .w_full()
                        .min_w_0()
                        .overflow_x_scroll()
                        .font_family(self.state.theme.mono_font)
                        .text_size(px(18.))
                        .line_height(px(26.))
                        .text_color(rgb(self.state.theme.colors.ink))
                        .child(crate::entry_editor::EntryTextElement { app: entity }),
                )
                .into_any_element();
        }
        let focus = self.state.composer_content_focus.clone();
        let entity = cx.entity().clone();
        let focused = focus.is_focused(window);
        let start = self
            .state
            .composer_content_selection
            .start
            .min(self.state.composer_content.len());
        let end = self
            .state
            .composer_content_selection
            .end
            .min(self.state.composer_content.len());
        let mut text = div()
            .flex()
            .items_center()
            .whitespace_nowrap()
            .font_family(self.state.theme.mono_font)
            .text_size(px(13.));
        if self.state.composer_content.is_empty() {
            text = text
                .when(focused, |view| {
                    view.child(
                        div()
                            .w(px(1.))
                            .h(px(16.))
                            .bg(rgb(self.state.theme.colors.accent)),
                    )
                })
                .child(
                    div()
                        .text_color(rgb(self.state.theme.colors.muted))
                        .child("e.g. Email/example.com"),
                );
        } else {
            text = text
                .child(self.state.composer_content[..start].to_owned())
                .when(start != end, |view| {
                    view.child(
                        div()
                            .bg(rgb(self.state.theme.colors.accent_surface))
                            .child(self.state.composer_content[start..end].to_owned()),
                    )
                })
                .when(focused && start == end, |view| {
                    view.child(
                        div()
                            .w(px(1.))
                            .h(px(16.))
                            .bg(rgb(self.state.theme.colors.accent)),
                    )
                })
                .child(self.state.composer_content[end..].to_owned());
        }
        div()
            .id("composer-entry-content")
            .track_focus(&self.state.composer_content_focus)
            .rounded(px(self.state.theme.radii.field))
            .h(px(44.))
            .w_full()
            .min_w_0()
            .flex()
            .items_center()
            .px(px(self.state.theme.spacing.field_padding))
            .border_1()
            .border_color(rgb(if focused {
                self.state.theme.colors.accent
            } else {
                self.state.theme.colors.border
            }))
            .bg(rgb(self.state.theme.colors.surface))
            .cursor_text()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|app, event: &gpui::MouseDownEvent, window, cx| {
                    app.state.composer_content_focus.focus(window);
                    if let Some(bounds) = app.state.composer_content_bounds {
                        let byte = input_byte_at_x(
                            window,
                            &app.state.composer_content,
                            event.position.x,
                            bounds.origin.x,
                            app.state.theme.mono_font,
                        );
                        app.state.composer_content_selection = byte..byte;
                        app.state.composer_content_cursor = byte;
                    }
                    cx.notify();
                }),
            )
            .child(
                div()
                    .id("composer-entry-content-input")
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h(px(20.))
                    .overflow_hidden()
                    .child(text)
                    .child(
                        canvas(
                            |_, _, _| (),
                            move |bounds, (), window, cx| {
                                entity.update(cx, |app, _| {
                                    app.state.composer_content_bounds = Some(bounds)
                                });
                                if focus.is_focused(window) {
                                    window.handle_input(
                                        &focus,
                                        ElementInputHandler::new(bounds, entity.clone()),
                                        cx,
                                    );
                                }
                            },
                        )
                        .absolute()
                        .size_full(),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn custom_entry_name_field(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let focus = self.state.custom_entry_name_focus.clone();
        let onboarding = self.state.onboarding.is_some();
        let entity = cx.entity().clone();
        let focused = focus.is_focused(window);
        let start = self
            .state
            .custom_entry_name_selection
            .start
            .min(self.state.custom_entry_name.len());
        let end = self
            .state
            .custom_entry_name_selection
            .end
            .min(self.state.custom_entry_name.len());
        let mut text = div()
            .flex()
            .items_center()
            .whitespace_nowrap()
            .font_family(self.state.theme.mono_font)
            .text_size(px(13.));
        if self.state.custom_entry_name.is_empty() {
            text = text
                .when(focused, |view| {
                    view.child(
                        div()
                            .w(px(1.))
                            .h(px(16.))
                            .bg(rgb(self.state.theme.colors.accent)),
                    )
                })
                .child(
                    div()
                        .text_color(rgb(self.state.theme.colors.muted))
                        .child(if onboarding {
                            "e.g. 0xABCD... or user@example.com"
                        } else {
                            "e.g. Email/example.com"
                        }),
                );
        } else {
            text = text
                .child(self.state.custom_entry_name[..start].to_owned())
                .when(start != end, |view| {
                    view.child(
                        div()
                            .bg(rgb(self.state.theme.colors.accent_surface))
                            .child(self.state.custom_entry_name[start..end].to_owned()),
                    )
                })
                .when(focused && start == end, |view| {
                    view.child(
                        div()
                            .w(px(1.))
                            .h(px(16.))
                            .bg(rgb(self.state.theme.colors.accent)),
                    )
                })
                .child(self.state.custom_entry_name[end..].to_owned());
        }
        div()
            .id(if onboarding {
                "onboarding-recipient-field"
            } else {
                "custom-entry-name-field"
            })
            .track_focus(&focus)
            .rounded(px(self.state.theme.radii.field))
            .h(px(44.))
            .w_full()
            .min_w_0()
            .flex()
            .items_center()
            .px(px(self.state.theme.spacing.field_padding))
            .border_1()
            .border_color(rgb(if focused {
                self.state.theme.colors.accent
            } else {
                self.state.theme.colors.border
            }))
            .bg(rgb(self.state.theme.colors.surface))
            .cursor_text()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|app, event: &gpui::MouseDownEvent, window, cx| {
                    app.state.custom_entry_name_focus.focus(window);
                    if let Some(bounds) = app.state.custom_entry_name_bounds {
                        let byte = input_byte_at_x(
                            window,
                            &app.state.custom_entry_name,
                            event.position.x,
                            bounds.origin.x,
                            app.state.theme.mono_font,
                        );
                        app.state.custom_entry_name_selection = byte..byte;
                        app.state.custom_entry_name_cursor = byte;
                    }
                    cx.notify();
                }),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h(px(20.))
                    .overflow_hidden()
                    .child(text)
                    .child(
                        canvas(
                            |_, _, _| (),
                            move |bounds, (), window, cx| {
                                entity.update(cx, |app, _| {
                                    app.state.custom_entry_name_bounds = Some(bounds)
                                });
                                if focus.is_focused(window) {
                                    window.handle_input(
                                        &focus,
                                        ElementInputHandler::new(bounds, entity.clone()),
                                        cx,
                                    );
                                }
                            },
                        )
                        .absolute()
                        .size_full(),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn search_field(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let focus = self.state.search_focus.clone();
        let entity = cx.entity();
        let focused = self.state.search_focus.is_focused(window);
        let start = self.state.selection.start.min(self.state.query.len());
        let end = self.state.selection.end.min(self.state.query.len());
        let mut text = div()
            .flex()
            .items_center()
            .whitespace_nowrap()
            .font_family(self.state.theme.mono_font)
            .text_size(px(13.));
        if self.state.query.is_empty() {
            text = text
                .when(focused, |view| {
                    view.child(
                        div()
                            .w(px(1.))
                            .h(px(16.))
                            .bg(rgb(self.state.theme.colors.accent)),
                    )
                })
                .child(
                    div()
                        .text_color(rgb(self.state.theme.colors.muted))
                        .child(crate::i18n::text("search-entries")),
                );
        } else {
            text = text
                .child(self.state.query[..start].to_owned())
                .when(start != end, |view| {
                    view.child(
                        div()
                            .bg(rgb(self.state.theme.colors.accent_surface))
                            .child(self.state.query[start..end].to_owned()),
                    )
                })
                .when(focused && start == end, |view| {
                    view.child(
                        div()
                            .w(px(1.))
                            .h(px(16.))
                            .bg(rgb(self.state.theme.colors.accent)),
                    )
                })
                .child(self.state.query[end..].to_owned());
        }
        div()
            .id("search-field")
            .track_focus(&self.state.search_focus)
            .rounded(px(self.state.theme.radii.field))
            .h(px(42.))
            .w_full()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(self.state.theme.spacing.field_padding))
            .border_1()
            .border_color(rgb(if focused {
                self.state.theme.colors.accent
            } else {
                self.state.theme.colors.border
            }))
            .bg(rgb(self.state.theme.colors.surface))
            .cursor_text()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|app, event: &gpui::MouseDownEvent, window, cx| {
                    app.state.search_focus.focus(window);
                    if let Some(bounds) = app.state.input_bounds {
                        let byte = input_byte_at_x(
                            window,
                            &app.state.query,
                            event.position.x,
                            bounds.origin.x,
                            app.state.theme.mono_font,
                        );
                        app.state.selection = byte..byte;
                    }
                    cx.notify();
                }),
            )
            .child(
                div()
                    .text_size(px(16.))
                    .text_color(rgb(self.state.theme.colors.muted))
                    .child("⌕"),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h(px(20.))
                    .overflow_hidden()
                    .child(text)
                    .child(
                        canvas(
                            |_, _, _| (),
                            move |bounds, (), window, cx| {
                                entity.update(cx, |app, _| app.state.input_bounds = Some(bounds));
                                if focus.is_focused(window) {
                                    window.handle_input(
                                        &focus,
                                        ElementInputHandler::new(bounds, entity.clone()),
                                        cx,
                                    );
                                }
                            },
                        )
                        .absolute()
                        .size_full(),
                    ),
            )
            .into_any_element()
    }
}
