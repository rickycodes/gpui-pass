#![forbid(unsafe_code)]

mod device_verification;
use device_verification::{DeviceVerification, State as VerificationState};

mod diagnostics;
use diagnostics::trace;

mod card_refresh;
use card_refresh::{CardProbe, CardRefresh, ProbeResult};

mod navigation;
use navigation::{Navigation, Page};

mod process_output;
mod clipboard;
use clipboard::ClipboardState;

mod editor_input;
mod entry_editor;
mod search;
mod theme;

use gpui::{
    App, Application, Bounds, Context, CursorStyle, Div, FocusHandle, FontWeight,
    Image, ImageFormat,
    KeyBinding, ResizeEdge, ScrollStrategy, SharedString, Subscription, TitlebarOptions,
    UniformListScrollHandle, Window, WindowBounds, WindowOptions, actions, canvas, div, prelude::*,
    img, px, rgb, size, uniform_list,
};
use gpui_pass_core::{
    Cancellation, CardEvent, CardObservation, Entry, EntryRevision, Error, GeneratorOptions, Gpg, OperationProgress, OperationStatus,
    RevealRequest, Secret, Session, Store, REVEAL_DURATION, default_store_path, harden_process,
};
use std::{
    borrow::Cow,
    io::{BufRead, BufReader},
    collections::{HashSet, VecDeque},
    ops::Range,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Arc, LazyLock, Mutex, mpsc::{self, Receiver}},
    time::{Duration, Instant},
};
use theme::Theme;
use zeroize::{Zeroize, Zeroizing};

const NOTIFICATION_TIMEOUT: Duration = Duration::from_secs(6);

static YUBIKEY_5_NFC_IMAGE: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Webp,
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/yubikey/yubikey-5-nfc.webp")).to_vec(),
    ))
});
static YUBIKEY_5C_NFC_IMAGE: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Webp,
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/yubikey/yubikey-5c-nfc.webp")).to_vec(),
    ))
});
static YUBIKEY_5C_IMAGE: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Webp,
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/yubikey/yubikey-5c.webp")).to_vec(),
    ))
});
static YUBIKEY_5_NANO_IMAGE: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Webp,
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/yubikey/yubikey-5-nano.webp")).to_vec(),
    ))
});
static YUBIKEY_5C_NANO_IMAGE: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Webp,
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/yubikey/yubikey-5c-nano.webp")).to_vec(),
    ))
});
static YUBIKEY_5CI_IMAGE: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Webp,
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/yubikey/yubikey-5ci.webp")).to_vec(),
    ))
});

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

#[derive(Clone, Copy)]
struct YubiRecord {
    observation: CardObservation,
    operation: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum YubiConnection {
    Checking,
    Connected,
    NotDetected,
    NoReader,
    Busy,
    Unavailable,
}

#[derive(Clone, PartialEq, Eq)]
struct CardDetails {
    application: String,
    reader: Option<String>,
    version: Option<String>,
    manufacturer: Option<String>,
    key_attributes: Vec<String>,
    pin_retries: Option<[u32; 3]>,
    signature_count: Option<u64>,
}

struct CardProbeResult {
    connection: YubiConnection,
    details: Option<CardDetails>,
}

#[derive(Clone, Copy)]
enum CardWatchEvent {
    Ready,
    Status(Option<bool>),
    Failed,
}

struct CardWatcher {
    child: Arc<Mutex<Child>>,
    events: Receiver<CardWatchEvent>,
}

enum UsbCardEvent {
    Presence(bool),
    Failed,
}

struct UsbCardWatcher {
    child: Arc<Mutex<Child>>,
    events: Receiver<UsbCardEvent>,
}

impl Drop for CardWatcher {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
    }
}

impl Drop for UsbCardWatcher {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
    }
}

