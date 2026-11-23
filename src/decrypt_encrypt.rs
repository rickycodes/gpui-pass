use super::*;

impl Gpg {
    #[cfg(unix)]
    pub(crate) fn encrypt_secret(
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
                        Ok(count) => {
                            written += count;
                            made_progress = true;
                        }
                        Err(error)
                            if matches!(
                                error.kind(),
                                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                            ) => {}
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
    pub(crate) fn encrypt_secret(
        &self,
        _: &Secret,
        _: &[String],
        _: &Cancellation,
        _: &OperationProgress,
        _: bool,
    ) -> Result<Vec<u8>, Error> {
        Err(Error::UnsupportedPlatform)
    }
}
