use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum GeneratorMode {
    New,
    Memorable,
    PassDefault,
}

pub(super) struct OverwriteConfirmation {
    entry: Entry,
}

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
        if self.state.demo || self.state.store.is_none() {
            self.set_message(i18n::text("custom-entry-store-required"));
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
        let words = 4 + (fraction * 4.).round() as usize;
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
                self.set_message(error.to_string());
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
            self.set_message(i18n::text("custom-entry-store-required"));
            cx.notify();
            return;
        }
        let name = self.state.custom_entry_name.trim();
        let name = name.strip_suffix(".gpg").unwrap_or(name);
        if name.is_empty() {
            self.set_message(i18n::text("custom-entry-name-required"));
            cx.notify();
            return;
        }
        let entry = match Entry::from_relative_file(format!("{name}.gpg")) {
            Ok(entry) => entry,
            Err(error) => {
                self.set_message(error.to_string());
                cx.notify();
                return;
            }
        };
        let Some(store) = self.state.store.clone() else {
            self.set_message(i18n::text("custom-entry-store-required"));
            cx.notify();
            return;
        };
        match store.entry_revision(&entry) {
            Ok(_) => {
                self.state.confirming_overwrite = Some(OverwriteConfirmation { entry });
                cx.notify();
            }
            Err(Error::EntryNotFound) => self.start_entry_save(entry, None, cx),
            Err(error) => {
                self.set_message(error.to_string());
                cx.notify();
            }
        }
    }

    pub(super) fn start_entry_save(
        &mut self,
        entry: Entry,
        revision: Option<EntryRevision>,
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
                        self.set_message(error.to_string());
                        cx.notify();
                        return;
                    }
                },
            )
        };
        let gpg = match Gpg::new(&self.state.gpg) {
            Ok(gpg) => gpg,
            Err(error) => {
                self.set_message(error.to_string());
                cx.notify();
                return;
            }
        };
        let Some(store) = self.state.store.clone() else {
            self.set_message(i18n::text("custom-entry-store-required"));
            cx.notify();
            return;
        };
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
            self.set_message(i18n::text("custom-entry-store-required"));
            cx.notify();
            return;
        };
        let revision = match store.entry_revision(&pending.entry) {
            Ok(revision) => revision,
            Err(error) => {
                self.set_message(error.to_string());
                cx.notify();
                return;
            }
        };
        self.start_entry_save(pending.entry, Some(revision), cx);
    }

    pub(super) fn overwrite_confirmation_view(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(pending) = &self.state.confirming_overwrite else {
            return div().into_any_element();
        };
        div()
            .id("overwrite-confirmation")
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
                self.state
                    .theme
                    .panel()
                    .w(px(520.))
                    .max_w(px(520.))
                    .p(px(32.))
                    .flex()
                    .flex_col()
                    .gap(px(20.))
                    .child(
                        div()
                            .text_size(px(18.))
                            .line_height(px(24.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(i18n::text("overwrite-entry-title")),
                    )
                    .child(
                        div()
                            .text_size(px(24.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(pending.entry.name().to_owned()),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .text_color(rgb(self.state.theme.colors.muted))
                            .child(i18n::text("overwrite-entry-help")),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(10.))
                            .child(
                                self.state
                                    .theme
                                    .button("cancel-overwrite", "Cancel", false)
                                    .on_click(cx.listener(|app, _, _, cx| {
                                        app.state.confirming_overwrite = None;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                self.state
                                    .theme
                                    .danger_button(
                                        "confirm-overwrite",
                                        i18n::text("overwrite-entry-confirm"),
                                    )
                                    .on_click(
                                        cx.listener(|app, _, _, cx| app.confirm_overwrite(cx)),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn generator_titlebar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        self.titlebar("generator-titlebar", None, cx)
    }

    pub(super) fn generator_view(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let options = self.generator_options();
        let slider_left = ((self.state.generation_words - 4) as f32 / 4.) * 360.;
        let entity = cx.entity().clone();
        let slider_track = div()
            .id("generator-strength-slider")
            .relative()
            .w(px(360.))
            .h(px(26.))
            .cursor_pointer()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|app, event: &gpui::MouseDownEvent, _, cx| {
                    app.set_generator_strength_from_x(event.position.x, cx);
                }),
            )
            .on_mouse_move(cx.listener(|app, event: &gpui::MouseMoveEvent, _, cx| {
                if event.dragging() {
                    app.set_generator_strength_from_x(event.position.x, cx);
                }
            }))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(11.))
                    .h(px(3.))
                    .bg(rgb(self.state.theme.colors.border)),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(11.))
                    .w(px(slider_left))
                    .h(px(3.))
                    .bg(rgb(self.state.theme.colors.accent)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(slider_left - 7.))
                    .top(px(5.))
                    .size(px(15.))
                    .border_1()
                    .border_color(rgb(self.state.theme.colors.accent))
                    .bg(rgb(self.state.theme.colors.surface)),
            )
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, (), _, cx| {
                        entity.update(cx, |app, _| {
                            app.state.generator_slider_bounds = Some(bounds)
                        });
                    },
                )
                .absolute()
                .size_full(),
            );
        let slider = div()
            .w(px(360.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(slider_track)
            .child(
                div()
                    .flex()
                    .justify_between()
                    .font_family(self.state.theme.mono_font)
                    .text_size(px(9.))
                    .text_color(rgb(self.state.theme.colors.muted))
                    .child("4 WORDS")
                    .child("8 WORDS"),
            );

        let mut panel = self
            .state
            .theme
            .panel()
            .w(px(640.))
            .max_w(px(640.))
            .p(px(36.))
            .flex()
            .flex_shrink_0()
            .flex_col()
            .gap(px(16.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        self.state
                            .theme
                            .panel_title(i18n::text("custom-entry-title")),
                    )
                    .child(
                        self.state
                            .theme
                            .button("close-generator", "← Back", false)
                            .on_click(cx.listener(|app, _, _, cx| app.close_generator(cx))),
                    ),
            )
            .child(self.state.theme.label(i18n::text("entry-name")))
            .child(self.custom_entry_name_field(window, cx))
            .child(self.state.theme.label(i18n::text("custom-entry-contents")))
            .when(
                self.state.generator_mode == GeneratorMode::PassDefault,
                |panel| {
                    panel.child(
                        div()
                            .w_full()
                            .min_h(px(180.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .px(px(24.))
                            .text_size(px(13.))
                            .line_height(px(21.))
                            .text_color(rgb(self.state.theme.colors.muted))
                            .child(i18n::text("pass-default-help")),
                    )
                },
            )
            .when(
                self.state.generator_mode != GeneratorMode::PassDefault,
                |panel| {
                    panel.child(
                        div()
                            .w_full()
                            .min_h(px(180.))
                            .child(self.generator_name_field(window, cx)),
                    )
                },
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        self.state
                            .theme
                            .button(
                                "mode-new",
                                i18n::text("new-entry"),
                                self.state.generator_mode == GeneratorMode::New,
                            )
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.select_generator_mode(GeneratorMode::New, cx);
                            })),
                    )
                    .child(
                        self.state
                            .theme
                            .button(
                                "mode-pass-default",
                                i18n::text("pass-default"),
                                self.state.generator_mode == GeneratorMode::PassDefault,
                            )
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.select_generator_mode(GeneratorMode::PassDefault, cx);
                            })),
                    )
                    .child(
                        self.state
                            .theme
                            .button(
                                "mode-memorable",
                                i18n::text("memorable-phrase"),
                                self.state.generator_mode == GeneratorMode::Memorable,
                            )
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.select_generator_mode(GeneratorMode::Memorable, cx);
                            })),
                    ),
            )
            .when(
                self.state.generator_mode == GeneratorMode::Memorable,
                |panel| {
                    panel
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(16.))
                                .child(slider)
                                .child(
                                    div()
                                        .min_w(px(126.))
                                        .font_family(self.state.theme.mono_font)
                                        .text_size(px(11.))
                                        .text_color(rgb(self.state.theme.colors.accent))
                                        .child(format!(
                                            "{} WORDS  ·  {:.0} BITS",
                                            options.words,
                                            options.entropy_bits()
                                        )),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(16.))
                                .child(
                                    div()
                                        .id("toggle-generator-digits")
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .text_size(px(12.))
                                        .text_color(rgb(self.state.theme.colors.muted))
                                        .cursor_pointer()
                                        .on_click(cx.listener(|app, _, _, cx| {
                                            app.state.generation_digits =
                                                !app.state.generation_digits;
                                            cx.notify();
                                        }))
                                        .child(
                                            div()
                                                .size(px(22.))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .border_2()
                                                .border_color(rgb(
                                                    if self.state.generation_digits {
                                                        self.state.theme.colors.accent
                                                    } else {
                                                        self.state.theme.colors.border
                                                    },
                                                ))
                                                .bg(rgb(if self.state.generation_digits {
                                                    self.state.theme.colors.accent_surface
                                                } else {
                                                    self.state.theme.colors.background
                                                }))
                                                .text_size(px(16.))
                                                .text_color(rgb(self.state.theme.colors.accent))
                                                .child(if self.state.generation_digits {
                                                    "✓"
                                                } else {
                                                    ""
                                                }),
                                        )
                                        .child(i18n::text("append-digit")),
                                )
                                .child(
                                    div()
                                        .id("toggle-generator-separators")
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .text_size(px(12.))
                                        .text_color(rgb(self.state.theme.colors.muted))
                                        .cursor_pointer()
                                        .on_click(cx.listener(|app, _, _, cx| {
                                            app.state.generation_separators =
                                                !app.state.generation_separators;
                                            cx.notify();
                                        }))
                                        .child(
                                            div()
                                                .size(px(22.))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .border_2()
                                                .border_color(rgb(
                                                    if self.state.generation_separators {
                                                        self.state.theme.colors.accent
                                                    } else {
                                                        self.state.theme.colors.border
                                                    },
                                                ))
                                                .bg(rgb(if self.state.generation_separators {
                                                    self.state.theme.colors.accent_surface
                                                } else {
                                                    self.state.theme.colors.background
                                                }))
                                                .text_size(px(16.))
                                                .text_color(rgb(self.state.theme.colors.accent))
                                                .child(if self.state.generation_separators {
                                                    "✓"
                                                } else {
                                                    ""
                                                }),
                                        )
                                        .child(i18n::text("separate-words")),
                                ),
                        )
                },
            );

        let mut footer = div().flex().items_center().justify_end().gap(px(8.));
        if self.state.generator_mode == GeneratorMode::Memorable {
            footer = footer.child(
                self.state
                    .theme
                    .button("generate-into-contents", i18n::text("generate"), false)
                    .on_click(cx.listener(|app, _, _, cx| app.generate_into_contents(cx))),
            );
        }
        let save_label = if self.state.saving.is_some() {
            if self.state.generator_mode == GeneratorMode::PassDefault {
                i18n::text("custom-entry-generating")
            } else {
                i18n::text("custom-entry-saving")
            }
        } else if self.state.generator_mode == GeneratorMode::PassDefault {
            i18n::text("custom-entry-generate-save")
        } else {
            i18n::text("custom-entry-save")
        };
        footer = footer.child(
            self.state
                .theme
                .button("save-entry", save_label, true)
                .on_click(cx.listener(|app, _, _, cx| app.save_entry(cx))),
        );
        panel = panel.child(footer);

        div()
            .id("generator-window")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(rgb(self.state.theme.colors.background))
            .child(self.generator_titlebar(cx))
            .child(
                div()
                    .id("generator-content-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .items_start()
                    .justify_center()
                    .px(px(32.))
                    .pt(px(48.))
                    .pb(px(48.))
                    .child(panel),
            )
            .into_any_element()
    }
}
