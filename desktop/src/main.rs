#![forbid(unsafe_code)]

mod operation_status;
mod search;
mod theme;

use gpui::{
    App, Application, Bounds, ClipboardItem, Context, CursorStyle, FocusHandle, FontWeight,
    KeyBinding, ResizeEdge, ScrollStrategy, SharedString, Subscription, TitlebarOptions,
    UniformListScrollHandle, Window, WindowBounds, WindowOptions, actions, canvas, div, prelude::*,
    px, rgb, size, uniform_list,
};
use gpui_pass_core::{
    Cancellation, Entry, Error, GeneratorOptions, Gpg, OperationProgress, OperationStatus,
    RevealRequest, Secret, Session, Store, default_store_path, harden_process,
};
use std::{
    ops::Range,
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};
use theme::*;
use zeroize::Zeroizing;

actions!(
    pass,
    [
        Hide,
        OpenGenerator,
        Reveal,
        Refresh,
        FocusSearch,
        NextEntry,
        PreviousEntry,
        Backspace,
        Delete,
        Left,
        Right,
        Start,
        End,
        SelectAll,
        SwitchFocus,
        ToggleFullscreen,
        Quit
    ]
);

struct Options {
    store: Option<PathBuf>,
    gpg: PathBuf,
    demo: bool,
}

impl Options {
    fn parse() -> Result<Option<Self>, String> {
        let mut options = Self {
            store: None,
            gpg: PathBuf::from("/usr/bin/gpg"),
            demo: false,
        };
        let mut args = std::env::args_os().skip(1);
        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("--demo") => options.demo = true,
                Some("--store") => {
                    options.store = Some(args.next().ok_or("--store requires a directory")?.into())
                }
                Some("--gpg") => {
                    options.gpg = args
                        .next()
                        .ok_or("--gpg requires an absolute executable path")?
                        .into()
                }
                Some("--help" | "-h") => {
                    println!(
                        "gpui-pass [--store DIRECTORY] [--gpg /absolute/path/to/gpg] [--demo]\n\nBrowse and create encrypted entries in a pass store. Defaults to PASSWORD_STORE_DIR or ~/.password-store.\n--demo uses fictional entries in memory and never opens a password store."
                    );
                    return Ok(None);
                }
                _ => return Err("Unknown argument. Use --help for usage.".into()),
            }
        }
        if options.demo && options.store.is_some() {
            return Err("Use --demo or --store, separately.".into());
        }
        Ok(Some(options))
    }
}

type LoadedStore = Result<(Store, Vec<Entry>), Error>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum GeneratorMode {
    Memorable,
    PassDefault,
}

pub(crate) struct PassApp {
    session: Session,
    entries: Vec<Entry>,
    filtered: Vec<usize>,
    list_scroll: UniformListScrollHandle,
    query: String,
    selection: Range<usize>,
    marked: Option<Range<usize>>,
    input_bounds: Option<Bounds<gpui::Pixels>>,
    focus: FocusHandle,
    search_focus: FocusHandle,
    generator_name_focus: FocusHandle,
    generator_name: String,
    generator_name_selection: Range<usize>,
    generator_name_marked: Option<Range<usize>>,
    generator_name_bounds: Option<Bounds<gpui::Pixels>>,
    active: bool,
    store: Option<Store>,
    path: Option<PathBuf>,
    gpg: PathBuf,
    demo: bool,
    load: Option<Receiver<LoadedStore>>,
    reveal: Option<(RevealRequest, Receiver<Result<Secret, Error>>)>,
    generated: Option<(Secret, Instant)>,
    generated_visible: bool,
    copied_password: Option<Zeroizing<String>>,
    generation_words: usize,
    generator_mode: GeneratorMode,
    generation_digits: bool,
    generation_separators: bool,
    generator_slider_bounds: Option<Bounds<gpui::Pixels>>,
    generator_open: bool,
    confirming_delete: Option<Entry>,
    saving: Option<(
        Cancellation,
        OperationProgress,
        Receiver<Result<(Entry, Secret), Error>>,
    )>,
    saved_entry_name: Option<String>,
    close_after_save: bool,
    message: Option<String>,
    message_until: Option<Instant>,
    seconds: u64,
    last_operation_status: Option<OperationStatus>,
    _activation: Subscription,
}

