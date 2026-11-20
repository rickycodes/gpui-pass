use super::*;

impl PassApp {
    pub(super) fn details(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let entry = self.state.session.selected();
        let now = Instant::now();
        let revealed = self.state.session.revealed(now);
        let pending = self.state.session.is_pending();
        let mut detail = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(self.state.theme.colors.surface));
        if let Some(entry) = entry {
            let folder = if entry.folder().is_empty() {
                "Root"
            } else {
                entry.folder()
            };
            detail = detail.child(
                div()
                    .px(px(40.))
                    .pt(px(32.))
                    .pb(px(28.))
                    .border_b_1()
                    .border_color(rgb(self.state.theme.colors.border))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(self.state.theme.label(folder.to_owned()))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(16.))
                            .child(
                                self.state
                                    .theme
                                    .page_title(entry.title().to_owned())
                                    .flex_1()
                                    .min_w_0()
                                    .truncate(),
                            )
                            .child(
                                self.state
                                    .theme
                                    .status_badge(i18n::text("encrypted-on-disk")),
                            ),
                    )
                    .child(
                        div()
                            .font_family(self.state.theme.mono_font)
                            .text_size(px(11.))
                            .text_color(rgb(self.state.theme.colors.muted))
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(entry.name().to_owned()),
                    ),
            );

            let mut body = div()
                .id("detail-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p(px(40.))
                .flex()
                .flex_col()
                .gap(px(24.));
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(self.state.theme.label(i18n::text("password")))
                    .child(
                        div()
                            .font_family(self.state.theme.mono_font)
                            .text_size(px(10.))
                            .text_color(rgb(if revealed.is_some() {
                                self.state.theme.colors.accent
                            } else {
                                self.state.theme.colors.muted
                            }))
                            .child(if revealed.is_some() {
                                format!("HIDES IN {:02}s", self.state.seconds)
                            } else if pending {
                                "UNLOCKING".into()
                            } else {
                                "HIDDEN".into()
                            }),
                    ),
            );
            let mut password = self
                .state
                .theme
                .panel()
                .bg(rgb(self.state.theme.colors.background))
                .w_full()
                .min_w_0()
                .min_h(px(96.));
            if self.state.editing_entry {
                password = password.child(self.generator_name_field(window, cx));
            } else if let Some(secret) = revealed {
                let text = secret.expose();
                password = password.child(
                    div()
                        .id("password-text")
                        .font_family(self.state.theme.mono_font)
                        .text_size(px(18.))
                        .line_height(px(26.))
                        .child(text.chars().take(16_384).collect::<String>()),
                );
            } else {
                password = password.child(
                    div().flex().items_center().justify_between().child(
                        div()
                            .text_size(px(24.))
                            .text_color(rgb(self.state.theme.colors.placeholder))
                            .child("••••••••••••"),
                    ),
                );
            }
            body = body.child(password);
            if self.state.editing_entry && self.state.edit_conflict {
                body = body.child(
                    div()
                        .id("edit-conflict")
                        .w_full()
                        .rounded(px(self.state.theme.radii.panel))
                        .border_1()
                        .border_color(rgb(self.state.theme.colors.danger))
                        .bg(rgb(self.state.theme.colors.surface))
                        .p(px(18.))
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .child(
                            div()
                                .text_size(px(15.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(self.state.theme.colors.ink))
                                .child(i18n::text("edit-conflict-title")),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(18.))
                                .text_color(rgb(self.state.theme.colors.muted))
                                .child(i18n::text("edit-conflict-help")),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(12.))
                                .child(
                                    self.state
                                        .theme
                                        .danger_button(
                                            "overwrite-conflicted-edit",
                                            i18n::text("edit-conflict-overwrite"),
                                        )
                                        .on_click(cx.listener(|app, _, _, cx| {
                                            app.overwrite_conflicted_edit(cx)
                                        })),
                                )
                                .child(
                                    self.state
                                        .theme
                                        .button(
                                            "discard-conflicted-edit",
                                            i18n::text("edit-conflict-discard"),
                                            false,
                                        )
                                        .on_click(cx.listener(|app, _, _, cx| {
                                            app.discard_conflicted_edit(cx)
                                        })),
                                ),
                        ),
                );
            }
            let controls = div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(
                    self.state
                        .theme
                        .button(
                            "reveal",
                            if pending {
                                "Cancel reveal"
                            } else if revealed.is_some() {
                                "Hide password"
                            } else {
                                "Reveal password"
                            },
                            false,
                        )
                        .on_click(cx.listener(|app, _, window, cx| {
                            if app.state.session.is_pending() {
                                app.hide(cx);
                            } else {
                                app.reveal(window, cx);
                            }
                        })),
                )
                .when(self.state.editing_entry, |controls| {
                    controls
                        .when(!self.state.edit_conflict, |controls| {
                            controls.child(
                                self.state
                                    .theme
                                    .button(
                                        "save-entry-edit",
                                        if self.state.saving_edit {
                                            "Saving…"
                                        } else {
                                            "Save changes"
                                        },
                                        true,
                                    )
                                    .on_click(cx.listener(|app, _, _, cx| app.save_edit(cx))),
                            )
                        })
                        .child(
                            self.state
                                .theme
                                .button("cancel-entry-edit", "Cancel", false)
                                .on_click(cx.listener(|app, _, _, cx| app.cancel_edit(cx))),
                        )
                })
                .when(!self.state.editing_entry, |controls| {
                    controls
                        .child(
                            self.state
                                .theme
                                .button("edit-entry", "Edit entry", false)
                                .on_click(
                                    cx.listener(|app, _, window, cx| app.begin_edit(window, cx)),
                                ),
                        )
                        .child(
                            self.state
                                .theme
                                .button(
                                    "copy-password",
                                    if self.state.copy_pending.is_some() {
                                        "Copying…"
                                    } else {
                                        "Copy entry"
                                    },
                                    true,
                                )
                                .on_click(cx.listener(|app, _, _, cx| app.copy_selected(cx))),
                        )
                })
                .when(pending, |controls| {
                    controls.child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(self.state.theme.colors.muted))
                            .child(i18n::text("esc-to-cancel")),
                    )
                });
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(16.))
                    .child(controls)
                    .when(!self.state.editing_entry, |row| {
                        row.child(
                            self.state
                                .theme
                                .danger_button("delete-entry", "Delete")
                                .on_click(cx.listener(|app, _, _, cx| app.request_delete(cx))),
                        )
                    }),
            );
            if let Some(secret) = revealed {
                if secret.expose().len() > 16_384 {
                    body = body.child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(self.state.theme.colors.muted))
                            .child("Long content is truncated in this preview."),
                    );
                }
            }
            detail = detail.child(body);
        } else {
            if self.store_is_loading() {
                detail = detail.child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(loading_spinner(self.state.theme.colors.accent)),
                );
            } else {
                detail = detail.child(div().flex_1().flex().flex_col().justify_center().px(px(60.)).gap(px(18.))
                    .child(self.brand_mark(56., self.state.theme.colors.accent))
                    .child(div().text_size(px(30.)).child(if !self.state.query.is_empty() { "Nothing here. Yet." } else { "Your passwords, at home." }))
                    .child(div().max_w(px(380.)).text_size(px(14.)).line_height(px(24.)).text_color(rgb(self.state.theme.colors.muted))
                        .child(if !self.state.query.is_empty() { "Search by an entry name or folder." } else { "Open an existing pass store to browse your entries. Your keys stay with GnuPG." }))
                    .child(self.state.theme.button("empty-open-store", "Open password store  ↗", true).w(px(208.)).on_click(cx.listener(|app, _, window, cx| app.open_store(window, cx))))
    );
            }
        }
        detail.into_any_element()
    }
}
