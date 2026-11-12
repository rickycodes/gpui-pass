use crate::{
    Entry, Error, GeneratorOptions, OperationProgress, Store, generate_memorable_password,
    gpg_status::{CHECK_KEY, ENCRYPTING, StatusReader, UNLOCKING},
};
use std::{
    fmt,
    fs::File,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

pub const MAX_SECRET_BYTES: usize = 1024 * 1024;
const MAX_CIPHERTEXT_BYTES: usize = 2 * MAX_SECRET_BYTES;

/// Plaintext has no Clone/Display implementation; Debug is always redacted.
pub struct Secret(Zeroizing<Vec<u8>>);
impl Secret {
    pub fn from_bytes(bytes: Zeroizing<Vec<u8>>) -> Result<Self, Error> {
        if bytes.len() > MAX_SECRET_BYTES {
            return Err(Error::SecretTooLarge);
        }
        std::str::from_utf8(&bytes).map_err(|_| Error::InvalidText)?;
        Ok(Self(bytes))
    }
    pub fn expose(&self) -> &str {
        std::str::from_utf8(&self.0).expect("UTF-8 checked on construction")
    }
}
impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([REDACTED])")
    }
}

#[derive(Clone, Default)]
pub struct Cancellation(Arc<AtomicBool>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Clone)]
pub struct Gpg {
    executable: PathBuf,
    timeout: Duration,
    helper_delay: Duration,
}

impl Gpg {
    pub fn new(executable: impl AsRef<Path>) -> Result<Self, Error> {
        let executable = executable.as_ref();
        if !executable.is_absolute() || !executable.is_file() {
            return Err(Error::InvalidExecutable);
        }
        Ok(Self {
            executable: executable.to_owned(),
            timeout: Duration::from_secs(120),
            helper_delay: Duration::from_secs(2),
        })
    }

    pub fn decrypt(
        &self,
        store: &Store,
        entry: &Entry,
        cancel: &Cancellation,
    ) -> Result<Secret, Error> {
        self.decrypt_with_progress(store, entry, cancel, &OperationProgress::new(UNLOCKING))
    }