enum ParsedCardStatus {
    Detected(CardDetails),
    NotDetected,
    Unrecognized,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GeneratorMode {
    Memorable,
    PassDefault,
}

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
    fn drop(&mut self) { self.cancellation.cancel(); }
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
    navigation: Navigation,
    confirming_delete: Option<Entry>,
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
            navigation: Navigation::default(),
            confirming_delete: None,
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
            if let Some((cancel, _, _)) = &app.saving { cancel.cancel(); }
            app.restore_pass_clipboard(cx);
            async {}
        }).detach();
        cx.on_release(|app, cx| {
            app.copy_pending = None;
            if let Some((cancel, _, _)) = &app.saving { cancel.cancel(); }
            app.restore_pass_clipboard(cx);
        }).detach();
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
        if self.load.is_some() || self.reveal.is_some() || self.copy_pending.is_some() || self.saving.is_some() {
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

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.saving.is_some() { return; }
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
        if self.device_verification.poll(self.card_refresh.generation()) { cx.notify(); }
        let verification_state = self.device_verification.state();
        if verification_state != self.verification_state_seen {
            self.verification_state_seen = verification_state;
            self.verification_notification_until = if matches!(verification_state, VerificationState::Idle | VerificationState::Pending) {
                None
            } else {
                Some(Instant::now() + NOTIFICATION_TIMEOUT)
            };
            cx.notify();
        }
        if self.verification_notification_until.is_some_and(|until| Instant::now() >= until) {
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
        if let Some(result) = self.copy_pending.as_ref().and_then(|pending| pending.receiver.try_recv().ok()) {
            let pending = self.copy_pending.take().expect("copy request available");
            let entry = pending.entry.clone();
            if self.active && self.session.selected() == Some(&entry) {
                match result {
                    Ok(secret) => {
                        self.copy_to_clipboard(secret.expose().to_owned(), entry.name().to_owned(), cx);

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
                    self.entries.sort_by_cached_key(|entry| (entry.name().to_lowercase(), entry.name().to_owned()));
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
                    self.saved_entry_name = Some(name.clone());
                    if !self.close_after_save && self.active {
                        let text = secret.expose().to_owned();
                        self.copy_to_clipboard(text, name, cx);
                        self.generated = Some((secret, Instant::now() + REVEAL_DURATION));
                    }
                    self.clear_message();
                }
                Err(error) => {
                    self.saving_edit = false;
                    self.set_message(error.to_string());
                },
            }
            let close = self.close_after_save;
            self.close_after_save = false;
            if close {
                self.close_generator(cx);
            }
            cx.notify();
        }
        let status = self.session.pending_status().or_else(|| {
            self.copy_pending
                .as_ref()
                .map(|pending| pending.progress.snapshot())
        }).or_else(|| {
            self.saving
                .as_ref()
                .map(|(_, progress, _)| progress.snapshot())
        });
        if self.last_operation_status != status {
            self.last_operation_status = status;
            cx.notify();
        }
    }

    fn start_yubikey_check(&mut self) {
        // A refresh request is not a physical card change. Coalesce requests
        // without making the running probe's result stale.
        if self.yubikey_check.is_some() { return; }
        if self.yubikey_info_check.is_some() || self.reveal.is_some() || self.copy_pending.is_some() || self.saving.is_some() || self.device_verification.running() {
            if !self.card_refresh_pending {
                trace!("probe.deferred ykman={} reveal={} saving={}",
                    self.yubikey_info_check.is_some(), self.reveal.is_some(), self.saving.is_some());
            }
            self.card_refresh_pending = true;
            return;
        }
        // Keep the initial loading state, then leave the last result on screen while the
        // background probe runs. An absent key should not flash "Checking" on every retry.
        if self.yubikey_last_checked.is_none()
            && self.yubikey_connection != YubiConnection::Connected
        {
            self.yubikey_connection = YubiConnection::Checking;
        }
        let executable = self.gpg.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        self.card_refresh_pending = false;
        let probe = CardProbe::new("gpg", self.card_refresh.generation(), receiver);
        let probe_id = probe.id();
        self.yubikey_check = Some(probe);
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

    fn start_card_watcher(&mut self) {
        let gpg_parent = self.gpg.parent().unwrap_or_else(|| std::path::Path::new("/usr/bin"));
        let sibling = gpg_parent.join("gpg-connect-agent");
        let executable = if sibling.is_file() {
            sibling
        } else {
            PathBuf::from("gpg-connect-agent")
        };
        let Ok(mut child) = Command::new(executable)
            // stdout is a pipe, so the helper otherwise buffers device events
            // indefinitely while this long-running command waits for changes.
            .arg("--unbuffered")
            // gpg-agent recognizes this exact uppercase spelling and routes it to
            // a dedicated connection. Lowercase bypasses that special case and
            // leaves the watch on the ordinary smartcard command connection.
            .arg("SCD DEVINFO --watch")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        else {
            self.card_watcher_supported = Some(false);
            self.card_watcher_retry_at = Some(Instant::now() + Duration::from_secs(2));
            return;
        };
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            self.card_watcher_supported = Some(false);
            self.card_watcher_retry_at = Some(Instant::now() + Duration::from_secs(2));
            return;
        };

        let child = Arc::new(Mutex::new(child));
        let thread_child = child.clone();
        let (sender, events) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            let mut ready = false;
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => {
                        let _ = sender.send(CardWatchEvent::Failed);
                        break;
                    }
                    Ok(_) => {
                        if (line.contains("DEVINFO_START") || line.contains("DEVINFO_END")) && !ready {
                            ready = true;
                            let _ = sender.send(CardWatchEvent::Ready);
                        }
                        if line.contains("DEVINFO_STATUS") {
                            ready = true;
                            let _ = sender.send(CardWatchEvent::Ready);
                            let words = line.split_whitespace().collect::<Vec<_>>();
                            let present = words
                                .iter()
                                .position(|word| *word == "DEVINFO_STATUS")
                                .and_then(|index| words.get(index + 1))
                                .and_then(|status| match *status {
                                    "new" => Some(true),
                                    "removal" => Some(false),
                                    _ => None,
                                });
                            let _ = sender.send(CardWatchEvent::Status(present));
                        }
                        if line.starts_with("ERR ") {
                            let code = line.split_whitespace().nth(1).and_then(|value| value.parse::<u32>().ok());
                            trace!("watcher.error kind=gpg code={code:?}");
                            let _ = sender.send(CardWatchEvent::Failed);
                            break;
                        }
                    }
                }
            }
            if let Ok(mut child) = thread_child.lock() {
                // Stop the helper before waiting, including when it returned an
                // Assuan error but has not exited yet.
                let _ = child.kill();
                let _ = child.wait();
            }
        });
        self.card_watcher = Some(CardWatcher { child, events });
        self.card_watcher_retry_at = None;
    }

    #[cfg(target_os = "linux")]
    fn start_usb_card_watcher(&mut self, cx: &mut Context<Self>) {
        let Ok(mut child) = Command::new("udevadm")
            .args(["monitor", "--udev", "--property", "--subsystem-match=usb"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        else {
            self.usb_watcher_retry_at = Some(Instant::now() + Duration::from_secs(2));
            return;
        };
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            self.usb_watcher_retry_at = Some(Instant::now() + Duration::from_secs(2));
            return;
        };

        let child = Arc::new(Mutex::new(child));
        let thread_child = child.clone();
        let (sender, events) = mpsc::channel();
        let mut known_devices = connected_yubikey_usb_paths();
        let initially_present = !known_devices.is_empty();
        let previous_presence = self.usb_yubikey_present;
        if previous_presence != Some(initially_present)
            && (initially_present || previous_presence == Some(true))
        {
            self.apply_yubikey_usb_presence(initially_present, cx);
        }

        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            let mut event = Vec::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => {
                        let _ = sender.send(UsbCardEvent::Failed);
                        break;
                    }
                    Ok(_) if line.trim().is_empty() => {
                        if let Some(present) = update_yubikey_usb_presence(&event, &mut known_devices)
                        {
                            let _ = sender.send(UsbCardEvent::Presence(present));
                        }
                        event.clear();
                    }
                    Ok(_) => event.push(line.trim_end().to_owned()),
                }
            }
            if let Ok(mut child) = thread_child.lock() {
                let _ = child.kill();
                let _ = child.wait();
            }
        });
        self.usb_card_watcher = Some(UsbCardWatcher { child, events });
        self.usb_watcher_retry_at = None;
    }

    #[cfg(not(target_os = "linux"))]
    fn start_usb_card_watcher(&mut self, _cx: &mut Context<Self>) {}

    fn apply_card_presence_hint(&mut self, present: bool, cx: &mut Context<Self>) {
        let connection = if present {
            YubiConnection::Connected
        } else {
            YubiConnection::NotDetected
        };
        if self.usb_yubikey_present != Some(true) {
            self.card_refresh.observe(present);
        }
        if self.yubikey_connection != connection {
            self.yubikey_connection = connection;
            self.yubikey_details = None;
            if !present {
                self.device_verification.invalidate();
                self.navigation.card_unavailable();
            }
            cx.notify();
        }
        if self.yubikey_check.is_some()
            || self.yubikey_info_check.is_some()
            || self.reveal.is_some()
            || self.saving.is_some()
        {
            self.card_refresh_pending = true;
        } else {
            self.start_yubikey_check();
        }
    }

    fn start_yubikey_info_probe(&mut self) {
        if self.device_verification.running()
            || self.reveal.is_some() || self.copy_pending.is_some() || self.saving.is_some()
            || self.yubikey_info.is_some()
            || self.yubikey_info_check.is_some()
            || self.usb_yubikey_present != Some(true)
            || !self
                .usb_reader_details
                .as_ref()
                .is_some_and(|details| details.vendor_id == 0x1050)
        {
            return;
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        let probe = CardProbe::new("ykman-hid", self.card_refresh.generation(), receiver);
        let probe_id = probe.id();
        self.yubikey_info_check = Some(probe);
        std::thread::spawn(move || {
            let _ = sender.send(query_yubikey_info(probe_id));
        });
    }

    fn apply_yubikey_usb_presence(&mut self, present: bool, cx: &mut Context<Self>) {
        // sysfs and udev can report the same transition. Do not reset retry
        // state or start another probe for a duplicate event.
        if self.usb_yubikey_present == Some(present) {
            return;
        }
        self.usb_yubikey_present = Some(present);
        self.card_refresh.observe(present);
        trace!("card.presence source=usb present={present} generation={}", self.card_refresh.generation());
        if present {
            self.card_detail_retry_at = None;
            self.usb_reader_details = usb_reader_for_card(None, true);
        } else {
            self.device_verification.invalidate();
            self.usb_reader_details = None;
            self.yubikey_info = None;
            // Keep tracking the old worker until it stops touching the device.
            self.card_detail_retry_at = None;
        }
        self.apply_card_presence_hint(present, cx);
    }

    fn poll_yubikey_connection(&mut self, cx: &mut Context<Self>) {
        #[cfg(target_os = "linux")]
        {
            if self
                .usb_last_scan
                .is_none_or(|last| last.elapsed() >= Duration::from_millis(250))
            {
                self.usb_last_scan = Some(Instant::now());
                let present = !connected_yubikey_usb_paths().is_empty();
                let previous = self.usb_yubikey_present;
                if previous != Some(present) && (present || previous == Some(true)) {
                    self.apply_yubikey_usb_presence(present, cx);
                }
            }
        }
        let paused = self.reveal.is_some() || self.copy_pending.is_some() || self.saving.is_some() || self.device_verification.running();
        if paused != self.card_poll_paused {
            self.card_poll_paused = paused;
            trace!("card.poll paused={} reveal={} saving={} verification={} details={}", paused,
                self.reveal.is_some(), self.saving.is_some(), self.device_verification.running(), self.yubikey_details.is_some());
        }
        if paused { return; }
        let watch_events = self
            .card_watcher
            .as_ref()
            .map(|watcher| watcher.events.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        for event in watch_events {
            match event {
                CardWatchEvent::Ready => {
                    if self.card_watcher_supported != Some(true) { trace!("watcher.ready kind=gpg"); }
                    self.card_watcher_supported = Some(true);
                },
                CardWatchEvent::Status(present) => {
                    trace!("card.presence source=gpg present={present:?} usb_present={:?}", self.usb_yubikey_present);
                    self.card_watcher_supported = Some(true);
                    if let Some(present) = present {
                        // A live USB event is authoritative for a YubiKey's physical
                        // presence; scdaemon can report a stale removal during reconnect.
                        if present || self.usb_yubikey_present != Some(true) {
                            self.apply_card_presence_hint(present, cx);
                        }
                    }
                    if present.is_none() {
                        if self.yubikey_check.is_some() {
                            self.card_refresh_pending = true;
                        } else {
                            self.start_yubikey_check();
                        }
                    }
                }
                CardWatchEvent::Failed => {
                    trace!("watcher.failed kind=gpg");
                    self.card_watcher_supported = Some(false);
                    self.card_watcher = None;
                    self.card_watcher_retry_at =
                        Some(Instant::now() + Duration::from_secs(2));
                }
            }
        }
        let usb_events = self
            .usb_card_watcher
            .as_ref()
            .map(|watcher| watcher.events.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        for event in usb_events {
            match event {
                UsbCardEvent::Presence(present) => {
                    self.apply_yubikey_usb_presence(present, cx);
                }
                UsbCardEvent::Failed => {
                    trace!("watcher.failed kind=usb");
                    self.usb_card_watcher = None;
                    self.usb_watcher_retry_at =
                        Some(Instant::now() + Duration::from_secs(2));
                }
            }
        }
        if let Some(completion) = self.yubikey_info_check.as_ref()
            .and_then(|probe| probe.poll(&self.card_refresh))
        {
            self.yubikey_info_check = None;
            if let ProbeResult::Current(info) = completion {
                if self.usb_yubikey_present == Some(true)
                    && self.yubikey_info.as_ref() != Some(&info)
                {
                    self.yubikey_info = Some(info);
                    cx.notify();
                }
            }
        }
        if let Some(completion) = self.yubikey_check.as_ref()
            .and_then(|probe| probe.poll(&self.card_refresh))
        {
            self.yubikey_check = None;
            self.yubikey_last_checked = Some(Instant::now());
            self.card_refresh_pending = false;
            let result = match completion {
                ProbeResult::Current(result) => Some(result),
                ProbeResult::Stale | ProbeResult::Disconnected => None,
            };
            if let Some(result) = result {
                self.apply_card_probe(result, cx);
            } else {
                trace!("[card-detect] Discarding obsolete or disconnected probe");
                self.start_yubikey_check();
            }
        }
        if self.card_watcher.is_none()
            && self
                .card_watcher_retry_at
                .is_some_and(|retry_at| retry_at <= Instant::now())
        {
            self.start_card_watcher();
        }
        if self.usb_card_watcher.is_none()
            && self
                .usb_watcher_retry_at
                .is_some_and(|retry_at| retry_at <= Instant::now())
        {
            self.start_usb_card_watcher(cx);
        }
        if self.yubikey_check.is_none()
            && self.yubikey_info_check.is_none()
            && self.card_refresh_pending
        {
            self.card_refresh_pending = false;
            self.start_yubikey_check();
        }
        if self.usb_yubikey_present == Some(true)
            && self.yubikey_details.is_none()
            && self.yubikey_check.is_none()
            && self
                .card_detail_retry_at
                .is_some_and(|retry_at| retry_at <= Instant::now())
        {
            self.card_detail_retry_at = None;
            self.start_yubikey_check();
        }
        if self.card_watcher_supported != Some(true)
            && self.yubikey_check.is_none()
            && self.yubikey_info_check.is_none()
            && !(self.usb_card_watcher.is_some() && self.usb_yubikey_present == Some(true))
            && self
                .yubikey_last_checked
                .is_none_or(|last| last.elapsed() >= Duration::from_secs(3))
        {
            self.start_yubikey_check();
            cx.notify();
        }
    }

    fn apply_card_probe(&mut self, result: CardProbeResult, cx: &mut Context<Self>) {
        trace!("[card-detect] Applying current probe: state={:?}, details={}",
            result.connection, result.details.is_some());
        if self.usb_yubikey_present == Some(true)
            && (result.connection != YubiConnection::Connected || result.details.is_none())
        {
            // A miss during enumeration does not invalidate physical presence or
            // details already obtained for this same insertion.
            self.yubikey_connection = YubiConnection::Connected;
            if self.yubikey_details.is_none() {
                let delay = self.card_refresh.retry_delay();
                self.card_detail_retry_at = Some(Instant::now() + delay);
                trace!("[card-detect] Card details not ready; retrying in {}ms", delay.as_millis());
            }
            cx.notify();
            return;
        }
        // Busy is inconclusive, so keep any already connected card on screen.
        if result.connection == YubiConnection::Busy {
            if self.yubikey_connection != YubiConnection::Connected {
                self.yubikey_connection = YubiConnection::Busy;
                self.yubikey_details = None;
                cx.notify();
            }
            return;
        }
        let changed = self.yubikey_connection != result.connection
            || self.yubikey_details != result.details;
        self.yubikey_connection = result.connection;
        self.yubikey_details = result.details;
        if self.yubikey_connection != YubiConnection::Connected {
            self.device_verification.invalidate();
            self.navigation.card_unavailable();
        } else if self.yubikey_details.is_some() {
            self.card_detail_retry_at = None;
            self.card_refresh.completed();
            self.start_yubikey_info_probe();
        }
        if changed { cx.notify(); }
    }

    fn capture_yubikey_status(&mut self, cx: &mut Context<Self>) {
        let observation = self
            .reveal
            .as_ref()
            .and_then(|(request, _)| {
                request
                    .progress
                    .card_observation()
                    .map(|observation| (observation, "Reveal entry"))
            })
            .or_else(|| {
                self.copy_pending.as_ref().and_then(|pending| {
                    pending.progress.card_observation().map(|observation| (observation, "Copy entry"))
                })
            })
            .or_else(|| {
                self.saving.as_ref().and_then(|(_, progress, _)| {
                    progress.card_observation().map(|observation| {
                        let operation = if self.saving_edit {
                            "Save edited entry"
                        } else {
                            "Save generated entry"
                        };
                        (observation, operation)
                    })
                })
            });
        let Some((observation, operation)) = observation else {
            return;
        };
        if self
            .last_yubikey_observation
            .is_some_and(|last| observation.observed_at <= last)
        {
            return;
        }
        self.last_yubikey_observation = Some(observation.observed_at);
        self.yubikey_history.push_front(YubiRecord {
            observation,
            operation,
        });
        self.yubikey_history.truncate(12);
        cx.notify();
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
        if self.saving.is_some() { return; }
        self.cancel_edit(cx);
        self.confirming_delete = None;
        self.session.hide();
        self.copy_pending = None;
        self.reveal = None;
        self.clear_generated(cx);
        self.clear_message();
        let terms: Vec<_> = self.query.split_whitespace().map(str::to_lowercase).collect();
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
        if self.saving.is_some() { return; }
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

    fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        if !self.editing_entry { return; }
        if self.saving_edit {
            if let Some((cancel, _, _)) = &self.saving {
                cancel.cancel();
            }
        }
        self.editing_entry = false;
        self.edit_revision = None;
        self.generator_name.zeroize();
        self.generator_name_selection = 0..0;
        self.generator_name_anchor = None;
        self.generator_name_cursor = 0;
        self.generator_name_marked = None;
        self.editor_layout = None;
        cx.notify();
    }

    fn request_delete(&mut self, cx: &mut Context<Self>) {
        if self.saving.is_some() { return; }
        if let Some(entry) = self.session.selected().cloned() {
            self.session.hide();
            self.reveal = None;
            self.clear_generated(cx);
            self.confirming_delete = Some(entry);
            cx.notify();
        }
    }

    fn begin_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving.is_some() { return; }
        let Some(secret) = self.session.revealed(Instant::now()) else {
            self.set_message("Reveal the entry before editing it.".into());
            cx.notify();
            return;
        };
        if !editor_input::supported(secret.expose()) {
            self.set_message(editor_input::EDIT_LIMIT_MESSAGE.into());
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
        self.generator_name_bounds = None;
        self.editor_layout = None;
        self.generator_name_focus.focus(window);
        cx.notify();
    }

    fn save_edit(&mut self, cx: &mut Context<Self>) {
        if self.saving.is_some() { return; }
        let Some(entry) = self.session.selected().cloned() else { return; };
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
        let secret = match Secret::from_bytes(Zeroizing::new(self.generator_name.as_bytes().to_vec())) {
            Ok(secret) => secret,
            Err(error) => { self.set_message(error.to_string()); cx.notify(); return; }
        };
        let gpg = match Gpg::new(&self.gpg) {
            Ok(gpg) => gpg,
            Err(error) => { self.set_message(error.to_string()); cx.notify(); return; }
        };
        let cancellation = Cancellation::default();
        let progress = OperationProgress::new(OperationStatus::working(
            "Preparing entry update", "The updated content stays in memory until GnuPG encrypts it."));
        let worker_cancel = cancellation.clone();
        let worker_progress = progress.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let result = gpg.update_entry_if_unchanged(&store, &entry, &secret, &revision, &worker_cancel, &worker_progress)
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
        if self.saving.is_some() { return; }
        self.cancel_edit(cx);
        self.device_verification.cancel();
        self.copy_pending = None;
        self.session.hide();
        self.reveal = None;
        self.clear_generated(cx);
        self.generator_name_focus.focus(window);
        self.generator_name_selection = self.generator_name.len()..self.generator_name.len();
        self.navigation.open_generator();
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
        self.navigation.close_generator();
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
        self.saving_edit = false;
        self.saving = Some((cancellation, progress, receiver));
        cx.notify();
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
                    .bg(rgb(self.theme.colors.border)),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(11.))
                    .w(px(slider_left))
                    .h(px(3.))
                    .bg(rgb(self.theme.colors.accent)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(slider_left - 7.))
                    .top(px(5.))
                    .size(px(15.))
                    .border_1()
                    .border_color(rgb(self.theme.colors.accent))
                    .bg(rgb(self.theme.colors.surface)),
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
                    .font_family(self.theme.mono_font)
                    .text_size(px(9.))
                    .text_color(rgb(self.theme.colors.muted))
                    .child("4 WORDS")
                    .child("8 WORDS"),
            );

        let mut panel = self.theme.panel()
            .w(px(640.))
            .max_w(px(640.))
            .p(px(36.))
            .flex()
            .flex_shrink_0()
            .flex_col()
            .gap(px(24.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(22.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(self.theme.colors.ink))
                            .child("Password generator"),
                    )
                    .child(
                        self.theme.button("close-generator", "← Back", false)
                            .on_click(cx.listener(|app, _, _, cx| app.close_generator(cx))),
                    ),
            )
            .child(self.theme.label("ENTRY NAME"))
            .child(self.generator_name_field(window, cx))
            .child(
                div().flex().items_center().gap(px(8.))
                    .child(self.theme.button("mode-pass-default", "Pass default", self.generator_mode == GeneratorMode::PassDefault)
                        .on_click(cx.listener(|app, _, _, cx| { app.generator_mode = GeneratorMode::PassDefault; cx.notify(); })))
                    .child(self.theme.button("mode-memorable", "Memorable phrase", self.generator_mode == GeneratorMode::Memorable)
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
                            .child(self.theme.label("PASSWORD GENERATOR"))
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
                    .text_color(rgb(self.theme.colors.muted))
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
                            .font_family(self.theme.mono_font)
                            .text_size(px(11.))
                            .text_color(rgb(self.theme.colors.accent))
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
                            .text_color(rgb(self.theme.colors.muted))
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
                            .text_color(rgb(self.theme.colors.muted))
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
                    .text_color(rgb(self.theme.colors.muted))
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
                    .border_color(rgb(self.theme.colors.border))
                    .bg(rgb(self.theme.colors.accent_surface))
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
                            .font_family(self.theme.mono_font)
                            .text_size(px(16.))
                            .child(if self.generated_visible {
                                secret.expose().to_owned()
                            } else {
                                "*".repeat(secret.expose().chars().count())
                            }),
                    )
                    .child(
                        self.theme.button(
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
                            .font_family(self.theme.mono_font)
                            .text_size(px(9.))
                            .text_color(rgb(self.theme.colors.muted))
                            .child(format!("HIDES IN {remaining:02}s")),
                    ),
            );
            panel = panel.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        self.theme.button(
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
                                let source = app.saved_entry_name.clone().unwrap_or_else(|| "generated password".into());
                                app.copy_to_clipboard(text, source, cx);
                                cx.notify();
                            }
                        })),
                    )
                    .child(div().text_size(px(11.)).text_color(rgb(self.theme.colors.muted)).child(
                        if self.saved_entry_name.is_some() {
                            "Saved and copied."
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
                    .border_color(rgb(self.theme.colors.border))
                    .bg(rgb(self.theme.colors.background))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(12.))
                    .text_color(rgb(self.theme.colors.muted))
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
                        .text_color(rgb(self.theme.colors.muted))
                        .child("New entries never overwrite existing entries."),
                )
                .child(
                    self.theme.button("generate-phrase", if self.saving.is_some() { "Saving…" } else { "Generate & Save" }, true)
                        .on_click(cx.listener(|app, _, _, cx| app.save_generated(cx))),
                ),
        );

        div()
            .id("generator-window")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(rgb(self.theme.colors.background))
            .child(
                div()
                    .id("generator-titlebar")
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
                            .id("generator-home-logo")
                            .ml(px(17.))
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            .on_click(cx.listener(|app, _, window, cx| {
                                app.device_verification.cancel();
                                app.navigation.go_to(Page::Entries);
                                app.close_generator(cx);
                                app.search_focus.focus(window);
                                cx.notify();
                            }))
                            .child(self.brand_logotype()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(9.))
                            .child(
                                div()
                                    .id("generator-minimize-window")
                                    .size(px(30.))
                                    .border_1()
                                    .border_color(rgb(self.theme.colors.nav_border))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(17.))
                                    .text_color(rgb(self.theme.colors.nav_foreground))
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(self.theme.colors.nav_hover)))
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                                    )
                                    .on_click(cx.listener(|_, _, window, _| window.minimize_window()))
                                    .child("−"),
                            )
                            .child(
                                div()
                                    .id("generator-close-window")
                                    .size(px(30.))
                                    .border_1()
                                    .border_color(rgb(self.theme.colors.nav_border))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(20.))
                                    .text_color(rgb(self.theme.colors.nav_foreground))
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(self.theme.colors.danger_hover)))
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                                    )
                                    .on_click(cx.listener(|_, _, _, cx| cx.quit()))
                                    .child("×"),
                            ),
                    ),
            )
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

    fn clear_generated(&mut self, _cx: &mut Context<Self>) {
        self.generated = None;
        self.generated_visible = false;
    }

    fn copy_to_clipboard(&mut self, content: String, source: String, cx: &mut Context<Self>) {
        let timeout = std::env::var("PASSWORD_STORE_CLIP_TIME").ok();
        let deadline = clipboard::clipboard_deadline(Instant::now(), timeout.as_deref());
        let item = self.clipboard.copy(content, source, cx.read_from_clipboard(), deadline);
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
        if self.copy_pending.is_some() || self.device_verification.running() || self.session.is_pending() || !window.is_window_active() {
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
                let _ = sender.send(gpg.decrypt_with_revision(&store, &entry, &cancel, &progress).map(|(secret, revision)| (secret, Some(revision))));
            });
            receiver
        };
        self.reveal = Some((request, result));
        cx.notify();
    }

    fn copy_selected(&mut self, cx: &mut Context<Self>) {
        if self.copy_pending.is_some() || self.reveal.is_some() || self.saving.is_some() || self.device_verification.running() { return; }
        let Some(entry) = self.session.selected().cloned() else { return; };
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
            std::thread::spawn(move || { let _ = sender.send(gpg.decrypt_with_progress(&store, &worker_entry, &worker_cancel, &worker_progress)); });
            self.copy_pending = Some(PendingCopy { entry, progress, cancellation, receiver });
            cx.notify();
            return;
        } else {
            self.set_message("Open a password store before copying an entry.".into());
            return;
        }
        self.copy_pending = Some(PendingCopy { entry, progress, cancellation, receiver });
        cx.notify();
    }

    fn open_store(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving.is_some() { return; }
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
                        if app.saving.is_some() { return; }
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
            .border_color(rgb(self.theme.colors.border))
            .border_l_2()
            .border_color(rgb(if active { self.theme.colors.accent } else { self.theme.colors.border }))
            .bg(rgb(if active { self.theme.colors.accent_surface } else { self.theme.colors.background }))
            .cursor_pointer()
            .hover(|style| style.bg(rgb(self.theme.colors.hover)))
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
                    .text_color(rgb(self.theme.colors.muted))
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
            .border_color(rgb(self.theme.colors.border))
            .bg(rgb(self.theme.colors.background))
            .child(
                div()
                    .px(px(22.))
                    .pt(px(26.))
                    .pb(px(18.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(self.theme.label("YOUR ENTRIES"))
                    .child(
                        div()
                            .font_family(self.theme.mono_font)
                            .text_size(px(11.))
                            .text_color(rgb(self.theme.colors.muted))
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
                                .text_color(rgb(self.theme.colors.muted))
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
                    .border_color(rgb(self.theme.colors.border))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(self.theme.label(if self.demo {
                        "DEMO STORE"
                    } else {
                        "CURRENT STORE"
                    }))
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_family(self.theme.mono_font)
                            .text_color(rgb(self.theme.colors.muted))
                            .overflow_hidden()
                            .child(self.path_label()),
                    )
                    .child(
                        div()
                            .id("open-store")
                            .mt(px(8.))
                            .text_size(px(12.))
                            .text_color(rgb(self.theme.colors.accent))
                            .cursor_pointer()
                            .on_click(cx.listener(|app, _, window, cx| app.open_store(window, cx)))
                            .child("Open another store  ↗"),
                    ),
            )
            .into_any_element()
    }

    fn verify_device(&mut self, cx: &mut Context<Self>) {
        if self.device_verification.running() { return; }
        self.clear_message();
        self.verification_state_seen = VerificationState::Idle;
        self.verification_notification_until = None;
        if self.reveal.is_some() || self.copy_pending.is_some() || self.saving.is_some() || self.yubikey_info_check.is_some() || self.yubikey_check.is_some() {
            self.device_verification.busy();
        } else if let Some(usb) = self.usb_reader_details.as_ref().filter(|usb| usb.vendor_id == 0x1050) {
            self.device_verification.start(self.card_refresh.generation(), usb.product_id);
        } else {
            self.device_verification.unavailable();
        }
        cx.notify();
    }

    fn leave_card_status(&mut self, cx: &mut Context<Self>) {
        self.device_verification.cancel();
        self.verification_state_seen = self.device_verification.state();
        self.verification_notification_until = None;
        self.navigation.go_to(Page::Entries);
        cx.notify();
    }

    fn yubikey_view(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let latest = self.yubikey_history.front().copied();
        let is_yubikey = self.usb_reader_details.as_ref()
            .is_some_and(|usb| usb.vendor_id == 0x1050)
            || self.yubikey_info.is_some()
            || self.yubikey_details.as_ref()
                .and_then(|details| details.manufacturer.as_ref())
                .is_some_and(|manufacturer| manufacturer.eq_ignore_ascii_case("Yubico"));
        let mut page = div()
            .id("yubikey-status-page")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(54.))
            .py(px(42.))
            .flex()
            .flex_col()
            .gap(px(26.))
            .bg(rgb(self.theme.colors.surface))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(32.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("Smartcard status"),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .when(is_yubikey, |actions| actions.child(
                                self.theme.button("verify-device", if self.device_verification.running() { "Verifying…" } else { "Verify" }, true)
                                    .on_click(cx.listener(|app, _, _, cx| app.verify_device(cx))),
                            ))
                            .when(self.device_verification.running(), |actions| actions.child(
                                self.theme.button("cancel-verification", "Cancel", false).on_click(cx.listener(|app, _, _, cx| {
                                    app.device_verification.cancel();
                                    cx.notify();
                                })),
                            ))
                            .child(self.theme.button("yubikey-back", "← Back", false).on_click(
                                cx.listener(|app, _, _, cx| app.leave_card_status(cx)),
                            )),
                    ),
            );

        if let Some(info) = &self.yubikey_info
            && let (Some(device_type), Some(image)) = (
                info.device_type.as_deref(),
                yubikey_product_image(info.device_type.as_deref()),
            )
        {
            let firmware = info
                .firmware_version
                .as_deref()
                .map(|version| format!("Firmware {version}"))
                .unwrap_or_else(|| "Connected to this computer".to_owned());
            page = page.child(
                div()
                    .w_full()
                    .max_w(px(760.))
                    .p(px(20.))
                    .border_1()
                    .border_color(rgb(self.theme.colors.border))
                    .rounded(px(self.theme.radii.panel))
                    .bg(rgb(self.theme.colors.background))
                    .flex()
                    .items_center()
                    .gap(px(28.))
                    .child(
                        div()
                            .size(px(190.))
                            .rounded(px(10.))
                            .bg(rgb(0xf2f0eb))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(img(image).size(px(176.))),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(10.))
                            .child(self.theme.label("CONNECTED DEVICE"))
                            .child(
                                div()
                                    .text_size(px(24.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(self.theme.colors.ink))
                                    .child(device_type.to_owned()),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgb(self.theme.colors.muted))
                                    .child(firmware),
                            ),
                    ),
            );
        }

        let mut rows = div()
            .max_w(px(760.))
            .flex()
            .flex_col()
            .gap(px(1.));
        let mut has_details = false;
        if let Some(usb) = &self.usb_reader_details {
            rows = rows
                .child(yubi_info_row(&self.theme,
                    "USB device",
                    format!("{} · {:04x}:{:04x}", usb.product, usb.vendor_id, usb.product_id),
                ))
                .child(yubi_info_row(&self.theme, "USB location", usb.location.clone()));
            has_details = true;
        }
        if let Some(info) = &self.yubikey_info {
            if let Some(device_type) = &info.device_type {
                rows = rows.child(yubi_info_row(&self.theme, "Device", device_type.clone()));
                has_details = true;
            }
            if let Some(form_factor) = &info.form_factor {
                rows = rows.child(yubi_info_row(&self.theme, "Form factor", form_factor.clone()));
                has_details = true;
            }
            if let Some(firmware_version) = &info.firmware_version {
                rows = rows.child(yubi_info_row(&self.theme, "Firmware", firmware_version.clone()));
                has_details = true;
            }
        }
        if let Some(details) = &self.yubikey_details {
            rows = rows.child(yubi_info_row(&self.theme, "Application", details.application.clone()));
            has_details = true;
            if let Some(reader) = &details.reader {
                rows = rows.child(yubi_info_row(&self.theme, "Reader", reader.clone()));
            }
            if let Some(version) = &details.version {
                rows = rows.child(yubi_info_row(&self.theme, "Card version", version.clone()));
            }
            if let Some(manufacturer) = &details.manufacturer {
                rows = rows.child(yubi_info_row(&self.theme, "Manufacturer", manufacturer.clone()));
            }
            if !details.key_attributes.is_empty() {
                rows = rows.child(yubi_info_row(&self.theme,
                    "Key algorithms",
                    details.key_attributes.join(" · "),
                ));
            }
            if let Some(retries) = details.pin_retries {
                rows = rows.child(yubi_info_row(&self.theme,
                    "PIN retries",
                    format!("{} · {} · {}", retries[0], retries[1], retries[2]),
                ));
            }
            if let Some(count) = details.signature_count {
                rows = rows.child(yubi_info_row(&self.theme, "Signature count", count.to_string()));
            }
        }
        if has_details {
            page = page.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(self.theme.label("CARD DETAILS"))
                    .child(rows),
            );
        }
        if self.yubikey_connection == YubiConnection::Connected
            && self.yubikey_details.is_none()
        {
            page = page.child(
                div()
                    .max_w(px(760.))
                    .text_size(px(12.))
                    .text_color(rgb(self.theme.colors.muted))
                    .child(if self.yubikey_check.is_some()
                        || self.card_detail_retry_at.is_some()
                    {
                        "Loading card details from GnuPG…"
                    } else {
                        "GnuPG has not reported additional card details."
                    }),
            );
        }

        if let Some(record) = latest {
            page = page.child(
                div()
                    .max_w(px(760.))
                    .text_size(px(11.))
                    .text_color(rgb(self.theme.colors.muted))
                    .child(format!(
                        "Recent activity: {} · {}",
                        card_event_title(record.observation.event),
                        record.operation
                    )),
            );
        }

        page.into_any_element()
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

    fn details(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
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
            .bg(rgb(self.theme.colors.surface));
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
                        .child(self.theme.label(format!("STORE   /   {}", folder.to_uppercase())))
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
                                        .border_color(rgb(self.theme.colors.border))
                                        .px(px(10.))
                                        .py(px(6.))
                                        .font_family(self.theme.mono_font)
                                        .text_size(px(10.))
                                        .text_color(rgb(self.theme.colors.muted))
                                        .child(".gpg"),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(rgb(self.theme.colors.muted))
                                .child(entry.name().to_owned()),
                        ),
                )
                .child(
                    div()
                        .px(px(46.))
                        .pb(px(24.))
                        .flex()
                        .items_center()
                        .border_b_1()
                        .border_color(rgb(self.theme.colors.border))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(7.))
                                .child(div().size(px(5.)).bg(rgb(self.theme.colors.accent)))
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(rgb(self.theme.colors.muted))
                                        .child("Encrypted on disk"),
                                ),
                        )
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
                    .child(self.theme.label("PASSWORD"))
                    .child(
                        div()
                            .font_family(self.theme.mono_font)
                            .text_size(px(10.))
                            .text_color(rgb(if revealed.is_some() { self.theme.colors.accent } else { self.theme.colors.muted }))
                            .child(if revealed.is_some() {
                                format!("HIDES IN {:02}s", self.seconds)
                            } else if pending {
                                "UNLOCKING".into()
                            } else {
                                "HIDDEN".into()
                            }),
                    ),
            );
            let mut password = self.theme.panel()
                .bg(rgb(self.theme.colors.background))
                .w_full()
                .min_w_0()
                .min_h(px(84.));
            if self.editing_entry {
                password = password.child(self.generator_name_field(window, cx));
            } else if let Some(secret) = revealed {
                let text = secret.expose();
                password = password.child(
                    div()
                        .id("password-text")
                        .font_family(self.theme.mono_font)
                        .text_size(px(18.))
                        .line_height(px(26.))
                        .child(text.chars().take(16_384).collect::<String>()),
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
                                .text_color(rgb(self.theme.colors.placeholder))
                                .child("••••••••••••"),
                        )
                        .child(
                            div()
                                .font_family(self.theme.mono_font)
                                .text_size(px(10.))
                                .text_color(rgb(self.theme.colors.muted))
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
                    self.theme.button(
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
                .when(self.editing_entry, |controls| controls
                    .child(self.theme.button("save-entry-edit", if self.saving_edit { "Saving…" } else { "Save changes" }, true)
                        .on_click(cx.listener(|app, _, _, cx| app.save_edit(cx))))
                    .child(self.theme.button("cancel-entry-edit", "Cancel", false)
                        .on_click(cx.listener(|app, _, _, cx| app.cancel_edit(cx)))))
                .when(!self.editing_entry, |controls| controls
                    .child(self.theme.button("edit-entry", "Edit entry", true)
                        .on_click(cx.listener(|app, _, window, cx| app.begin_edit(window, cx))))
                    .child(self.theme.button("copy-password", if self.copy_pending.is_some() { "Copying…" } else { "Copy entry" }, false)
                        .on_click(cx.listener(|app, _, _, cx| app.copy_selected(cx))))
                    .child(
                        self.theme.danger_button("delete-entry", "Delete")
                            .on_click(cx.listener(|app, _, _, cx| app.request_delete(cx))),
                    ))
                .when(pending, |controls| {
                    controls.child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(self.theme.colors.muted))
                            .child("Esc to cancel"),
                    )
                });
            body = body.child(controls);
            if let Some(secret) = revealed {
                if secret.expose().len() > 16_384 {
                    body = body.child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(self.theme.colors.muted))
                            .child("Long content is truncated in this preview."),
                    );
                }
            }
            detail = detail.child(body);
        } else {
            detail = detail.child(div().flex_1().flex().flex_col().justify_center().px(px(60.)).gap(px(18.))
                .child(div().text_size(px(56.)).font_weight(FontWeight::LIGHT).text_color(rgb(self.theme.colors.accent)).child("[ / ]"))
                .child(div().text_size(px(30.)).child(if self.load.is_some() { "Opening your store." } else if !self.query.is_empty() { "Nothing here. Yet." } else { "Your passwords, at home." }))
                .child(div().max_w(px(380.)).text_size(px(14.)).line_height(px(24.)).text_color(rgb(self.theme.colors.muted))
                    .child(if !self.query.is_empty() { "Search by an entry name or folder." } else { "Open an existing pass store to browse your entries. Your keys stay with GnuPG." }))
                .child(self.theme.button("empty-open-store", "Open password store  ↗", true).w(px(208.)).on_click(cx.listener(|app, _, window, cx| app.open_store(window, cx))))
);
        }
        detail
            .child(
                div()
                    .px(px(46.))
                    .py(px(20.))
                    .border_t_1()
                    .border_color(rgb(self.theme.colors.border))
                    .flex()
                    .justify_between()
                    .child(self.theme.label(if self.demo { "FICTIONAL DATA" } else { "LOCAL STORE" })),
            )
            .into_any_element()
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
        let status = self.session.pending_status()
            .or_else(|| self.copy_pending.as_ref().map(|pending| pending.progress.snapshot()))
            .or_else(|| self.saving.as_ref().map(|(_, progress, _)| progress.snapshot()));
        let footer = if let Some(message) = &self.message {
            notification_bar(&self.theme, message.clone())
        } else if let Some(status) = status {
            notification_bar(&self.theme, format!("{} · {}", status.title, status.detail))
        } else if self.device_verification.running() || self.verification_notification_until.is_some() {
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
                        .child(format!("Copied {entry_name}"))
                        .child(div().text_color(rgb(self.theme.colors.muted)).child(format!("Clipboard restores in {seconds}s"))),
                )
                .child(
                    self.theme.button("restore-clipboard-now", "Restore now", false)
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
                        "Ctrl F  Search     ↑ ↓  Browse     Enter  Reveal     Ctrl G  Generate     Esc  Hide",
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
            .on_action(cx.listener(|app, _: &OpenGenerator, window, cx| {
                app.open_generator(window, cx)
            }))
            .on_action(cx.listener(|app, _: &Reveal, window, cx| {
                if !app.navigation.generator_open()
                    && app.confirming_delete.is_none()
                    && !app.generator_name_focus.is_focused(window)
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
                if app.generator_name_focus.is_focused(window) && app.editing_entry {
                    app.move_vertical(1, window, cx);
                } else {
                    app.navigate(1, cx);
                }
            }))
            .on_action(cx.listener(|app, _: &PreviousEntry, window, cx| {
                if app.generator_name_focus.is_focused(window) && app.editing_entry {
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
                if app.editing_entry && app.generator_name_focus.is_focused(window) {
                    app.select_edit_text(-1, false, cx);
                }
            }))
            .on_action(cx.listener(|app, _: &SelectRight, window, cx| {
                if app.editing_entry && app.generator_name_focus.is_focused(window) {
                    app.select_edit_text(1, false, cx);
                }
            }))
            .on_action(cx.listener(|app, _: &SelectUp, window, cx| {
                if app.editing_entry && app.generator_name_focus.is_focused(window) {
                    app.select_edit_text(-1, true, cx);
                }
            }))
            .on_action(cx.listener(|app, _: &SelectDown, window, cx| {
                if app.editing_entry && app.generator_name_focus.is_focused(window) {
                    app.select_edit_text(1, true, cx);
                }
            }))
            .on_action(cx.listener(|app, _: &Copy, window, cx| app.copy_edit_selection(window, cx)))
            .on_action(cx.listener(|app, _: &Cut, window, cx| app.cut_edit_selection(window, cx)))
            .on_action(cx.listener(|app, _: &Paste, window, cx| app.paste_edit_text(window, cx)))
            .on_action(cx.listener(|app, _: &Start, window, cx| app.move_edge(false, window, cx)))
            .on_action(cx.listener(|app, _: &End, window, cx| app.move_edge(true, window, cx)))
            .on_action(cx.listener(|app, _: &SelectAll, window, cx| {
                if app.generator_name_focus.is_focused(window) {
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
            .when(!self.navigation.generator_open(), |view| view.child(
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
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
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
                                    .child("Generate ↗"),
                            )
                            .when(self.yubikey_connection == YubiConnection::Connected, |header| {
                                header.child(
                                    div()
                                    .id("open-yubikey-status")
                                    .text_size(px(12.))
                                    .text_color(rgb(if self.navigation.page() == Page::CardDetails { self.theme.colors.nav_selected } else { self.theme.colors.nav_foreground }))
                                    .cursor_pointer()
                                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation();
                                    })
                                    .on_click(cx.listener(|app, _, _, cx| {
                                        app.navigation.go_to(Page::CardDetails);
                                        cx.notify();
                                    }))
                                    .child("Card status ↗"),
                                )
                            })
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
                                    .child("↻  Refresh"),
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
                                            .hover(|style| style.bg(rgb(self.theme.colors.nav_hover)))
                                            .on_mouse_down(
                                                gpui::MouseButton::Left,
                                                cx.listener(|_, _, _, cx| cx.stop_propagation()),
                                            )
                                            .on_click(cx.listener(|_, _, window, _| window.minimize_window()))
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
                                            .hover(|style| style.bg(rgb(self.theme.colors.danger_hover)))
                                            .on_mouse_down(
                                                gpui::MouseButton::Left,
                                                cx.listener(|_, _, _, cx| cx.stop_propagation()),
                                            )
                                            .on_click(cx.listener(|_, _, _, cx| cx.quit()))
                                            .child("×"),
                                    ),
                            ),
                    ),
            ))
            .child(main_content)
            .child(footer)
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

