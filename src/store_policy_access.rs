use super::*;

impl Store {
    pub(crate) fn recipients_for_entry(&self, entry: &Entry) -> Result<RecipientPolicy, Error> {
        if let Some(keys) = env::var_os("PASSWORD_STORE_KEY").filter(|value| !value.is_empty()) {
            return Ok(RecipientPolicy {
                recipients: parse_recipients(&keys.to_string_lossy())?,
                contents: None,
                signature: None,
                signing_keys: Vec::new(),
            });
        }
        let signing_keys = configured_signing_keys()?;
        #[cfg(unix)]
        {
            use rustix::{
                fs::{Mode, OFlags, openat},
                io::Errno,
            };
            let mut directories = vec![self.directory.try_clone()?];
            if let Some(parent) = entry.relative_file().parent() {
                for component in parent.components() {
                    let Component::Normal(name) = component else {
                        return Err(Error::InvalidEntry);
                    };
                    let current = directories.last().expect("root directory");
                    match openat(
                        current,
                        name,
                        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                        Mode::empty(),
                    ) {
                        Ok(directory) => directories.push(File::from(directory)),
                        Err(error) if error == Errno::NOENT => break,
                        Err(_) => return Err(Error::InvalidEntry),
                    }
                }
            }
            for directory in directories.iter().rev() {
                let config = match openat(
                    directory,
                    GPG_ID_FILE,
                    OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
                    Mode::empty(),
                ) {
                    Ok(config) => File::from(config),
                    Err(error) if error == Errno::NOENT => continue,
                    Err(_) => return Err(Error::InvalidRecipients),
                };
                // A detached signature declares a signed recipient policy even
                // when PASSWORD_STORE_SIGNING_KEY is absent in this process.
                let signature = match openat(
                    directory,
                    GPG_ID_SIGNATURE_FILE,
                    OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
                    Mode::empty(),
                ) {
                    Ok(signature) => {
                        let signature = File::from(signature);
                        let metadata = signature.metadata()?;
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::MetadataExt;
                            if !metadata.is_file() || metadata.nlink() != 1 {
                                return Err(Error::InvalidRecipients);
                            }
                        }
                        if !metadata.is_file() || metadata.len() > MAX_RECIPIENTS_BYTES as u64 {
                            return Err(Error::InvalidRecipients);
                        }
                        let mut bytes = Vec::with_capacity(metadata.len() as usize);
                        signature
                            .take((MAX_RECIPIENTS_BYTES + 1) as u64)
                            .read_to_end(&mut bytes)
                            .map_err(|_| Error::InvalidRecipients)?;
                        if bytes.len() > MAX_RECIPIENTS_BYTES {
                            return Err(Error::InvalidRecipients);
                        }
                        Some(bytes)
                    }
                    Err(error) if error == Errno::NOENT => None,
                    Err(_) => return Err(Error::InvalidRecipients),
                };
                if !signing_keys.is_empty() && signature.is_none() {
                    return Err(Error::UnsignedRecipients);
                }
                if signing_keys.is_empty() && signature.is_some() {
                    return Err(Error::RecipientSignatureRequired);
                }
                let metadata = config.metadata()?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if !metadata.is_file() || metadata.nlink() != 1 {
                        return Err(Error::InvalidRecipients);
                    }
                }
                if !metadata.is_file() {
                    return Err(Error::InvalidRecipients);
                }
                let mut contents = Vec::with_capacity(1024);
                config
                    .take(65_537)
                    .read_to_end(&mut contents)
                    .map_err(|_| Error::InvalidRecipients)?;
                if contents.len() > MAX_RECIPIENTS_BYTES {
                    return Err(Error::InvalidRecipients);
                }
                let recipients_text =
                    std::str::from_utf8(&contents).map_err(|_| Error::InvalidRecipients)?;
                return Ok(RecipientPolicy {
                    recipients: parse_recipients(recipients_text)?,
                    contents: Some(contents),
                    signature,
                    signing_keys,
                });
            }
            Err(Error::MissingRecipients)
        }
        #[cfg(not(unix))]
        {
            let _ = entry;
            Err(Error::UnsupportedPlatform)
        }
    }
}
