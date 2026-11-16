use super::*;

static YUBIKEY_5_NFC_IMAGE: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Webp,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../assets/yubikey/yubikey-5-nfc.webp"
        ))
        .to_vec(),
    ))
});
static YUBIKEY_5C_NFC_IMAGE: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Webp,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../assets/yubikey/yubikey-5c-nfc.webp"
        ))
        .to_vec(),
    ))
});
static YUBIKEY_5C_IMAGE: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Webp,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../assets/yubikey/yubikey-5c.webp"
        ))
        .to_vec(),
    ))
});
static YUBIKEY_5_NANO_IMAGE: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Webp,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../assets/yubikey/yubikey-5-nano.webp"
        ))
        .to_vec(),
    ))
});
static YUBIKEY_5C_NANO_IMAGE: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Webp,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../assets/yubikey/yubikey-5c-nano.webp"
        ))
        .to_vec(),
    ))
});
static YUBIKEY_5CI_IMAGE: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Webp,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../assets/yubikey/yubikey-5ci.webp"
        ))
        .to_vec(),
    ))
});

#[derive(Clone, Copy)]
pub(super) struct YubiRecord {
    pub(super) observation: CardObservation,
    pub(super) operation: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum YubiConnection {
    Checking,
    Connected,
    NotDetected,
    NoReader,
    Busy,
    Unavailable,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct CardDetails {
    pub(super) application: String,
    pub(super) reader: Option<String>,
    pub(super) version: Option<String>,
    pub(super) manufacturer: Option<String>,
    pub(super) key_attributes: Vec<String>,
    pub(super) pin_retries: Option<[u32; 3]>,
    pub(super) signature_count: Option<u64>,
}

pub(super) struct CardProbeResult {
    pub(super) connection: YubiConnection,
    pub(super) details: Option<CardDetails>,
}

#[derive(Clone, Copy)]
pub(super) enum CardWatchEvent {
    Ready,
    Status(Option<bool>),
    Failed,
}

pub(super) struct CardWatcher {
    pub(super) child: Arc<Mutex<Child>>,
    pub(super) events: Receiver<CardWatchEvent>,
}

pub(super) enum UsbCardEvent {
    Presence(bool),
    Failed,
}

pub(super) struct UsbCardWatcher {
    pub(super) child: Arc<Mutex<Child>>,
    pub(super) events: Receiver<UsbCardEvent>,
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

pub(super) enum ParsedCardStatus {
    Detected(CardDetails),
    NotDetected,
    Unrecognized,
}

pub(super) fn check_card_status(executable: &std::path::Path, probe_id: u64) -> CardProbeResult {
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
        Err(_) => {
            return CardProbeResult {
                connection: YubiConnection::Busy,
                details: None,
            };
        }
    };
    trace!(
        "probe.output id={probe_id} stdout_bytes={} stderr_bytes={} elapsed_ms={}",
        output.stdout.len(),
        output.stderr.len(),
        started.elapsed().as_millis()
    );
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
            if line
                .strip_prefix("[GNUPG:] ")
                .and_then(|line| line.strip_prefix(status))
                .is_some_and(|rest| rest.starts_with(' '))
            {
                let code = line
                    .split_whitespace()
                    .last()
                    .and_then(|value| value.parse::<u32>().ok());
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
pub(super) struct YubiKeyInfo {
    pub(super) device_type: Option<String>,
    pub(super) firmware_version: Option<String>,
    pub(super) form_factor: Option<String>,
}

pub(super) fn yubikey_product_image(device_type: Option<&str>) -> Option<Arc<Image>> {
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
pub(super) struct UsbReaderDetails {
    pub(super) product: String,
    pub(super) vendor_id: u16,
    pub(super) product_id: u16,
    pub(super) location: String,
}

pub(super) fn query_yubikey_info(probe_id: u64) -> YubiKeyInfo {
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
    trace!(
        "probe.output id={probe_id} stdout_bytes={} stderr_bytes={} elapsed_ms={}",
        output.stdout.len(),
        output.stderr.len(),
        started.elapsed().as_millis()
    );
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

pub(super) fn usb_reader_for_card(reader: Option<&str>, yubico: bool) -> Option<UsbReaderDetails> {
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
            && (reader.contains(&normalized_product) || reader.contains(&normalized_display_name));
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

pub(super) fn is_ccid_usb_device(path: &std::path::Path) -> bool {
    let Ok(interfaces) = std::fs::read_dir(path) else {
        return false;
    };
    interfaces.flatten().any(|interface| {
        std::fs::read_to_string(interface.path().join("bInterfaceClass"))
            .is_ok_and(|class| class.trim().eq_ignore_ascii_case("0b"))
    })
}

pub(super) fn connected_yubikey_usb_paths() -> HashSet<String> {
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
            let devpath = canonical
                .strip_prefix("/sys")
                .ok()?
                .to_string_lossy()
                .into_owned();
            Some(devpath)
        })
        .collect()
}

pub(super) fn update_yubikey_usb_presence(
    event: &[String],
    known_devices: &mut HashSet<String>,
) -> Option<bool> {
    let property = |name: &str| event.iter().find_map(|line| line.strip_prefix(name));
    if property("SUBSYSTEM=") != Some("usb") || property("DEVTYPE=") != Some("usb_device") {
        return None;
    }
    let devpath = property("DEVPATH=")?;
    match property("ACTION=")? {
        "add"
            if property("ID_VENDOR_ID=")
                .is_some_and(|vendor| vendor.eq_ignore_ascii_case("1050"))
                && property("ID_USB_INTERFACES=").is_some_and(|interfaces| {
                    interfaces.split(':').any(|class| class.starts_with("0b"))
                }) =>
        {
            known_devices.insert(devpath.to_owned()).then_some(true)
        }
        "remove" if known_devices.remove(devpath) => Some(!known_devices.is_empty()),
        _ => None,
    }
}

pub(super) fn normalize_usb_label(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

pub(super) fn parse_card_status(output: &str) -> ParsedCardStatus {
    // card-status all may return several cards. Select the first available
    // record; never combine one reader's version with another card's PIN data.
    let mut start = 0;
    let mut offset = 0;
    let mut reader_seen = false;
    for line in output.split_inclusive('\n') {
        let label = line
            .split_once(':')
            .map(|(label, _)| label.trim().trim_matches('.').trim());
        if label.is_some_and(|label| label.eq_ignore_ascii_case("reader")) {
            if reader_seen {
                if let ParsedCardStatus::Detected(details) =
                    parse_one_card_status(&output[start..offset])
                {
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

pub(super) fn parse_one_card_status(output: &str) -> ParsedCardStatus {
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
                key_attributes.extend(
                    value
                        .split_whitespace()
                        .map(|attribute| format!("Key: {}", safe_card_text(attribute))),
                );
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

pub(super) fn card_key_attribute(slot: &str, algorithm: &str, parameter: &str) -> Option<String> {
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

pub(super) fn decode_card_version(value: &str) -> Option<String> {
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

pub(super) fn safe_card_text(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_digit(bytes[index + 1]), hex_digit(bytes[index + 2]))
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

pub(super) fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

impl PassApp {
    pub(super) fn start_yubikey_check(&mut self) {
        // A refresh request is not a physical card change. Coalesce requests
        // without making the running probe's result stale.
        if self.state.yubikey_check.is_some() {
            return;
        }
        if self.state.yubikey_info_check.is_some()
            || self.state.reveal.is_some()
            || self.state.copy_pending.is_some()
            || self.state.saving.is_some()
            || self.state.device_verification.running()
        {
            if !self.state.card_refresh_pending {
                trace!(
                    "probe.deferred ykman={} reveal={} saving={}",
                    self.state.yubikey_info_check.is_some(),
                    self.state.reveal.is_some(),
                    self.state.saving.is_some()
                );
            }
            self.state.card_refresh_pending = true;
            return;
        }
        // Keep the initial loading state, then leave the last result on screen while the
        // background probe runs. An absent key should not flash "Checking" on every retry.
        if self.state.yubikey_last_checked.is_none()
            && self.state.yubikey_connection != YubiConnection::Connected
        {
            self.state.yubikey_connection = YubiConnection::Checking;
        }
        let executable = self.state.gpg.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        self.state.card_refresh_pending = false;
        let probe = CardProbe::new("gpg", self.state.card_refresh.generation(), receiver);
        let probe_id = probe.id();
        self.state.yubikey_check = Some(probe);
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

    pub(super) fn start_card_watcher(&mut self) {
        let gpg_parent = self
            .state
            .gpg
            .parent()
            .unwrap_or_else(|| std::path::Path::new("/usr/bin"));
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
            self.state.card_watcher_supported = Some(false);
            self.state.card_watcher_retry_at = Some(Instant::now() + Duration::from_secs(2));
            return;
        };
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            self.state.card_watcher_supported = Some(false);
            self.state.card_watcher_retry_at = Some(Instant::now() + Duration::from_secs(2));
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
                        if (line.contains("DEVINFO_START") || line.contains("DEVINFO_END"))
                            && !ready
                        {
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
                            let code = line
                                .split_whitespace()
                                .nth(1)
                                .and_then(|value| value.parse::<u32>().ok());
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
        self.state.card_watcher = Some(CardWatcher { child, events });
        self.state.card_watcher_retry_at = None;
    }

    #[cfg(target_os = "linux")]
    pub(super) fn start_usb_card_watcher(&mut self, cx: &mut Context<Self>) {
        let Ok(mut child) = Command::new("udevadm")
            .args(["monitor", "--udev", "--property", "--subsystem-match=usb"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        else {
            self.state.usb_watcher_retry_at = Some(Instant::now() + Duration::from_secs(2));
            return;
        };
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            self.state.usb_watcher_retry_at = Some(Instant::now() + Duration::from_secs(2));
            return;
        };

        let child = Arc::new(Mutex::new(child));
        let thread_child = child.clone();
        let (sender, events) = mpsc::channel();
        let mut known_devices = connected_yubikey_usb_paths();
        let initially_present = !known_devices.is_empty();
        let previous_presence = self.state.usb_yubikey_present;
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
                        if let Some(present) =
                            update_yubikey_usb_presence(&event, &mut known_devices)
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
        self.state.usb_card_watcher = Some(UsbCardWatcher { child, events });
        self.state.usb_watcher_retry_at = None;
    }

    #[cfg(not(target_os = "linux"))]
    pub(super) fn start_usb_card_watcher(&mut self, _cx: &mut Context<Self>) {}

    pub(super) fn apply_card_presence_hint(&mut self, present: bool, cx: &mut Context<Self>) {
        let connection = if present {
            YubiConnection::Connected
        } else {
            YubiConnection::NotDetected
        };
        if self.state.usb_yubikey_present != Some(true) {
            self.state.card_refresh.observe(present);
        }
        if self.state.yubikey_connection != connection {
            self.state.yubikey_connection = connection;
            self.state.yubikey_details = None;
            if !present {
                self.state.device_verification.invalidate();
                self.state.navigation.card_unavailable();
            }
            cx.notify();
        }
        if self.state.yubikey_check.is_some()
            || self.state.yubikey_info_check.is_some()
            || self.state.reveal.is_some()
            || self.state.saving.is_some()
        {
            self.state.card_refresh_pending = true;
        } else {
            self.start_yubikey_check();
        }
    }

    pub(super) fn start_yubikey_info_probe(&mut self) {
        if self.state.device_verification.running()
            || self.state.reveal.is_some()
            || self.state.copy_pending.is_some()
            || self.state.saving.is_some()
            || self.state.yubikey_info.is_some()
            || self.state.yubikey_info_check.is_some()
            || self.state.usb_yubikey_present != Some(true)
            || !self
                .state
                .usb_reader_details
                .as_ref()
                .is_some_and(|details| details.vendor_id == 0x1050)
        {
            return;
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        let probe = CardProbe::new("ykman-hid", self.state.card_refresh.generation(), receiver);
        let probe_id = probe.id();
        self.state.yubikey_info_check = Some(probe);
        std::thread::spawn(move || {
            let _ = sender.send(query_yubikey_info(probe_id));
        });
    }

    pub(super) fn apply_yubikey_usb_presence(&mut self, present: bool, cx: &mut Context<Self>) {
        // sysfs and udev can report the same transition. Do not reset retry
        // state or start another probe for a duplicate event.
        if self.state.usb_yubikey_present == Some(present) {
            return;
        }
        self.state.usb_yubikey_present = Some(present);
        self.state.card_refresh.observe(present);
        trace!(
            "card.presence source=usb present={present} generation={}",
            self.state.card_refresh.generation()
        );
        if present {
            self.state.card_detail_retry_at = None;
            self.state.usb_reader_details = usb_reader_for_card(None, true);
        } else {
            self.state.device_verification.invalidate();
            self.state.usb_reader_details = None;
            self.state.yubikey_info = None;
            // Keep tracking the old worker until it stops touching the device.
            self.state.card_detail_retry_at = None;
        }
        self.apply_card_presence_hint(present, cx);
    }

    pub(super) fn poll_yubikey_connection(&mut self, cx: &mut Context<Self>) {
        #[cfg(target_os = "linux")]
        {
            if self
                .state
                .usb_last_scan
                .is_none_or(|last| last.elapsed() >= Duration::from_millis(250))
            {
                self.state.usb_last_scan = Some(Instant::now());
                let present = !connected_yubikey_usb_paths().is_empty();
                let previous = self.state.usb_yubikey_present;
                if previous != Some(present) && (present || previous == Some(true)) {
                    self.apply_yubikey_usb_presence(present, cx);
                }
            }
        }
        let paused = self.state.reveal.is_some()
            || self.state.copy_pending.is_some()
            || self.state.saving.is_some()
            || self.state.device_verification.running();
        if paused != self.state.card_poll_paused {
            self.state.card_poll_paused = paused;
            trace!(
                "card.poll paused={} reveal={} saving={} verification={} details={}",
                paused,
                self.state.reveal.is_some(),
                self.state.saving.is_some(),
                self.state.device_verification.running(),
                self.state.yubikey_details.is_some()
            );
        }
        if paused {
            return;
        }
        let watch_events = self
            .state
            .card_watcher
            .as_ref()
            .map(|watcher| watcher.events.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        for event in watch_events {
            match event {
                CardWatchEvent::Ready => {
                    if self.state.card_watcher_supported != Some(true) {
                        trace!("watcher.ready kind=gpg");
                    }
                    self.state.card_watcher_supported = Some(true);
                }
                CardWatchEvent::Status(present) => {
                    trace!(
                        "card.presence source=gpg present={present:?} usb_present={:?}",
                        self.state.usb_yubikey_present
                    );
                    self.state.card_watcher_supported = Some(true);
                    if let Some(present) = present {
                        // A live USB event is authoritative for a YubiKey's physical
                        // presence; scdaemon can report a stale removal during reconnect.
                        if present || self.state.usb_yubikey_present != Some(true) {
                            self.apply_card_presence_hint(present, cx);
                        }
                    }
                    if present.is_none() {
                        if self.state.yubikey_check.is_some() {
                            self.state.card_refresh_pending = true;
                        } else {
                            self.start_yubikey_check();
                        }
                    }
                }
                CardWatchEvent::Failed => {
                    trace!("watcher.failed kind=gpg");
                    self.state.card_watcher_supported = Some(false);
                    self.state.card_watcher = None;
                    self.state.card_watcher_retry_at =
                        Some(Instant::now() + Duration::from_secs(2));
                }
            }
        }
        let usb_events = self
            .state
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
                    self.state.usb_card_watcher = None;
                    self.state.usb_watcher_retry_at = Some(Instant::now() + Duration::from_secs(2));
                }
            }
        }
        if let Some(completion) = self
            .state
            .yubikey_info_check
            .as_ref()
            .and_then(|probe| probe.poll(&self.state.card_refresh))
        {
            self.state.yubikey_info_check = None;
            if let ProbeResult::Current(info) = completion {
                if self.state.usb_yubikey_present == Some(true)
                    && self.state.yubikey_info.as_ref() != Some(&info)
                {
                    self.state.yubikey_info = Some(info);
                    cx.notify();
                }
            }
        }
        if let Some(completion) = self
            .state
            .yubikey_check
            .as_ref()
            .and_then(|probe| probe.poll(&self.state.card_refresh))
        {
            self.state.yubikey_check = None;
            self.state.yubikey_last_checked = Some(Instant::now());
            self.state.card_refresh_pending = false;
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
        if self.state.card_watcher.is_none()
            && self
                .state
                .card_watcher_retry_at
                .is_some_and(|retry_at| retry_at <= Instant::now())
        {
            self.start_card_watcher();
        }
        if self.state.usb_card_watcher.is_none()
            && self
                .state
                .usb_watcher_retry_at
                .is_some_and(|retry_at| retry_at <= Instant::now())
        {
            self.start_usb_card_watcher(cx);
        }
        if self.state.yubikey_check.is_none()
            && self.state.yubikey_info_check.is_none()
            && self.state.card_refresh_pending
        {
            self.state.card_refresh_pending = false;
            self.start_yubikey_check();
        }
        if self.state.usb_yubikey_present == Some(true)
            && self.state.yubikey_details.is_none()
            && self.state.yubikey_check.is_none()
            && self
                .state
                .card_detail_retry_at
                .is_some_and(|retry_at| retry_at <= Instant::now())
        {
            self.state.card_detail_retry_at = None;
            self.start_yubikey_check();
        }
        if self.state.card_watcher_supported != Some(true)
            && self.state.yubikey_check.is_none()
            && self.state.yubikey_info_check.is_none()
            && !(self.state.usb_card_watcher.is_some()
                && self.state.usb_yubikey_present == Some(true))
            && self
                .state
                .yubikey_last_checked
                .is_none_or(|last| last.elapsed() >= Duration::from_secs(3))
        {
            self.start_yubikey_check();
            cx.notify();
        }
    }

    pub(super) fn apply_card_probe(&mut self, result: CardProbeResult, cx: &mut Context<Self>) {
        trace!(
            "[card-detect] Applying current probe: state={:?}, details={}",
            result.connection,
            result.details.is_some()
        );
        if self.state.usb_yubikey_present == Some(true)
            && (result.connection != YubiConnection::Connected || result.details.is_none())
        {
            // A miss during enumeration does not invalidate physical presence or
            // details already obtained for this same insertion.
            self.state.yubikey_connection = YubiConnection::Connected;
            if self.state.yubikey_details.is_none() {
                let delay = self.state.card_refresh.retry_delay();
                self.state.card_detail_retry_at = Some(Instant::now() + delay);
                trace!(
                    "[card-detect] Card details not ready; retrying in {}ms",
                    delay.as_millis()
                );
            }
            cx.notify();
            return;
        }
        // Busy is inconclusive, so keep any already connected card on screen.
        if result.connection == YubiConnection::Busy {
            if self.state.yubikey_connection != YubiConnection::Connected {
                self.state.yubikey_connection = YubiConnection::Busy;
                self.state.yubikey_details = None;
                cx.notify();
            }
            return;
        }
        let changed = self.state.yubikey_connection != result.connection
            || self.state.yubikey_details != result.details;
        self.state.yubikey_connection = result.connection;
        self.state.yubikey_details = result.details;
        if self.state.yubikey_connection != YubiConnection::Connected {
            self.state.device_verification.invalidate();
            self.state.navigation.card_unavailable();
        } else if self.state.yubikey_details.is_some() {
            self.state.card_detail_retry_at = None;
            self.state.card_refresh.completed();
            self.start_yubikey_info_probe();
        }
        if changed {
            cx.notify();
        }
    }

    pub(super) fn capture_yubikey_status(&mut self, cx: &mut Context<Self>) {
        let observation = self
            .state
            .reveal
            .as_ref()
            .and_then(|(request, _)| {
                request
                    .progress
                    .card_observation()
                    .map(|observation| (observation, "Reveal entry"))
            })
            .or_else(|| {
                self.state.copy_pending.as_ref().and_then(|pending| {
                    pending
                        .progress
                        .card_observation()
                        .map(|observation| (observation, "Copy entry"))
                })
            })
            .or_else(|| {
                self.state.saving.as_ref().and_then(|(_, progress, _)| {
                    progress.card_observation().map(|observation| {
                        let operation = if self.state.saving_edit {
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
            .state
            .last_yubikey_observation
            .is_some_and(|last| observation.observed_at <= last)
        {
            return;
        }
        self.state.last_yubikey_observation = Some(observation.observed_at);
        self.state.yubikey_history.push_front(YubiRecord {
            observation,
            operation,
        });
        self.state.yubikey_history.truncate(12);
        cx.notify();
    }
}