impl PassApp {
    fn new(options: Options, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let search_focus = cx.focus_handle();
        let generator_name_focus = cx.focus_handle();
        let subscription = cx.observe_window_activation(window, |app, window, cx| {
            app.active = window.is_window_active();
            if !app.active {
                app.session.deactivate();
                app.clear_generated(cx);
            }
            cx.notify();
        });
        search_focus.focus(window);
        let mut app = Self {
            session: Session::default(),
            entries: Vec::new(),
            filtered: Vec::new(),
            list_scroll: UniformListScrollHandle::new(),
            query: String::new(),
            selection: 0..0,
            marked: None,
            input_bounds: None,
            focus,
            search_focus,
            generator_name_focus,
            generator_name: String::new(),
            generator_name_selection: 0..0,
            generator_name_marked: None,
            generator_name_bounds: None,
            active: window.is_window_active(),
            store: None,
            path: options.store.or_else(|| default_store_path().ok()),
            gpg: options.gpg,
            demo: options.demo,
            load: None,
            reveal: None,
            generated: None,
            generated_visible: false,
            copied_password: None,
            generation_words: 6,
            generator_mode: GeneratorMode::PassDefault,
            generation_digits: true,
            generation_separators: true,
            generator_slider_bounds: None,
            generator_open: false,
            confirming_delete: None,
            saving: None,
            saved_entry_name: None,
            close_after_save: false,
            message: None,
            message_until: None,
            seconds: 0,
            last_operation_status: None,
            _activation: subscription,
        };
        app.refresh(cx);
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

    fn tick_interval(&self) -> Duration {
        if self.load.is_some() || self.reveal.is_some() || self.saving.is_some() {
            Duration::from_millis(100)
        } else if self.session.revealed(Instant::now()).is_some()
            || self.generated.is_some()
            || self.message_until.is_some()
        {
            Duration::from_millis(250)
        } else {
            Duration::from_secs(2)
        }
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.load.is_some() {
            return;
        }
        self.session.select(None);
        self.reveal = None;
        self.clear_generated(cx);
        self.clear_message();
        self.store = None;
        self.entries.clear();
        self.filtered.clear();
        if self.demo {
            self.entries = [
                "Development/github.com.gpg",
                "Development/linear.app.gpg",
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
        } else if let Some(path) = self.path.clone() {
            let (sender, receiver) = mpsc::sync_channel(1);
            self.load = Some(receiver);
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

    fn tick(&mut self, cx: &mut Context<Self>) {
        if let Some(result) = self
            .load
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
        {
            self.load = None;
            match result {
                Ok((store, entries)) => {
                    self.store = Some(store);
                    self.entries = entries;
                    self.filter(cx);
                }
                Err(error) => self.set_message(error.to_string()),
            }
            cx.notify();
        }
        if let Some(result) = self
            .reveal
            .as_ref()
            .and_then(|(_, receiver)| receiver.try_recv().ok())
        {
            let (request, _) = self.reveal.take().expect("request available");
            match result {
                Ok(secret) => {
                    if !self
                        .session
                        .finish_reveal(&request, secret, self.active, Instant::now())
                    {
                        self.set_message(
                            "Entry stayed hidden. Return to this window and reveal again.".into(),
                        );
                    }
                }
                Err(error) => {
                    if self.session.fail_reveal(&request) {
                        self.set_message(error.to_string());
                    }
                }
            }
            cx.notify();
        }
        if self
            .message_until
            .is_some_and(|until| Instant::now() >= until)
        {
            self.message = None;
            self.message_until = None;
            cx.notify();
        }
        if self
            .generated
            .as_ref()
            .is_some_and(|(_, deadline)| Instant::now() >= *deadline)
        {
            self.clear_generated(cx);
            cx.notify();
        }
        if self.session.expire(Instant::now()) {
            cx.notify();
        }
        let seconds = self.session.remaining_seconds(Instant::now());
        if self.seconds != seconds {
            self.seconds = seconds;
            cx.notify();
        }
        if let Some(result) = self
            .saving
            .as_ref()
            .and_then(|(_, _, receiver)| receiver.try_recv().ok())
        {
            self.saving = None;
            match result {
                Ok((entry, secret)) => {
                    let name = entry.name().to_owned();
                    self.entries.push(entry);
                    self.entries.sort_by(|a, b| {
                        a.name()
                            .to_lowercase()
                            .cmp(&b.name().to_lowercase())
                            .then_with(|| a.name().cmp(b.name()))
                    });
                    self.query.clear();
                    self.selection = 0..0;
                    self.filter(cx);
                    self.session.select(
                        self.entries
                            .iter()
                            .find(|entry| entry.name() == name)
                            .cloned(),
                    );
                    self.saved_entry_name = Some(name);
                    if !self.close_after_save {
                        let text = secret.expose().to_owned();
                        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                        self.copied_password = Some(Zeroizing::new(text));
                        self.generated = Some((secret, Instant::now() + Duration::from_secs(30)));
                    }
                    self.clear_message();
                }
                Err(error) => self.set_message(error.to_string()),
            }
            let close = self.close_after_save;
            self.close_after_save = false;
            if close {
                self.close_generator(cx);
            }
            cx.notify();
        }
        let status = self.session.pending_status().or_else(|| {
            self.saving
                .as_ref()
                .map(|(_, progress, _)| progress.snapshot())
        });
        if self.last_operation_status != status {
            self.last_operation_status = status;
            cx.notify();
        }
    }

    fn set_message(&mut self, message: String) {
        self.message = Some(message);
        self.message_until = Some(Instant::now() + Duration::from_secs(6));
    }

    fn clear_message(&mut self) {
        self.message = None;
        self.message_until = None;
    }

    fn filter(&mut self, cx: &mut Context<Self>) {
        self.session.hide();
        self.reveal = None;
        self.clear_generated(cx);
        self.clear_message();
        self.filtered = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, entry)| entry.matches(&self.query).then_some(i))
            .collect();
        let first = self
            .filtered
            .first()
            .map(|index| self.entries[*index].clone());
        self.session.select(first);
        self.list_scroll.scroll_to_item(0, ScrollStrategy::Top);
    }

    fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        self.session.select(self.entries.get(index).cloned());
        self.reveal = None;
        self.clear_generated(cx);
        self.clear_message();
        cx.notify();
    }

    fn navigate(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.filtered.is_empty() {
            return;
        }
        let position = self
            .filtered
            .iter()
            .position(|index| self.session.selected() == self.entries.get(*index))
            .unwrap_or(0);
        let next = (position as isize + delta).clamp(0, self.filtered.len() as isize - 1) as usize;
        self.choose(self.filtered[next], cx);
        self.list_scroll
            .scroll_to_item(next, ScrollStrategy::Center);
    }

    fn hide(&mut self, cx: &mut Context<Self>) {
        if self.confirming_delete.take().is_some() {
            cx.notify();
            return;
        }
        if self.generator_open {
            self.close_generator(cx);
            return;
        }
        self.session.hide();
        self.reveal = None;
        self.clear_generated(cx);
        self.clear_message();
        cx.notify();
    }

    fn request_delete(&mut self, cx: &mut Context<Self>) {
        if let Some(entry) = self.session.selected().cloned() {
            self.session.hide();
            self.reveal = None;
            self.clear_generated(cx);
            self.confirming_delete = Some(entry);
            cx.notify();
        }
    }

    fn confirm_delete(&mut self, cx: &mut Context<Self>) {
        let Some(entry) = self.confirming_delete.take() else {
            return;
        };
        let result = self
            .store
            .as_ref()
            .ok_or(Error::InvalidEntry)
            .and_then(|store| store.remove_entry(&entry));
        match result {
            Ok(()) => {
                self.session.hide();
                self.reveal = None;
                self.clear_generated(cx);
                self.entries.retain(|existing| existing != &entry);
                self.filter(cx);
            }
            Err(error) => self.set_message(error.to_string()),
        }
        cx.notify();
    }

    fn delete_confirmation_view(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(entry) = &self.confirming_delete else {
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
            .bg(rgb(PAPER))
            .child(
                div()
                    .w(px(520.))
                    .max_w(px(520.))
                    .p(px(32.))
                    .border_1()
                    .border_color(rgb(LINE))
                    .bg(rgb(WHITE))
                    .flex()
                    .flex_col()
                    .gap(px(20.))
                    .child(label("REMOVE PASSWORD ENTRY"))
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
                            .text_color(rgb(MUTED))
                            .child("This removes the encrypted entry from the current store. It does not erase copies in Git history or backups."),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(10.))
                            .child(button("cancel-delete", "Cancel", false).on_click(
                                cx.listener(|app, _, _, cx| {
                                    app.confirming_delete = None;
                                    cx.notify();
                                }),
                            ))
                            .child(
                                div()
                                    .id("confirm-delete")
                                    .h(px(40.))
                                    .px(px(18.))
                                    .border_1()
                                    .border_color(rgb(ERROR))
                                    .bg(rgb(ERROR))
                                    .text_color(rgb(WHITE))
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(0x80352d)))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .on_click(cx.listener(|app, _, _, cx| app.confirm_delete(cx)))
                                    .child("Delete entry"),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn generator_options(&self) -> GeneratorOptions {
        GeneratorOptions {
            words: self.generation_words,
            add_digits: self.generation_digits,
            add_separators: self.generation_separators,
        }
    }

    fn set_generator_strength_from_x(&mut self, x: gpui::Pixels, cx: &mut Context<Self>) {
        let Some(bounds) = self.generator_slider_bounds else {
            return;
        };
        let fraction = ((x - bounds.origin.x) / bounds.size.width).clamp(0., 1.);
        let words = 4 + (fraction * 4.).round() as usize;
        if words != self.generation_words {
            self.generation_words = words;
            cx.notify();
        }
    }

    fn open_generator(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.session.hide();
        self.reveal = None;
        self.clear_generated(cx);
        self.generator_name_focus.focus(window);
        self.generator_name_selection = self.generator_name.len()..self.generator_name.len();
        self.generator_open = true;
        cx.notify();
    }

    fn close_generator(&mut self, cx: &mut Context<Self>) {
        if let Some((cancellation, _, _)) = &self.saving {
            cancellation.cancel();
            self.close_after_save = true;
            self.set_message("Cancelling entry creation…".into());
            cx.notify();
            return;
        }
        self.generator_open = false;
        self.clear_generated(cx);
        cx.notify();
    }

    fn save_generated(&mut self, cx: &mut Context<Self>) {
        if self.saving.is_some() {
            return;
        }
        if self.demo {
            self.set_message("Saving is disabled in fictional demo mode.".into());
            cx.notify();
            return;
        }
        let name = self
            .generator_name
            .trim()
            .strip_suffix(".gpg")
            .unwrap_or(self.generator_name.trim());
        if name.is_empty() {
            self.set_message("Enter a name for this password entry.".into());
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
        let gpg = match Gpg::new(&self.gpg) {
            Ok(gpg) => gpg,
            Err(error) => {
                self.set_message(error.to_string());
                cx.notify();
                return;
            }
        };
        let Some(store) = self.store.clone() else {
            self.set_message("Open a password store before saving a generated entry.".into());
            cx.notify();
            return;
        };
        let mode = self.generator_mode;
        let options = self.generator_options();
        let cancellation = Cancellation::default();
        let progress = OperationProgress::new(OperationStatus::working(
            "Preparing new entry",
            "The generated password stays in memory until GnuPG encrypts it.",
        ));
        let worker_cancel = cancellation.clone();
        let worker_progress = progress.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let result = match mode {
                GeneratorMode::Memorable => {
                    gpg.generate_entry(&store, &entry, options, &worker_cancel, &worker_progress)
                }
                GeneratorMode::PassDefault => gpg.generate_pass_default_entry(
                    &store,
                    &entry,
                    &worker_cancel,
                    &worker_progress,
                ),
            }
            .map(|secret| (entry, secret));
            let _ = sender.send(result);
        });
        self.clear_generated(cx);
        self.clear_message();
        self.saved_entry_name = None;
        self.saving = Some((cancellation, progress, receiver));
        cx.notify();
    }

    fn generator_view(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let options = self.generator_options();
        let slider_left = ((self.generation_words - 4) as f32 / 4.) * 360.;
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
                    .bg(rgb(LINE)),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(11.))
                    .w(px(slider_left))
                    .h(px(3.))
                    .bg(rgb(GREEN)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(slider_left - 7.))
                    .top(px(5.))
                    .size(px(15.))
                    .border_1()
                    .border_color(rgb(GREEN))
                    .bg(rgb(WHITE)),
            )
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, (), _, cx| {
                        entity.update(cx, |app, _| app.generator_slider_bounds = Some(bounds));
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
                    .font_family(MONO)
                    .text_size(px(9.))
                    .text_color(rgb(MUTED))
                    .child("4 WORDS")
                    .child("8 WORDS"),
            );

