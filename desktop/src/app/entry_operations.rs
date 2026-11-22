use super::*;

impl PassApp {
    pub(super) fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        if !self.state.editing_entry {
            return;
        }
        let had_conflict = self.state.edit_conflict;
        if self.state.saving_edit {
            if let Some((cancel, _, _)) = &self.state.saving {
                cancel.cancel();
            }
        }
        self.state.editing_entry = false;
        self.state.edit_conflict = false;
        self.state.edit_revision = None;
        if had_conflict {
            self.state.session.hide();
            self.state.revealed_revision = None;
        }
        self.state.generator_name.zeroize();
        self.state.generator_name_selection = 0..0;
        self.state.generator_name_anchor = None;
        self.state.generator_name_cursor = 0;
        self.state.generator_name_marked = None;
        self.state.editor_layout = None;
        cx.notify();
    }

    pub(super) fn request_delete(&mut self, cx: &mut Context<Self>) {
        if self.state.saving.is_some() {
            return;
        }
        if let Some(entry) = self.state.session.selected().cloned() {
            self.state.session.hide();
            self.state.reveal = None;
            self.state.confirming_delete = Some(entry);
            cx.notify();
        }
    }

    pub(super) fn begin_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.state.saving.is_some() {
            return;
        }
        let Some(secret) = self.state.session.revealed(Instant::now()) else {
            self.set_error("Reveal the entry before editing it.".into());
            cx.notify();
            return;
        };
        if !editor_input::supported(secret.expose()) {
            self.set_error(i18n::text("editor-limit"));
            cx.notify();
            return;
        }
        self.state.generator_name = Zeroizing::new(secret.expose().to_owned());
        self.state.generator_name_selection =
            self.state.generator_name.len()..self.state.generator_name.len();
        self.state.generator_name_anchor = None;
        self.state.generator_name_cursor = self.state.generator_name.len();
        self.state.generator_name_marked = None;
        self.state.edit_revision = self.state.revealed_revision;
        self.state.editing_entry = true;
        self.state.edit_conflict = false;
        self.state.generator_name_bounds = None;
        self.state.editor_layout = None;
        self.state.generator_name_focus.focus(window);
        cx.notify();
    }

    pub(super) fn overwrite_conflicted_edit(&mut self, cx: &mut Context<Self>) {
        if !self.state.edit_conflict || self.state.saving.is_some() {
            return;
        }
        let Some(entry) = self.state.session.selected() else {
            return;
        };
        let Some(store) = self.state.store.as_ref() else {
            return;
        };
        match store.entry_revision(entry) {
            Ok(revision) => {
                self.state.edit_revision = Some(revision);
                self.save_edit(cx);
            }
            Err(error) => {
                self.set_error(error.to_string());
                cx.notify();
            }
        }
    }

    pub(super) fn discard_conflicted_edit(&mut self, cx: &mut Context<Self>) {
        if !self.state.edit_conflict || self.state.saving.is_some() {
            return;
        }
        self.cancel_edit(cx);
        self.set_message(i18n::text("edit-conflict-discarded"));
        cx.notify();
    }

    pub(super) fn save_edit(&mut self, cx: &mut Context<Self>) {
        if self.state.saving.is_some() {
            return;
        }
        let Some(entry) = self.state.session.selected().cloned() else {
            return;
        };
        let Some(store) = self.state.store.clone() else {
            self.set_error("Open a password store before editing entries.".into());
            cx.notify();
            return;
        };
        let Some(revision) = self.state.edit_revision else {
            self.set_error("Reopen and reveal the entry before saving.".into());
            cx.notify();
            return;
        };
        let secret = match Secret::from_bytes(Zeroizing::new(
            self.state.generator_name.as_bytes().to_vec(),
        )) {
            Ok(secret) => secret,
            Err(error) => {
                self.set_error(error.to_string());
                cx.notify();
                return;
            }
        };
        let gpg = match Gpg::new(&self.state.gpg) {
            Ok(gpg) => gpg,
            Err(error) => {
                self.set_error(error.to_string());
                cx.notify();
                return;
            }
        };
        let cancellation = Cancellation::default();
        let progress = OperationProgress::new(OperationStatus::working(
            "Preparing entry update",
            "The updated content stays in memory until GnuPG encrypts it.",
        ));
        let worker_cancel = cancellation.clone();
        let worker_progress = progress.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let result = gpg
                .update_entry_if_unchanged(
                    &store,
                    &entry,
                    &secret,
                    &revision,
                    &worker_cancel,
                    &worker_progress,
                )
                .map(|()| (entry, secret));
            let _ = sender.send(result);
        });
        self.state.saving_edit = true;
        self.state.saving = Some((cancellation, progress, receiver));
        self.clear_message();
        cx.notify();
    }

    pub(super) fn confirm_delete(&mut self, cx: &mut Context<Self>) {
        let Some(entry) = self.state.confirming_delete.take() else {
            return;
        };
        let result = self
            .state
            .store
            .as_ref()
            .ok_or(Error::InvalidEntry)
            .and_then(|store| store.remove_entry(&entry));
        match result {
            Ok(()) => {
                self.state.session.hide();
                self.state.reveal = None;
                self.state.entries.retain(|existing| existing != &entry);
                self.filter(cx);
            }
            Err(error) => self.set_error(error.to_string()),
        }
        cx.notify();
    }

    pub(super) fn delete_confirmation_view(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(entry) = &self.state.confirming_delete else {
            return div().into_any_element();
        };
        div()
            .id("delete-confirmation")
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bottom_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(self.state.theme.colors.background))
            .child(
                self.state.theme.panel()
                    .w(px(520.))
                    .max_w(px(520.))
                    .p(px(32.))
                    .flex()
                    .flex_col()
                    .gap(px(20.))
                    .child(self.state.theme.label("REMOVE PASSWORD ENTRY"))
                    .child(
                        div()
                            .text_size(px(24.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(entry.name().to_owned()),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(21.))
                            .text_color(rgb(self.state.theme.colors.muted))
                            .child("This removes the encrypted entry from the current store. It does not erase copies in Git history or backups."),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(10.))
                            .child(self.state.theme.button("cancel-delete", "Cancel", false).on_click(
                                cx.listener(|app, _, _, cx| {
                                    app.state.confirming_delete = None;
                                    cx.notify();
                                }),
                            ))
                            .child(
                                self.state.theme
                                    .destructive_button("confirm-delete", "Delete entry")
                                    .on_click(cx.listener(|app, _, _, cx| app.confirm_delete(cx))),
                            ),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn copy_to_clipboard(
        &mut self,
        content: String,
        source: String,
        cx: &mut Context<Self>,
    ) {
        let timeout = std::env::var("PASSWORD_STORE_CLIP_TIME").ok();
        let deadline = clipboard::clipboard_deadline(Instant::now(), timeout.as_deref());
        let item = self
            .state
            .clipboard
            .copy(content, source, cx.read_from_clipboard(), deadline);
        cx.write_to_clipboard(item);
        self.state.clipboard_seconds = None;
        cx.notify();
    }

    pub(super) fn restore_pass_clipboard(&mut self, cx: &mut App) {
        if let Some(item) = self.state.clipboard.restore(cx.read_from_clipboard()) {
            cx.write_to_clipboard(item);
        }
    }

    pub(super) fn reveal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state.focus.focus(window);
        if self.state.copy_pending.is_some()
            || self.state.device_verification.running()
            || self.state.session.is_pending()
            || !window.is_window_active()
        {
            return;
        }
        if self.state.session.revealed(Instant::now()).is_some() {
            self.hide(cx);
            return;
        }
        self.state.revealed_revision = None;
        let Some(request) = self.state.session.begin_reveal() else {
            return;
        };
        self.clear_message();
        let result = if self.state.demo {
            let secret =
                Secret::from_bytes(Zeroizing::new(constants::DEMO_SECRET.as_bytes().to_vec()));
            let (sender, receiver) = mpsc::sync_channel(1);
            let _ = sender.send(secret.map(|secret| (secret, None)));
            receiver
        } else {
            let gpg = match Gpg::new(&self.state.gpg) {
                Ok(gpg) => gpg,
                Err(error) => {
                    self.state.session.fail_reveal(&request);
                    self.set_error(error.to_string());
                    cx.notify();
                    return;
                }
            };
            let Some(store) = self.state.store.clone() else {
                self.state.session.fail_reveal(&request);
                return;
            };
            let entry = request.entry.clone();
            let cancel = request.cancellation.clone();
            let progress = request.progress.clone();
            let (sender, receiver) = mpsc::sync_channel(1);
            std::thread::spawn(move || {
                let _ = sender.send(
                    gpg.decrypt_with_revision(&store, &entry, &cancel, &progress)
                        .map(|(secret, revision)| (secret, Some(revision))),
                );
            });
            receiver
        };
        self.state.reveal = Some((request, result));
        cx.notify();
    }

    pub(super) fn copy_selected(&mut self, cx: &mut Context<Self>) {
        if self.state.copy_pending.is_some()
            || self.state.reveal.is_some()
            || self.state.saving.is_some()
            || self.state.device_verification.running()
        {
            return;
        }
        let Some(entry) = self.state.session.selected().cloned() else {
            return;
        };
        self.clear_message();
        let (sender, receiver) = mpsc::sync_channel(1);
        let cancellation = Cancellation::default();
        let progress = OperationProgress::new(OperationStatus::working(
            "Copying entry",
            "Follow the system prompt. If your YubiKey blinks, touch its gold contact.",
        ));
        if self.state.demo {
            let secret =
                Secret::from_bytes(Zeroizing::new(constants::DEMO_SECRET.as_bytes().to_vec()));
            let _ = sender.send(secret);
        } else if let (Ok(gpg), Some(store)) = (Gpg::new(&self.state.gpg), self.state.store.clone())
        {
            let worker_entry = entry.clone();
            let worker_progress = progress.clone();
            let worker_cancel = cancellation.clone();
            std::thread::spawn(move || {
                let _ = sender.send(gpg.decrypt_with_progress(
                    &store,
                    &worker_entry,
                    &worker_cancel,
                    &worker_progress,
                ));
            });
            self.state.copy_pending = Some(PendingCopy {
                entry,
                progress,
                cancellation,
                receiver,
            });
            cx.notify();
            return;
        } else {
            self.set_error("Open a password store before copying an entry.".into());
            return;
        }
        self.state.copy_pending = Some(PendingCopy {
            entry,
            progress,
            cancellation,
            receiver,
        });
        cx.notify();
    }

    fn apply_open_store_selection(&mut self, path: PathBuf, preview: bool, cx: &mut Context<Self>) {
        if self.state.saving.is_some() {
            return;
        }
        self.state.load = None;
        self.state.onboarding = None;
        self.state.onboarding_pending = None;
        self.clear_create_entry();
        self.state.onboarding_demo = false;
        self.state.demo = false;
        self.state.demo_empty_store = false;
        self.state.preview_store_load = preview;
        self.state.path = Some(path);
        self.refresh(cx);
    }

    pub(super) fn open_store(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.state.saving.is_some() {
            return;
        }
        let preview = self
            .state
            .onboarding
            .as_ref()
            .is_some_and(|onboarding| onboarding.preview);
        if !preview {
            self.hide(cx);
        }
        window.activate_window();
        let picker = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open password store".into()),
        });
        cx.spawn_in(window, async move |entity, cx| {
            let path = match picker.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) => None,
                Ok(Err(_)) | Err(_) => {
                    #[cfg(target_os = "linux")]
                    {
                        cx.background_spawn(async { pick_folder_with_zenity() })
                            .await
                    }
                    #[cfg(not(target_os = "linux"))]
                    {
                        None
                    }
                }
            };
            if let Some(path) = path {
                let _ = entity.update(cx, |app, cx| {
                    app.apply_open_store_selection(path, preview, cx);
                });
            } else {
                let _ = entity.update(cx, |app, cx| {
                    app.set_error("Unable to open the folder picker.".into());
                    cx.notify();
                });
            }
        })
        .detach();
    }
}

#[cfg(target_os = "linux")]
fn pick_folder_with_zenity() -> Option<PathBuf> {
    let output = Command::new("zenity")
        .args([
            "--file-selection",
            "--directory",
            "--title",
            "Open password store",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?;
    let path = path.trim();
    (!path.is_empty()).then(|| PathBuf::from(path))
}
