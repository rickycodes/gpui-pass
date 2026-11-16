use super::*;

impl PassApp {
    pub(super) fn entry_row(&self, index: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let entry = &self.entries[index];
        let active = self.session.selected() == Some(entry);
        div()
            .id(("entry", index))
            .w_full()
            .flex()
            .flex_col()
            .gap(px(5.))
            .px(px(22.))
            .py(px(16.))
            .h(px(76.))
            .overflow_hidden()
            .border_b_1()
            .border_color(rgb(self.theme.colors.border))
            .border_l_2()
            .border_color(rgb(if active {
                self.theme.colors.accent
            } else {
                self.theme.colors.border
            }))
            .bg(rgb(if active {
                self.theme.colors.accent_surface
            } else {
                self.theme.colors.background
            }))
            .cursor_pointer()
            .hover(|style| style.bg(rgb(self.theme.colors.hover)))
            .on_click(cx.listener(move |app, _, window, cx| {
                app.focus.focus(window);
                app.choose(index, cx);
            }))
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(14.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(entry.title().to_owned()),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .truncate()
                    .text_size(px(11.))
                    .text_color(rgb(self.theme.colors.muted))
                    .child(if entry.folder().is_empty() {
                        "Root".to_owned()
                    } else {
                        entry.folder().to_owned()
                    }),
            )
            .into_any_element()
    }

