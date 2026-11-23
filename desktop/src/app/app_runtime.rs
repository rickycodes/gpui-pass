use super::*;

impl PassApp {
    pub(crate) fn new(options: Options, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let search_focus = cx.focus_handle();
        let composer_content_focus = cx.focus_handle();
        let custom_entry_name_focus = cx.focus_handle();
        let subscription = cx.observe_window_activation(window, |app, window, cx| {
            app.state.active = window.is_window_active();
            if !app.state.active {
                app.state.sidebar_scroll.cancel();
                app.state.session.deactivate();
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
                composer_content_focus,
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
        if !app.skips_external_services() {
            app.start_card_watcher();
            app.start_usb_card_watcher(cx);
            if app.state.yubikey_check.is_none() {
                app.start_yubikey_check();
            }
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

    pub(crate) fn skips_external_services(&self) -> bool {
        self.state.demo
            || self.state.onboarding_demo
            || self.state.preview_store_load
            || self
                .state
                .onboarding
                .as_ref()
                .is_some_and(|onboarding| onboarding.preview)
    }

    pub(crate) fn tick_interval(&self) -> Duration {
        if self.state.reveal.is_some()
            || self.state.copy_pending.is_some()
            || self.state.saving.is_some()
            || self.state.load.is_some()
            || self.state.device_verification.running()
            || self.state.yubikey_check.is_some()
            || self.state.yubikey_info_check.is_some()
        {
            constants::ACTIVE_POLL_INTERVAL
        } else if cfg!(target_os = "linux")
            || self.state.card_watcher.is_some()
            || self.state.usb_card_watcher.is_some()
        {
            // Drain hotplug events promptly and reconcile USB presence even if
            // a watcher stays alive without delivering a removal notification.
            constants::ACTIVE_POLL_INTERVAL
        } else if self.state.session.revealed(Instant::now()).is_some()
            || self.state.clipboard.active()
            || self.state.message_until.is_some()
        {
            constants::UI_TIMER_INTERVAL
        } else {
            constants::IDLE_POLL_INTERVAL
        }
    }

    pub(crate) fn store_is_loading(&self) -> bool {
        self.state.load.is_some() || self.state.loading_demo
    }

    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        self.state.sidebar_scroll.cancel();
        if self.state.saving.is_some() {
            return;
        }
        if self.state.onboarding.is_some() {
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
        self.clear_message();
        self.state.store = None;
        self.state.entries.clear();
        self.state.filtered.clear();
        self.state.sidebar_rows.clear();
        self.state.sidebar_cursor = 0;
        self.state.expanded_folders.clear();
        if self.state.loading_demo {
            cx.notify();
            return;
        }
        if self.state.onboarding_demo {
            let path = self
                .state
                .path
                .clone()
                .unwrap_or_else(|| PathBuf::from("~/.password-store"));
            self.open_onboarding(path, true, cx);
            return;
        }
        if self.state.demo {
            if !self.state.demo_empty_store {
                self.state.entries = constants::DEMO_ENTRIES
                    .iter()
                    .copied()
                    .map(|path| Entry::from_relative_file(path).expect("static demo path"))
                    .collect();
            }
            self.filter(cx);
        } else if let Some(path) = self.state.path.clone() {
            if !path.exists() {
                self.open_onboarding(path, false, cx);
                return;
            }
            let (sender, receiver) = mpsc::sync_channel(1);
            self.state.load = Some(receiver);
            std::thread::spawn(move || {
                let result = Store::open(path)
                    .and_then(|store| store.entries().map(|entries| (store, entries)));
                let _ = sender.send(result);
            });
        } else {
            self.set_error(Error::MissingHome.to_string());
        }
        cx.notify();
    }
}
