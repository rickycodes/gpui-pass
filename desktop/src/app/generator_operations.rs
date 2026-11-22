use super::*;

impl PassApp {
    pub(super) fn reset_generator_cursor(&mut self) {
        self.state.generator_name_selection =
            self.state.generator_name.len()..self.state.generator_name.len();
        self.state.generator_name_anchor = None;
        self.state.generator_name_cursor = self.state.generator_name.len();
        self.state.generator_name_marked = None;
        self.state.generator_name_bounds = None;
        self.state.editor_layout = None;
    }

    pub(super) fn clear_create_entry(&mut self) {
        self.state.custom_entry_name.zeroize();
        self.state.custom_entry_name_selection = 0..0;
        self.state.custom_entry_name_cursor = 0;
        self.state.custom_entry_name_anchor = None;
        self.state.custom_entry_name_marked = None;
        self.state.custom_entry_name_bounds = None;
        self.state.generator_name.zeroize();
        self.state.content_generated = false;
        self.state.generator_mode = GeneratorMode::New;
        self.reset_generator_cursor();
    }

    pub(super) fn open_create_entry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.state.saving.is_some() {
            return;
        }
        if self.state.demo || self.state.store.is_none() || self.state.onboarding.is_some() {
            self.set_error(i18n::text("custom-entry-store-required"));
            cx.notify();
            return;
        }
        self.cancel_edit(cx);
        self.state.device_verification.cancel();
        self.state.copy_pending = None;
        self.state.session.hide();
        self.state.reveal = None;
        self.clear_create_entry();
        self.state.custom_entry_name_focus.focus(window);
        self.state.navigation.open_generator();
        self.clear_message();
        cx.notify();
    }

    pub(super) fn generator_options(&self) -> GeneratorOptions {
        GeneratorOptions {
            words: self.state.generation_words,
            add_digits: self.state.generation_digits,
            add_separators: self.state.generation_separators,
        }
    }
    pub(super) fn select_generator_mode(&mut self, mode: GeneratorMode, cx: &mut Context<Self>) {
        self.state.generator_mode = mode;
        self.state.content_generated = false;
        cx.notify();
    }

    pub(super) fn set_generator_strength_from_x(
        &mut self,
        x: gpui::Pixels,
        cx: &mut Context<Self>,
    ) {
        let Some(bounds) = self.state.generator_slider_bounds else {
            return;
        };
        let fraction = ((x - bounds.origin.x) / bounds.size.width).clamp(0., 1.);
        let words = MIN_MEMORABLE_WORDS
            + (fraction * (MAX_MEMORABLE_WORDS - MIN_MEMORABLE_WORDS) as f32).round() as usize;
        if words != self.state.generation_words {
            self.state.generation_words = words;
            cx.notify();
        }
    }

    pub(super) fn generate_into_contents(&mut self, cx: &mut Context<Self>) {
        let result = match self.state.generator_mode {
            GeneratorMode::New => return,
            GeneratorMode::Memorable => generate_memorable_password(self.generator_options()),
            GeneratorMode::PassDefault => return,
        };
        match result {
            Ok(secret) => {
                self.state.generator_name = Zeroizing::new(secret.expose().to_owned());
                self.state.content_generated = true;
                self.reset_generator_cursor();
                cx.notify();
            }
            Err(error) => {
                self.set_error(error.to_string());
                cx.notify();
            }
        }
    }

    pub(super) fn close_generator(&mut self, cx: &mut Context<Self>) {
        if let Some((cancellation, _, _)) = &self.state.saving {
            cancellation.cancel();
            self.state.close_after_save = true;
            self.set_message("Cancelling entry creation…".into());
            cx.notify();
            return;
        }
        self.state.navigation.close_generator();
        self.clear_create_entry();
        cx.notify();
    }

    pub(super) fn save_entry(&mut self, cx: &mut Context<Self>) {
        if self.state.saving.is_some() {
            return;
        }
        if self.state.demo {
            self.set_error(i18n::text("custom-entry-store-required"));
            cx.notify();
            return;
        }
        let name = self.state.custom_entry_name.trim();
        let name = name.strip_suffix(ENTRY_SUFFIX).unwrap_or(name);
        if name.is_empty() {
            self.set_error(i18n::text("custom-entry-name-required"));
            cx.notify();
            return;
        }
        let entry = match Entry::from_relative_file(format!("{name}{ENTRY_SUFFIX}")) {
            Ok(entry) => entry,
            Err(error) => {
                self.set_error(error.to_string());
                cx.notify();
                return;
            }
        };
        let Some(store) = self.state.store.clone() else {
            self.set_error(i18n::text("custom-entry-store-required"));
            cx.notify();
            return;
        };
        match store.entry_revision(&entry) {
            Ok(_) => {
                self.state.confirming_overwrite = Some(OverwriteConfirmation { entry });
                cx.notify();
            }
            Err(Error::EntryNotFound) => self.start_entry_save(entry, None, false, cx),
            Err(error) => {
                self.set_error(error.to_string());
                cx.notify();
            }
        }
    }

    pub(super) fn start_entry_save(
        &mut self,
        entry: Entry,
        revision: Option<EntryRevision>,
        clear_form: bool,
        cx: &mut Context<Self>,
    ) {
        if self.state.saving.is_some() {
            return;
        }
        let pass_default = self.state.generator_mode == GeneratorMode::PassDefault;
        let secret = if pass_default {
            None
        } else {
            Some(
                match Secret::from_bytes(Zeroizing::new(
                    self.state.generator_name.as_bytes().to_vec(),
                )) {
                    Ok(secret) => secret,
                    Err(error) => {
                        self.set_error(error.to_string());
                        cx.notify();
                        return;
                    }
                },
            )
        };
        let gpg = match Gpg::new(&self.state.gpg) {
            Ok(gpg) => gpg,
            Err(error) => {
                self.set_error(error.to_string());
                cx.notify();
                return;
            }
        };
        let Some(store) = self.state.store.clone() else {
            self.set_error(i18n::text("custom-entry-store-required"));
            cx.notify();
            return;
        };
        if clear_form {
            self.clear_create_entry();
        }
        let cancellation = Cancellation::default();
        let progress = OperationProgress::new(OperationStatus::working(
            if pass_default {
                "Generating and encrypting entry"
            } else {
                "Encrypting new entry"
            },
            if pass_default {
                "pass will generate the password before GnuPG encrypts it."
            } else {
                "Your text stays in memory until GnuPG encrypts it."
            },
        ));
        let worker_cancel = cancellation.clone();
        let worker_progress = progress.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let result = (|| {
                if pass_default && revision.is_none() {
                    let secret = gpg.generate_pass_default_entry(
                        &store,
                        &entry,
                        &worker_cancel,
                        &worker_progress,
                    )?;
                    return Ok((entry, secret));
                }
                let secret = match secret {
                    Some(secret) => secret,
                    None => generate_pass_default_password()?,
                };
                let result = if let Some(revision) = revision {
                    if pass_default {
                        gpg.update_generated_entry_if_unchanged(
                            &store,
                            &entry,
                            &secret,
                            &revision,
                            &worker_cancel,
                            &worker_progress,
                        )
                    } else {
                        gpg.update_entry_if_unchanged(
                            &store,
                            &entry,
                            &secret,
                            &revision,
                            &worker_cancel,
                            &worker_progress,
                        )
                    }
                } else {
                    gpg.create_entry(&store, &entry, &secret, &worker_cancel, &worker_progress)
                };
                result.map(|()| (entry, secret))
            })();
            let _ = sender.send(result);
        });
        self.clear_message();
        self.state.content_generated = pass_default;
        self.state.saving_edit = false;
        self.state.saving_new_entry = true;
        self.state.saving = Some((cancellation, progress, receiver));
        cx.notify();
    }

    pub(super) fn confirm_overwrite(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self.state.confirming_overwrite.take() else {
            return;
        };
        let Some(store) = self.state.store.as_ref() else {
            self.set_error(i18n::text("custom-entry-store-required"));
            cx.notify();
            return;
        };
        let revision = match store.entry_revision(&pending.entry) {
            Ok(revision) => revision,
            Err(error) => {
                self.set_error(error.to_string());
                cx.notify();
                return;
            }
        };
        self.start_entry_save(pending.entry, Some(revision), true, cx);
    }
}
