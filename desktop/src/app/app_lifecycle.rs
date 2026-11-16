use super::*;

impl PassApp {
    pub(super) fn new(options: Options, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let search_focus = cx.focus_handle();
        let generator_name_focus = cx.focus_handle();
        let custom_entry_name_focus = cx.focus_handle();
        let subscription = cx.observe_window_activation(window, |app, window, cx| {
            app.state.active = window.is_window_active();
            if !app.state.active {
                app.state.session.deactivate();
                app.clear_generated(cx);
                app.cancel_edit(cx);
            }
            cx.notify();
        });
        search_focus.focus(window);
        let mut app = Self {
            state: AppState::new(
                options,
                window,
                focus,
                search_focus,
                generator_name_focus,
                custom_entry_name_focus,
            ),
            _activation: subscription,
        };
        cx.on_app_quit(|app, cx| {
            app.state.copy_pending = None;
            app.state.session.hide();
            app.state.device_verification.cancel();
            if let Some((cancel, _, _)) = &app.state.saving {
                cancel.cancel();
            }
            app.restore_pass_clipboard(cx);
            async {}
        })
        .detach();
        cx.on_release(|app, cx| {
            app.state.copy_pending = None;
            if let Some((cancel, _, _)) = &app.state.saving {
                cancel.cancel();
            }
            app.restore_pass_clipboard(cx);
        })
        .detach();
        app.refresh(cx);
        app.start_card_watcher();
        app.start_usb_card_watcher(cx);
        if app.state.yubikey_check.is_none() {
            app.start_yubikey_check();
        }
        cx.spawn(async move |entity, cx| {
            loop {
                let delay = entity
                    .update(cx, |app, _| app.tick_interval())
                    .unwrap_or(Duration::from_secs(2));
                cx.background_executor().timer(delay).await;
                if entity.update(cx, |app, cx| app.tick(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        app
    }

    pub(super) fn tick_interval(&self) -> Duration {
        if self.state.reveal.is_some()
            || self.state.copy_pending.is_some()
            || self.state.saving.is_some()
            || self.state.load.is_some()
            || self.state.device_verification.running()
            || self.state.yubikey_check.is_some()
            || self.state.yubikey_info_check.is_some()
        {
            ACTIVE_POLL_INTERVAL
        } else if self.state.session.revealed(Instant::now()).is_some()
            || self.state.generated.is_some()
            || self.state.clipboard.active()
            || self.state.message_until.is_some()
        {
            UI_TIMER_INTERVAL
        } else if self.state.card_watcher.is_some()
            || self.state.usb_card_watcher.is_some()
            || self.state.card_watcher_retry_at.is_some()
            || self.state.usb_watcher_retry_at.is_some()
        {
            // Watchers are event-driven; this only bounds how long their queued events wait.
            UI_TIMER_INTERVAL
        } else {
            IDLE_POLL_INTERVAL
        }
    }

    pub(super) fn store_is_loading(&self) -> bool {
        self.state.load.is_some() || self.state.loading_demo
    }

    pub(super) fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.state.saving.is_some() {
            return;
        }
        self.cancel_edit(cx);
        self.state.confirming_delete = None;
        if self.state.load.is_some() {
            return;
        }
        self.state.session.select(None);
        self.state.copy_pending = None;
        self.state.reveal = None;
        self.clear_generated(cx);
        self.clear_message();
        self.state.store = None;
        self.state.entries.clear();
        self.state.filtered.clear();
        if self.state.loading_demo {
            cx.notify();
            return;
        }
        if self.state.demo {
            self.state.entries = [
                "Development/github.com.gpg",
                "Development/linear.app.state.gpg",
                "Email/proton.me.gpg",
                "Personal/airbnb.com.gpg",
                "Personal/figma.com.gpg",
                "Personal/notion.so.gpg",
                "Servers/atlas.gpg",
                "Servers/backup.gpg",
            ]
            .into_iter()
            .map(|path| Entry::from_relative_file(path).expect("static demo path"))
            .collect();
            self.filter(cx);
        } else if let Some(path) = self.state.path.clone() {
            let (sender, receiver) = mpsc::sync_channel(1);
            self.state.load = Some(receiver);
            std::thread::spawn(move || {
                let result = Store::open(path)
                    .and_then(|store| store.entries().map(|entries| (store, entries)));
                let _ = sender.send(result);
            });
        } else {
            self.set_message(Error::MissingHome.to_string());
        }
        cx.notify();
    }

    pub(super) fn tick(&mut self, cx: &mut Context<Self>) {
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
                Some(Instant::now() + NOTIFICATION_TIMEOUT)
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
        self.capture_yubikey_status(cx);
        self.poll_yubikey_connection(cx);
        if let Some(result) = self
            .state
            .load
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
        {
            self.state.load = None;
            match result {
                Ok((store, entries)) => {
                    self.state.store = Some(store);
                    self.state.entries = entries;
                    self.filter(cx);
                }
                Err(error) => {
                    self.state.saving_edit = false;
                    self.set_message(error.to_string());
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
                        self.set_message(
                            "Entry stayed hidden. Return to this window and reveal again.".into(),
                        );
                    } else {
                        self.state.revealed_revision = revision;
                    }
                }
                Err(error) => {
                    if self.state.session.fail_reveal(&request) {
                        self.set_message(error.to_string());
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
                    Err(error) => self.set_message(error.to_string()),
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
        if self
            .state
            .generated
            .as_ref()
            .is_some_and(|(_, deadline)| Instant::now() >= *deadline)
        {
            self.clear_generated(cx);
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
            let was_custom_entry = self.state.saving_custom_entry;
            self.state.saving_custom_entry = false;
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
                    self.state.entries.push(entry);
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
                    if was_custom_entry {
                        self.state.saved_entry_name = None;
                        self.close_generator(cx);
                        self.set_message(i18n::text("custom-entry-saved"));
                    } else {
                        self.state.saved_entry_name = Some(name.clone());
                        if !self.state.close_after_save && self.state.active {
                            let text = secret.expose().to_owned();
                            self.copy_to_clipboard(text, name, cx);
                            self.state.generated = Some((secret, Instant::now() + REVEAL_DURATION));
                        }
                        self.clear_message();
                    }
                }
                Err(error) => {
                    self.state.saving_edit = false;
                    self.state.saving_custom_entry = false;
                    if matches!(&error, Error::EntryConflict) && was_edit {
                        self.state.edit_conflict = true;
                        self.set_message(i18n::text("edit-conflict-notice"));
                    } else if matches!(&error, Error::EntryConflict) {
                        self.state.edit_conflict = false;
                        self.set_message(i18n::text("overwrite-entry-conflict"));
                    } else {
                        self.state.edit_conflict = false;
                        self.set_message(error.to_string());
                    }
                }
            }
            let close = self.state.close_after_save;
            self.state.close_after_save = false;
            if close {
                self.close_generator(cx);
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

    pub(super) fn set_message(&mut self, message: String) {
        self.state.message = Some(message);
        self.state.message_until = Some(Instant::now() + NOTIFICATION_TIMEOUT);
    }

    pub(super) fn clear_message(&mut self) {
        self.state.message = None;
        self.state.message_until = None;
    }

    pub(super) fn filter(&mut self, cx: &mut Context<Self>) {
        if self.state.saving.is_some() {
            return;
        }
        self.cancel_edit(cx);
        self.state.confirming_delete = None;
        self.state.session.hide();
        self.state.copy_pending = None;
        self.state.reveal = None;
        self.clear_generated(cx);
        self.clear_message();
        let terms: Vec<_> = self
            .state
            .query
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        self.state.filtered = self
            .state
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, entry)| entry.matches_terms(&terms).then_some(i))
            .collect();
        let first = self
            .state
            .filtered
            .first()
            .map(|index| self.state.entries[*index].clone());
        self.state.session.select(first);
        self.state
            .list_scroll
            .scroll_to_item(0, ScrollStrategy::Top);
    }

    pub(super) fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.state.saving.is_some() {
            return;
        }
        self.cancel_edit(cx);
        self.state
            .session
            .select(self.state.entries.get(index).cloned());
        self.state.reveal = None;
        self.state.copy_pending = None;
        self.clear_generated(cx);
        self.clear_message();
        cx.notify();
    }

    pub(super) fn navigate(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.state.filtered.is_empty() {
            return;
        }
        let position = self
            .state
            .filtered
            .iter()
            .position(|index| self.state.session.selected() == self.state.entries.get(*index))
            .unwrap_or(0);
        let next =
            (position as isize + delta).clamp(0, self.state.filtered.len() as isize - 1) as usize;
        self.choose(self.state.filtered[next], cx);
        self.state
            .list_scroll
            .scroll_to_item(next, ScrollStrategy::Center);
    }

    pub(super) fn hide(&mut self, cx: &mut Context<Self>) {
        if self.state.confirming_overwrite.take().is_some() {
            cx.notify();
            return;
        }
        if self.state.confirming_delete.take().is_some() {
            cx.notify();
            return;
        }
        if self.state.navigation.generator_open() {
            self.close_generator(cx);
            return;
        }
        if self.state.editing_entry {
            self.cancel_edit(cx);
            cx.notify();
            return;
        }
        if self.state.navigation.page() == Page::CardDetails {
            self.leave_card_status(cx);
            return;
        }
        self.state.session.hide();
        self.state.copy_pending = None;
        self.state.reveal = None;
        self.clear_generated(cx);
        self.clear_message();
        cx.notify();
    }
}