        let mut panel = div()
            .w(px(640.))
            .max_w(px(640.))
            .p(px(36.))
            .border_1()
            .border_color(rgb(LINE))
            .bg(rgb(WHITE))
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(label("ENTRY NAME"))
            .child(self.generator_name_field(window, cx))
            .when(self.saving.is_some(), |panel| {
                let status = self.saving.as_ref().map(|(_, progress, _)| progress.snapshot()).unwrap();
                panel.child(operation_status::panel(status))
            })
            .child(
                div().flex().items_center().gap(px(8.))
                    .child(button("mode-pass-default", "Pass default", self.generator_mode == GeneratorMode::PassDefault)
                        .on_click(cx.listener(|app, _, _, cx| { app.generator_mode = GeneratorMode::PassDefault; cx.notify(); })))
                    .child(button("mode-memorable", "Memorable phrase", self.generator_mode == GeneratorMode::Memorable)
                        .on_click(cx.listener(|app, _, _, cx| { app.generator_mode = GeneratorMode::Memorable; cx.notify(); })))
            )
            .when(self.generator_mode == GeneratorMode::Memorable, |panel| panel
            .child(
                div()
                    .flex()
                    .items_start()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(label("PASSWORD GENERATOR"))
                            .child(
                                div()
                                    .text_size(px(28.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("A phrase worth remembering."),
                            ),
                    )
            )
            .child(
                div()
                    .max_w(px(500.))
                    .text_size(px(13.))
                    .line_height(px(21.))
                    .text_color(rgb(MUTED))
                    .child("Choose the number of random whole words. More words make a stronger phrase; digits and punctuation add variety."),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(24.))
                    .child(slider)
                    .child(
                        div()
                            .min_w(px(126.))
                            .font_family(MONO)
                            .text_size(px(11.))
                            .text_color(rgb(GREEN))
                            .child(format!("{} WORDS  ·  {:.0} BITS", options.words, options.entropy_bits())),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(24.))
                    .child(
                        div()
                            .id("toggle-generator-digits")
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(12.))
                            .text_color(rgb(MUTED))
                            .cursor_pointer()
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.generation_digits = !app.generation_digits;
                                cx.notify();
                            }))
                            .child(if self.generation_digits { "☑" } else { "☐" })
                            .child("Append a digit to each word"),
                    )
                    .child(
                        div()
                            .id("toggle-generator-separators")
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(12.))
                            .text_color(rgb(MUTED))
                            .cursor_pointer()
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.generation_separators = !app.generation_separators;
                                cx.notify();
                            }))
                            .child(if self.generation_separators { "☑" } else { "☐" })
                            .child("Separate words with punctuation"),
                    ),
            ));

        panel = panel.when(self.generator_mode == GeneratorMode::PassDefault, |panel| {
            panel.child(
                div()
                    .max_w(px(500.))
                    .text_size(px(13.))
                    .line_height(px(21.))
                    .text_color(rgb(MUTED))
                    .child("Random characters using pass’s default punctuation and letters. Respects PASSWORD_STORE_GENERATED_LENGTH and PASSWORD_STORE_CHARACTER_SET when set."),
            )
        });

        if let Some((secret, deadline)) = &self.generated {
            let remaining = deadline
                .saturating_duration_since(Instant::now())
                .as_secs()
                .saturating_add(1);
            panel = panel.child(
                div()
                    .border_1()
                    .border_color(rgb(LINE))
                    .bg(rgb(TINT))
                    .p(px(18.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(16.))
                    .child(
                        div()
                            .id("generated-password-text")
                            .min_w_0()
                            .overflow_x_scroll()
                            .whitespace_nowrap()
                            .font_family(MONO)
                            .text_size(px(16.))
                            .child(if self.generated_visible {
                                secret.expose().to_owned()
                            } else {
                                "*".repeat(secret.expose().chars().count())
                            }),
                    )
                    .child(
                        button(
                            "toggle-generated-visibility",
                            if self.generated_visible {
                                "Hide"
                            } else {
                                "Show"
                            },
                            false,
                        )
                        .on_click(cx.listener(|app, _, _, cx| {
                            app.generated_visible = !app.generated_visible;
                            cx.notify();
                        })),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .font_family(MONO)
                            .text_size(px(9.))
                            .text_color(rgb(MUTED))
                            .child(format!("HIDES IN {remaining:02}s")),
                    ),
            );
            panel = panel.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        button(
                            "copy-generated",
                            if self.generator_mode == GeneratorMode::Memorable {
                                "Copy phrase"
                            } else {
                                "Copy password"
                            },
                            true,
                        )
                        .on_click(cx.listener(|app, _, _, cx| {
                            if let Some((secret, _)) = &app.generated {
                                let text = secret.expose().to_owned();
                                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                                app.copied_password = Some(Zeroizing::new(text));
                                cx.notify();
                            }
                        })),
                    )
                    .child(div().text_size(px(11.)).text_color(rgb(MUTED)).child(
                        if self.saved_entry_name.is_some() {
                            "Saved · copied to clipboard; clears in 30 seconds."
                        } else {
                            "Available for 30 seconds."
                        },
                    )),
            );
        } else {
            panel = panel.child(
                div()
                    .h(px(76.))
                    .border_1()
                    .border_color(rgb(LINE))
                    .bg(rgb(PAPER))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(12.))
                    .text_color(rgb(MUTED))
                    .child(if self.saving.is_some() {
                        "Encrypting and saving entry…"
                    } else {
                        "Choose a name, then generate and save."
                    }),
            );
        }
        panel = panel.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .max_w(px(380.))
                        .text_size(px(11.))
                        .line_height(px(17.))
                        .text_color(rgb(MUTED))
                        .child("Creates a new encrypted entry. Existing entries are never overwritten."),
                )
                .child(
                    button("generate-phrase", if self.saving.is_some() { "Saving…" } else { "Generate & Save" }, true)
                        .on_click(cx.listener(|app, _, _, cx| app.save_generated(cx))),
                ),
        );

        div()
            .id("generator-window")
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bottom_0()
            .flex()
            .flex_col()
            .bg(rgb(PAPER))
            .child(
                div()
                    .id("generator-titlebar")
                    .w_full()
                    .h(px(48.))
                    .flex_shrink_0()
                    .px(px(26.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .bg(rgb(INK))
                    .on_mouse_down(gpui::MouseButton::Left, |event, window, cx| {
                        if event.click_count == 2 {
                            window.toggle_fullscreen();
                        } else {
                            window.start_window_move();
                        }
                        cx.stop_propagation();
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(14.))
                            .child(
                                div()
                                    .w(px(32.))
                                    .h(px(32.))
                                    .border_1()
                                    .border_color(rgb(0x809287))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .font_family(MONO)
                                    .text_color(rgb(0xe0edcb))
                                    .child("/"),
                            )
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(WHITE))
                                    .child("pass"),
                            )
                            .child(
                                div()
                                    .ml(px(18.))
                                    .text_size(px(11.))
                                    .text_color(rgb(0xc7d6be))
                                    .child("PASSWORD GENERATOR"),
                            ),
                    )
                    .child(
                        div()
                            .id("close-generator")
                            .size(px(34.))
                            .border_1()
                            .border_color(rgb(0x809287))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(22.))
                            .text_color(rgb(WHITE))
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(0x39433e)))
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(|app, _, _, cx| {
                                    cx.stop_propagation();
                                    app.close_generator(cx);
                                }),
                            )
                            .child("×"),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_start()
                    .justify_center()
                    .px(px(32.))
                    .pt(px(48.))
                    .child(panel),
            )
            .into_any_element()
    }

    fn clear_generated(&mut self, cx: &mut Context<Self>) {
        self.generated = None;
        self.generated_visible = false;
        let Some(expected) = self.copied_password.take() else {
            return;
        };
        let current = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .map(Zeroizing::new);
        if current
            .as_deref()
            .is_some_and(|text| text == expected.as_str())
        {
            cx.write_to_clipboard(ClipboardItem::new_string(String::new()));
        }
    }

    fn reveal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window);
        if self.session.is_pending() || !window.is_window_active() {
            return;
        }
        if self.session.revealed(Instant::now()).is_some() {
            self.hide(cx);
            return;
        }
        self.clear_generated(cx);
        let Some(request) = self.session.begin_reveal() else {
            return;
        };
        self.clear_message();
        let result = if self.demo {
            let secret = Secret::from_bytes(Zeroizing::new(b"demo-only-not-a-real-password\nusername: alex@example.test\n\nA fictional entry for exploring the interface.\n".to_vec()));
            let (sender, receiver) = mpsc::sync_channel(1);
            let _ = sender.send(secret);
            receiver
        } else {
            let gpg = match Gpg::new(&self.gpg) {
                Ok(gpg) => gpg,
                Err(error) => {
                    self.session.fail_reveal(&request);
                    self.set_message(error.to_string());
                    cx.notify();
                    return;
                }
            };
            let Some(store) = self.store.clone() else {
                self.session.fail_reveal(&request);
                return;
            };
            let entry = request.entry.clone();
            let cancel = request.cancellation.clone();
            let progress = request.progress.clone();
            let (sender, receiver) = mpsc::sync_channel(1);
            std::thread::spawn(move || {
                let _ = sender.send(gpg.decrypt_with_progress(&store, &entry, &cancel, &progress));
            });
            receiver
        };
        self.reveal = Some((request, result));
        cx.notify();
    }

    fn open_store(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.hide(cx);
        let picker = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open password store".into()),
        });
        cx.spawn_in(window, async move |entity, cx| {
            if let Ok(Ok(Some(paths))) = picker.await {
                if let Some(path) = paths.into_iter().next() {
                    let _ = entity.update(cx, |app, cx| {
                        app.load = None;
                        app.demo = false;
                        app.path = Some(path);
                        app.refresh(cx);
                    });
                }
            }
        })
        .detach();
    }

    fn entry_row(&self, index: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let entry = &self.entries[index];
        let active = self.session.selected() == Some(entry);
        div()
            .id(("entry", index))
            .w_full()
            .flex()
            .flex_col()
            .gap(px(5.))
            .px(px(22.))
            .py(px(16.))
            .h(px(76.))
            .overflow_hidden()
            .border_b_1()
            .border_color(rgb(LINE))
            .border_l_2()
            .border_color(rgb(if active { GREEN } else { LINE }))
            .bg(rgb(if active { TINT } else { PAPER }))
            .cursor_pointer()
            .hover(|style| style.bg(rgb(0xebeee6)))
            .on_click(cx.listener(move |app, _, window, cx| {
                app.focus.focus(window);
                app.choose(index, cx);
            }))
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(14.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(entry.title().to_owned()),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .truncate()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child(if entry.folder().is_empty() {
                        "Root".to_owned()
                    } else {
                        entry.folder().to_owned()
                    }),
            )
            .into_any_element()
    }

    fn sidebar(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .w(px(302.))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(rgb(LINE))
            .bg(rgb(PAPER))
            .child(
                div()
                    .px(px(22.))
                    .pt(px(26.))
                    .pb(px(18.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(label("YOUR ENTRIES"))
                    .child(
                        div()
                            .font_family(MONO)
                            .text_size(px(11.))
                            .text_color(rgb(MUTED))
                            .child(format!("{:02}", self.filtered.len())),
                    ),
            )
            .child(
                div()
                    .px(px(18.))
                    .pb(px(20.))
                    .child(self.search_field(window, cx)),
            )
            .child(
                div()
                    .id("entry-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .when(!self.filtered.is_empty(), |view| {
                        view.child(
                            uniform_list(
                                "entries",
                                self.filtered.len(),
                                cx.processor(|app, range: Range<usize>, _, cx| {
                                    range
                                        .map(|position| app.entry_row(app.filtered[position], cx))
                                        .collect::<Vec<_>>()
                                }),
                            )
                            .track_scroll(self.list_scroll.clone())
                            .size_full(),
                        )
                    })
                    .when(self.filtered.is_empty(), |view| {
                        view.child(
                            div()
                                .p(px(24.))
                                .text_size(px(13.))
                                .text_color(rgb(MUTED))
                                .child(if self.load.is_some() {
                                    "Reading entry names…"
                                } else if self.query.is_empty() {
                                    "No entries to show."
                                } else {
                                    "No matching entries.\nTry another search."
                                }),
                        )
                    }),
            )
            .child(
                div()
                    .p(px(22.))
                    .border_t_1()
                    .border_color(rgb(LINE))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(label(if self.demo {
                        "DEMO STORE"
                    } else {
                        "CURRENT STORE"
                    }))
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_family(MONO)
                            .text_color(rgb(MUTED))
                            .overflow_hidden()
                            .child(self.path_label()),
                    )
                    .child(
                        div()
                            .id("open-store")
                            .mt(px(8.))
                            .text_size(px(12.))
                            .text_color(rgb(GREEN))
                            .cursor_pointer()
                            .on_click(cx.listener(|app, _, window, cx| app.open_store(window, cx)))
                            .child("Open another store  ↗"),
                    ),
            )
            .into_any_element()
    }

    fn path_label(&self) -> String {
        if self.demo {
            return "Fictional entries · in memory".into();
        }
        self.path.as_ref().map_or_else(
            || "No store selected".into(),
            |path| {
                path.to_string_lossy()
                    .chars()
                    .filter(|c| !c.is_control())
                    .collect()
            },
        )
    }

    fn details(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let entry = self.session.selected();
        let now = Instant::now();
        let revealed = self.session.revealed(now);
        let pending = self.session.is_pending();
        let mut detail = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(WHITE));
        if let Some(entry) = entry {
            let folder = if entry.folder().is_empty() {
                "Root"
            } else {
                entry.folder()
            };
            detail = detail
                .child(
                    div()
                        .px(px(46.))
                        .pt(px(40.))
                        .pb(px(28.))
                        .flex()
                        .flex_col()
                        .gap(px(17.))
                        .child(label(format!("STORE   /   {}", folder.to_uppercase())))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(16.))
                                .child(
                                    div()
                                        .text_size(px(36.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(entry.title().to_owned()),
                                )
                                .child(
                                    div()
                                        .border_1()
                                        .border_color(rgb(LINE))
                                        .px(px(10.))
                                        .py(px(6.))
                                        .font_family(MONO)
                                        .text_size(px(10.))
                                        .text_color(rgb(MUTED))
                                        .child(".gpg"),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(rgb(MUTED))
                                .child(entry.name().to_owned()),
                        ),
                )
                .child(
                    div()
                        .px(px(46.))
                        .pb(px(24.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .border_b_1()
                        .border_color(rgb(LINE))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(7.))
                                .child(div().size(px(5.)).bg(rgb(GREEN)))
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(rgb(MUTED))
                                        .child("Encrypted on disk"),
                                ),
                        )
                        .child(
                            button("delete-entry", "Delete entry", false)
                                .on_click(cx.listener(|app, _, _, cx| app.request_delete(cx))),
                        ),
                );

            let mut body = div()
                .id("detail-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p(px(46.))
                .flex()
                .flex_col()
                .gap(px(24.));
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(label("PASSWORD"))
                    .child(
                        div()
                            .font_family(MONO)
                            .text_size(px(10.))
                            .text_color(rgb(if revealed.is_some() { GREEN } else { MUTED }))
                            .child(if revealed.is_some() {
                                format!("HIDES IN {:02}s", self.seconds)
                            } else if pending {
                                "UNLOCKING".into()
                            } else {
                                "HIDDEN".into()
                            }),
                    ),
            );
            let mut password = div()
                .border_1()
                .border_color(rgb(LINE))
                .bg(rgb(PAPER))
                .p(px(22.))
                .min_h(px(84.));
            if let Some(secret) = revealed {
                let text = secret.expose();
                let first_line = text.split('\n').next().unwrap_or("").trim_end_matches('\r');
                password = password.child(
                    div()
                        .id("password-text")
                        .overflow_x_scroll()
                        .font_family(MONO)
                        .text_size(px(18.))
                        .whitespace_nowrap()
                        .child(first_line.chars().take(4096).collect::<String>()),
                );
            } else {
                password = password.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(24.))
                                .text_color(rgb(0x8c968b))
                                .child("••••••••••••"),
                        )
                        .child(
                            div()
                                .font_family(MONO)
                                .text_size(px(10.))
                                .text_color(rgb(MUTED))
                                .child("CONCEALED"),
                        ),
                );
            }
            body = body.child(password);
            let controls = div()
                .flex()
                .items_center()
                .gap(px(18.))
                .child(
                    button(
                        "reveal",
                        if pending {
                            "Cancel reveal"
                        } else if revealed.is_some() {
                            "Hide password"
                        } else {
                            "Reveal password  ↗"
                        },
                        true,
                    )
                    .on_click(cx.listener(|app, _, window, cx| {
                        if app.session.is_pending() {
                            app.hide(cx);
                        } else {
                            app.reveal(window, cx);
                        }
                    })),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(MUTED))
                        .child(if pending {
                            "Esc to cancel"
                        } else {
                            "Enter to reveal · Esc to hide"
                        }),
                );
            body = body.child(controls);
            if let Some(status) = self.session.pending_status() {
                body = body.child(operation_status::panel(status));
            }

            if let Some(secret) = revealed {
                if let Some((_, notes)) = secret
                    .expose()
                    .split_once('\n')
                    .filter(|(_, notes)| !notes.trim().is_empty())
                {
                    body = body.child(
                        div()
                            .mt(px(12.))
                            .pt(px(24.))
                            .border_t_1()
                            .border_color(rgb(LINE))
                            .flex()
                            .flex_col()
                            .gap(px(18.))
                            .child(label("ENTRY CONTENT"))
                            .child(
                                div()
                                    .font_family(MONO)
                                    .text_size(px(12.))
                                    .line_height(px(23.))
                                    .child(notes.chars().take(16_384).collect::<String>()),
                            ),
                    );
                }
                if secret.expose().len() > 16_384 {
                    body = body.child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(MUTED))
                            .child("Long content is truncated in this preview."),
                    );
                }
            } else if !pending {
                body = body.child(div().mt(px(16.)).pt(px(24.)).border_t_1().border_color(rgb(LINE)).flex().flex_col().gap(px(10.))
                    .child(label("ONLY WHEN YOU NEED IT"))
                    .child(div().max_w(px(385.)).text_size(px(13.)).line_height(px(23.)).text_color(rgb(MUTED))
                        .child("Reveal this entry for 30 seconds. It hides when you leave the window or choose another entry.")));
            }
            detail = detail.child(body);
        } else {
            detail = detail.child(div().flex_1().flex().flex_col().justify_center().px(px(60.)).gap(px(18.))
                .child(div().text_size(px(56.)).font_weight(FontWeight::LIGHT).text_color(rgb(GREEN)).child("[ / ]"))
                .child(div().text_size(px(30.)).child(if self.load.is_some() { "Opening your store." } else if !self.query.is_empty() { "Nothing here. Yet." } else { "Your passwords, at home." }))
                .child(div().max_w(px(380.)).text_size(px(14.)).line_height(px(24.)).text_color(rgb(MUTED))
                    .child(if !self.query.is_empty() { "Search by an entry name or folder." } else { "Open an existing pass store to browse your entries. Your keys stay with GnuPG." }))
                .child(button("empty-open-store", "Open password store  ↗", true).w(px(208.)).on_click(cx.listener(|app, _, window, cx| app.open_store(window, cx))))
);
        }
        detail
            .child(
                div()
                    .px(px(46.))
                    .py(px(20.))
                    .border_t_1()
                    .border_color(rgb(LINE))
                    .flex()
                    .justify_between()
                    .child(if let Some(message) = &self.message {
                        div()
                            .max_w(px(560.))
                            .text_size(px(11.))
                            .text_color(rgb(ERROR))
                            .child(message.clone())
                    } else {
                        label(if self.demo {
                            "FICTIONAL DATA"
                        } else {
                            "LOCAL STORE"
                        })
                    }),
            )
            .into_any_element()
    }
}

