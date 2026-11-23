use super::*;

pub(crate) static YUBIKEY_5_NFC_IMAGE: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Webp,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../assets/yubikey/yubikey-5-nfc.webp"
        ))
        .to_vec(),
    ))
});
pub(crate) static YUBIKEY_5C_NFC_IMAGE: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Webp,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../assets/yubikey/yubikey-5c-nfc.webp"
        ))
        .to_vec(),
    ))
});
pub(crate) static YUBIKEY_5C_IMAGE: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Webp,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../assets/yubikey/yubikey-5c.webp"
        ))
        .to_vec(),
    ))
});
pub(crate) static YUBIKEY_5_NANO_IMAGE: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Webp,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../assets/yubikey/yubikey-5-nano.webp"
        ))
        .to_vec(),
    ))
});
pub(crate) static YUBIKEY_5C_NANO_IMAGE: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Webp,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../assets/yubikey/yubikey-5c-nano.webp"
        ))
        .to_vec(),
    ))
});
pub(crate) static YUBIKEY_5CI_IMAGE: LazyLock<Arc<Image>> = LazyLock::new(|| {
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
pub(crate) struct YubiRecord {
    pub(crate) observation: CardObservation,
    pub(crate) operation: YubiOperation,
}

#[derive(Clone, Copy)]
pub(crate) enum YubiOperation {
    Reveal,
    Copy,
    SaveEdited,
    SaveGenerated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum YubiConnection {
    Checking,
    Connected,
    NotDetected,
    NoReader,
    Busy,
    Unavailable,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct CardDetails {
    pub(crate) application: String,
    pub(crate) reader: Option<String>,
    pub(crate) version: Option<String>,
    pub(crate) manufacturer: Option<String>,
    pub(crate) key_attributes: Vec<String>,
    pub(crate) pin_retries: Option<[u32; 3]>,
    pub(crate) signature_count: Option<u64>,
}

pub(crate) struct CardProbeResult {
    pub(crate) connection: YubiConnection,
    pub(crate) details: Option<CardDetails>,
}

#[derive(Clone, Copy)]
pub(crate) enum CardWatchEvent {
    Ready,
    Status(Option<bool>),
    Failed,
}

pub(crate) struct CardWatcher {
    pub(crate) child: Arc<Mutex<Child>>,
    pub(crate) events: Receiver<CardWatchEvent>,
}

pub(crate) enum UsbCardEvent {
    Presence(bool),
    Failed,
}

pub(crate) struct UsbCardWatcher {
    pub(crate) child: Arc<Mutex<Child>>,
    pub(crate) events: Receiver<UsbCardEvent>,
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

#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct YubiKeyInfo {
    pub(crate) device_type: Option<String>,
    pub(crate) firmware_version: Option<String>,
    pub(crate) form_factor: Option<String>,
}

#[derive(Clone)]
pub(crate) struct UsbReaderDetails {
    pub(crate) product: String,
    pub(crate) vendor_id: u16,
    pub(crate) product_id: u16,
    pub(crate) location: String,
}

#[derive(Clone)]
pub(crate) enum ParsedCardStatus {
    Detected(CardDetails),
    NotDetected,
    Unrecognized,
}
