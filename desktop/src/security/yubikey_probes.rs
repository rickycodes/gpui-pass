use super::*;

impl PassApp {
    pub(crate) fn start_yubikey_check(&mut self) {
        // A refresh request is not a physical card change. Coalesce requests
        // without making the running probe's result stale.
        if self.state.yubikey_check.is_some() {
            return;
        }
        if self.state.yubikey_info_check.is_some()
            || self.state.reveal.is_some()
            || self.state.copy_pending.is_some()
            || self.state.saving.is_some()
            || self.state.device_verification.running()
        {
            if !self.state.card_refresh_pending {
                trace!(
                    "probe.deferred ykman={} reveal={} saving={}",
                    self.state.yubikey_info_check.is_some(),
                    self.state.reveal.is_some(),
                    self.state.saving.is_some()
                );
            }
            self.state.card_refresh_pending = true;
            return;
        }
        // Keep the initial loading state, then leave the last result on screen while the
        // background probe runs. An absent key should not flash "Checking" on every retry.
        if self.state.yubikey_last_checked.is_none()
            && self.state.yubikey_connection != YubiConnection::Connected
        {
            self.state.yubikey_connection = YubiConnection::Checking;
        }
        let executable = self.state.gpg.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        self.state.card_refresh_pending = false;
        let probe = CardProbe::new("gpg", self.state.card_refresh.generation(), receiver);
        let probe_id = probe.id();
        self.state.yubikey_check = Some(probe);
        std::thread::spawn(move || {
            let result = check_card_status(&executable, probe_id);
            trace!(
                "[card-detect] GnuPG probe completed: state={:?}, details={}",
                result.connection,
                result.details.is_some()
            );
            let _ = sender.send(result);
        });
    }

    pub(crate) fn start_yubikey_info_probe(&mut self) {
        if self.state.device_verification.running()
            || self.state.reveal.is_some()
            || self.state.copy_pending.is_some()
            || self.state.saving.is_some()
            || self.state.yubikey_info.is_some()
            || self.state.yubikey_info_check.is_some()
            || self.state.usb_yubikey_present != Some(true)
            || !self
                .state
                .usb_reader_details
                .as_ref()
                .is_some_and(|details| details.vendor_id == 0x1050)
        {
            return;
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        let probe = CardProbe::new("ykman-hid", self.state.card_refresh.generation(), receiver);
        let probe_id = probe.id();
        self.state.yubikey_info_check = Some(probe);
        std::thread::spawn(move || {
            let _ = sender.send(query_yubikey_info(probe_id));
        });
    }

    pub(crate) fn apply_card_probe(&mut self, result: CardProbeResult, cx: &mut Context<Self>) {
        trace!(
            "[card-detect] Applying current probe: state={:?}, details={}",
            result.connection,
            result.details.is_some()
        );
        if self.state.usb_yubikey_present == Some(true)
            && (result.connection != YubiConnection::Connected || result.details.is_none())
        {
            // A miss during enumeration does not invalidate physical presence or
            // details already obtained for this same insertion.
            self.state.yubikey_connection = YubiConnection::Connected;
            if self.state.yubikey_details.is_none() {
                let delay = self.state.card_refresh.retry_delay();
                self.state.card_detail_retry_at = Some(Instant::now() + delay);
                trace!(
                    "[card-detect] Card details not ready; retrying in {}ms",
                    delay.as_millis()
                );
            }
            cx.notify();
            return;
        }
        // Busy is inconclusive, so keep any already connected card on screen.
        if result.connection == YubiConnection::Busy {
            if self.state.yubikey_connection != YubiConnection::Connected {
                self.state.yubikey_connection = YubiConnection::Busy;
                self.state.yubikey_details = None;
                cx.notify();
            }
            return;
        }
        let changed = self.state.yubikey_connection != result.connection
            || self.state.yubikey_details != result.details;
        self.state.yubikey_connection = result.connection;
        self.state.yubikey_details = result.details;
        if self.state.yubikey_connection != YubiConnection::Connected {
            self.state.device_verification.invalidate();
            self.state.navigation.card_unavailable();
        } else if self.state.yubikey_details.is_some() {
            self.state.card_detail_retry_at = None;
            self.state.card_refresh.completed();
            self.start_yubikey_info_probe();
        }
        if changed {
            cx.notify();
        }
    }
}
