use super::*;

impl PassApp {
    pub(super) fn entry_row(
        &self,
        index: usize,
        depth: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let entry = &self.state.entries[index];
        let active = self.state.session.selected() == Some(entry);
        div()
            .id(("entry", index))
            .w_full()
            .flex()
            .flex_col()
            .gap(px(5.))
            .px(px(22.))
            .pl(px(22. + depth as f32 * 16.))
            .py(px(16.))
            .h(px(76.))
            .overflow_hidden()
            .border_b_1()
            .border_color(rgb(self.state.theme.colors.border))
            .border_l_2()
            .border_color(rgb(if active {
                self.state.theme.colors.accent
            } else {
                self.state.theme.colors.border
            }))
            .bg(rgb(if active {
                self.state.theme.colors.accent_surface
            } else {
                self.state.theme.colors.background
            }))
            .cursor_pointer()
            .hover(|style| style.bg(rgb(self.state.theme.colors.hover)))
            .on_click(cx.listener(move |app, _, window, cx| {
                app.state.focus.focus(window);
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
                    .text_color(rgb(self.state.theme.colors.muted))
                    .child(if entry.folder().is_empty() {
                        "Root".to_owned()
                    } else {
                        entry.folder().to_owned()
                    }),
            )
            .into_any_element()
    }

    pub(super) fn folder_row(
        &self,
        path: &str,
        depth: usize,
        count: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let expanded = self.state.expanded_folders.contains(path);
        let focused = self
            .state
            .sidebar_rows
            .get(self.state.sidebar_cursor)
            .is_some_and(
                |row| matches!(row, SidebarRow::Folder { path: row_path, .. } if row_path == path),
            );
        let folder_path = path.to_owned();
        let folder_id = folder_path
            .bytes()
            .fold(14_695_981_039_346_656_037_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(1_099_511_628_211_u64)
            });
        let label = path.rsplit('/').next().unwrap_or(path).to_owned();
        div()
            .id(("folder", folder_id))
            .w_full()
            .h(px(76.))
            .px(px(22.))
            .pl(px(22. + depth as f32 * 16.))
            .flex()
            .items_center()
            .gap(px(8.))
            .border_b_1()
            .border_color(rgb(self.state.theme.colors.border))
            .bg(rgb(if focused {
                self.state.theme.colors.nav_highlight
            } else {
                self.state.theme.colors.background
            }))
            .cursor_pointer()
            .hover(|style| style.bg(rgb(self.state.theme.colors.hover)))
            .on_click(cx.listener(move |app, _, window, cx| {
                app.state.focus.focus(window);
                app.toggle_folder(&folder_path, cx);
            }))
            .child(
                div()
                    .w(px(16.))
                    .text_color(rgb(self.state.theme.colors.muted))
                    .child(if expanded { "⌄" } else { "›" }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(13.))
                    .font_weight(FontWeight::MEDIUM)
                    .child(label),
            )
            .child(
                div()
                    .font_family(self.state.theme.mono_font)
                    .text_size(px(11.))
                    .text_color(rgb(self.state.theme.colors.muted))
                    .child(format!("{count:02}")),
            )
            .into_any_element()
    }

    pub(super) fn sidebar_row(&self, row: &SidebarRow, cx: &mut Context<Self>) -> gpui::AnyElement {
        match row {
            SidebarRow::Folder { path, depth, count } => self.folder_row(path, *depth, *count, cx),
            SidebarRow::Entry { index, depth } => self.entry_row(*index, *depth, cx),
        }
    }

    pub(super) fn sidebar(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .w(px(302.))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(rgb(self.state.theme.colors.border))
            .bg(rgb(self.state.theme.colors.background))
            .child(
                div()
                    .px(px(22.))
                    .pt(px(26.))
                    .pb(px(18.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(self.state.theme.label(i18n::text("your-entries")))
                    .child(
                        div()
                            .font_family(self.state.theme.mono_font)
                            .text_size(px(11.))
                            .text_color(rgb(self.state.theme.colors.muted))
                            .child(format!("{:02}", self.state.filtered.len())),
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
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .when(!self.state.sidebar_rows.is_empty(), |view| {
                        view.child(
                            uniform_list(
                                "entries",
                                self.state.sidebar_rows.len(),
                                cx.processor(|app, range: Range<usize>, _, cx| {
                                    range
                                        .map(|position| {
                                            let row = app.state.sidebar_rows[position].clone();
                                            app.sidebar_row(&row, cx)
                                        })
                                        .collect::<Vec<_>>()
                                }),
                            )
                            .track_scroll(self.state.list_scroll.clone())
                            .size_full(),
                        )
                        .child(self.sidebar_scroll_input(cx))
                    })
                    .when(self.state.filtered.is_empty(), |view| {
                        view.child(
                            div()
                                .p(px(24.))
                                .text_size(px(13.))
                                .text_color(rgb(self.state.theme.colors.muted))
                                .child(if self.state.load.is_some() {
                                    "Reading entry names…"
                                } else if self.state.query.is_empty() {
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
                    .border_color(rgb(self.state.theme.colors.border))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(self.state.theme.label(if self.state.demo {
                        "DEMO STORE"
                    } else {
                        "CURRENT STORE"
                    }))
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_family(self.state.theme.mono_font)
                            .text_color(rgb(self.state.theme.colors.muted))
                            .overflow_hidden()
                            .child(self.path_label()),
                    )
                    .child(
                        div()
                            .id("open-store")
                            .mt(px(8.))
                            .text_size(px(12.))
                            .text_color(rgb(self.state.theme.colors.accent))
                            .cursor_pointer()
                            .on_click(cx.listener(|app, _, window, cx| app.open_store(window, cx)))
                            .child(i18n::text("open-another-store")),
                    ),
            )
            .into_any_element()
    }
}
