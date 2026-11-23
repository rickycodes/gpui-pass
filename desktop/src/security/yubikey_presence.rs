use super::*;

impl PassApp {
    pub(crate) fn apply_card_presence_hint(&mut self, present: bool, cx: &mut Context<Self>) {
        let connection = if present {
            YubiConnection::Connected
        } else {
            YubiConnection::NotDetected
        };
        if self.state.usb_yubikey_present != Some(true) {
            self.state.card_refresh.observe(present);
        }
        if self.state.yubikey_connection != connection {
            self.state.yubikey_connection = connection;
            self.state.yubikey_details = None;
            if !present {
                self.state.device_verification.invalidate();
                self.state.navigation.card_unavailable();
            }
            cx.notify();
        }
        if self.state.yubikey_check.is_some()
            || self.state.yubikey_info_check.is_some()
            || self.state.reveal.is_some()
            || self.state.saving.is_some()
        {
            self.state.card_refresh_pending = true;
        } else {
            self.start_yubikey_check();
        }
    }

    pub(crate) fn apply_yubikey_usb_presence(&mut self, present: bool, cx: &mut Context<Self>) {
        // sysfs and udev can report the same transition. Do not reset retry
        // state or start another probe for a duplicate event.
        if self.state.usb_yubikey_present == Some(present) {
            return;
        }
        self.state.usb_yubikey_present = Some(present);
        self.state.card_refresh.observe(present);
        trace!(
            "card.presence source=usb present={present} generation={}",
            self.state.card_refresh.generation()
        );
        if present {
            self.state.card_detail_retry_at = None;
            self.state.usb_reader_details = usb_reader_for_card(None, true);
        } else {
            self.state.device_verification.invalidate();
            self.state.usb_reader_details = None;
            self.state.yubikey_info = None;
            // Keep tracking the old worker until it stops touching the device.
            self.state.card_detail_retry_at = None;
        }
        self.apply_card_presence_hint(present, cx);
    }

