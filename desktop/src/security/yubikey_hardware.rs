use super::*;

pub(crate) fn yubikey_product_image(device_type: Option<&str>) -> Option<Arc<Image>> {
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

pub(crate) fn query_yubikey_info(probe_id: u64) -> YubiKeyInfo {
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

pub(crate) fn usb_reader_for_card(reader: Option<&str>, yubico: bool) -> Option<UsbReaderDetails> {
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

pub(crate) fn is_ccid_usb_device(path: &std::path::Path) -> bool {
    let Ok(interfaces) = std::fs::read_dir(path) else {
        return false;
    };
    interfaces.flatten().any(|interface| {
        std::fs::read_to_string(interface.path().join("bInterfaceClass"))
            .is_ok_and(|class| class.trim().eq_ignore_ascii_case("0b"))
    })
}
