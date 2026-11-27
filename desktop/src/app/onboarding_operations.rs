use super::*;
use crate::app_state::OnboardingState;
use std::path::Path;

fn initialize_store(
    path: &Path,
    executable: &Path,
    recipients: &[String],
    signing_keys: &[String],
) -> Result<(), Error> {
    let gpg = Gpg::new(executable)?;
    gpg.validate_recipients(recipients)?;
    let contents = recipients.join("\n") + "\n";
    let signature = if signing_keys.is_empty() {
        None
    } else {
        Some(gpg.sign_detached(contents.as_bytes(), signing_keys)?)
    };
    Store::initialize(path, recipients, signature.as_deref()).map(|_| ())
}

impl PassApp {
    pub(super) fn open_onboarding(&mut self, path: PathBuf, preview: bool, cx: &mut Context<Self>) {
        self.clear_create_entry();
        self.state.onboarding = Some(OnboardingState { path, preview });
        if preview {
            self.state.custom_entry_name = constants::DEMO_RECIPIENT.into();
            self.state.custom_entry_name_selection =
                self.state.custom_entry_name.len()..self.state.custom_entry_name.len();
            self.state.custom_entry_name_cursor = self.state.custom_entry_name.len();
        }
        self.state.onboarding_pending = None;
        self.clear_message();
        cx.notify();
    }
    pub(super) fn finish_onboarding_preview(&mut self, cx: &mut Context<Self>) {
        self.state.onboarding = None;
        self.state.onboarding_pending = None;
        self.state.onboarding_demo = false;
        self.state.demo = true;
        self.state.demo_empty_store = true;
        self.state.path = None;
        self.clear_create_entry();
        self.refresh(cx);
    }

    pub(super) fn initialize_onboarding(&mut self, cx: &mut Context<Self>) {
        if self.state.onboarding_pending.is_some() || self.state.store_picker_open {
            return;
        }
        let Some(onboarding) = self.state.onboarding.as_ref() else {
            return;
        };
        let preview = onboarding.preview;
        let recipients: Vec<_> = self
            .state
            .custom_entry_name
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        if recipients.is_empty() {
            self.set_error("Enter at least one GPG recipient.".into());
            cx.notify();
            return;
        }
        if preview {
            self.finish_onboarding_preview(cx);
            return;
        }
        let path = onboarding.path.clone();
        let executable = self.state.gpg.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let result = (|| {
                let signing_keys = configured_signing_keys()?;
                initialize_store(&path, &executable, &recipients, &signing_keys)
            })();
            let _ = sender.send(result);
        });
        self.clear_message();
        self.state.onboarding_pending = Some(receiver);
        cx.notify();
    }

    pub(super) fn poll_onboarding(&mut self, cx: &mut Context<Self>) {
        let Some(result) = self
            .state
            .onboarding_pending
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
        else {
            return;
        };
        self.state.onboarding_pending = None;
        match result {
            Ok(()) => {
                let path = self
                    .state
                    .onboarding
                    .as_ref()
                    .map(|onboarding| onboarding.path.clone());
                self.state.onboarding = None;
                self.state.custom_entry_name.clear();
                self.state.custom_entry_name_selection = 0..0;
                self.state.custom_entry_name_cursor = 0;
                self.state.path = path.clone();
                self.state.persist_path_on_open = None;
                self.refresh(cx);
                if let Some(path) = path {
                    if let Err(error) = store_config::save_store_path(&path) {
                        self.set_error(format!(
                            "Store initialized, but its location could not be saved: {error}"
                        ));
                    }
                }
            }
            Err(error) => self.set_error(error.to_string()),
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn fixture_root(label: &str) -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after the Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "gpui-pass-onboarding-{label}-{}-{timestamp}",
            std::process::id()
        ))
    }

    fn fake_gpg(path: &Path, signing_succeeds: bool) {
        let signing_body = if signing_succeeds {
            "    sys.stdout.buffer.write(b'signature')\n    sys.exit(0)"
        } else {
            "    sys.exit(1)"
        };
        let script = format!(
            "#!/usr/bin/python3\nimport sys\nif '--list-keys' in sys.argv:\n    sys.exit(0)\nif '--detach-sign' in sys.argv:\n    assert sys.stdin.buffer.read() == b'alice@example.com\\n'\n{signing_body}\nsys.exit(1)\n"
        );
        fs::write(path, script).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }

    #[test]
    fn signed_initialization_writes_policy_and_preserves_non_empty_targets() {
        let root = fixture_root("signed");
        fs::create_dir_all(&root).unwrap();
        let executable = root.join("gpg");
        fake_gpg(&executable, true);
        let recipients = vec!["alice@example.com".to_owned()];
        let signing_keys = vec!["TEST-FINGERPRINT".to_owned()];
        let store = root.join("store");

        initialize_store(&store, &executable, &recipients, &signing_keys).unwrap();

        assert_eq!(
            fs::read(store.join(".gpg-id")).unwrap(),
            b"alice@example.com\n"
        );
        assert_eq!(fs::read(store.join(".gpg-id.sig")).unwrap(), b"signature");
        assert_eq!(
            fs::metadata(&store).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(store.join(".gpg-id"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        let non_empty = root.join("non-empty");
        fs::create_dir(&non_empty).unwrap();
        fs::write(non_empty.join("KEEP.txt"), b"preserve").unwrap();
        assert!(matches!(
            initialize_store(&non_empty, &executable, &recipients, &signing_keys),
            Err(Error::StoreNotEmpty)
        ));
        assert_eq!(fs::read(non_empty.join("KEEP.txt")).unwrap(), b"preserve");
        assert!(!non_empty.join(".gpg-id").exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_signature_does_not_create_a_store() {
        let root = fixture_root("failed-signature");
        fs::create_dir_all(&root).unwrap();
        let executable = root.join("gpg");
        fake_gpg(&executable, false);
        let store = root.join("store");
        let recipients = vec!["alice@example.com".to_owned()];
        let signing_keys = vec!["TEST-FINGERPRINT".to_owned()];

        assert!(matches!(
            initialize_store(&store, &executable, &recipients, &signing_keys),
            Err(Error::SigningFailed)
        ));
        assert!(!store.exists());

        fs::remove_dir_all(root).unwrap();
    }
}
