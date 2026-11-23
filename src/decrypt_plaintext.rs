use super::*;

impl Gpg {
    #[cfg(unix)]
    pub(crate) fn decrypt_file(
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
    pub(crate) fn decrypt_file(
        &self,
        _: File,
        _: &Cancellation,
        _: &OperationProgress,
    ) -> Result<Secret, Error> {
        Err(Error::UnsupportedPlatform)
    }
}
