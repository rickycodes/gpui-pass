use super::yubikey_detection::yubikey_product_image;
use super::*;

fn card_event_title(event: CardEvent) -> &'static str {
    match event {
        CardEvent::PinentryOpened => "System PIN or passphrase prompt opened",
        CardEvent::CardDetected => "GnuPG detected a card",
        CardEvent::InsertionRequested => "GnuPG requested a card",
        CardEvent::NoCardAvailable => "GnuPG reported no card available",
        CardEvent::NoReaderAvailable => "GnuPG reported no card reader",
        CardEvent::CardBusy => "GnuPG reported a busy card",
        CardEvent::AgentBusy => "GnuPG agent is busy",
    }
}

fn yubi_info_row(theme: &Theme, title: &'static str, detail: impl Into<SharedString>) -> gpui::Div {
    let detail: SharedString = detail.into();
    div()
        .flex()
        .items_start()
        .gap(px(18.))
        .border_b_1()
        .border_color(rgb(theme.colors.border))
        .py(px(13.))
        .child(
            div()
                .w(px(140.))
                .flex_shrink_0()
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .child(title),
        )
        .child(
            div()
                .max_w(px(560.))
                .text_size(px(12.))
                .line_height(px(19.))
                .text_color(rgb(theme.colors.muted))
                .child(detail),
        )
}

impl PassApp {
    pub(super) fn verify_device(&mut self, cx: &mut Context<Self>) {
        if self.state.device_verification.running() {
            return;
        }
        self.clear_message();
        self.state.verification_state_seen = VerificationState::Idle;
        self.state.verification_notification_until = None;
        if self.state.reveal.is_some()
            || self.state.copy_pending.is_some()
            || self.state.saving.is_some()
            || self.state.yubikey_info_check.is_some()
            || self.state.yubikey_check.is_some()
        {
            self.state.device_verification.busy();
        } else if let Some(usb) = self
            .state
            .usb_reader_details
            .as_ref()
            .filter(|usb| usb.vendor_id == 0x1050)
        {
            self.state
                .device_verification
                .start(self.state.card_refresh.generation(), usb.product_id);
        } else {
            self.state.device_verification.unavailable();
        }
        cx.notify();
    }

    pub(super) fn leave_card_status(&mut self, cx: &mut Context<Self>) {
        self.state.device_verification.cancel();
        self.state.verification_state_seen = self.state.device_verification.state();
        self.state.verification_notification_until = None;
        self.state.navigation.go_to(Page::Entries);
        cx.notify();
    }

