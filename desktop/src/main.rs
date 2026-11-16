#![forbid(unsafe_code)]

mod device_verification;
use device_verification::{DeviceVerification, State as VerificationState};

mod diagnostics;
use diagnostics::trace;

mod card_refresh;
use card_refresh::{CardProbe, CardRefresh, ProbeResult};

mod navigation;
use navigation::{Navigation, Page};

mod clipboard;
mod process_output;
use clipboard::ClipboardState;

mod card_status_view;
mod editor_input;
mod entries_view;
mod entry_editor;
mod generator_view;
mod i18n;
mod loading_spinner;
mod search;
mod theme;
mod yubikey_detection;

use fluent_bundle::FluentArgs;
use generator_view::{GeneratorMode, OverwriteConfirmation};
use gpui::{
    App, Application, Bounds, Context, CursorStyle, Div, FocusHandle, FontWeight, Image,
    ImageFormat, KeyBinding, ResizeEdge, ScrollStrategy, SharedString, Subscription,
    TitlebarOptions, UniformListScrollHandle, Window, WindowBounds, WindowOptions, actions, canvas,
    div, img, prelude::*, px, rgb, size, uniform_list,
};
use gpui_pass_core::{
    Cancellation, CardEvent, CardObservation, Entry, EntryRevision, Error, GeneratorOptions, Gpg,
    OperationProgress, OperationStatus, REVEAL_DURATION, RevealRequest, Secret, Session, Store,
    default_store_path, generate_memorable_password, generate_pass_default_password,
    harden_process,
};
use loading_spinner::loading_spinner;
use std::{
    borrow::Cow,
    collections::{HashSet, VecDeque},
    io::{BufRead, BufReader},
    ops::Range,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc, LazyLock, Mutex,
        mpsc::{self, Receiver},
    },
    time::{Duration, Instant},
};
use theme::Theme;
use yubikey_detection::{
    CardDetails, CardProbeResult, CardWatcher, UsbCardWatcher, UsbReaderDetails, YubiConnection,
    YubiKeyInfo, YubiRecord,
};
use zeroize::{Zeroize, Zeroizing};

const NOTIFICATION_TIMEOUT: Duration = Duration::from_secs(6);

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
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        Copy,
        CopyEntry,
        Cut,
        Paste,
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
    loading_demo: bool,
}

impl Options {
    fn parse() -> Result<Option<Self>, String> {
        let mut options = Self {
            store: None,
            gpg: PathBuf::from("/usr/bin/gpg"),
            demo: false,
            loading_demo: false,
        };
        let mut args = std::env::args_os().skip(1);
        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("--demo") => options.demo = true,
                Some("--loading-demo") => options.loading_demo = true,
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
                        "gpui-pass [--store DIRECTORY] [--gpg /absolute/path/to/gpg] [--demo | --loading-demo]\n\nBrowse and create encrypted entries in a pass store. Defaults to PASSWORD_STORE_DIR or ~/.password-store.\n--demo uses fictional entries in memory and never opens a password store.\n--loading-demo holds the UI on its loading spinner without opening a password store."
                    );
                    return Ok(None);
                }
                _ => return Err("Unknown argument. Use --help for usage.".into()),
            }
        }
        if options.demo && options.store.is_some() {
            return Err("Use --demo or --store, separately.".into());
        }
        if options.loading_demo && (options.demo || options.store.is_some()) {
            return Err("Use --loading-demo by itself.".into());
        }
        Ok(Some(options))
    }
}

type LoadedStore = Result<(Store, Vec<Entry>), Error>;

type VersionedSecretResult = Result<(Secret, Option<EntryRevision>), Error>;
type SaveResult = Result<(Entry, Secret), Error>;
type PendingSave = (Cancellation, OperationProgress, Receiver<SaveResult>);

struct PendingCopy {
    entry: Entry,
    progress: OperationProgress,
    cancellation: Cancellation,
    receiver: Receiver<Result<Secret, Error>>,
}

