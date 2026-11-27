use super::*;

impl PassApp {
    pub(crate) fn tick(&mut self, cx: &mut Context<Self>) {
        diagnostics::performance_tick(self.store_is_loading(), self.state.active);
        self.poll_onboarding(cx);
        if self
            .state
            .device_verification
            .poll(self.state.card_refresh.generation())
        {
            cx.notify();
        }
        let verification_state = self.state.device_verification.state();
        if verification_state != self.state.verification_state_seen {
            self.state.verification_state_seen = verification_state;
            self.state.verification_notification_until = if matches!(
                verification_state,
                VerificationState::Idle | VerificationState::Pending
            ) {
                None
            } else {
                Some(Instant::now() + constants::NOTIFICATION_TIMEOUT)
            };
            cx.notify();
        }
        if self
            .state
            .verification_notification_until
            .is_some_and(|until| Instant::now() >= until)
        {
            self.state.verification_notification_until = None;
            cx.notify();
        }
        if !self.skips_external_services() {
            self.capture_yubikey_status(cx);
            self.poll_yubikey_connection(cx);
        }
        if let Some(result) = self
            .state
            .load
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
        {
            self.state.load = None;
            let preview = self.state.preview_store_load;
            self.state.preview_store_load = false;
            match result {
                Ok((store, entries)) => {
                    let initialized = store.has_root_recipient_policy().unwrap_or(true);
                    self.state.store = Some(store);
                    self.state.entries = entries;
                    if !initialized && self.state.entries.is_empty() {
                        if let Some(path) = self.state.path.clone() {
                            self.open_onboarding(path, preview, cx);
                        }
                    } else {
                        self.filter(cx);
                    }
                    if let Some(path) = self.state.persist_path_on_open.take() {
                        if let Err(error) = store_config::save_store_path(&path) {
                            self.set_error(format!(
                                "Store opened, but its location could not be saved: {error}"
                            ));
                        }
                    }
                }
                Err(error) => {
                    self.state.saving_edit = false;
                    if preview {
                        if let Some(path) = self.state.path.clone() {
                            self.open_onboarding(path, true, cx);
                            self.set_error(error.to_string());
                        }
                    } else if self.state.path.as_ref().is_some_and(|path| !path.exists()) {
                        if let Some(path) = self.state.path.clone() {
                            self.open_onboarding(path, false, cx);
                        }
                        self.state.persist_path_on_open = None;
                    } else {
                        self.state.persist_path_on_open = None;
                        self.set_error(error.to_string());
                    }
                }
            }
            cx.notify();
        }
        if let Some(result) = self
            .state
            .reveal
            .as_ref()
            .and_then(|(_, receiver)| receiver.try_recv().ok())
        {
            let (request, _) = self.state.reveal.take().expect("request available");
            match result {
                Ok((secret, revision)) => {
                    if !self.state.session.finish_reveal(
                        &request,
                        secret,
                        self.state.active,
                        Instant::now(),
                    ) {
                        self.set_error(
                            "Entry stayed hidden. Return to this window and reveal again.".into(),
                        );
                    } else {
                        self.state.revealed_revision = revision;
                    }
                }
                Err(error) => {
                    if self.state.session.fail_reveal(&request) {
                        self.set_error(error.to_string());
                    }
                }
            }
            cx.notify();
        }
        if let Some(result) = self
            .state
            .copy_pending
            .as_ref()
            .and_then(|pending| pending.receiver.try_recv().ok())
        {
            let pending = self
                .state
                .copy_pending
                .take()
                .expect("copy request available");
            let entry = pending.entry.clone();
            if self.state.active && self.state.session.selected() == Some(&entry) {
                match result {
                    Ok(secret) => {
                        self.copy_to_clipboard(
                            secret.expose().to_owned(),
                            entry.name().to_owned(),
                            cx,
                        );
                    }
                    Err(error) => self.set_error(error.to_string()),
                }
            }
            cx.notify();
        }
        if self
            .state
            .message_until
            .is_some_and(|until| Instant::now() >= until)
        {
            self.state.message = None;
            self.state.message_until = None;
            cx.notify();
        }
        let now = Instant::now();
        let clipboard_seconds = self.state.clipboard.remaining_seconds(now);
        if self.state.clipboard_seconds != clipboard_seconds {
            if let Some(item) = self.state.clipboard.poll(cx.read_from_clipboard(), now) {
                cx.write_to_clipboard(item);
            }
            self.state.clipboard_seconds = self.state.clipboard.remaining_seconds(now);
            cx.notify();
        }
        if self.state.session.expire(Instant::now()) {
            cx.notify();
        }
        let seconds = self.state.session.remaining_seconds(Instant::now());
        if self.state.seconds != seconds {
            self.state.seconds = seconds;
            cx.notify();
        }
        if let Some(result) = self
            .state
            .saving
            .as_ref()
            .and_then(|(_, _, receiver)| receiver.try_recv().ok())
        {
            self.state.saving = None;
            let was_edit = self.state.saving_edit;
            let was_new_entry = self.state.saving_new_entry;
            let was_generated = self.state.content_generated;
            self.state.saving_new_entry = false;
            self.state.content_generated = false;
            match result {
                Ok((entry, secret)) => {
                    let name = entry.name().to_owned();
                    if self.state.saving_edit {
                        self.state.saving_edit = false;
                        self.cancel_edit(cx);
                        self.state.session.select(Some(entry));
                        self.set_message("Entry updated.".into());
                        cx.notify();
                        return;
                    }
                    if let Some(index) = self
                        .state
                        .entries
                        .iter()
                        .position(|existing| existing.name() == name)
                    {
                        self.state.entries[index] = entry;
                    } else {
                        self.state.entries.push(entry);
                    }
                    self.state.entries.sort_by_cached_key(|entry| {
                        (entry.name().to_lowercase(), entry.name().to_owned())
                    });
                    self.state.query.clear();
                    self.state.selection = 0..0;
                    self.state.marked = None;
                    self.filter(cx);
                    self.state.session.select(
                        self.state
                            .entries
                            .iter()
                            .find(|entry| entry.name() == name)
                            .cloned(),
                    );
                    if was_new_entry {
                        if was_generated {
                            self.copy_to_clipboard(secret.expose().to_owned(), name.clone(), cx);
                        }
                        self.clear_create_entry();
                        self.set_message(i18n::text("custom-entry-saved"));
                    } else {
                        self.clear_message();
                    }
                }
                Err(error) => {
                    self.state.saving_edit = false;
                    self.state.saving_new_entry = false;
                    if matches!(&error, Error::EntryConflict) && was_edit {
                        self.state.edit_conflict = true;
                        self.set_error(i18n::text("edit-conflict-notice"));
                    } else if matches!(&error, Error::EntryConflict) {
                        self.state.edit_conflict = false;
                        self.set_error(i18n::text("overwrite-entry-conflict"));
                    } else {
                        self.state.edit_conflict = false;
                        self.set_error(error.to_string());
                    }
                }
            }
            let close = self.state.close_after_save;
            self.state.close_after_save = false;
            if close {
                self.close_composer(cx);
            }
            cx.notify();
        }
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
        if self.state.last_operation_status != status {
            self.state.last_operation_status = status;
            cx.notify();
        }
    }

    pub(crate) fn set_message(&mut self, message: String) {
        self.state.message = Some(message);
        self.state.message_is_error = false;
        self.state.message_until = Some(Instant::now() + constants::NOTIFICATION_TIMEOUT);
    }

    pub(crate) fn set_error(&mut self, message: String) {
        self.state.message = Some(message);
        self.state.message_is_error = true;
        self.state.message_until = Some(Instant::now() + constants::NOTIFICATION_TIMEOUT);
    }

    pub(crate) fn clear_message(&mut self) {
        self.state.message = None;
        self.state.message_is_error = false;
        self.state.message_until = None;
    }
}