    pub(super) fn yubikey_view(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let latest = self.state.yubikey_history.front().copied();
        let is_yubikey = self
            .state
            .usb_reader_details
            .as_ref()
            .is_some_and(|usb| usb.vendor_id == 0x1050)
            || self.state.yubikey_info.is_some()
            || self
                .state
                .yubikey_details
                .as_ref()
                .and_then(|details| details.manufacturer.as_ref())
                .is_some_and(|manufacturer| manufacturer.eq_ignore_ascii_case("Yubico"));
        let mut page = div()
            .id("yubikey-status-page")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(40.))
            .py(px(32.))
            .flex()
            .flex_col()
            .gap(px(26.))
            .bg(rgb(self.state.theme.colors.surface))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(self.state.theme.page_title(i18n::text("smartcard-status"))),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .when(is_yubikey, |actions| {
                                actions.child(
                                    self.state
                                        .theme
                                        .button(
                                            "verify-device",
                                            if self.state.device_verification.running() {
                                                "Verifying…"
                                            } else {
                                                "Verify"
                                            },
                                            true,
                                        )
                                        .on_click(
                                            cx.listener(|app, _, _, cx| app.verify_device(cx)),
                                        ),
                                )
                            })
                            .when(self.state.device_verification.running(), |actions| {
                                actions.child(
                                    self.state
                                        .theme
                                        .button("cancel-verification", "Cancel", false)
                                        .on_click(cx.listener(|app, _, _, cx| {
                                            app.state.device_verification.cancel();
                                            cx.notify();
                                        })),
                                )
                            })
                            .child(
                                self.state
                                    .theme
                                    .button("yubikey-back", "← Back", false)
                                    .on_click(
                                        cx.listener(|app, _, _, cx| app.leave_card_status(cx)),
                                    ),
                            ),
                    ),
            );

        if let Some(info) = &self.state.yubikey_info
            && let (Some(device_type), Some(image)) = (
                info.device_type.as_deref(),
                yubikey_product_image(info.device_type.as_deref()),
            )
        {
            let firmware = info
                .firmware_version
                .as_deref()
                .map(|version| format!("Firmware {version}"))
                .unwrap_or_else(|| "Connected to this computer".to_owned());
            page = page.child(
                div()
                    .w_full()
                    .max_w(px(760.))
                    .p(px(20.))
                    .border_1()
                    .border_color(rgb(self.state.theme.colors.border))
                    .rounded(px(self.state.theme.radii.panel))
                    .bg(rgb(self.state.theme.colors.background))
                    .flex()
                    .items_center()
                    .gap(px(28.))
                    .child(
                        div()
                            .size(px(190.))
                            .rounded(px(self.state.theme.radii.panel))
                            .bg(rgb(0xf2f0eb))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(img(image).size(px(176.))),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_start()
                            .gap(px(10.))
                            .child(
                                self.state
                                    .theme
                                    .status_badge(i18n::text("device-connected")),
                            )
                            .child(
                                div()
                                    .text_size(px(24.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(self.state.theme.colors.ink))
                                    .child(device_type.to_owned()),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgb(self.state.theme.colors.muted))
                                    .child(firmware),
                            ),
                    ),
            );
        }

        let mut rows = div().max_w(px(760.)).flex().flex_col().gap(px(1.));
        let mut has_details = false;
        if let Some(usb) = &self.state.usb_reader_details {
            rows = rows
                .child(yubi_info_row(
                    &self.state.theme,
                    "USB device",
                    format!(
                        "{} · {:04x}:{:04x}",
                        usb.product, usb.vendor_id, usb.product_id
                    ),
                ))
                .child(yubi_info_row(
                    &self.state.theme,
                    "USB location",
                    usb.location.clone(),
                ));
            has_details = true;
        }
        if let Some(info) = &self.state.yubikey_info {
            if let Some(device_type) = &info.device_type {
                rows = rows.child(yubi_info_row(
                    &self.state.theme,
                    "Device",
                    device_type.clone(),
                ));
                has_details = true;
            }
            if let Some(form_factor) = &info.form_factor {
                rows = rows.child(yubi_info_row(
                    &self.state.theme,
                    "Form factor",
                    form_factor.clone(),
                ));
                has_details = true;
            }
            if let Some(firmware_version) = &info.firmware_version {
                rows = rows.child(yubi_info_row(
                    &self.state.theme,
                    "Firmware",
                    firmware_version.clone(),
                ));
                has_details = true;
            }
        }
        if let Some(details) = &self.state.yubikey_details {
            rows = rows.child(yubi_info_row(
                &self.state.theme,
                "Application",
                details.application.clone(),
            ));
            has_details = true;
            if let Some(reader) = &details.reader {
                rows = rows.child(yubi_info_row(&self.state.theme, "Reader", reader.clone()));
            }
            if let Some(version) = &details.version {
                rows = rows.child(yubi_info_row(
                    &self.state.theme,
                    "Card version",
                    version.clone(),
                ));
            }
            if let Some(manufacturer) = &details.manufacturer {
                rows = rows.child(yubi_info_row(
                    &self.state.theme,
                    "Manufacturer",
                    manufacturer.clone(),
                ));
            }
            if !details.key_attributes.is_empty() {
                rows = rows.child(yubi_info_row(
                    &self.state.theme,
                    "Key algorithms",
                    details.key_attributes.join(" · "),
                ));
            }
            if let Some(retries) = details.pin_retries {
                rows = rows.child(yubi_info_row(
                    &self.state.theme,
                    "PIN retries",
                    format!("{} · {} · {}", retries[0], retries[1], retries[2]),
                ));
            }
            if let Some(count) = details.signature_count {
                rows = rows.child(yubi_info_row(
                    &self.state.theme,
                    "Signature count",
                    count.to_string(),
                ));
            }
        }
        if has_details {
            page = page.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(self.state.theme.label(i18n::text("card-details")))
                    .child(rows),
            );
        }
        if self.state.yubikey_connection == YubiConnection::Connected
            && self.state.yubikey_details.is_none()
        {
            page = page.child(
                div()
                    .max_w(px(760.))
                    .text_size(px(12.))
                    .text_color(rgb(self.state.theme.colors.muted))
                    .child(
                        if self.state.yubikey_check.is_some()
                            || self.state.card_detail_retry_at.is_some()
                        {
                            "Loading card details from GnuPG…"
                        } else {
                            "GnuPG has not reported additional card details."
                        },
                    ),
            );
        }

        if let Some(record) = latest {
            page = page.child(
                div()
                    .max_w(px(760.))
                    .text_size(px(11.))
                    .text_color(rgb(self.state.theme.colors.muted))
                    .child(format!(
                        "Recent activity: {} · {}",
                        card_event_title(record.observation.event),
                        record.operation
                    )),
            );
        }

        page.into_any_element()
    }

    pub(super) fn path_label(&self) -> String {
        if self.state.demo {
            return "Fictional entries · in memory".into();
        }
        self.state.path.as_ref().map_or_else(
            || "No store selected".into(),
            |path| {
                path.to_string_lossy()
                    .chars()
                    .filter(|c| !c.is_control())
                    .collect()
            },
        )
    }
}
