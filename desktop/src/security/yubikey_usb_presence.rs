use super::*;

pub(crate) const USB_PRESENCE_SCAN_INTERVAL: Duration = Duration::from_millis(250);

pub(crate) fn connected_yubikey_usb_paths() -> HashSet<String> {
    scan_yubikey_usb_paths(std::path::Path::new("/sys"))
}

fn scan_yubikey_usb_paths(sys_root: &std::path::Path) -> HashSet<String> {
    let Ok(devices) = std::fs::read_dir(sys_root.join("bus/usb/devices")) else {
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
            // Match udev's absolute DEVPATH when seeding the watcher's set.
            let devpath = std::path::Path::new("/")
                .join(canonical.strip_prefix(sys_root).ok()?)
                .to_string_lossy()
                .into_owned();
            Some(devpath)
        })
        .collect()
}

#[cfg(all(test, target_os = "linux"))]
mod usb_presence_tests {
    use super::{scan_yubikey_usb_paths, update_yubikey_usb_presence};
    use std::{fs, os::unix::fs::symlink, path::PathBuf};

    struct UsbFixture(PathBuf);

    impl UsbFixture {
        fn new() -> Self {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "gpui-pass-usb-presence-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(root.join("bus/usb/devices")).unwrap();
            Self(root)
        }

        fn connect(&self, name: &str, vendor: &str, class: &str) {
            let device = self.0.join("devices").join(name);
            fs::create_dir_all(device.join("interface")).unwrap();
            fs::write(device.join("idVendor"), vendor).unwrap();
            fs::write(device.join("interface/bInterfaceClass"), class).unwrap();
            symlink(device, self.0.join("bus/usb/devices").join(name)).unwrap();
        }

        fn disconnect(&self, name: &str) {
            fs::remove_file(self.0.join("bus/usb/devices").join(name)).unwrap();
            fs::remove_dir_all(self.0.join("devices").join(name)).unwrap();
        }
    }

    impl Drop for UsbFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn scans_detect_unplug_and_replug_without_watcher_events() {
        let usb = UsbFixture::new();
        usb.connect("unrelated", "1234\n", "0b\n");
        usb.connect("hid-only", "1050\n", "03\n");
        assert!(scan_yubikey_usb_paths(&usb.0).is_empty());
        usb.connect("key", "1050\n", "0b\n");
        assert_eq!(
            scan_yubikey_usb_paths(&usb.0),
            ["/devices/key".into()].into()
        );
        usb.disconnect("key");
        assert!(scan_yubikey_usb_paths(&usb.0).is_empty());
        usb.connect("key", "1050\n", "0b\n");
        assert_eq!(
            scan_yubikey_usb_paths(&usb.0),
            ["/devices/key".into()].into()
        );
    }

    #[test]
    fn unplugging_one_of_two_keys_preserves_the_other() {
        let usb = UsbFixture::new();
        usb.connect("first", "1050\n", "0b\n");
        usb.connect("second", "1050\n", "0b\n");
        assert_eq!(scan_yubikey_usb_paths(&usb.0).len(), 2);
        usb.disconnect("first");
        assert_eq!(
            scan_yubikey_usb_paths(&usb.0),
            ["/devices/second".into()].into()
        );
    }
    #[test]
    fn udev_removal_matches_devices_seeded_from_sysfs() {
        let usb = UsbFixture::new();
        usb.connect("key", "1050\n", "0b\n");
        let mut known_devices = scan_yubikey_usb_paths(&usb.0);
        let event = |action: &str| {
            vec![
                format!("ACTION={action}"),
                "SUBSYSTEM=usb".into(),
                "DEVTYPE=usb_device".into(),
                "DEVPATH=/devices/key".into(),
            ]
        };
        usb.disconnect("key");
        assert_eq!(
            update_yubikey_usb_presence(&event("remove"), &mut known_devices),
            Some(false)
        );
        assert!(known_devices.is_empty());
        usb.connect("key", "1050\n", "0b\n");
        let mut added = event("add");
        added.extend([
            "ID_VENDOR_ID=1050".into(),
            "ID_USB_INTERFACES=:0b0000:".into(),
        ]);
        assert_eq!(
            update_yubikey_usb_presence(&added, &mut known_devices),
            Some(true)
        );
        assert_eq!(known_devices, scan_yubikey_usb_paths(&usb.0));
    }
}

pub(crate) fn update_yubikey_usb_presence(
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

pub(crate) fn normalize_usb_label(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
