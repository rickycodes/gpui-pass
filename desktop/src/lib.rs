#![forbid(unsafe_code)]

#[path = "security/device_verification.rs"]
mod device_verification;
use device_verification::{DeviceVerification, State as VerificationState};

#[path = "system/diagnostics.rs"]
mod diagnostics;
use diagnostics::trace;

#[path = "security/card_refresh.rs"]
mod card_refresh;
use card_refresh::{CardProbe, CardRefresh, ProbeResult};

#[path = "app/navigation.rs"]
mod navigation;
use navigation::{Navigation, Page};

#[path = "system/clipboard.rs"]
mod clipboard;
#[path = "system/process_output.rs"]
mod process_output;
use clipboard::ClipboardState;

#[path = "app/app_actions.rs"]
mod app_actions;
#[path = "app/app_lifecycle.rs"]
mod app_lifecycle;
#[path = "app/app_state.rs"]
mod app_state;
#[path = "app/app_view.rs"]
mod app_view;
#[path = "ui/brand.rs"]
mod brand;
#[path = "ui/card_status_view.rs"]
mod card_status_view;
#[path = "input/editor_input.rs"]
mod editor_input;
#[path = "ui/entries_view.rs"]
mod entries_view;
#[path = "input/entry_editor.rs"]
mod entry_editor;
#[path = "app/entry_operations.rs"]
mod entry_operations;
#[path = "ui/generator_view.rs"]
mod generator_view;
#[path = "ui/i18n.rs"]
mod i18n;
#[path = "ui/loading_spinner.rs"]
mod loading_spinner;
#[path = "input/search.rs"]
mod search;
#[path = "ui/sidebar_scroll.rs"]
mod sidebar_scroll;
#[path = "input/text_input_ime.rs"]
mod text_input_ime;
#[path = "input/text_input_views.rs"]
mod text_input_views;
#[path = "ui/theme.rs"]
mod theme;
#[path = "ui/window_control_glyph.rs"]
mod window_control_glyph;
#[path = "security/yubikey_detection.rs"]
mod yubikey_detection;

use app_state::AppState;
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
use window_control_glyph::window_control_glyph;
use yubikey_detection::{
    CardDetails, CardProbeResult, CardWatcher, UsbCardWatcher, UsbReaderDetails, YubiConnection,
    YubiKeyInfo, YubiRecord,
};
use zeroize::{Zeroize, Zeroizing};

const NOTIFICATION_TIMEOUT: Duration = Duration::from_secs(6);
const ACTIVE_POLL_INTERVAL: Duration = Duration::from_millis(100);
const UI_TIMER_INTERVAL: Duration = Duration::from_millis(500);
const IDLE_POLL_INTERVAL: Duration = Duration::from_secs(2);

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
    pub(crate) state: AppState,
    _activation: Subscription,
}

impl PassApp {
    pub(crate) fn text_editor_active(&self) -> bool {
        self.state.editing_entry || self.state.generator_mode == GeneratorMode::Custom
    }
}

pub fn run() {
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
        app_actions::bind_keys(cx);
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
                move |window, cx| {
                    if let Some(gpu) = window.gpu_specs() {
                        trace!(
                            "renderer.start device={} software={} debug_build={}",
                            gpu.device_name,
                            gpu.is_software_emulated,
                            cfg!(debug_assertions)
                        );
                    }
                    cx.new(|cx| PassApp::new(options, window, cx))
                },
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