impl Render for PassApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(PAPER))
            .text_color(rgb(INK))
            .font_family("DejaVu Sans")
            .text_size(px(14.))
            .key_context("Pass")
            .track_focus(&self.focus)
            .on_action(cx.listener(|app, _: &Hide, _, cx| app.hide(cx)))
            .on_action(cx.listener(|app, _: &OpenGenerator, window, cx| {
                app.open_generator(window, cx)
            }))
            .on_action(cx.listener(|app, _: &Reveal, window, cx| {
                if !app.generator_open && app.confirming_delete.is_none() {
                    app.reveal(window, cx);
                }
            }))
            .on_action(cx.listener(|app, _: &Refresh, _, cx| app.refresh(cx)))
            .on_action(cx.listener(|app, _: &FocusSearch, window, cx| {
                app.hide(cx);
                app.search_focus.focus(window);
                app.selection = 0..app.query.len();
                cx.notify();
            }))
            .on_action(cx.listener(|app, _: &NextEntry, _, cx| app.navigate(1, cx)))
            .on_action(cx.listener(|app, _: &PreviousEntry, _, cx| app.navigate(-1, cx)))
            .on_action(cx.listener(|app, _: &Backspace, window, cx| app.backspace(window, cx)))
            .on_action(cx.listener(|app, _: &Delete, window, cx| app.delete(window, cx)))
            .on_action(cx.listener(|app, _: &Left, window, cx| app.move_cursor(-1, window, cx)))
            .on_action(cx.listener(|app, _: &Right, window, cx| app.move_cursor(1, window, cx)))
            .on_action(cx.listener(|app, _: &Start, window, cx| app.move_edge(false, window, cx)))
            .on_action(cx.listener(|app, _: &End, window, cx| app.move_edge(true, window, cx)))
            .on_action(cx.listener(|app, _: &SelectAll, window, cx| {
                if app.generator_name_focus.is_focused(window) {
                    app.generator_name_selection = 0..app.generator_name.len();
                    cx.notify();
                } else if app.search_focus.is_focused(window) {
                    app.selection = 0..app.query.len();
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|app, _: &SwitchFocus, window, cx| {
                if app.search_focus.is_focused(window) {
                    app.focus.focus(window);
                } else {
                    app.search_focus.focus(window);
                }
                cx.notify();
            }))
            .on_action(cx.listener(|_, _: &Quit, _, cx| cx.quit()))
            .on_action(cx.listener(|_, _: &ToggleFullscreen, window, _| {
                window.toggle_fullscreen();
            }))
            .child(
                div()
                    .id("titlebar")
                    .w_full()
                    .h(px(48.))
                    .flex_shrink_0()
                    .px(px(26.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .bg(rgb(INK))
                    .on_mouse_down(gpui::MouseButton::Left, |event, window, cx| {
                        if event.click_count == 2 {
                            window.toggle_fullscreen();
                        } else {
                            window.start_window_move();
                        }
                        cx.stop_propagation();
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(14.))
                            .child(
                                div()
                                    .w(px(32.))
                                    .h(px(32.))
                                    .border_1()
                                    .border_color(rgb(0x809287))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(16.))
                                    .text_color(rgb(0xe0edcb))
                                    .font_family(MONO)
                                    .child("/"),
                            )
                            .child(
                                div()
                                    .text_size(px(22.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(WHITE))
                                    .child("pass"),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(18.))
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .font_family(MONO)
                                    .text_color(rgb(0xc7d6be))
                                    .child(if self.demo { "DEMO / 01" } else { "LOCAL / 01" }),
                            )
                            .child(
                                div()
                                    .id("open-generator")
                                    .text_size(px(12.))
                                    .text_color(rgb(WHITE))
                                    .cursor_pointer()
                                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation();
                                    })
                                    .on_click(cx.listener(|app, _, window, cx| {
                                        app.open_generator(window, cx)
                                    }))
                                    .child("Generate ↗"),
                            )
                            .child(
                                div()
                                    .id("refresh")
                                    .text_size(px(12.))
                                    .text_color(rgb(WHITE))
                                    .cursor_pointer()
                                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation();
                                    })
                                    .on_click(cx.listener(|app, _, _, cx| app.refresh(cx)))
                                    .child("↻  Refresh"),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.sidebar(window, cx))
                    .child(self.details(cx)),
            )
            .child(
                div()
                    .h(px(30.))
                    .flex_shrink_0()
                    .px(px(24.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_t_1()
                    .border_color(rgb(LINE))
                    .child(
                        div().text_size(px(10.)).text_color(rgb(MUTED)).child(
                            "Ctrl F  Search     ↑ ↓  Browse     Enter  Reveal     Ctrl G  Generate     Esc  Hide",
                        ),
                    ),
            )
            .when(self.generator_open, |view| {
                view.child(self.generator_view(window, cx))
            })
            .when(self.confirming_delete.is_some(), |view| {
                view.child(self.delete_confirmation_view(cx))
            })
            .child(resize_handle(
                "resize-top",
                ResizeEdge::Top,
                CursorStyle::ResizeUpDown,
                Some(0.),
                Some(8.),
                Some(8.),
                None,
                None,
                Some(8.),
            ))
            .child(resize_handle(
                "resize-bottom",
                ResizeEdge::Bottom,
                CursorStyle::ResizeUpDown,
                None,
                Some(8.),
                Some(8.),
                Some(0.),
                None,
                Some(8.),
            ))
            .child(resize_handle(
                "resize-left",
                ResizeEdge::Left,
                CursorStyle::ResizeLeftRight,
                Some(8.),
                Some(0.),
                None,
                Some(8.),
                Some(8.),
                None,
            ))
            .child(resize_handle(
                "resize-right",
                ResizeEdge::Right,
                CursorStyle::ResizeLeftRight,
                Some(8.),
                None,
                Some(0.),
                Some(8.),
                Some(8.),
                None,
            ))
            .child(resize_handle(
                "resize-top-left",
                ResizeEdge::TopLeft,
                CursorStyle::ResizeUpLeftDownRight,
                Some(0.),
                Some(0.),
                None,
                None,
                Some(14.),
                Some(14.),
            ))
            .child(resize_handle(
                "resize-top-right",
                ResizeEdge::TopRight,
                CursorStyle::ResizeUpRightDownLeft,
                Some(0.),
                None,
                Some(0.),
                None,
                Some(14.),
                Some(14.),
            ))
            .child(resize_handle(
                "resize-bottom-left",
                ResizeEdge::BottomLeft,
                CursorStyle::ResizeUpRightDownLeft,
                None,
                Some(0.),
                None,
                Some(0.),
                Some(14.),
                Some(14.),
            ))
            .child(resize_handle(
                "resize-bottom-right",
                ResizeEdge::BottomRight,
                CursorStyle::ResizeUpLeftDownRight,
                None,
                None,
                Some(0.),
                Some(0.),
                Some(14.),
                Some(14.),
            ))
    }
}