    pub(super) fn sidebar(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .w(px(302.))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(rgb(self.theme.colors.border))
            .bg(rgb(self.theme.colors.background))
            .child(
                div()
                    .px(px(22.))
                    .pt(px(26.))
                    .pb(px(18.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(self.theme.label(i18n::text("your-entries")))
                    .child(
                        div()
                            .font_family(self.theme.mono_font)
                            .text_size(px(11.))
                            .text_color(rgb(self.theme.colors.muted))
                            .child(format!("{:02}", self.filtered.len())),
                    ),
            )
            .child(
                div()
                    .px(px(18.))
                    .pb(px(20.))
                    .child(self.search_field(window, cx)),
            )
            .child(
                div()
                    .id("entry-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .when(!self.filtered.is_empty(), |view| {
                        view.child(
                            uniform_list(
                                "entries",
                                self.filtered.len(),
                                cx.processor(|app, range: Range<usize>, _, cx| {
                                    range
                                        .map(|position| app.entry_row(app.filtered[position], cx))
                                        .collect::<Vec<_>>()
                                }),
                            )
                            .track_scroll(self.list_scroll.clone())
                            .size_full(),
                        )
                    })
                    .when(self.filtered.is_empty(), |view| {
                        view.child(
                            div()
                                .p(px(24.))
                                .text_size(px(13.))
                                .text_color(rgb(self.theme.colors.muted))
                                .child(if self.load.is_some() {
                                    "Reading entry names…"
                                } else if self.query.is_empty() {
                                    "No entries to show."
                                } else {
                                    "No matching entries.\nTry another search."
                                }),
                        )
                    }),
            )
            .child(
                div()
                    .p(px(22.))
                    .border_t_1()
                    .border_color(rgb(self.theme.colors.border))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(self.theme.label(if self.demo {
                        "DEMO STORE"
                    } else {
                        "CURRENT STORE"
                    }))
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_family(self.theme.mono_font)
                            .text_color(rgb(self.theme.colors.muted))
                            .overflow_hidden()
                            .child(self.path_label()),
                    )
                    .child(
                        div()
                            .id("open-store")
                            .mt(px(8.))
                            .text_size(px(12.))
                            .text_color(rgb(self.theme.colors.accent))
                            .cursor_pointer()
                            .on_click(cx.listener(|app, _, window, cx| app.open_store(window, cx)))
                            .child(i18n::text("open-another-store")),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn details(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let entry = self.session.selected();
        let now = Instant::now();
        let revealed = self.session.revealed(now);
        let pending = self.session.is_pending();
        let mut detail = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(self.theme.colors.surface));
        if let Some(entry) = entry {
            let folder = if entry.folder().is_empty() {
                "Root"
            } else {
                entry.folder()
            };
            detail = detail
                .child(
                    div()
                        .px(px(46.))
                        .pt(px(40.))
                        .pb(px(28.))
                        .flex()
                        .flex_col()
                        .gap(px(17.))
                        .child(
                            self.theme
                                .label(format!("STORE   /   {}", folder.to_uppercase())),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(16.))
                                .child(
                                    div()
                                        .text_size(px(36.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(entry.title().to_owned()),
                                )
                                .child(
                                    div()
                                        .border_1()
                                        .border_color(rgb(self.theme.colors.border))
                                        .px(px(10.))
                                        .py(px(6.))
                                        .font_family(self.theme.mono_font)
                                        .text_size(px(10.))
                                        .text_color(rgb(self.theme.colors.muted))
                                        .child(".gpg"),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(rgb(self.theme.colors.muted))
                                .child(entry.name().to_owned()),
                        ),
                )
                .child(
                    div()
                        .px(px(46.))
                        .pb(px(24.))
                        .flex()
                        .items_center()
                        .border_b_1()
                        .border_color(rgb(self.theme.colors.border))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(7.))
                                .child(div().size(px(5.)).bg(rgb(self.theme.colors.accent)))
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(rgb(self.theme.colors.muted))
                                        .child(i18n::text("encrypted-on-disk")),
                                ),
                        ),
                );

            let mut body = div()
                .id("detail-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p(px(46.))
                .flex()
                .flex_col()
                .gap(px(24.));
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(self.theme.label("PASSWORD"))
                    .child(
                        div()
                            .font_family(self.theme.mono_font)
                            .text_size(px(10.))
                            .text_color(rgb(if revealed.is_some() {
                                self.theme.colors.accent
                            } else {
                                self.theme.colors.muted
                            }))
                            .child(if revealed.is_some() {
                                format!("HIDES IN {:02}s", self.seconds)
                            } else if pending {
                                "UNLOCKING".into()
                            } else {
                                "HIDDEN".into()
                            }),
                    ),
            );
            let mut password = self
                .theme
                .panel()
                .bg(rgb(self.theme.colors.background))
                .w_full()
                .min_w_0()
                .min_h(px(84.));
            if self.editing_entry {
                password = password.child(self.generator_name_field(window, cx));
            } else if let Some(secret) = revealed {
                let text = secret.expose();
                password = password.child(
                    div()
                        .id("password-text")
                        .font_family(self.theme.mono_font)
                        .text_size(px(18.))
                        .line_height(px(26.))
                        .child(text.chars().take(16_384).collect::<String>()),
                );
            } else {
                password = password.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(24.))
                                .text_color(rgb(self.theme.colors.placeholder))
                                .child("••••••••••••"),
                        )
                        .child(
                            div()
                                .font_family(self.theme.mono_font)
                                .text_size(px(10.))
                                .text_color(rgb(self.theme.colors.muted))
                                .child(i18n::text("concealed")),
                        ),
                );
            }
            body = body.child(password);
            if self.editing_entry && self.edit_conflict {
                body = body.child(
                    div()
                        .id("edit-conflict")
                        .w_full()
                        .rounded(px(self.theme.radii.panel))
                        .border_1()
                        .border_color(rgb(self.theme.colors.danger))
                        .bg(rgb(self.theme.colors.surface))
                        .p(px(18.))
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .child(
                            div()
                                .text_size(px(15.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(self.theme.colors.ink))
                                .child(i18n::text("edit-conflict-title")),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(18.))
                                .text_color(rgb(self.theme.colors.muted))
                                .child(i18n::text("edit-conflict-help")),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(12.))
                                .child(
                                    self.theme
                                        .danger_button(
                                            "overwrite-conflicted-edit",
                                            i18n::text("edit-conflict-overwrite"),
                                        )
                                        .on_click(cx.listener(|app, _, _, cx| {
                                            app.overwrite_conflicted_edit(cx)
                                        })),
                                )
                                .child(
                                    self.theme
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
                .gap(px(18.))
                .child(
                    self.theme
                        .button(
                            "reveal",
                            if pending {
                                "Cancel reveal"
                            } else if revealed.is_some() {
                                "Hide password"
                            } else {
                                "Reveal password  ↗"
                            },
                            true,
                        )
                        .on_click(cx.listener(|app, _, window, cx| {
                            if app.session.is_pending() {
                                app.hide(cx);
                            } else {
                                app.reveal(window, cx);
                            }
                        })),
                )
                .when(self.editing_entry, |controls| {
                    controls
                        .when(!self.edit_conflict, |controls| {
                            controls.child(
                                self.theme
                                    .button(
                                        "save-entry-edit",
                                        if self.saving_edit {
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
                            self.theme
                                .button("cancel-entry-edit", "Cancel", false)
                                .on_click(cx.listener(|app, _, _, cx| app.cancel_edit(cx))),
                        )
                })
                .when(!self.editing_entry, |controls| {
                    controls
                        .child(
                            self.theme
                                .button("edit-entry", "Edit entry", true)
                                .on_click(
                                    cx.listener(|app, _, window, cx| app.begin_edit(window, cx)),
                                ),
                        )
                        .child(
                            self.theme
                                .button(
                                    "copy-password",
                                    if self.copy_pending.is_some() {
                                        "Copying…"
                                    } else {
                                        "Copy entry"
                                    },
                                    false,
                                )
                                .on_click(cx.listener(|app, _, _, cx| app.copy_selected(cx))),
                        )
                        .child(
                            self.theme
                                .danger_button("delete-entry", "Delete")
                                .on_click(cx.listener(|app, _, _, cx| app.request_delete(cx))),
                        )
                })
                .when(pending, |controls| {
                    controls.child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(self.theme.colors.muted))
                            .child(i18n::text("esc-to-cancel")),
                    )
                });
            body = body.child(controls);
            if let Some(secret) = revealed {
                if secret.expose().len() > 16_384 {
                    body = body.child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(self.theme.colors.muted))
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
                        .child(loading_spinner(self.theme.colors.accent)),
                );
            } else {
                detail = detail.child(div().flex_1().flex().flex_col().justify_center().px(px(60.)).gap(px(18.))
                    .child(div().text_size(px(56.)).font_weight(FontWeight::LIGHT).text_color(rgb(self.theme.colors.accent)).child("[ / ]"))
                    .child(div().text_size(px(30.)).child(if !self.query.is_empty() { "Nothing here. Yet." } else { "Your passwords, at home." }))
                    .child(div().max_w(px(380.)).text_size(px(14.)).line_height(px(24.)).text_color(rgb(self.theme.colors.muted))
                        .child(if !self.query.is_empty() { "Search by an entry name or folder." } else { "Open an existing pass store to browse your entries. Your keys stay with GnuPG." }))
                    .child(self.theme.button("empty-open-store", "Open password store  ↗", true).w(px(208.)).on_click(cx.listener(|app, _, window, cx| app.open_store(window, cx))))
    );
            }
        }
        detail
            .child(
                div()
                    .px(px(46.))
                    .py(px(20.))
                    .border_t_1()
                    .border_color(rgb(self.theme.colors.border))
                    .flex()
                    .justify_between()
                    .child(self.theme.label(if self.demo {
                        "FICTIONAL DATA"
                    } else {
                        "LOCAL STORE"
                    })),
            )
            .into_any_element()
    }
}