    pub fn decrypt_with_progress(
        &self,
        store: &Store,
        entry: &Entry,
        cancel: &Cancellation,
        progress: &OperationProgress,
    ) -> Result<Secret, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        progress.update(UNLOCKING);
        self.decrypt_file(store.open_entry(entry)?, cancel, progress)
    }

    pub fn generate_entry(
        &self,
        store: &Store,
        entry: &Entry,
        options: GeneratorOptions,
        cancel: &Cancellation,
        progress: &OperationProgress,
    ) -> Result<Secret, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let secret = generate_memorable_password(options)?;
        self.encrypt_new_entry(store, entry, &secret, cancel, progress)?;
        Ok(secret)
    }

    pub fn generate_pass_default_entry(
        &self,
        store: &Store,
        entry: &Entry,
        cancel: &Cancellation,
        progress: &OperationProgress,
    ) -> Result<Secret, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let secret = crate::generate_pass_default_password()?;
        self.encrypt_new_entry(store, entry, &secret, cancel, progress)?;
        Ok(secret)
    }

    pub fn update_entry(
        &self,
        store: &Store,
        entry: &Entry,
        secret: &Secret,
        cancel: &Cancellation,
        progress: &OperationProgress,
    ) -> Result<(), Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let recipients = store.recipients_for_entry(entry)?;
        let ciphertext = self.encrypt_secret(secret, &recipients, cancel, progress, false)?;
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        store.replace_entry(entry, &ciphertext)
    }

    fn encrypt_new_entry(
        &self,
        store: &Store,
        entry: &Entry,
        secret: &Secret,
        cancel: &Cancellation,
        progress: &OperationProgress,
    ) -> Result<(), Error> {
        let recipients = store.recipients_for_entry(entry)?;
        let ciphertext = self.encrypt_secret(secret, &recipients, cancel, progress, true)?;
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        store.create_entry(entry, &ciphertext)
    }

    #[cfg(unix)]
    fn encrypt_secret(
        &self,
        secret: &Secret,
        recipients: &[String],
        cancel: &Cancellation,
        progress: &OperationProgress,
        append_newline: bool,
    ) -> Result<Vec<u8>, Error> {
        use rustix::fs::{OFlags, fcntl_setfl};

        if recipients.is_empty() {
            return Err(Error::MissingRecipients);
        }
        progress.update(ENCRYPTING);
        let mut command = Command::new(&self.executable);
        command.args([
            "--no-options",
            "--batch",
            "--no-tty",
            "--pinentry-mode",
            "ask",
            "--no-auto-key-retrieve",
            "--no-auto-key-import",
            "--no-encrypt-to",
            "--quiet",
            "--yes",
            "--compress-algo=none",
            "--output",
            "-",
            "--status-fd",
            "2",
        ]);
        for recipient in recipients {
            command.arg("--recipient").arg(recipient);
        }
        let mut child = ChildGuard(
            command
                .arg("--encrypt")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|_| Error::EncryptionFailed)?,
        );
        let stdin = child.0.stdin.take().ok_or(Error::EncryptionFailed)?;
        let mut stdout = child.0.stdout.take().ok_or(Error::EncryptionFailed)?;
        let mut stderr = child.0.stderr.take().ok_or(Error::EncryptionFailed)?;
        fcntl_setfl(&stdout, OFlags::NONBLOCK).map_err(|_| Error::EncryptionFailed)?;
        fcntl_setfl(&stderr, OFlags::NONBLOCK).map_err(|_| Error::EncryptionFailed)?;
        fcntl_setfl(&stdin, OFlags::NONBLOCK).map_err(|_| Error::EncryptionFailed)?;
        let mut stdin = Some(stdin);
        let mut written = 0;
        let input = secret.expose().as_bytes();
        let input_len = input.len() + usize::from(append_newline);

        let started = Instant::now();
        let mut statuses = StatusReader::new();
        let mut ciphertext = Vec::with_capacity(8192);
        let mut chunk = [0u8; 8192];
        let mut eof = false;
        loop {
            let mut made_progress = false;
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            if started.elapsed() >= self.timeout {
                return Err(Error::TimedOut);
            }
            statuses
                .drain(&mut stderr, progress)
                .map_err(|_| Error::EncryptionFailed)?;
            // Feed stdin while draining both output pipes so backpressure cannot
            // bypass cancellation/timeouts or deadlock a child emitting diagnostics.
            if let Some(pipe) = stdin.as_mut() {
                let pending = if written < input.len() {
                    &input[written..input.len().min(written + 8192)]
                } else if written < input_len {
                    &b"\n"[..]
                } else {
                    &[]
                };
                if !pending.is_empty() {
                    match pipe.write(pending) {
                        Ok(0) => return Err(Error::EncryptionFailed),
                        Ok(count) => { written += count; made_progress = true; },
                        Err(error) if matches!(error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => {},
                        Err(_) => return Err(Error::EncryptionFailed),
                    }
                }
                if written == input_len {
                    stdin = None;
                }
            }
            // Some card backends stop reporting status while waiting for the physical touch.
            // Keep the prompt conditional because GnuPG cannot tell us whether the user has
            // finished the PIN dialog or whether this particular key requires a touch.
            if started.elapsed() >= self.helper_delay
                && (progress.snapshot() == UNLOCKING
                    || progress.snapshot() == crate::gpg_status::PINENTRY)
            {
                progress.update(CHECK_KEY);
            }
            if !eof {
                match stdout.read(&mut chunk) {
                    Ok(0) => eof = true,
                    Ok(count) => {
                        made_progress = true;
                        if ciphertext.len().saturating_add(count) > MAX_CIPHERTEXT_BYTES {
                            return Err(Error::EncryptionFailed);
                        }
                        ciphertext.extend_from_slice(&chunk[..count]);
                    }
                    Err(error)
                        if error.kind() == io::ErrorKind::WouldBlock
                            || error.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => return Err(Error::EncryptionFailed),
                }
            }
            if let Some(status) = child.0.try_wait().map_err(|_| Error::EncryptionFailed)? {
                if !status.success() {
                    return Err(Error::EncryptionFailed);
                }
                if eof {
                    break;
                }
            }
            if !made_progress {
                thread::sleep(Duration::from_millis(10));
            }
        }
        if cancel.is_cancelled() || ciphertext.is_empty() || written != input_len {
            return Err(if cancel.is_cancelled() {
                Error::Cancelled
            } else {
                Error::EncryptionFailed
            });
        }
        Ok(ciphertext)
    }

    #[cfg(not(unix))]
    fn encrypt_secret(
        &self,
        _: &Secret,
        _: &[String],
        _: &Cancellation,
        _: &OperationProgress,
        _: bool,
    ) -> Result<Vec<u8>, Error> {
        Err(Error::UnsupportedPlatform)
    }

    #[cfg(unix)]
    fn decrypt_file(
        &self,
        encrypted: File,
        cancel: &Cancellation,
        progress: &OperationProgress,
    ) -> Result<Secret, Error> {
        use rustix::fs::{OFlags, fcntl_setfl};
        // No shell, entry arguments, plaintext files, inherited stdin, or raw diagnostics.
        // Ignore gpg.conf (which could redirect plaintext); agent/scdaemon configuration
        // and GNUPGHOME remain in effect, including existing YubiKey key stubs and PIN/touch policy.
        let mut child = ChildGuard(
            Command::new(&self.executable)
                .args([
                    "--no-options",
                    "--batch",
                    "--no-tty",
                    "--pinentry-mode",
                    "ask",
                    "--no-auto-key-retrieve",
                    "--no-auto-key-import",
                    "--output",
                    "-",
                    "--max-output",
                    "1048576",
                    "--status-fd",
                    "2",
                    "--enable-progress-filter",
                    "--decrypt",
                ])
                .stdin(Stdio::from(encrypted))
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|_| Error::DecryptionFailed)?,
        );
        let mut stdout = child.0.stdout.take().ok_or(Error::DecryptionFailed)?;
        let mut stderr = child.0.stderr.take().ok_or(Error::DecryptionFailed)?;
        fcntl_setfl(&stdout, OFlags::NONBLOCK).map_err(|_| Error::DecryptionFailed)?;
        fcntl_setfl(&stderr, OFlags::NONBLOCK).map_err(|_| Error::DecryptionFailed)?;
        let mut statuses = StatusReader::new();
        // Fixed capacity prevents reallocations leaving old plaintext allocations behind.
        let mut bytes = Zeroizing::new(vec![0_u8; MAX_SECRET_BYTES + 1]);
        let mut used = 0;
        let mut eof = false;
        let started = Instant::now();
        loop {
            let mut made_progress = false;
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            if started.elapsed() >= self.timeout {
                return Err(Error::TimedOut);
            }
            statuses
                .drain(&mut stderr, progress)
                .map_err(|_| Error::DecryptionFailed)?;
            // Some GnuPG/card backends do not forward busy/touch progress during decryption.
            // This is an explicitly conditional helper, never a claim that touch is required.
            if started.elapsed() >= self.helper_delay
                && (progress.snapshot() == UNLOCKING
                    || progress.snapshot() == crate::gpg_status::PINENTRY)
            {
                progress.update(CHECK_KEY);
            }
            if !eof {
                match stdout.read(&mut bytes[used..]) {
                    Ok(0) => eof = true,
                    Ok(count) => {
                        made_progress = true;
                        used += count;
                        if used > MAX_SECRET_BYTES {
                            return Err(Error::SecretTooLarge);
                        }
                    }
                    Err(e)
                        if e.kind() == io::ErrorKind::WouldBlock
                            || e.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => return Err(Error::DecryptionFailed),
                }
            }
            if let Some(status) = child.0.try_wait().map_err(|_| Error::DecryptionFailed)? {
                // GPG can emit plaintext before detecting an integrity failure. Never publish it.
                if !status.success() {
                    return Err(Error::DecryptionFailed);
                }
                if eof {
                    break;
                }
            }
            if !made_progress {
                thread::sleep(Duration::from_millis(10));
            }
        }
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        bytes.truncate(used);
        Secret::from_bytes(bytes)
    }

    #[cfg(not(unix))]
    fn decrypt_file(
        &self,
        _: File,
        _: &Cancellation,
        _: &OperationProgress,
    ) -> Result<Secret, Error> {
        Err(Error::UnsupportedPlatform)
    }
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };
    use tempfile::{TempDir, tempdir};

    fn fake_gpg(body: &str) -> (TempDir, Gpg, Store, Entry) {
        let directory = tempdir().unwrap();
        let script = directory.path().join("gpg");
        fs::write(
            &script,
            format!("#!/usr/bin/python3\nimport sys, time\n{body}\n"),
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(directory.path().join("entry.gpg"), b"ciphertext").unwrap();
        let gpg = Gpg::new(script).unwrap();
        let store = Store::open(directory.path()).unwrap();
        (
            directory,
            gpg,
            store,
            Entry::from_relative_file("entry.gpg").unwrap(),
        )
    }

    fn wait_for_status(progress: &OperationProgress, title: &str) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while progress.snapshot().title != title {
            assert!(Instant::now() < deadline, "operation hint was not received");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn edits_preserve_exact_multiline_bytes_and_generated_entries_add_one_newline() {
        let (dir, gpg, store, entry) = fake_gpg(
            "sys.stdout.buffer.write(b'cipher:' + sys.stdin.buffer.read())",
        );
        fs::write(dir.path().join(".gpg-id"), b"fixture-recipient\n").unwrap();
        let progress = OperationProgress::new(ENCRYPTING);
        for text in ["password\nnotes\n", "password\nnotes", ""] {
            let secret = Secret::from_bytes(Zeroizing::new(text.as_bytes().to_vec())).unwrap();
            for _ in 0..2 {
                gpg.update_entry(&store, &entry, &secret, &Cancellation::default(), &progress).unwrap();
                assert_eq!(&fs::read(dir.path().join("entry.gpg")).unwrap()[7..], text.as_bytes());
            }
        }
        let secret = Secret::from_bytes(Zeroizing::new(b"generated".to_vec())).unwrap();
        let new_entry = Entry::from_relative_file("new.gpg").unwrap();
        gpg.encrypt_new_entry(&store, &new_entry, &secret, &Cancellation::default(), &progress).unwrap();
        assert_eq!(fs::read(dir.path().join("new.gpg")).unwrap(), b"cipher:generated\n");
    }

    #[test]
    fn encryption_drains_output_before_child_reads_large_input() {
        let (_dir, mut gpg, _, _) = fake_gpg(
            "sys.stderr.buffer.write(b'x' * 131072 + b'\\n')\nsys.stderr.flush()\nsys.stdout.buffer.write(sys.stdin.buffer.read())",
        );
        gpg.timeout = Duration::from_secs(5);
        let secret = Secret::from_bytes(Zeroizing::new(vec![b'x'; MAX_SECRET_BYTES])).unwrap();
        let output = gpg.encrypt_secret(&secret, &["fixture".into()], &Cancellation::default(),
            &OperationProgress::new(ENCRYPTING), false).unwrap();
        assert_eq!(output.len(), MAX_SECRET_BYTES);
    }

    #[test]
    fn encryption_deadline_applies_when_child_never_reads_stdin() {
        let (_dir, mut gpg, _, _) = fake_gpg("time.sleep(10)");
        gpg.timeout = Duration::from_millis(80);
        let secret = Secret::from_bytes(Zeroizing::new(vec![b'x'; MAX_SECRET_BYTES])).unwrap();
        let started = Instant::now();
        assert!(matches!(gpg.encrypt_secret(&secret, &["fixture".into()],
            &Cancellation::default(), &OperationProgress::new(ENCRYPTING), false), Err(Error::TimedOut)));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn detached_recipient_signatures_block_updates() {
        let (dir, gpg, store, entry) = fake_gpg("raise AssertionError('must not encrypt')");
        fs::write(dir.path().join(".gpg-id"), b"fixture\n").unwrap();
        fs::write(dir.path().join(".gpg-id.sig"), b"signature").unwrap();
        let secret = Secret::from_bytes(Zeroizing::new(b"edited".to_vec())).unwrap();
        assert!(matches!(gpg.update_entry(&store, &entry, &secret, &Cancellation::default(),
            &OperationProgress::new(ENCRYPTING)), Err(Error::SignedRecipientsUnsupported)));
        assert_eq!(fs::read(dir.path().join("entry.gpg")).unwrap(), b"ciphertext");
    }

    #[test]
    fn reports_card_wait_before_completion_but_still_rejects_failed_plaintext() {
        let (dir, mut gpg, store, entry) = fake_gpg(
            "from pathlib import Path\nsys.stdout.write('untrusted plaintext')\nsys.stdout.flush()\nsys.stderr.write('private diagnostic to discard\\n[GNUPG:] PROGRESS card_busy w 0 0\\n')\nsys.stderr.flush()\nwhile not Path(__file__).with_name('finish').exists(): time.sleep(0.005)\nsys.exit(2)",
        );
        gpg.timeout = Duration::from_secs(3);
        let progress = OperationProgress::new(UNLOCKING);
        let worker_progress = progress.clone();
        let worker = thread::spawn(move || {
            gpg.decrypt_with_progress(&store, &entry, &Cancellation::default(), &worker_progress)
        });
        wait_for_status(&progress, "Check your YubiKey");
        assert!(
            progress
                .snapshot()
                .detail
                .contains("If your YubiKey is blinking")
        );
        assert!(!worker.is_finished());
        fs::write(dir.path().join("finish"), b"").unwrap();
        assert!(matches!(
            worker.join().unwrap(),
            Err(Error::DecryptionFailed)
        ));
    }

    #[test]
    fn older_gpg_without_status_gets_a_conditional_helper() {
        let (dir, mut gpg, store, entry) = fake_gpg(
            "from pathlib import Path\nwhile not Path(__file__).with_name('finish').exists(): time.sleep(0.005)\nprint('fixture password')",
        );
        gpg.timeout = Duration::from_secs(3);
        gpg.helper_delay = Duration::from_millis(30);
        let progress = OperationProgress::new(UNLOCKING);
        let worker_progress = progress.clone();
        let worker = thread::spawn(move || {
            gpg.decrypt_with_progress(&store, &entry, &Cancellation::default(), &worker_progress)
        });
        wait_for_status(&progress, CHECK_KEY.title);
        assert_eq!(progress.snapshot(), CHECK_KEY);
        fs::write(dir.path().join("finish"), b"").unwrap();
        assert_eq!(
            worker.join().unwrap().unwrap().expose(),
            "fixture password\n"
        );
    }

    #[test]
    fn drains_diagnostic_floods_without_deadlocking_the_child() {
        let (_dir, mut gpg, store, entry) = fake_gpg(
            "sys.stderr.buffer.write(b'x' * 131072 + b'\\n')\nsys.stderr.flush()\nprint('fixture password')",
        );
        gpg.timeout = Duration::from_secs(3);
        let progress = OperationProgress::new(UNLOCKING);
        assert_eq!(
            gpg.decrypt_with_progress(&store, &entry, &Cancellation::default(), &progress)
                .unwrap()
                .expose(),
            "fixture password\n"
        );
        assert_eq!(progress.snapshot(), UNLOCKING);
    }

    #[test]
    fn diagnostic_floods_do_not_starve_cancellation() {
        let (_dir, mut gpg, store, entry) =
            fake_gpg("while True:\n sys.stderr.buffer.write(b'x' * 65536)\n sys.stderr.flush()");
        gpg.timeout = Duration::from_secs(3);
        let cancel = Cancellation::default();
        let worker_cancel = cancel.clone();
        let worker = thread::spawn(move || gpg.decrypt(&store, &entry, &worker_cancel));
        thread::sleep(Duration::from_millis(50));
        cancel.cancel();
        assert!(matches!(worker.join().unwrap(), Err(Error::Cancelled)));
    }

    #[test]
    fn accepts_complete_multiline_output_only_after_success() {
        let (_dir, gpg, store, entry) =
            fake_gpg("sys.stdout.write('password\\nusername: test\\n\\nnotes\\n')");
        let secret = gpg
            .decrypt(&store, &entry, &Cancellation::default())
            .unwrap();
        assert_eq!(secret.expose(), "password\nusername: test\n\nnotes\n");
    }

    #[test]
    fn never_exposes_plaintext_or_stderr_from_a_failed_decryption() {
        let (_dir, gpg, store, entry) = fake_gpg(
            "print('untrusted plaintext')\nprint('sensitive diagnostics', file=sys.stderr)\nsys.exit(2)",
        );
        assert!(matches!(
            gpg.decrypt(&store, &entry, &Cancellation::default()),
            Err(Error::DecryptionFailed)
        ));
    }

    #[test]
    fn bounded_output_rejects_oversized_and_binary_content() {
        let (_dir, gpg, store, entry) = fake_gpg("sys.stdout.buffer.write(b'x' * 1048577)");
        assert!(matches!(
            gpg.decrypt(&store, &entry, &Cancellation::default()),
            Err(Error::SecretTooLarge)
        ));
        let (_dir, gpg, store, entry) = fake_gpg("sys.stdout.buffer.write(b'\\xff')");
        assert!(matches!(
            gpg.decrypt(&store, &entry, &Cancellation::default()),
            Err(Error::InvalidText)
        ));
    }

    #[test]
    fn cancellation_and_deadline_stop_running_children() {
        let (_dir, mut gpg, store, entry) = fake_gpg("time.sleep(10)");
        gpg.timeout = Duration::from_millis(60);
        let now = Instant::now();
        assert!(matches!(
            gpg.decrypt(&store, &entry, &Cancellation::default()),
            Err(Error::TimedOut)
        ));
        assert!(now.elapsed() < Duration::from_secs(2));
        gpg.timeout = Duration::from_secs(10);
        let cancel = Cancellation::default();
        let child_cancel = cancel.clone();
        let worker = thread::spawn(move || gpg.decrypt(&store, &entry, &child_cancel));
        thread::sleep(Duration::from_millis(40));
        cancel.cancel();
        assert!(matches!(worker.join().unwrap(), Err(Error::Cancelled)));
    }

    #[test]
    fn refuses_symlink_swaps_hardlinks_and_parent_directory_swaps() {
        let (dir, gpg, store, entry) = fake_gpg("print('must never be reached')");
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join("outside.gpg");
        fs::write(&outside_file, b"outside ciphertext").unwrap();
        fs::remove_file(dir.path().join("entry.gpg")).unwrap();
        symlink(&outside_file, dir.path().join("entry.gpg")).unwrap();
        assert!(matches!(
            gpg.decrypt(&store, &entry, &Cancellation::default()),
            Err(Error::InvalidEntry)
        ));
        fs::remove_file(dir.path().join("entry.gpg")).unwrap();
        fs::hard_link(&outside_file, dir.path().join("entry.gpg")).unwrap();
        assert!(matches!(
            gpg.decrypt(&store, &entry, &Cancellation::default()),
            Err(Error::InvalidEntry)
        ));
        symlink(outside.path(), dir.path().join("nested")).unwrap();
        let entry = Entry::from_relative_file("nested/outside.gpg").unwrap();
        assert!(matches!(
            gpg.decrypt(&store, &entry, &Cancellation::default()),
            Err(Error::InvalidEntry)
        ));
    }

    #[test]
    fn metacharacter_names_are_read_from_stdin_not_command_arguments() {
        let (dir, gpg, store, _) = fake_gpg(
            "assert sys.stdin.buffer.read() == b'ciphertext'\nassert not any('touch' in arg for arg in sys.argv)\nprint('safe')",
        );
        let entry = Entry::from_relative_file("--output $(touch marker); 'quoted'.gpg").unwrap();
        fs::write(dir.path().join(entry.relative_file()), b"ciphertext").unwrap();
        assert_eq!(
            gpg.decrypt(&store, &entry, &Cancellation::default())
                .unwrap()
                .expose(),
            "safe\n"
        );
        assert!(!dir.path().join("marker").exists());
    }

    #[test]
    fn pinned_root_survives_store_path_replacement() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("store");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("original.gpg"), b"original").unwrap();
        let store = Store::open(&root).unwrap();
        fs::rename(&root, directory.path().join("moved")).unwrap();
        fs::create_dir(&root).unwrap();
        fs::write(root.join("replacement.gpg"), b"replacement").unwrap();
        assert_eq!(store.entries().unwrap()[0].name(), "original");
        assert!(
            store
                .open_entry(&Entry::from_relative_file("replacement.gpg").unwrap())
                .is_err()
        );
    }
}