fn resize_handle(
    id: &'static str,
    edge: ResizeEdge,
    cursor: CursorStyle,
    top: Option<f32>,
    left: Option<f32>,
    right: Option<f32>,
    bottom: Option<f32>,
    width: Option<f32>,
    height: Option<f32>,
) -> gpui::Stateful<gpui::Div> {
    let mut handle = div().id(id).absolute().cursor(cursor).on_mouse_down(
        gpui::MouseButton::Left,
        move |_, window, cx| {
            window.start_window_resize(edge);
            cx.stop_propagation();
        },
    );
    if let Some(top) = top {
        handle = handle.top(px(top));
    }
    if let Some(left) = left {
        handle = handle.left(px(left));
    }
    if let Some(right) = right {
        handle = handle.right(px(right));
    }
    if let Some(bottom) = bottom {
        handle = handle.bottom(px(bottom));
    }
    if let Some(width) = width {
        handle = handle.w(px(width));
    }
    if let Some(height) = height {
        handle = handle.h(px(height));
    }
    handle
}

fn main() {
    let options = match Options::parse() {
        Ok(Some(options)) => options,
        Ok(None) => return,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    if harden_process().is_err() {
        eprintln!("Could not disable process dumps; refusing to open the password client.");
        std::process::exit(1);
    }
    Application::new().run(move |cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("escape", Hide, Some("Pass")),
            KeyBinding::new("ctrl-g", OpenGenerator, Some("Pass")),
            KeyBinding::new("enter", Reveal, Some("Pass")),
            KeyBinding::new("ctrl-r", Refresh, Some("Pass")),
            KeyBinding::new("ctrl-f", FocusSearch, Some("Pass")),
            KeyBinding::new("down", NextEntry, Some("Pass")),
            KeyBinding::new("up", PreviousEntry, Some("Pass")),
            KeyBinding::new("backspace", Backspace, Some("Pass")),
            KeyBinding::new("delete", Delete, Some("Pass")),
            KeyBinding::new("left", Left, Some("Pass")),
            KeyBinding::new("right", Right, Some("Pass")),
            KeyBinding::new("home", Start, Some("Pass")),
            KeyBinding::new("end", End, Some("Pass")),
            KeyBinding::new("ctrl-a", SelectAll, Some("Pass")),
            KeyBinding::new("tab", SwitchFocus, Some("Pass")),
            KeyBinding::new("shift-tab", SwitchFocus, Some("Pass")),
            KeyBinding::new("ctrl-q", Quit, Some("Pass")),
            KeyBinding::new("f11", ToggleFullscreen, Some("Pass")),
        ]);
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let bounds = Bounds::centered(None, size(px(1120.), px(780.)), cx);
        if cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(940.), px(640.))),
                    is_resizable: true,
                    titlebar: Some(TitlebarOptions {
                        title: Some(SharedString::from("pass — Password store")),
                        ..Default::default()
                    }),
                    app_id: Some("dev.gpui-pass.desktop".into()),
                    ..Default::default()
                },
                move |window, cx| cx.new(|cx| PassApp::new(options, window, cx)),
            )
            .is_err()
        {
            eprintln!("Could not open a GPUI window. Check your display and Vulkan driver.");
            cx.quit();
        }
        cx.activate(true);
    });
}