fn check_card_status(executable: &std::path::Path, probe_id: u64) -> CardProbeResult {
    trace!("probe.query id={probe_id} mode=card-status-all");
    let Ok(child) = Command::new(executable)
        .args([
            "--batch",
            "--no-tty",
            "--with-colons",
            "--status-fd=2",
            // Plain card-status only LEARNs the current session. The explicit
            // all form issues SERIALNO first, rescanning readers after replug.
            "--card-status",
            "all",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    else {
        return CardProbeResult {
            connection: YubiConnection::Unavailable,
            details: None,
        };
    };

    trace!("probe.spawned id={probe_id} child_pid={}", child.id());
    let started = Instant::now();
    let output = match process_output::collect(child, Duration::from_secs(5)) {
        Ok(output) => output,
        Err(_) => return CardProbeResult { connection: YubiConnection::Busy, details: None },
    };
    trace!("probe.output id={probe_id} stdout_bytes={} stderr_bytes={} elapsed_ms={}",
        output.stdout.len(), output.stderr.len(), started.elapsed().as_millis());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined_output = format!("{stdout}\n{stderr}");
    match parse_card_status(&combined_output) {
        ParsedCardStatus::Detected(details) => {
            return CardProbeResult {
                connection: YubiConnection::Connected,
                details: Some(details),
            };
        }
        ParsedCardStatus::NotDetected if output.status.success() => {
            return CardProbeResult {
                connection: YubiConnection::NotDetected,
                details: None,
            };
        }
        // The all form can successfully return no records when no card was found.
        ParsedCardStatus::Unrecognized if output.status.success() => {
            return CardProbeResult {
                connection: YubiConnection::NotDetected,
                details: None,
            };
        }
        ParsedCardStatus::NotDetected | ParsedCardStatus::Unrecognized => {}
    }

    // Diagnostics are consulted only to classify failures and are never retained or displayed.
    for line in stderr.lines() {
        for status in ["ERROR", "FAILURE", "SC_OP_FAILURE"] {
            if line.strip_prefix("[GNUPG:] ").and_then(|line| line.strip_prefix(status))
                .is_some_and(|rest| rest.starts_with(' '))
            {
                let code = line.split_whitespace().last().and_then(|value| value.parse::<u32>().ok());
                trace!("probe.gpg_error id={probe_id} status={status} code={code:?}");
            }
        }
    }
    let diagnostic = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    let connection = if diagnostic.contains("no agent running")
        || diagnostic.contains("can't connect to the agent")
        || diagnostic.contains("cannot connect to the agent")
    {
        YubiConnection::Unavailable
    } else if diagnostic.contains("busy") || diagnostic.contains("locked") {
        YubiConnection::Busy
    } else if diagnostic.contains("no reader") || diagnostic.contains("reader not found") {
        YubiConnection::NoReader
    } else if [
        "no such device",
        "no card",
        "card not present",
        "selecting card failed",
        "card not available",
    ]
    .iter()
    .any(|message| diagnostic.contains(message))
    {
        YubiConnection::NotDetected
    } else {
        YubiConnection::Unavailable
    };
    CardProbeResult {
        connection,
        details: None,
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
struct YubiKeyInfo {
    device_type: Option<String>,
    firmware_version: Option<String>,
    form_factor: Option<String>,
}

fn yubikey_product_image(device_type: Option<&str>) -> Option<Arc<Image>> {
    let device_type = device_type?.trim();
    [
        ("YubiKey 5Ci", &YUBIKEY_5CI_IMAGE),
        ("YubiKey 5C Nano", &YUBIKEY_5C_NANO_IMAGE),
        ("YubiKey 5 Nano", &YUBIKEY_5_NANO_IMAGE),
        ("YubiKey 5C NFC", &YUBIKEY_5C_NFC_IMAGE),
        ("YubiKey 5 NFC", &YUBIKEY_5_NFC_IMAGE),
        ("YubiKey 5C", &YUBIKEY_5C_IMAGE),
    ]
    .into_iter()
    .find(|(model, _)| device_type.eq_ignore_ascii_case(model))
    .map(|(_, image)| Arc::clone(&**image))
}

#[derive(Clone)]
struct UsbReaderDetails {
    product: String,
    vendor_id: u16,
    product_id: u16,
    location: String,
}

fn query_yubikey_info(probe_id: u64) -> YubiKeyInfo {
    // The ykman CLI enumerates PC/SC, starting pcscd on systems with socket
    // activation. pcscd can then claim CCID on replug until its idle timeout,
    // starving GnuPG's built-in driver. Restrict metadata reads to FIDO HID.
    let mut command = Command::new("python3");
    let Ok(child) = command
        .args(["-c", include_str!("yubikey_info.py")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    else {
        return YubiKeyInfo::default();
    };

    trace!("probe.spawned id={probe_id} child_pid={}", child.id());
    let started = Instant::now();
    let Ok(output) = process_output::collect(child, Duration::from_secs(2)) else {
        return YubiKeyInfo::default();
    };
    trace!("probe.output id={probe_id} stdout_bytes={} stderr_bytes={} elapsed_ms={}",
        output.stdout.len(), output.stderr.len(), started.elapsed().as_millis());
    if !output.status.success() {
        return YubiKeyInfo::default();
    }

    let mut info = YubiKeyInfo::default();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some((label, value)) = line.split_once(':') else {
            continue;
        };
        let value = safe_card_text(value.trim());
        if value.is_empty() {
            continue;
        }
        match label.trim().to_ascii_lowercase().as_str() {
            "device type" => info.device_type = Some(value),
            "firmware version" => info.firmware_version = Some(value),
            "form factor" => info.form_factor = Some(value),
            // Serial numbers and capability details are intentionally not retained.
            _ => {}
        }
    }
    info
}

fn usb_reader_for_card(reader: Option<&str>, yubico: bool) -> Option<UsbReaderDetails> {
    let reader = normalize_usb_label(reader.unwrap_or_default());
    let devices = std::fs::read_dir("/sys/bus/usb/devices").ok()?;
    let mut readers = Vec::new();
    for entry in devices.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some((bus, port)) = name.split_once('-') else {
            continue;
        };
        if bus.is_empty() || port.is_empty() {
            continue;
        }

        if !is_ccid_usb_device(&path) {
            continue;
        }

        let vendor_id = std::fs::read_to_string(path.join("idVendor"))
            .ok()
            .and_then(|value| u16::from_str_radix(value.trim(), 16).ok());
        let product_id = std::fs::read_to_string(path.join("idProduct"))
            .ok()
            .and_then(|value| u16::from_str_radix(value.trim(), 16).ok());
        let (Some(vendor_id), Some(product_id)) = (vendor_id, product_id) else {
            continue;
        };
        let product = std::fs::read_to_string(path.join("product")).unwrap_or_default();
        let product = safe_card_text(product.trim());
        let manufacturer = std::fs::read_to_string(path.join("manufacturer")).unwrap_or_default();
        let manufacturer = safe_card_text(manufacturer.trim());
        let display_name = if manufacturer.is_empty()
            || normalize_usb_label(&product).starts_with(&normalize_usb_label(&manufacturer))
        {
            product.clone()
        } else {
            format!("{manufacturer} {product}")
        };
        let normalized_product = normalize_usb_label(&product);
        let normalized_display_name = normalize_usb_label(&display_name);
        let reader_match = !reader.is_empty()
            && normalized_product.len() >= 4
            && (reader.contains(&normalized_product)
                || reader.contains(&normalized_display_name));
        let location = format!("Bus {bus} · port {port}");
        readers.push((
            UsbReaderDetails {
                product: display_name,
                vendor_id,
                product_id,
                location,
            },
            reader_match,
        ));
    }

    let mut matching = readers
        .iter()
        .filter(|(_, reader_match)| *reader_match)
        .map(|(details, _)| details)
        .collect::<Vec<_>>();
    if matching.len() == 1 {
        return matching.pop().cloned();
    }

    if yubico {
        let mut yubico_readers = readers
            .iter()
            .filter(|(details, _)| details.vendor_id == 0x1050)
            .map(|(details, _)| details)
            .collect::<Vec<_>>();
        if yubico_readers.len() == 1 {
            return yubico_readers.pop().cloned();
        }
    }

    if readers.len() == 1 {
        return readers.pop().map(|(details, _)| details);
    }

    None
}

fn is_ccid_usb_device(path: &std::path::Path) -> bool {
    let Ok(interfaces) = std::fs::read_dir(path) else {
        return false;
    };
    interfaces.flatten().any(|interface| {
        std::fs::read_to_string(interface.path().join("bInterfaceClass"))
            .is_ok_and(|class| class.trim().eq_ignore_ascii_case("0b"))
    })
}

fn connected_yubikey_usb_paths() -> HashSet<String> {
    let Ok(devices) = std::fs::read_dir("/sys/bus/usb/devices") else {
        return HashSet::new();
    };
    devices
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let vendor_id = std::fs::read_to_string(path.join("idVendor")).ok()?;
            if !vendor_id.trim().eq_ignore_ascii_case("1050") || !is_ccid_usb_device(&path) {
                return None;
            }
            let canonical = path.canonicalize().ok()?;
            let devpath = canonical.strip_prefix("/sys").ok()?.to_string_lossy().into_owned();
            Some(devpath)
        })
        .collect()
}

fn update_yubikey_usb_presence(event: &[String], known_devices: &mut HashSet<String>) -> Option<bool> {
    let property = |name: &str| {
        event
            .iter()
            .find_map(|line| line.strip_prefix(name))
    };
    if property("SUBSYSTEM=") != Some("usb")
        || property("DEVTYPE=") != Some("usb_device")
    {
        return None;
    }
    let devpath = property("DEVPATH=")?;
    match property("ACTION=")? {
        "add"
            if property("ID_VENDOR_ID=").is_some_and(|vendor| vendor.eq_ignore_ascii_case("1050"))
                && property("ID_USB_INTERFACES=")
                    .is_some_and(|interfaces| interfaces.split(':').any(|class| class.starts_with("0b"))) =>
        {
            known_devices.insert(devpath.to_owned()).then_some(true)
        }
        "remove" if known_devices.remove(devpath) => Some(!known_devices.is_empty()),
        _ => None,
    }
}

fn normalize_usb_label(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn parse_card_status(output: &str) -> ParsedCardStatus {
    // card-status all may return several cards. Select the first available
    // record; never combine one reader's version with another card's PIN data.
    let mut start = 0;
    let mut offset = 0;
    let mut reader_seen = false;
    for line in output.split_inclusive('\n') {
        let label = line.split_once(':').map(|(label, _)| label.trim().trim_matches('.').trim());
        if label.is_some_and(|label| label.eq_ignore_ascii_case("reader")) {
            if reader_seen {
                if let ParsedCardStatus::Detected(details) = parse_one_card_status(&output[start..offset]) {
                    return ParsedCardStatus::Detected(details);
                }
                start = offset;
            }
            reader_seen = true;
        }
        offset += line.len();
    }
    parse_one_card_status(&output[start..])
}

fn parse_one_card_status(output: &str) -> ParsedCardStatus {
    let mut application = None;
    let mut reader = None;
    let mut version = None;
    let mut manufacturer = None;
    let mut key_attributes = Vec::new();
    let mut pin_retries = None;
    let mut signature_count = None;
    let mut aid_record_seen = false;
    let mut aid_present = false;

    for line in output.lines() {
        let fields: Vec<_> = line.split(':').collect();
        let (raw_label, raw_value) = line.split_once(':').unwrap_or((line, ""));
        let label = raw_label
            .trim()
            .trim_matches('.')
            .trim()
            .to_ascii_lowercase();
        let value = raw_value.trim().trim_end_matches(':').trim();
        match label.as_str() {
            "reader" => reader = Some(safe_card_text(value)),
            "aid" => {
                aid_record_seen = true;
                aid_present = fields.get(1).is_some_and(|value| !value.is_empty());
            }
            "openpgp-card" => application = Some("OpenPGP".to_owned()),
            "piv-card" => application = Some("PIV".to_owned()),
            "netkey-card" => application = Some("NetKey".to_owned()),
            "dinsig-card" => application = Some("DINSIG".to_owned()),
            "pkcs15-card" => application = Some("PKCS#15".to_owned()),
            "geldkarte-card" => application = Some("Geldkarte".to_owned()),
            "unknown-card" => application = Some("Unknown card application".to_owned()),
            tag if tag.ends_with("-card") => {
                application = Some(
                    tag.trim_end_matches("-card")
                        .replace('-', " ")
                        .to_ascii_uppercase(),
                );
            }
            "application type" => {
                if !value.is_empty() {
                    application = Some(safe_card_text(value));
                }
            }
            "version" => {
                version = decode_card_version(if value.is_empty() {
                    fields.get(1).copied().unwrap_or_default()
                } else {
                    value
                });
            }
            "vendor" => {
                manufacturer = fields.get(2).map(|value| safe_card_text(value));
            }
            "manufacturer" => manufacturer = Some(safe_card_text(value)),
            "keyattr" => {
                if let (Some(slot), Some(algorithm), Some(parameter)) =
                    (fields.get(1), fields.get(2), fields.get(3))
                    && let Some(attribute) = card_key_attribute(slot, algorithm, parameter)
                {
                    key_attributes.push(attribute);
                }
            }
            "key attributes" => {
                key_attributes.extend(value.split_whitespace().map(|attribute| {
                    format!("Key: {}", safe_card_text(attribute))
                }));
            }
            "pinretry" => {
                if let (Some(first), Some(second), Some(third)) = (
                    fields.get(1).and_then(|value| value.parse().ok()),
                    fields.get(2).and_then(|value| value.parse().ok()),
                    fields.get(3).and_then(|value| value.parse().ok()),
                ) {
                    pin_retries = Some([first, second, third]);
                }
            }
            "pin retry counter" => {
                let retries: Vec<_> = value
                    .split_whitespace()
                    .filter_map(|value| value.parse().ok())
                    .collect();
                if let [first, second, third] = retries.as_slice() {
                    pin_retries = Some([*first, *second, *third]);
                }
            }
            "sigcount" => signature_count = fields.get(1).and_then(|value| value.parse().ok()),
            "signature counter" => signature_count = value.parse().ok(),
            _ => {}
        }
    }

    if aid_present
        || application.is_some()
        || reader.as_ref().is_some_and(|value| !value.is_empty())
        || version.is_some()
        || manufacturer.is_some()
        || !key_attributes.is_empty()
        || pin_retries.is_some()
        || signature_count.is_some()
    {
        ParsedCardStatus::Detected(CardDetails {
            application: application.unwrap_or_else(|| "Smartcard".to_owned()),
            reader: reader.filter(|value| !value.is_empty()),
            version,
            manufacturer: manufacturer.filter(|value| !value.is_empty()),
            key_attributes,
            pin_retries,
            signature_count,
        })
    } else if aid_record_seen {
        ParsedCardStatus::NotDetected
    } else {
        ParsedCardStatus::Unrecognized
    }
}

fn card_key_attribute(slot: &str, algorithm: &str, parameter: &str) -> Option<String> {
    let slot = match slot {
        "1" => "Signing",
        "2" => "Encryption",
        "3" => "Authentication",
        _ => return None,
    };
    let algorithm = match algorithm {
        "1" => format!("RSA {}", parameter),
        "18" => format!("ECDH {}", safe_card_text(parameter)),
        "19" => format!("ECDSA {}", safe_card_text(parameter)),
        "22" => format!("EdDSA {}", safe_card_text(parameter)),
        value => format!("algorithm {value}"),
    };
    Some(format!("{slot}: {algorithm}"))
}

fn decode_card_version(value: &str) -> Option<String> {
    if value.len() == 4 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        let major = u8::from_str_radix(&value[..2], 16).ok()?;
        let minor = u8::from_str_radix(&value[2..], 16).ok()?;
        Some(format!("{major}.{minor}"))
    } else if value.is_empty() {
        None
    } else {
        Some(safe_card_text(value))
    }
}

fn safe_card_text(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len()
            && let (Some(high), Some(low)) = (hex_digit(bytes[index + 1]), hex_digit(bytes[index + 2]))
        {
            decoded.push(high * 16 + low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&decoded)
        .chars()
        .filter(|character| !character.is_control())
        .collect()
}

fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn card_event_title(event: CardEvent) -> &'static str {
    match event {
        CardEvent::PinentryOpened => "System PIN or passphrase prompt opened",
        CardEvent::CardDetected => "GnuPG detected a card",
        CardEvent::InsertionRequested => "GnuPG requested a card",
        CardEvent::NoCardAvailable => "GnuPG reported no card available",
        CardEvent::NoReaderAvailable => "GnuPG reported no card reader",
        CardEvent::CardBusy => "GnuPG reported a busy card",
        CardEvent::AgentBusy => "GnuPG agent is busy",
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

fn yubi_info_row(theme: &Theme, title: &'static str, detail: impl Into<SharedString>) -> gpui::Div {
    let detail: SharedString = detail.into();
    div()
        .flex()
        .items_start()
        .gap(px(18.))
        .border_b_1()
        .border_color(rgb(theme.colors.border))
        .py(px(13.))
        .child(
            div()
                .w(px(140.))
                .flex_shrink_0()
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .child(title),
        )
        .child(
            div()
                .max_w(px(560.))
                .text_size(px(12.))
                .line_height(px(19.))
                .text_color(rgb(theme.colors.muted))
                .child(detail),
        )
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

#[cfg(test)]
mod card_detection_tests {
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
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("gpui-pass-card-probe-{}-{unique}", std::process::id()));
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
            update_yubikey_usb_presence(
                &usb_event("add", "1234", ":0b0000:"), &mut devices
            ),
            None
        );
        assert_eq!(
            update_yubikey_usb_presence(
                &usb_event("add", "1050", ":030101:"), &mut devices
            ),
            None
        );
        let inserted = usb_event("add", "1050", ":0b0000:");
        assert_eq!(update_yubikey_usb_presence(&inserted, &mut devices), Some(true));
        assert_eq!(update_yubikey_usb_presence(&inserted, &mut devices), None);
        let removed = usb_event("remove", "", "");
        assert_eq!(update_yubikey_usb_presence(&removed, &mut devices), Some(false));
        assert_eq!(update_yubikey_usb_presence(&removed, &mut devices), None);
    }
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
                include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/fonts/InterVariable.ttf")).as_slice(),
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
