use crate::{Entry, Error, Store};
use std::{
    fmt,
    fs::File,
    io::{self, Read},
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
        })
    }

    pub fn decrypt(
        &self,
        store: &Store,
        entry: &Entry,
        cancel: &Cancellation,
    ) -> Result<Secret, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        self.decrypt_file(store.open_entry(entry)?, cancel)
    }

    #[cfg(unix)]
    fn decrypt_file(&self, encrypted: File, cancel: &Cancellation) -> Result<Secret, Error> {
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
                    "--decrypt",
                ])
                .stdin(Stdio::from(encrypted))
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| Error::DecryptionFailed)?,
        );
        let mut stdout = child.0.stdout.take().ok_or(Error::DecryptionFailed)?;
        fcntl_setfl(&stdout, OFlags::NONBLOCK).map_err(|_| Error::DecryptionFailed)?;
        // Fixed capacity prevents reallocations leaving old plaintext allocations behind.
        let mut bytes = Zeroizing::new(vec![0_u8; MAX_SECRET_BYTES + 1]);
        let mut used = 0;
        let mut eof = false;
        let started = Instant::now();
        loop {
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            if started.elapsed() >= self.timeout {
                return Err(Error::TimedOut);
            }
            if !eof {
                match stdout.read(&mut bytes[used..]) {
                    Ok(0) => eof = true,
                    Ok(count) => {
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
            thread::sleep(Duration::from_millis(10));
        }
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        bytes.truncate(used);
        Secret::from_bytes(bytes)
    }

    #[cfg(not(unix))]
    fn decrypt_file(&self, _: File, _: &Cancellation) -> Result<Secret, Error> {
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