    pub(crate) fn poll_yubikey_connection(&mut self, cx: &mut Context<Self>) {
        #[cfg(target_os = "linux")]
        {
            // Watcher liveness does not guarantee event delivery. Reconcile
            // with physical presence even while udev is running, so a missed
            // event cannot leave an unplugged key marked as connected.
            if self
                .state
                .usb_last_scan
                .is_none_or(|last| last.elapsed() >= USB_PRESENCE_SCAN_INTERVAL)
            {
                self.state.usb_last_scan = Some(Instant::now());
                let present = !connected_yubikey_usb_paths().is_empty();
                let previous = self.state.usb_yubikey_present;
                if previous != Some(present) && (present || previous == Some(true)) {
                    self.apply_yubikey_usb_presence(present, cx);
                }
            }
        }
        let usb_events = self
            .state
            .usb_card_watcher
            .as_ref()
            .map(|watcher| watcher.events.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        for event in usb_events {
            match event {
                UsbCardEvent::Presence(present) => {
                    self.apply_yubikey_usb_presence(present, cx);
                }
                UsbCardEvent::Failed => {
                    trace!("watcher.failed kind=usb");
                    self.state.usb_card_watcher = None;
                    self.state.usb_watcher_retry_at = Some(Instant::now() + Duration::from_secs(2));
                }
            }
        }
        let paused = self.state.reveal.is_some()
            || self.state.copy_pending.is_some()
            || self.state.saving.is_some()
            || self.state.device_verification.running();
        if paused != self.state.card_poll_paused {
            self.state.card_poll_paused = paused;
            trace!(
                "card.poll paused={} reveal={} saving={} verification={} details={}",
                paused,
                self.state.reveal.is_some(),
                self.state.saving.is_some(),
                self.state.device_verification.running(),
                self.state.yubikey_details.is_some()
            );
        }
        if paused {
            return;
        }
        let watch_events = self
            .state
            .card_watcher
            .as_ref()
            .map(|watcher| watcher.events.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        for event in watch_events {
            match event {
                CardWatchEvent::Ready => {
                    if self.state.card_watcher_supported != Some(true) {
                        trace!("watcher.ready kind=gpg");
                    }
                    self.state.card_watcher_supported = Some(true);
                }
                CardWatchEvent::Status(present) => {
                    trace!(
                        "card.presence source=gpg present={present:?} usb_present={:?}",
                        self.state.usb_yubikey_present
                    );
                    self.state.card_watcher_supported = Some(true);
                    if let Some(present) = present {
                        // A live USB event is authoritative for a YubiKey's physical
                        // presence; scdaemon can report a stale removal during reconnect.
                        if present || self.state.usb_yubikey_present != Some(true) {
                            self.apply_card_presence_hint(present, cx);
                        }
                    }
                    if present.is_none() {
                        if self.state.yubikey_check.is_some() {
                            self.state.card_refresh_pending = true;
                        } else {
                            self.start_yubikey_check();
                        }
                    }
                }
                CardWatchEvent::Failed => {
                    trace!("watcher.failed kind=gpg");
                    self.state.card_watcher_supported = Some(false);
                    self.state.card_watcher = None;
                    self.state.card_watcher_retry_at =
                        Some(Instant::now() + Duration::from_secs(2));
                }
            }
        }
        if let Some(completion) = self
            .state
            .yubikey_info_check
            .as_ref()
            .and_then(|probe| probe.poll(&self.state.card_refresh))
        {
            self.state.yubikey_info_check = None;
            if let ProbeResult::Current(info) = completion {
                if self.state.usb_yubikey_present == Some(true)
                    && self.state.yubikey_info.as_ref() != Some(&info)
                {
                    self.state.yubikey_info = Some(info);
                    cx.notify();
                }
            }
        }
        if let Some(completion) = self
            .state
            .yubikey_check
            .as_ref()
            .and_then(|probe| probe.poll(&self.state.card_refresh))
        {
            self.state.yubikey_check = None;
            self.state.yubikey_last_checked = Some(Instant::now());
            self.state.card_refresh_pending = false;
            let result = match completion {
                ProbeResult::Current(result) => Some(result),
                ProbeResult::Stale | ProbeResult::Disconnected => None,
            };
            if let Some(result) = result {
                self.apply_card_probe(result, cx);
            } else {
                trace!("[card-detect] Discarding obsolete or disconnected probe");
                self.start_yubikey_check();
            }
        }
        if self.state.card_watcher.is_none()
            && self
                .state
                .card_watcher_retry_at
                .is_some_and(|retry_at| retry_at <= Instant::now())
        {
            self.start_card_watcher();
        }
        if self.state.usb_card_watcher.is_none()
            && self
                .state
                .usb_watcher_retry_at
                .is_some_and(|retry_at| retry_at <= Instant::now())
        {
            self.start_usb_card_watcher(cx);
        }
        if self.state.yubikey_check.is_none()
            && self.state.yubikey_info_check.is_none()
            && self.state.card_refresh_pending
        {
            self.state.card_refresh_pending = false;
            self.start_yubikey_check();
        }
        if self.state.usb_yubikey_present == Some(true)
            && self.state.yubikey_details.is_none()
            && self.state.yubikey_check.is_none()
            && self
                .state
                .card_detail_retry_at
                .is_some_and(|retry_at| retry_at <= Instant::now())
        {
            self.state.card_detail_retry_at = None;
            self.start_yubikey_check();
        }
        if self.state.card_watcher_supported != Some(true)
            && self.state.yubikey_check.is_none()
            && self.state.yubikey_info_check.is_none()
            && !(self.state.usb_card_watcher.is_some()
                && self.state.usb_yubikey_present == Some(true))
            && self
                .state
                .yubikey_last_checked
                .is_none_or(|last| last.elapsed() >= Duration::from_secs(3))
        {
            self.start_yubikey_check();
            cx.notify();
        }
    }
}
