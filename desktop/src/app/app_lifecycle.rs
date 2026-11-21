use super::*;

#[derive(Default)]
struct FolderTree {
    entries: Vec<usize>,
    children: BTreeMap<String, FolderTree>,
}

impl FolderTree {
    fn entry_count(&self) -> usize {
        self.entries.len()
            + self
                .children
                .values()
                .map(FolderTree::entry_count)
                .sum::<usize>()
    }
}

impl PassApp {
    pub(super) fn new(options: Options, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let search_focus = cx.focus_handle();
        let generator_name_focus = cx.focus_handle();
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
        } else if cfg!(target_os = "linux")
            || self.state.card_watcher.is_some()
            || self.state.usb_card_watcher.is_some()
        {
            // Drain hotplug events promptly and reconcile USB presence even if
            // a watcher stays alive without delivering a removal notification.
            ACTIVE_POLL_INTERVAL
        } else if self.state.session.revealed(Instant::now()).is_some()
            || self.state.clipboard.active()
            || self.state.message_until.is_some()
        {
            UI_TIMER_INTERVAL
        } else {
            IDLE_POLL_INTERVAL
        }
    }

    pub(super) fn store_is_loading(&self) -> bool {
        self.state.load.is_some() || self.state.loading_demo
    }

    pub(super) fn refresh(&mut self, cx: &mut Context<Self>) {
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
                self.state.entries = [
                    "Development/github.com.gpg",
                    "Development/linear.app.state.gpg",
                    "Development/gitlab.com.gpg",
                    "Development/docker.com.gpg",
                    "Development/npmjs.com.gpg",
                    "Development/crates.io.gpg",
                    "Development/aws.console.gpg",
                    "Development/cloudflare.com.gpg",
                    "Development/sentry.io.gpg",
                    "Development/vercel.com.gpg",
                    "Email/proton.me.gpg",
                    "Email/gmail.com.gpg",
                    "Email/fastmail.com.gpg",
                    "Email/hey.com.gpg",
                    "Email/mailgun.com.gpg",
                    "Email/sendgrid.com.gpg",
                    "Email/postmark.app.gpg",
                    "Email/tutanota.com.gpg",
                    "Finance/banking.com.gpg",
                    "Finance/wise.com.gpg",
                    "Finance/stripe.com.gpg",
                    "Finance/coinbase.com.gpg",
                    "Finance/fidelity.com.gpg",
                    "Finance/taxes.gov.gpg",
                    "Personal/airbnb.com.gpg",
                    "Personal/figma.com.gpg",
                    "Personal/notion.so.gpg",
                    "Personal/calendar.google.com.gpg",
                    "Personal/photos.google.com.gpg",
                    "Personal/spotify.com.gpg",
                    "Personal/netflix.com.gpg",
                    "Personal/amazon.com.gpg",
                    "Personal/homeassistant.local.gpg",
                    "Personal/recipes.gpg",
                    "Servers/atlas.gpg",
                    "Servers/backup.gpg",
                    "Servers/edge.gpg",
                    "Servers/monitoring.gpg",
                    "Servers/postgres.gpg",
                    "Servers/redis.gpg",
                    "Servers/staging.gpg",
                    "Servers/production.gpg",
                    "Servers/vpn.gpg",
                    "Servers/bastion.gpg",
                    "Services/slack.com.gpg",
                    "Services/discord.com.gpg",
                    "Services/todoist.com.gpg",
                    "Services/zoom.us.gpg",
                    "Services/dropbox.com.gpg",
                    "Services/tailscale.com.gpg",
                ]
                .into_iter()
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
            self.set_message(Error::MissingHome.to_string());
        }
        cx.notify();
    }

    pub(super) fn tick(&mut self, cx: &mut Context<Self>) {
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
                }
                Err(error) => {
                    self.state.saving_edit = false;
                    if preview {
                        if let Some(path) = self.state.path.clone() {
                            self.open_onboarding(path, true, cx);
                            self.set_message(error.to_string());
                        }
                    } else if self.state.path.as_ref().is_some_and(|path| !path.exists()) {
                        if let Some(path) = self.state.path.clone() {
                            self.open_onboarding(path, false, cx);
                        }
                    } else {
                        self.set_message(error.to_string());
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
        self.state.sidebar_scroll.cancel();
        if self.state.saving.is_some() {
            return;
        }
        self.cancel_edit(cx);
        self.state.confirming_delete = None;
        self.state.session.hide();
        self.state.copy_pending = None;
        self.state.reveal = None;
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
        self.rebuild_sidebar_rows();
        self.state
            .list_scroll
            .scroll_to_item(0, ScrollStrategy::Top);
    }

    fn add_folder_paths(folder: &str, folders: &mut HashSet<String>) {
        let mut path = String::new();
        for part in folder.split('/').filter(|part| !part.is_empty()) {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(part);
            folders.insert(path.clone());
        }
    }

    fn insert_entry(tree: &mut FolderTree, entry: &Entry, index: usize) {
        let mut node = tree;
        for part in entry.folder().split('/').filter(|part| !part.is_empty()) {
            node = node.children.entry(part.to_owned()).or_default();
        }
        node.entries.push(index);
    }

    fn flatten_sidebar_rows(
        tree: &FolderTree,
        prefix: &str,
        depth: usize,
        expanded: &HashSet<String>,
        rows: &mut Vec<SidebarRow>,
    ) {
        for index in &tree.entries {
            rows.push(SidebarRow::Entry {
                index: *index,
                depth,
            });
        }
        for (name, child) in &tree.children {
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            rows.push(SidebarRow::Folder {
                path: path.clone(),
                depth,
                count: child.entry_count(),
            });
            if expanded.contains(&path) {
                Self::flatten_sidebar_rows(child, &path, depth + 1, expanded, rows);
            }
        }
    }

    fn rebuild_sidebar_rows(&mut self) {
        let mut tree = FolderTree::default();
        for index in &self.state.filtered {
            Self::insert_entry(&mut tree, &self.state.entries[*index], *index);
        }
        if !self.state.query.trim().is_empty() {
            for index in &self.state.filtered {
                Self::add_folder_paths(
                    self.state.entries[*index].folder(),
                    &mut self.state.expanded_folders,
                );
            }
        }
        let previous = self
            .state
            .sidebar_rows
            .get(self.state.sidebar_cursor)
            .cloned();
        let mut rows = Vec::new();
        Self::flatten_sidebar_rows(&tree, "", 0, &self.state.expanded_folders, &mut rows);
        self.state.sidebar_rows = rows;
        self.state.sidebar_cursor = previous
            .and_then(|row| {
                self.state
                    .sidebar_rows
                    .iter()
                    .position(|candidate| *candidate == row)
            })
            .or_else(|| {
                self.state.session.selected().and_then(|selected| {
                    self.state.sidebar_rows.iter().position(|row| {
                        matches!(
                            row,
                            SidebarRow::Entry { index, .. }
                                if self.state.entries[*index] == *selected
                        )
                    })
                })
            })
            .or_else(|| {
                self.state
                    .sidebar_rows
                    .iter()
                    .position(|row| matches!(row, SidebarRow::Entry { .. }))
            })
            .unwrap_or(0);
    }

    pub(super) fn toggle_folder(&mut self, path: &str, cx: &mut Context<Self>) {
        if self.state.expanded_folders.contains(path) {
            self.state.expanded_folders.remove(path);
        } else {
            self.state.expanded_folders.insert(path.to_owned());
        }
        self.rebuild_sidebar_rows();
        self.state.sidebar_cursor = self
            .state
            .sidebar_rows
            .iter()
            .position(
                |row| matches!(row, SidebarRow::Folder { path: row_path, .. } if row_path == path),
            )
            .unwrap_or(self.state.sidebar_cursor);
        cx.notify();
    }

    pub(super) fn move_sidebar_horizontally(
        &mut self,
        delta: isize,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.state.custom_entry_name_focus.is_focused(window)
            || self.state.generator_name_focus.is_focused(window)
            || !self.state.query.is_empty()
        {
            return false;
        }
        let Some(row) = self
            .state
            .sidebar_rows
            .get(self.state.sidebar_cursor)
            .cloned()
        else {
            return false;
        };
        let path = match row {
            SidebarRow::Folder { path, .. } => path,
            SidebarRow::Entry { index, .. } => self.state.entries[index].folder().to_owned(),
        };
        if path.is_empty() {
            return true;
        }
        if delta < 0 {
            self.state.expanded_folders.remove(&path);
        } else {
            self.state.expanded_folders.insert(path.clone());
        }
        self.rebuild_sidebar_rows();
        self.state.sidebar_cursor = self
            .state
            .sidebar_rows
            .iter()
            .position(|row| match row {
                SidebarRow::Folder { path: row_path, .. } => row_path == &path,
                SidebarRow::Entry { index, .. } => self.state.entries[*index].folder() == path,
            })
            .unwrap_or(self.state.sidebar_cursor);
        cx.notify();
        true
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
        self.clear_message();
        if let Some(position) = self.state.sidebar_rows.iter().position(
            |row| matches!(row, SidebarRow::Entry { index: row_index, .. } if *row_index == index),
        ) {
            self.state.sidebar_cursor = position;
        }
        cx.notify();
    }

    pub(super) fn navigate(&mut self, delta: isize, cx: &mut Context<Self>) {
        self.state.sidebar_scroll.cancel();
        if self.state.sidebar_rows.is_empty() {
            return;
        }
        let position = self
            .state
            .sidebar_cursor
            .min(self.state.sidebar_rows.len().saturating_sub(1));
        let next = (position as isize + delta).clamp(0, self.state.sidebar_rows.len() as isize - 1)
            as usize;
        self.state.sidebar_cursor = next;
        if let SidebarRow::Entry { index, .. } = self.state.sidebar_rows[next] {
            self.choose(index, cx);
        } else {
            cx.notify();
        }
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
        self.clear_message();
        cx.notify();
    }
}
