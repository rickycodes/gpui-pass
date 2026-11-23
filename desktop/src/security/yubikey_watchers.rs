use super::*;

impl PassApp {
    pub(crate) fn start_card_watcher(&mut self) {
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
    pub(crate) fn start_usb_card_watcher(&mut self, cx: &mut Context<Self>) {
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
    pub(crate) fn start_usb_card_watcher(&mut self, _cx: &mut Context<Self>) {}
}
