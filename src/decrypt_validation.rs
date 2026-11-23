use super::*;

impl Gpg {
    pub fn validate_recipients(&self, recipients: &[String]) -> Result<(), Error> {
        if recipients.is_empty() {
            return Err(Error::MissingRecipients);
        }
        let mut command = Command::new(&self.executable);
        command.args([
            "--no-options",
            "--batch",
            "--no-tty",
            "--no-auto-key-retrieve",
            "--no-auto-key-import",
            "--with-colons",
            "--list-keys",
        ]);
        command.arg("--");
        for recipient in recipients {
            command.arg(recipient);
        }
        let output = command.output().map_err(|_| Error::InvalidRecipients)?;
        if output.status.success() {
            Ok(())
        } else {
            Err(Error::InvalidRecipients)
        }
    }

    pub fn sign_detached(
        &self,
        contents: &[u8],
        signing_keys: &[String],
    ) -> Result<Vec<u8>, Error> {
        if signing_keys.is_empty() {
            return Err(Error::InvalidSigningKey);
        }
        let mut command = Command::new(&self.executable);
        command.args([
            "--no-options",
            "--batch",
            "--no-tty",
            "--pinentry-mode",
            "ask",
            "--no-auto-key-retrieve",
            "--no-auto-key-import",
            "--quiet",
            "--yes",
            "--output",
            "-",
        ]);
        for key in signing_keys {
            command.arg("--default-key").arg(key);
        }
        let mut child = command
            .arg("--detach-sign")
            .arg("--")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| Error::SigningFailed)?;
        child
            .stdin
            .take()
            .ok_or(Error::SigningFailed)?
            .write_all(contents)
            .map_err(|_| Error::SigningFailed)?;
        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if started.elapsed() < self.timeout => {
                    thread::sleep(Duration::from_millis(10));
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(Error::TimedOut);
                }
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(Error::SigningFailed);
                }
            }
        }
        let output = child.wait_with_output().map_err(|_| Error::SigningFailed)?;
        if output.status.success() && !output.stdout.is_empty() {
            Ok(output.stdout)
        } else {
            Err(Error::SigningFailed)
        }
    }

    pub(crate) fn recipient_policy(
        &self,
        store: &Store,
        entry: &Entry,
    ) -> Result<RecipientPolicy, Error> {
        let policy = store.recipients_for_entry(entry)?;
        if let Some(signature) = &policy.signature {
            self.verify_recipient_signature(
                policy.contents.as_deref().ok_or(Error::InvalidRecipients)?,
                signature,
                &policy.signing_keys,
            )?;
        }
        Ok(policy)
    }

    #[cfg(unix)]
    fn verify_recipient_signature(
        &self,
        contents: &[u8],
        signature: &[u8],
        trusted_keys: &[String],
    ) -> Result<(), Error> {
        use std::os::unix::fs::OpenOptionsExt;

        let mut random = [0u8; 16];
        getrandom::fill(&mut random).map_err(|_| Error::RandomUnavailable)?;
        let name = random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let signature_path = std::env::temp_dir().join(format!("gpui-pass-{name}.sig"));
        let contents_path = std::env::temp_dir().join(format!("gpui-pass-{name}.data"));
        let mut temporary = TemporaryFiles(Vec::new());
        let mut signature_file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&signature_path)?;
        temporary.0.push(signature_path.clone());
        signature_file.write_all(signature)?;
        signature_file.sync_all()?;
        drop(signature_file);
        let mut contents_file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&contents_path)?;
        temporary.0.push(contents_path.clone());
        contents_file.write_all(contents)?;
        contents_file.sync_all()?;
        drop(contents_file);

        let mut child = Command::new(&self.executable)
            .args([
                "--no-options",
                "--batch",
                "--no-tty",
                "--no-auto-key-retrieve",
                "--no-auto-key-import",
                "--status-fd=1",
                "--verify",
            ])
            .arg(&signature_path)
            .arg(&contents_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(Error::Io)?;
        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if started.elapsed() < SIGNATURE_VERIFY_TIMEOUT => {
                    thread::sleep(Duration::from_millis(10));
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(Error::TimedOut);
                }
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(Error::Io(error));
                }
            }
        }
        let output = child.wait_with_output()?;
        if !output.status.success() {
            return Err(Error::InvalidRecipientSignature);
        }
        let trusted = String::from_utf8_lossy(&output.stdout).lines().any(|line| {
            let Some(fields) = line.strip_prefix("[GNUPG:] VALIDSIG ") else {
                return false;
            };
            fields.split_whitespace().last().is_some_and(|fingerprint| {
                trusted_keys.iter().any(|trusted| trusted == fingerprint)
            })
        });
        if trusted {
            Ok(())
        } else {
            Err(Error::InvalidRecipientSignature)
        }
    }

    #[cfg(not(unix))]
    fn verify_recipient_signature(&self, _: &[u8], _: &[u8], _: &[String]) -> Result<(), Error> {
        Err(Error::UnsupportedPlatform)
    }
}