impl Drop for PendingCopy {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

pub(crate) struct PassApp {
    pub(crate) theme: Theme,
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
    pub(crate) generator_name_focus: FocusHandle,
    pub(crate) custom_entry_name_focus: FocusHandle,
    pub(crate) generator_name: Zeroizing<String>,
    pub(crate) generator_name_selection: Range<usize>,
    generator_name_anchor: Option<usize>,
    pub(crate) generator_name_cursor: usize,
    generator_name_marked: Option<Range<usize>>,
    generator_name_bounds: Option<Bounds<gpui::Pixels>>,
    active: bool,
    store: Option<Store>,
    path: Option<PathBuf>,
    gpg: PathBuf,
    demo: bool,
    loading_demo: bool,
    load: Option<Receiver<LoadedStore>>,
    reveal: Option<(RevealRequest, Receiver<VersionedSecretResult>)>,
    copy_pending: Option<PendingCopy>,
    generated: Option<(Secret, Instant)>,
    generated_visible: bool,
    clipboard: ClipboardState,
    clipboard_seconds: Option<u64>,
    generation_words: usize,
    generator_mode: GeneratorMode,
    generation_digits: bool,
    generation_separators: bool,
    generator_slider_bounds: Option<Bounds<gpui::Pixels>>,
    custom_entry_name: String,
    pub(crate) custom_entry_name_selection: Range<usize>,
    pub(crate) custom_entry_name_cursor: usize,
    pub(crate) custom_entry_name_anchor: Option<usize>,
    pub(crate) custom_entry_name_marked: Option<Range<usize>>,
    pub(crate) custom_entry_name_bounds: Option<Bounds<gpui::Pixels>>,
    navigation: Navigation,
    confirming_delete: Option<Entry>,
    confirming_overwrite: Option<OverwriteConfirmation>,
    saving: Option<PendingSave>,
    saved_entry_name: Option<String>,
    yubikey_history: VecDeque<YubiRecord>,
    last_yubikey_observation: Option<Instant>,
    yubikey_connection: YubiConnection,
    yubikey_details: Option<CardDetails>,
    yubikey_check: Option<CardProbe<CardProbeResult>>,
    yubikey_last_checked: Option<Instant>,
    yubikey_info: Option<YubiKeyInfo>,
    yubikey_info_check: Option<CardProbe<YubiKeyInfo>>,
    usb_reader_details: Option<UsbReaderDetails>,
    card_detail_retry_at: Option<Instant>,
    card_refresh: CardRefresh,
    device_verification: DeviceVerification,
    verification_state_seen: VerificationState,
    verification_notification_until: Option<Instant>,
    card_watcher: Option<CardWatcher>,
    card_watcher_supported: Option<bool>,
    card_watcher_retry_at: Option<Instant>,
    card_refresh_pending: bool,
    card_poll_paused: bool,
    usb_card_watcher: Option<UsbCardWatcher>,
    usb_watcher_retry_at: Option<Instant>,
    usb_yubikey_present: Option<bool>,
    usb_last_scan: Option<Instant>,
    close_after_save: bool,
    pub(crate) editing_entry: bool,
    pub(crate) editor_layout: Option<entry_editor::TextLayout>,
    saving_edit: bool,
    saving_custom_entry: bool,
    edit_conflict: bool,
    revealed_revision: Option<EntryRevision>,
    edit_revision: Option<EntryRevision>,
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
        let custom_entry_name_focus = cx.focus_handle();
        let subscription = cx.observe_window_activation(window, |app, window, cx| {
            app.active = window.is_window_active();
            if !app.active {
                app.session.deactivate();
                app.clear_generated(cx);
                app.cancel_edit(cx);
            }
            cx.notify();
        });
        search_focus.focus(window);
        let mut app = Self {
            theme: Theme::load_user(),
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
            custom_entry_name_focus,
            generator_name: Zeroizing::new(String::new()),
            generator_name_selection: 0..0,
            generator_name_anchor: None,
            generator_name_cursor: 0,
            generator_name_marked: None,
            generator_name_bounds: None,
            active: window.is_window_active(),
            store: None,
            path: options.store.or_else(|| default_store_path().ok()),
            gpg: options.gpg,
            demo: options.demo,
            loading_demo: options.loading_demo,
            load: None,
            reveal: None,
            copy_pending: None,
            generated: None,
            generated_visible: false,
            clipboard: ClipboardState::default(),
            clipboard_seconds: None,
            generation_words: 6,
            generator_mode: GeneratorMode::PassDefault,
            generation_digits: true,
            generation_separators: true,
            generator_slider_bounds: None,
            custom_entry_name: String::new(),
            custom_entry_name_selection: 0..0,
            custom_entry_name_cursor: 0,
            custom_entry_name_anchor: None,
            custom_entry_name_marked: None,
            custom_entry_name_bounds: None,
            navigation: Navigation::default(),
            confirming_delete: None,
            confirming_overwrite: None,
            saving: None,
            saved_entry_name: None,
            yubikey_history: VecDeque::new(),
            last_yubikey_observation: None,
            yubikey_connection: YubiConnection::Checking,
            yubikey_details: None,
            yubikey_check: None,
            yubikey_last_checked: None,
            yubikey_info: None,
            yubikey_info_check: None,
            usb_reader_details: None,
            card_detail_retry_at: None,
            card_refresh: CardRefresh::default(),
            device_verification: DeviceVerification::default(),
            verification_state_seen: VerificationState::Idle,
            verification_notification_until: None,
            card_watcher: None,
            card_watcher_supported: None,
            card_watcher_retry_at: None,
            card_refresh_pending: false,
            card_poll_paused: false,
            usb_card_watcher: None,
            usb_watcher_retry_at: None,
            usb_yubikey_present: None,
            usb_last_scan: None,
            close_after_save: false,
            editing_entry: false,
            editor_layout: None,
            saving_edit: false,
            saving_custom_entry: false,
            edit_conflict: false,
            revealed_revision: None,
            edit_revision: None,
            message: None,
            message_until: None,
            seconds: 0,
            last_operation_status: None,
            _activation: subscription,
        };
        cx.on_app_quit(|app, cx| {
            app.copy_pending = None;
            app.session.hide();
            app.device_verification.cancel();
            if let Some((cancel, _, _)) = &app.saving {
                cancel.cancel();
            }
            app.restore_pass_clipboard(cx);
            async {}
        })
        .detach();
        cx.on_release(|app, cx| {
            app.copy_pending = None;
            if let Some((cancel, _, _)) = &app.saving {
                cancel.cancel();
            }
            app.restore_pass_clipboard(cx);
        })
        .detach();
        app.refresh(cx);
        app.start_card_watcher();
        app.start_usb_card_watcher(cx);
        if app.yubikey_check.is_none() {
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

    fn tick_interval(&self) -> Duration {
        if self.reveal.is_some() || self.copy_pending.is_some() || self.saving.is_some() {
            Duration::from_millis(100)
        } else if cfg!(target_os = "linux")
            || self.card_watcher.is_some()
            || self.usb_card_watcher.is_some()
        {
            // The watcher is event-driven. Poll its receiver frequently so hotplug events
            // reach the UI promptly instead of waiting for the idle refresh interval.
            Duration::from_millis(100)
        } else if self.session.revealed(Instant::now()).is_some()
            || self.generated.is_some()
            || self.clipboard.active()
            || self.message_until.is_some()
        {
            Duration::from_millis(250)
        } else {
            Duration::from_secs(2)
        }
    }

    fn store_is_loading(&self) -> bool {
        self.load.is_some() || self.loading_demo
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.saving.is_some() {
            return;
        }
        self.cancel_edit(cx);
        self.confirming_delete = None;
        if self.load.is_some() {
            return;
        }
        self.session.select(None);
        self.copy_pending = None;
        self.reveal = None;
        self.clear_generated(cx);
        self.clear_message();
        self.store = None;
        self.entries.clear();
        self.filtered.clear();
        if self.loading_demo {
            cx.notify();
            return;
        }
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
        if self
            .device_verification
            .poll(self.card_refresh.generation())
        {
            cx.notify();
        }
        let verification_state = self.device_verification.state();
        if verification_state != self.verification_state_seen {
            self.verification_state_seen = verification_state;
            self.verification_notification_until = if matches!(
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
            .verification_notification_until
            .is_some_and(|until| Instant::now() >= until)
        {
            self.verification_notification_until = None;
            cx.notify();
        }
        self.capture_yubikey_status(cx);
        self.poll_yubikey_connection(cx);
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
                Err(error) => {
                    self.saving_edit = false;
                    self.set_message(error.to_string());
                }
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
                Ok((secret, revision)) => {
                    if !self
                        .session
                        .finish_reveal(&request, secret, self.active, Instant::now())
                    {
                        self.set_message(
                            "Entry stayed hidden. Return to this window and reveal again.".into(),
                        );
                    } else {
                        self.revealed_revision = revision;
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
        if let Some(result) = self
            .copy_pending
            .as_ref()
            .and_then(|pending| pending.receiver.try_recv().ok())
        {
            let pending = self.copy_pending.take().expect("copy request available");
            let entry = pending.entry.clone();
            if self.active && self.session.selected() == Some(&entry) {
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
        let now = Instant::now();
        let clipboard_seconds = self.clipboard.remaining_seconds(now);
        if self.clipboard_seconds != clipboard_seconds {
            if let Some(item) = self.clipboard.poll(cx.read_from_clipboard(), now) {
                cx.write_to_clipboard(item);
            }
            self.clipboard_seconds = self.clipboard.remaining_seconds(now);
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
            let was_edit = self.saving_edit;
            let was_custom_entry = self.saving_custom_entry;
            self.saving_custom_entry = false;
            match result {
                Ok((entry, secret)) => {
                    let name = entry.name().to_owned();
                    if self.saving_edit {
                        self.saving_edit = false;
                        self.cancel_edit(cx);
                        self.session.select(Some(entry));
                        self.set_message("Entry updated.".into());
                        cx.notify();
                        return;
                    }
                    self.entries.push(entry);
                    self.entries.sort_by_cached_key(|entry| {
                        (entry.name().to_lowercase(), entry.name().to_owned())
                    });
                    self.query.clear();
                    self.selection = 0..0;
                    self.marked = None;
                    self.filter(cx);
                    self.session.select(
                        self.entries
                            .iter()
                            .find(|entry| entry.name() == name)
                            .cloned(),
                    );
                    if was_custom_entry {
                        self.saved_entry_name = None;
                        self.close_generator(cx);
                        self.set_message(i18n::text("custom-entry-saved"));
                    } else {
                        self.saved_entry_name = Some(name.clone());
                        if !self.close_after_save && self.active {
                            let text = secret.expose().to_owned();
                            self.copy_to_clipboard(text, name, cx);
                            self.generated = Some((secret, Instant::now() + REVEAL_DURATION));
                        }
                        self.clear_message();
                    }
                }
                Err(error) => {
                    self.saving_edit = false;
                    self.saving_custom_entry = false;
                    if matches!(&error, Error::EntryConflict) && was_edit {
                        self.edit_conflict = true;
                        self.set_message(i18n::text("edit-conflict-notice"));
                    } else if matches!(&error, Error::EntryConflict) {
                        self.edit_conflict = false;
                        self.set_message(i18n::text("overwrite-entry-conflict"));
                    } else {
                        self.edit_conflict = false;
                        self.set_message(error.to_string());
                    }
                }
            }
            let close = self.close_after_save;
            self.close_after_save = false;
            if close {
                self.close_generator(cx);
            }
            cx.notify();
        }
        let status = self
            .session
            .pending_status()
            .or_else(|| {
                self.copy_pending
                    .as_ref()
                    .map(|pending| pending.progress.snapshot())
            })
            .or_else(|| {
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
        self.message_until = Some(Instant::now() + NOTIFICATION_TIMEOUT);
    }

    fn clear_message(&mut self) {
        self.message = None;
        self.message_until = None;
    }

    fn filter(&mut self, cx: &mut Context<Self>) {
        if self.saving.is_some() {
            return;
        }
        self.cancel_edit(cx);
        self.confirming_delete = None;
        self.session.hide();
        self.copy_pending = None;
        self.reveal = None;
        self.clear_generated(cx);
        self.clear_message();
        let terms: Vec<_> = self
            .query
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        self.filtered = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, entry)| entry.matches_terms(&terms).then_some(i))
            .collect();
        let first = self
            .filtered
            .first()
            .map(|index| self.entries[*index].clone());
        self.session.select(first);
        self.list_scroll.scroll_to_item(0, ScrollStrategy::Top);
    }

    fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.saving.is_some() {
            return;
        }
        self.cancel_edit(cx);
        self.session.select(self.entries.get(index).cloned());
        self.reveal = None;
        self.copy_pending = None;
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
        if self.confirming_overwrite.take().is_some() {
            cx.notify();
            return;
        }
        if self.confirming_delete.take().is_some() {
            cx.notify();
            return;
        }
        if self.navigation.generator_open() {
            self.close_generator(cx);
            return;
        }
        if self.editing_entry {
            self.cancel_edit(cx);
            cx.notify();
            return;
        }
        if self.navigation.page() == Page::CardDetails {
            self.leave_card_status(cx);
            return;
        }
        self.session.hide();
        self.copy_pending = None;
        self.reveal = None;
        self.clear_generated(cx);
        self.clear_message();
        cx.notify();
    }

    pub(crate) fn text_editor_active(&self) -> bool {
        self.editing_entry || self.generator_mode == GeneratorMode::Custom
    }

    fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        if !self.editing_entry {
            return;
        }
        let had_conflict = self.edit_conflict;
        if self.saving_edit {
            if let Some((cancel, _, _)) = &self.saving {
                cancel.cancel();
            }
        }
        self.editing_entry = false;
        self.edit_conflict = false;
        self.edit_revision = None;
        if had_conflict {
            self.session.hide();
            self.revealed_revision = None;
        }
        self.generator_name.zeroize();
        self.generator_name_selection = 0..0;
        self.generator_name_anchor = None;
        self.generator_name_cursor = 0;
        self.generator_name_marked = None;
        self.editor_layout = None;
        cx.notify();
    }

    fn request_delete(&mut self, cx: &mut Context<Self>) {
        if self.saving.is_some() {
            return;
        }
        if let Some(entry) = self.session.selected().cloned() {
            self.session.hide();
            self.reveal = None;
            self.clear_generated(cx);
            self.confirming_delete = Some(entry);
            cx.notify();
        }
    }

    fn begin_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving.is_some() {
            return;
        }
        let Some(secret) = self.session.revealed(Instant::now()) else {
            self.set_message("Reveal the entry before editing it.".into());
            cx.notify();
            return;
        };
        if !editor_input::supported(secret.expose()) {
            self.set_message(i18n::text("editor-limit"));
            cx.notify();
            return;
        }
        self.generator_name = Zeroizing::new(secret.expose().to_owned());
        self.generator_name_selection = self.generator_name.len()..self.generator_name.len();
        self.generator_name_anchor = None;
        self.generator_name_cursor = self.generator_name.len();
        self.generator_name_marked = None;
        self.edit_revision = self.revealed_revision;
        self.editing_entry = true;
        self.edit_conflict = false;
        self.generator_name_bounds = None;
        self.editor_layout = None;
        self.generator_name_focus.focus(window);
        cx.notify();
    }

    fn overwrite_conflicted_edit(&mut self, cx: &mut Context<Self>) {
        if !self.edit_conflict || self.saving.is_some() {
            return;
        }
        let Some(entry) = self.session.selected() else {
            return;
        };
        let Some(store) = self.store.as_ref() else {
            return;
        };
        match store.entry_revision(entry) {
            Ok(revision) => {
                self.edit_revision = Some(revision);
                self.save_edit(cx);
            }
            Err(error) => {
                self.set_message(error.to_string());
                cx.notify();
            }
        }
    }

    fn discard_conflicted_edit(&mut self, cx: &mut Context<Self>) {
        if !self.edit_conflict || self.saving.is_some() {
            return;
        }
        self.cancel_edit(cx);
        self.set_message(i18n::text("edit-conflict-discarded"));
        cx.notify();
    }

    fn save_edit(&mut self, cx: &mut Context<Self>) {
        if self.saving.is_some() {
            return;
        }
        let Some(entry) = self.session.selected().cloned() else {
            return;
        };
        let Some(store) = self.store.clone() else {
            self.set_message("Open a password store before editing entries.".into());
            cx.notify();
            return;
        };
        let Some(revision) = self.edit_revision else {
            self.set_message("Reopen and reveal the entry before saving.".into());
            cx.notify();
            return;
        };
        let secret =
            match Secret::from_bytes(Zeroizing::new(self.generator_name.as_bytes().to_vec())) {
                Ok(secret) => secret,
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
        self.saving_edit = true;
        self.saving = Some((cancellation, progress, receiver));
        self.clear_message();
        cx.notify();
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
            .bg(rgb(self.theme.colors.background))
            .child(
                self.theme.panel()
                    .w(px(520.))
                    .max_w(px(520.))
                    .p(px(32.))
                    .flex()
                    .flex_col()
                    .gap(px(20.))
                    .child(self.theme.label("REMOVE PASSWORD ENTRY"))
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
                            .text_color(rgb(self.theme.colors.muted))
                            .child("This removes the encrypted entry from the current store. It does not erase copies in Git history or backups."),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(10.))
                            .child(self.theme.button("cancel-delete", "Cancel", false).on_click(
                                cx.listener(|app, _, _, cx| {
                                    app.confirming_delete = None;
                                    cx.notify();
                                }),
                            ))
                            .child(
                                self.theme
                                    .destructive_button("confirm-delete", "Delete entry")
                                    .on_click(cx.listener(|app, _, _, cx| app.confirm_delete(cx))),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn brand_logotype(&self) -> Div {
        div()
            .flex()
            .items_center()
            .gap(px(14.))
            .child(
                div()
                    .w(px(32.))
                    .h(px(32.))
                    .border_1()
                    .border_color(rgb(self.theme.colors.nav_foreground))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(16.))
                    .text_color(rgb(self.theme.colors.nav_foreground))
                    .font_family(self.theme.mono_font)
                    .child("/"),
            )
            .child(
                div()
                    .text_size(px(22.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(self.theme.colors.nav_foreground))
                    .child("pass"),
            )
    }

    fn copy_to_clipboard(&mut self, content: String, source: String, cx: &mut Context<Self>) {
        let timeout = std::env::var("PASSWORD_STORE_CLIP_TIME").ok();
        let deadline = clipboard::clipboard_deadline(Instant::now(), timeout.as_deref());
        let item = self
            .clipboard
            .copy(content, source, cx.read_from_clipboard(), deadline);
        cx.write_to_clipboard(item);
        self.clipboard_seconds = None;
        cx.notify();
    }

    fn restore_pass_clipboard(&mut self, cx: &mut App) {
        if let Some(item) = self.clipboard.restore(cx.read_from_clipboard()) {
            cx.write_to_clipboard(item);
        }
    }

    fn reveal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window);
        if self.copy_pending.is_some()
            || self.device_verification.running()
            || self.session.is_pending()
            || !window.is_window_active()
        {
            return;
        }
        if self.session.revealed(Instant::now()).is_some() {
            self.hide(cx);
            return;
        }
        self.revealed_revision = None;
        self.clear_generated(cx);
        let Some(request) = self.session.begin_reveal() else {
            return;
        };
        self.clear_message();
        let result = if self.demo {
            let secret = Secret::from_bytes(Zeroizing::new(b"demo-only-not-a-real-password\nusername: alex@example.test\n\nA fictional entry for exploring the interface.\n".to_vec()));
            let (sender, receiver) = mpsc::sync_channel(1);
            let _ = sender.send(secret.map(|secret| (secret, None)));
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
                let _ = sender.send(
                    gpg.decrypt_with_revision(&store, &entry, &cancel, &progress)
                        .map(|(secret, revision)| (secret, Some(revision))),
                );
            });
            receiver
        };
        self.reveal = Some((request, result));
        cx.notify();
    }

    fn copy_selected(&mut self, cx: &mut Context<Self>) {
        if self.copy_pending.is_some()
            || self.reveal.is_some()
            || self.saving.is_some()
            || self.device_verification.running()
        {
            return;
        }
        let Some(entry) = self.session.selected().cloned() else {
            return;
        };
        self.clear_generated(cx);
        self.clear_message();
        let (sender, receiver) = mpsc::sync_channel(1);
        let cancellation = Cancellation::default();
        let progress = OperationProgress::new(OperationStatus::working(
            "Copying entry",
            "Follow the system prompt. If your YubiKey blinks, touch its gold contact.",
        ));
        if self.demo {
            let secret = Secret::from_bytes(Zeroizing::new(b"demo-only-not-a-real-password\nusername: alex@example.test\n\nA fictional entry for exploring the interface.\n".to_vec()));
            let _ = sender.send(secret);
        } else if let (Ok(gpg), Some(store)) = (Gpg::new(&self.gpg), self.store.clone()) {
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
            self.copy_pending = Some(PendingCopy {
                entry,
                progress,
                cancellation,
                receiver,
            });
            cx.notify();
            return;
        } else {
            self.set_message("Open a password store before copying an entry.".into());
            return;
        }
        self.copy_pending = Some(PendingCopy {
            entry,
            progress,
            cancellation,
            receiver,
        });
        cx.notify();
    }

    fn open_store(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving.is_some() {
            return;
        }
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
                        if app.saving.is_some() {
                            return;
                        }
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
}

impl Render for PassApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let main_content = if self.navigation.generator_open() {
            self.generator_view(window, cx)
        } else if self.navigation.page() == Page::CardDetails {
            self.yubikey_view(cx)
        } else {
            div()
                .flex()
                .flex_1()
                .min_h_0()
                .child(self.sidebar(window, cx))
                .child(self.details(window, cx))
                .into_any_element()
        };
        let status = self
            .session
            .pending_status()
            .or_else(|| {
                self.copy_pending
                    .as_ref()
                    .map(|pending| pending.progress.snapshot())
            })
            .or_else(|| {
                self.saving
                    .as_ref()
                    .map(|(_, progress, _)| progress.snapshot())
            });
        let footer = if let Some(message) = &self.message {
            notification_bar(&self.theme, message.clone())
        } else if let Some(status) = status {
            notification_bar(&self.theme, format!("{} · {}", status.title, status.detail))
        } else if self.device_verification.running()
            || self.verification_notification_until.is_some()
        {
            notification_bar(&self.theme, self.device_verification.state().message())
        } else if let Some(seconds) = self.clipboard.remaining_seconds(Instant::now()) {
            let entry_name = self.clipboard.source().unwrap_or("entry");
            div()
                .h(px(48.))
                .flex_shrink_0()
                .px(px(24.))
                .flex()
                .items_center()
                .justify_between()
                .border_t_1()
                .border_color(rgb(self.theme.colors.border))
                .bg(rgb(self.theme.colors.surface))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .text_size(px(11.))
                        .text_color(rgb(self.theme.colors.ink))
                        .child({
                            let mut args = FluentArgs::new();
                            args.set("entry", entry_name);
                            i18n::text_with_args("copied-entry", &args)
                        })
                        .child(div().text_color(rgb(self.theme.colors.muted)).child({
                            let mut args = FluentArgs::new();
                            args.set("seconds", seconds.min(i64::MAX as u64) as i64);
                            i18n::text_with_args("clipboard-restores-in", &args)
                        })),
                )
                .child(
                    self.theme
                        .button("restore-clipboard-now", "Restore now", false)
                        .h(px(28.))
                        .on_click(cx.listener(|app, _, _, cx| app.restore_pass_clipboard(cx))),
                )
        } else {
            div()
                .h(px(48.))
                .flex_shrink_0()
                .px(px(24.))
                .flex()
                .items_center()
                .border_t_1()
                .border_color(rgb(self.theme.colors.border))
                .child(
                    div().text_size(px(10.)).text_color(rgb(self.theme.colors.muted)).child(
                        "Ctrl F  Search     ↑ ↓  Browse     Enter  Reveal     Ctrl Shift C  Copy     Ctrl G  Generate     Esc  Hide",
                    ),
                )
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(self.theme.colors.background))
            .text_color(rgb(self.theme.colors.ink))
            .font_family("Inter")
            .text_size(px(15.))
            .key_context("Pass")
            .track_focus(&self.focus)
            .on_action(cx.listener(|app, _: &Hide, _, cx| app.hide(cx)))
            .on_action(
                cx.listener(|app, _: &OpenGenerator, window, cx| app.open_generator(window, cx)),
            )
            .on_action(cx.listener(|app, _: &Reveal, window, cx| {
                if !app.navigation.generator_open()
                    && app.confirming_delete.is_none()
                    && app.confirming_overwrite.is_none()
                    && !app.generator_name_focus.is_focused(window)
                    && !app.custom_entry_name_focus.is_focused(window)
                {
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
            .on_action(cx.listener(|app, _: &NextEntry, window, cx| {
                if app.custom_entry_name_focus.is_focused(window) {
                } else if app.generator_name_focus.is_focused(window) && app.text_editor_active() {
                    app.move_vertical(1, window, cx);
                } else {
                    app.navigate(1, cx);
                }
            }))
            .on_action(cx.listener(|app, _: &PreviousEntry, window, cx| {
                if app.custom_entry_name_focus.is_focused(window) {
                } else if app.generator_name_focus.is_focused(window) && app.text_editor_active() {
                    app.move_vertical(-1, window, cx);
                } else {
                    app.navigate(-1, cx);
                }
            }))
            .on_action(cx.listener(|app, _: &Backspace, window, cx| app.backspace(window, cx)))
            .on_action(cx.listener(|app, _: &Delete, window, cx| app.delete(window, cx)))
            .on_action(cx.listener(|app, _: &Left, window, cx| app.move_cursor(-1, window, cx)))
            .on_action(cx.listener(|app, _: &Right, window, cx| app.move_cursor(1, window, cx)))
            .on_action(cx.listener(|app, _: &SelectLeft, window, cx| {
                if app.custom_entry_name_focus.is_focused(window) {
                    app.select_custom_entry_name(-1, cx);
                } else if app.text_editor_active() && app.generator_name_focus.is_focused(window) {
                    app.select_edit_text(-1, false, cx);
                }
            }))
            .on_action(cx.listener(|app, _: &SelectRight, window, cx| {
                if app.custom_entry_name_focus.is_focused(window) {
                    app.select_custom_entry_name(1, cx);
                } else if app.text_editor_active() && app.generator_name_focus.is_focused(window) {
                    app.select_edit_text(1, false, cx);
                }
            }))
            .on_action(cx.listener(|app, _: &SelectUp, window, cx| {
                if app.text_editor_active() && app.generator_name_focus.is_focused(window) {
                    app.select_edit_text(-1, true, cx);
                }
            }))
            .on_action(cx.listener(|app, _: &SelectDown, window, cx| {
                if app.text_editor_active() && app.generator_name_focus.is_focused(window) {
                    app.select_edit_text(1, true, cx);
                }
            }))
            .on_action(cx.listener(|app, _: &Copy, window, cx| app.copy_edit_selection(window, cx)))
            .on_action(cx.listener(|app, _: &CopyEntry, window, cx| {
                if !app.navigation.generator_open()
                    && !app.editing_entry
                    && !app.generator_name_focus.is_focused(window)
                    && !app.custom_entry_name_focus.is_focused(window)
                    && app.confirming_delete.is_none()
                    && app.confirming_overwrite.is_none()
                {
                    app.copy_selected(cx);
                }
            }))
            .on_action(cx.listener(|app, _: &Cut, window, cx| app.cut_edit_selection(window, cx)))
            .on_action(cx.listener(|app, _: &Paste, window, cx| app.paste_edit_text(window, cx)))
            .on_action(cx.listener(|app, _: &Start, window, cx| app.move_edge(false, window, cx)))
            .on_action(cx.listener(|app, _: &End, window, cx| app.move_edge(true, window, cx)))
            .on_action(cx.listener(|app, _: &SelectAll, window, cx| {
                if app.custom_entry_name_focus.is_focused(window) {
                    app.custom_entry_name_selection = 0..app.custom_entry_name.len();
                    app.custom_entry_name_anchor = Some(0);
                    app.custom_entry_name_cursor = app.custom_entry_name.len();
                    cx.notify();
                } else if app.generator_name_focus.is_focused(window) {
                    app.generator_name_selection = 0..app.generator_name.len();
                    app.generator_name_anchor = Some(0);
                    app.generator_name_cursor = app.generator_name.len();
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
            .when(!self.navigation.generator_open(), |view| {
                view.child(
                    div()
                        .id("titlebar")
                        .w_full()
                        .h(px(48.))
                        .flex_shrink_0()
                        .px(px(9.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .bg(rgb(self.theme.colors.nav_background))
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
                                .id("home-logo")
                                .ml(px(17.))
                                .cursor_pointer()
                                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation()
                                })
                                .on_click(cx.listener(|app, _, window, cx| {
                                    app.device_verification.cancel();
                                    app.navigation.go_to(Page::Entries);
                                    app.search_focus.focus(window);
                                    cx.notify();
                                }))
                                .child(self.brand_logotype()),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(18.))
                                .child(
                                    div()
                                        .text_size(px(10.))
                                        .font_family(self.theme.mono_font)
                                        .text_color(rgb(self.theme.colors.nav_muted))
                                        .child(if self.demo { "DEMO / 01" } else { "LOCAL / 01" }),
                                )
                                .when(!self.demo && self.store.is_some(), |header| {
                                    header.child(
                                        div()
                                            .id("open-custom-entry")
                                            .text_size(px(12.))
                                            .text_color(rgb(self.theme.colors.nav_foreground))
                                            .cursor_pointer()
                                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                                cx.stop_propagation()
                                            })
                                            .on_click(cx.listener(|app, _, window, cx| {
                                                app.open_custom_entry(window, cx)
                                            }))
                                            .child(i18n::text("new-entry")),
                                    )
                                })
                                .child(
                                    div()
                                        .id("open-generator")
                                        .text_size(px(12.))
                                        .text_color(rgb(self.theme.colors.nav_foreground))
                                        .cursor_pointer()
                                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation();
                                        })
                                        .on_click(cx.listener(|app, _, window, cx| {
                                            app.open_generator(window, cx)
                                        }))
                                        .child(i18n::text("generate")),
                                )
                                .when(
                                    self.yubikey_connection == YubiConnection::Connected,
                                    |header| {
                                        header.child(
                                            div()
                                                .id("open-yubikey-status")
                                                .text_size(px(12.))
                                                .text_color(rgb(
                                                    if self.navigation.page() == Page::CardDetails {
                                                        self.theme.colors.nav_selected
                                                    } else {
                                                        self.theme.colors.nav_foreground
                                                    },
                                                ))
                                                .cursor_pointer()
                                                .on_mouse_down(
                                                    gpui::MouseButton::Left,
                                                    |_, _, cx| {
                                                        cx.stop_propagation();
                                                    },
                                                )
                                                .on_click(cx.listener(|app, _, _, cx| {
                                                    app.navigation.go_to(Page::CardDetails);
                                                    cx.notify();
                                                }))
                                                .child(i18n::text("card-status")),
                                        )
                                    },
                                )
                                .child(
                                    div()
                                        .id("refresh")
                                        .text_size(px(12.))
                                        .text_color(rgb(self.theme.colors.nav_foreground))
                                        .cursor_pointer()
                                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation();
                                        })
                                        .on_click(cx.listener(|app, _, _, cx| app.refresh(cx)))
                                        .child(i18n::text("refresh")),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(9.))
                                        .child(
                                            div()
                                                .id("minimize-window")
                                                .size(px(30.))
                                                .border_1()
                                                .border_color(rgb(self.theme.colors.nav_border))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .text_size(px(17.))
                                                .text_color(rgb(self.theme.colors.nav_foreground))
                                                .cursor_pointer()
                                                .hover(|style| {
                                                    style.bg(rgb(self.theme.colors.nav_hover))
                                                })
                                                .on_mouse_down(
                                                    gpui::MouseButton::Left,
                                                    cx.listener(|_, _, _, cx| {
                                                        cx.stop_propagation()
                                                    }),
                                                )
                                                .on_click(cx.listener(|_, _, window, _| {
                                                    window.minimize_window()
                                                }))
                                                .child("−"),
                                        )
                                        .child(
                                            div()
                                                .id("close-window")
                                                .size(px(30.))
                                                .border_1()
                                                .border_color(rgb(self.theme.colors.nav_border))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .text_size(px(20.))
                                                .text_color(rgb(self.theme.colors.nav_foreground))
                                                .cursor_pointer()
                                                .hover(|style| {
                                                    style.bg(rgb(self.theme.colors.danger_hover))
                                                })
                                                .on_mouse_down(
                                                    gpui::MouseButton::Left,
                                                    cx.listener(|_, _, _, cx| {
                                                        cx.stop_propagation()
                                                    }),
                                                )
                                                .on_click(cx.listener(|_, _, _, cx| cx.quit()))
                                                .child("×"),
                                        ),
                                ),
                        ),
                )
            })
            .child(main_content)
            .child(footer)
            .when(self.confirming_delete.is_some(), |view| {
                view.child(self.delete_confirmation_view(cx))
            })
            .when(self.confirming_overwrite.is_some(), |view| {
                view.child(self.overwrite_confirmation_view(cx))
            })
            .child(resize_handle(
                "resize-top",
                ResizeEdge::Top,
                CursorStyle::ResizeUpDown,
                ResizeHandleBounds {
                    top: Some(0.),
                    left: Some(8.),
                    right: Some(8.),
                    height: Some(8.),
                    ..Default::default()
                },
            ))
            .child(resize_handle(
                "resize-bottom",
                ResizeEdge::Bottom,
                CursorStyle::ResizeUpDown,
                ResizeHandleBounds {
                    left: Some(8.),
                    right: Some(8.),
                    bottom: Some(0.),
                    height: Some(8.),
                    ..Default::default()
                },
            ))
            .child(resize_handle(
                "resize-left",
                ResizeEdge::Left,
                CursorStyle::ResizeLeftRight,
                ResizeHandleBounds {
                    top: Some(8.),
                    left: Some(0.),
                    bottom: Some(8.),
                    width: Some(8.),
                    ..Default::default()
                },
            ))
            .child(resize_handle(
                "resize-right",
                ResizeEdge::Right,
                CursorStyle::ResizeLeftRight,
                ResizeHandleBounds {
                    top: Some(8.),
                    right: Some(0.),
                    bottom: Some(8.),
                    width: Some(8.),
                    ..Default::default()
                },
            ))
            .child(resize_handle(
                "resize-top-left",
                ResizeEdge::TopLeft,
                CursorStyle::ResizeUpLeftDownRight,
                ResizeHandleBounds {
                    top: Some(0.),
                    left: Some(0.),
                    width: Some(14.),
                    height: Some(14.),
                    ..Default::default()
                },
            ))
            .child(resize_handle(
                "resize-top-right",
                ResizeEdge::TopRight,
                CursorStyle::ResizeUpRightDownLeft,
                ResizeHandleBounds {
                    top: Some(0.),
                    right: Some(0.),
                    width: Some(14.),
                    height: Some(14.),
                    ..Default::default()
                },
            ))
            .child(resize_handle(
                "resize-bottom-left",
                ResizeEdge::BottomLeft,
                CursorStyle::ResizeUpRightDownLeft,
                ResizeHandleBounds {
                    left: Some(0.),
                    bottom: Some(0.),
                    width: Some(14.),
                    height: Some(14.),
                    ..Default::default()
                },
            ))
            .child(resize_handle(
                "resize-bottom-right",
                ResizeEdge::BottomRight,
                CursorStyle::ResizeUpLeftDownRight,
                ResizeHandleBounds {
                    right: Some(0.),
                    bottom: Some(0.),
                    width: Some(14.),
                    height: Some(14.),
                    ..Default::default()
                },
            ))
    }
}

fn notification_bar(theme: &Theme, message: impl Into<SharedString>) -> Div {
    div()
        .h(px(48.))
        .flex_shrink_0()
        .px(px(24.))
        .flex()
        .items_center()
        .border_t_1()
        .border_color(rgb(theme.colors.border))
        .bg(rgb(theme.colors.accent_surface))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(px(13.))
                .line_height(px(18.))
                .text_color(rgb(theme.colors.ink))
                .child(message.into()),
        )
}

