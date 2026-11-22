use super::*;

impl PassApp {
    pub(super) fn footer(&self, cx: &mut Context<Self>) -> Div {
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
        if let Some(message) = &self.state.message {
            notification_bar(
                &self.state.theme,
                message.clone(),
                self.state.message_is_error,
            )
        } else if let Some(status) = status {
            notification_bar(
                &self.state.theme,
                format!("{} · {}", status.title, status.detail),
                false,
            )
        } else if self.state.device_verification.running()
            || self.state.verification_notification_until.is_some()
        {
            notification_bar(
                &self.state.theme,
                self.state.device_verification.state().message(),
                false,
            )
        } else if let Some(seconds) = self.state.clipboard.remaining_seconds(Instant::now()) {
            let entry_name = self.state.clipboard.source().unwrap_or("entry");
            div()
                .h(px(self.state.theme.spacing.chrome_height))
                .flex_shrink_0()
                .px(px(self.state.theme.spacing.footer_horizontal))
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
                        .button("restore-clipboard-now", i18n::text("restore-now"), false)
                        .h(px(28.))
                        .on_click(cx.listener(|app, _, _, cx| app.restore_pass_clipboard(cx))),
                )
        } else {
            div()
                .h(px(self.state.theme.spacing.chrome_height))
                .flex_shrink_0()
                .px(px(self.state.theme.spacing.footer_horizontal))
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
                        .shortcut("Ctrl N", i18n::text("shortcut-new-entry")),
                )
                .child(
                    self.state
                        .theme
                        .shortcut("Esc", i18n::text("shortcut-hide")),
                )
        }
    }
}

pub(super) fn notification_bar(
    theme: &Theme,
    message: impl Into<SharedString>,
    is_error: bool,
) -> Div {
    let background = if is_error {
        theme.colors.danger_surface
    } else {
        theme.colors.accent_surface
    };
    let border = if is_error {
        theme.colors.danger_border
    } else {
        theme.colors.border
    };
    let text = if is_error {
        theme.colors.danger
    } else {
        theme.colors.ink
    };
    div()
        .h(px(theme.spacing.chrome_height))
        .flex_shrink_0()
        .px(px(theme.spacing.footer_horizontal))
        .flex()
        .items_center()
        .border_t_1()
        .border_color(rgb(border))
        .bg(rgb(background))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(px(13.))
                .line_height(px(18.))
                .text_color(rgb(text))
                .child(message.into()),
        )
}