#[derive(Default)]
struct ResizeHandleBounds {
    top: Option<f32>,
    left: Option<f32>,
    right: Option<f32>,
    bottom: Option<f32>,
    width: Option<f32>,
    height: Option<f32>,
}

fn resize_handle(
    id: &'static str,
    edge: ResizeEdge,
    cursor: CursorStyle,
    bounds: ResizeHandleBounds,
) -> gpui::Stateful<gpui::Div> {
    let mut handle = div().id(id).absolute().cursor(cursor).on_mouse_down(
        gpui::MouseButton::Left,
        move |_, window, cx| {
            window.start_window_resize(edge);
            cx.stop_propagation();
        },
    );
    if let Some(top) = bounds.top {
        handle = handle.top(px(top));
    }
    if let Some(left) = bounds.left {
        handle = handle.left(px(left));
    }
    if let Some(right) = bounds.right {
        handle = handle.right(px(right));
    }
    if let Some(bottom) = bounds.bottom {
        handle = handle.bottom(px(bottom));
    }
    if let Some(width) = bounds.width {
        handle = handle.w(px(width));
    }
    if let Some(height) = bounds.height {
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
    diagnostics::init();
    Application::new().run(move |cx: &mut App| {
        cx.text_system()
            .add_fonts(vec![Cow::Borrowed(
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../assets/fonts/InterVariable.ttf"
                ))
                .as_slice(),
            )])
            .expect("load bundled Inter font");
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
            KeyBinding::new("shift-left", SelectLeft, Some("Pass")),
            KeyBinding::new("shift-right", SelectRight, Some("Pass")),
            KeyBinding::new("shift-up", SelectUp, Some("Pass")),
            KeyBinding::new("shift-down", SelectDown, Some("Pass")),
            KeyBinding::new("ctrl-c", Copy, Some("Pass")),
            KeyBinding::new("ctrl-shift-c", CopyEntry, Some("Pass")),
            KeyBinding::new("ctrl-x", Cut, Some("Pass")),
            KeyBinding::new("ctrl-v", Paste, Some("Pass")),
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

#[cfg(test)]
mod card_detection_tests {
    use super::yubikey_detection::{
        ParsedCardStatus, check_card_status, parse_card_status, update_yubikey_usb_presence,
    };
    use super::*;

    #[test]
    fn dropping_a_copy_request_cancels_its_worker() {
        let cancellation = Cancellation::default();
        let (_, receiver) = mpsc::sync_channel(1);
        let pending = PendingCopy {
            entry: Entry::from_relative_file("test.gpg").unwrap(),
            progress: OperationProgress::new(OperationStatus::working("Copying", "Working")),
            cancellation: cancellation.clone(),
            receiver,
        };
        drop(pending);
        assert!(cancellation.is_cancelled());
    }

    #[test]
    fn multiple_card_records_are_not_merged() {
        let output = "Reader:First:\nversion:0304:\nvendor:0006:Yubico:\npinretry:3:0:3:\nReader:Second:\nversion:0200:\nvendor:1234:Other:\nsigcount:999:\n";
        let ParsedCardStatus::Detected(details) = parse_card_status(output) else {
            panic!("expected first card details");
        };
        assert_eq!(details.reader.as_deref(), Some("First"));
        assert_eq!(details.manufacturer.as_deref(), Some("Yubico"));
        assert_eq!(details.pin_retries, Some([3, 0, 3]));
        assert_eq!(details.signature_count, None);
    }

    #[cfg(unix)]
    #[test]
    fn status_query_requests_a_reader_rescan_and_handles_no_cards() {
        use std::{fs, os::unix::fs::PermissionsExt};
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "gpui-pass-card-probe-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let executable = root.join("gpg");
        let fixture = |body: &str| {
            fs::write(&executable, format!("#!/usr/bin/python3\nimport sys\nassert sys.argv[-2:] == ['--card-status', 'all']\n{body}\n")).unwrap();
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        };
        fixture("print('Reader:Fixture:\\nversion:0304:')");
        let result = check_card_status(&executable, 0);
        assert_eq!(result.connection, YubiConnection::Connected);
        assert_eq!(result.details.unwrap().reader.as_deref(), Some("Fixture"));
        fixture("sys.exit(0)");
        let result = check_card_status(&executable, 0);
        assert_eq!(result.connection, YubiConnection::NotDetected);
        assert!(result.details.is_none());
        fs::remove_dir_all(root).unwrap();
    }

    fn usb_event(action: &str, vendor: &str, interfaces: &str) -> Vec<String> {
        vec![
            format!("ACTION={action}"),
            "SUBSYSTEM=usb".into(),
            "DEVTYPE=usb_device".into(),
            "DEVPATH=/devices/pci0000:00/usb3/3-2".into(),
            format!("ID_VENDOR_ID={vendor}"),
            format!("ID_USB_INTERFACES={interfaces}"),
        ]
    }

    #[test]
    fn udev_detects_insert_remove_and_reinsert() {
        let mut devices = HashSet::new();
        let inserted = usb_event("add", "1050", ":030101:0b0000:");
        // Removal events need not include vendor/interface metadata.
        let removed = usb_event("remove", "", "");
        for event in [&inserted, &removed, &inserted] {
            let expected = event != &removed;
            assert_eq!(
                update_yubikey_usb_presence(event, &mut devices),
                Some(expected)
            );
            assert_eq!(devices.len(), usize::from(expected));
        }
    }

    #[test]
    fn udev_ignores_duplicates_and_unrelated_devices() {
        let mut devices = HashSet::new();
        assert_eq!(
            update_yubikey_usb_presence(&usb_event("add", "1234", ":0b0000:"), &mut devices),
            None
        );
        assert_eq!(
            update_yubikey_usb_presence(&usb_event("add", "1050", ":030101:"), &mut devices),
            None
        );
        let inserted = usb_event("add", "1050", ":0b0000:");
        assert_eq!(
            update_yubikey_usb_presence(&inserted, &mut devices),
            Some(true)
        );
        assert_eq!(update_yubikey_usb_presence(&inserted, &mut devices), None);
        let removed = usb_event("remove", "", "");
        assert_eq!(
            update_yubikey_usb_presence(&removed, &mut devices),
            Some(false)
        );
        assert_eq!(update_yubikey_usb_presence(&removed, &mut devices), None);
    }
}
