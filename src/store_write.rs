use super::*;

#[cfg(unix)]
const ENTRY_LOCK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

#[cfg(unix)]
fn writer_lock_directory() -> Result<PathBuf, Error> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    let uid = rustix::process::geteuid().as_raw();
    let runtime = env::var_os("XDG_RUNTIME_DIR")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from);
    if let Some(runtime) = runtime.as_ref() {
        let metadata = fs::symlink_metadata(runtime)?;
        if !metadata.file_type().is_dir() || metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
            return Err(Error::UnsafeLockDirectory);
        }
    }
    let directory = runtime
        .as_ref()
        .map(|path| path.join("gpui-pass-locks"))
        .unwrap_or_else(|| env::temp_dir().join(format!("gpui-pass-locks-{uid}")));

    match fs::DirBuilder::new().mode(0o700).create(&directory) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(Error::Io(error)),
    }
    let metadata = fs::symlink_metadata(&directory)?;
    if !metadata.file_type().is_dir() || metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
        return Err(Error::UnsafeLockDirectory);
    }
    Ok(directory)
}

#[cfg(unix)]
fn writer_lock_name(root: &Path, entry: &Entry) -> String {
    use std::os::unix::ffi::OsStrExt;
    // Stable FNV-1a 128-bit name: lock identity must agree across processes.
    let mut hash = 0x6c62272e07bb014262b821756295c58du128;
    for byte in root
        .as_os_str()
        .as_bytes()
        .iter()
        .chain([0u8].iter())
        .chain(entry.relative_file().as_os_str().as_bytes())
    {
        hash ^= u128::from(*byte);
        hash = hash.wrapping_mul(0x0000000001000000000000000000013bu128);
    }
    format!("{hash:032x}.lock")
}

impl Store {
    /// Atomically create a new encrypted entry without following links or replacing a file.
    pub(crate) fn create_entry(
        &self,
        entry: &Entry,
        ciphertext: &[u8],
        policy: &RecipientPolicy,
    ) -> Result<(), Error> {
        self.write_entry(entry, ciphertext, None, policy)
    }

    /// Atomically replace an existing encrypted entry within the pinned store.
    pub(crate) fn replace_entry(
        &self,
        entry: &Entry,
        ciphertext: &[u8],
        expected: &EntryRevision,
        policy: &RecipientPolicy,
    ) -> Result<(), Error> {
        self.check_revision(entry, expected)?;
        self.write_entry(entry, ciphertext, Some(expected), policy)
    }

    pub fn entry_revision(&self, entry: &Entry) -> Result<EntryRevision, Error> {
        EntryRevision::from_file(&self.open_entry(entry)?)
    }

    pub(crate) fn check_revision(
        &self,
        entry: &Entry,
        expected: &EntryRevision,
    ) -> Result<(), Error> {
        match self.entry_revision(entry) {
            Ok(current) if current == *expected => Ok(()),
            _ => Err(Error::EntryConflict),
        }
    }

    #[cfg(unix)]
    pub(crate) fn acquire_entry_write_lock(&self, entry: &Entry) -> Result<File, Error> {
        use rustix::fs::{FlockOperation, Mode, OFlags, fcntl_lock, open, openat};
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let runtime_directory = writer_lock_directory()?;
        let directory = File::from(
            open(
                &runtime_directory,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(std::io::Error::from)?,
        );
        let lock_name = writer_lock_name(&self.root, entry);
        let lock = File::from(
            openat(
                &directory,
                std::ffi::OsStr::new(&lock_name),
                OFlags::RDWR
                    | OFlags::CREATE
                    | OFlags::NOFOLLOW
                    | OFlags::NONBLOCK
                    | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            )
            .map_err(std::io::Error::from)?,
        );
        let metadata = lock.metadata()?;
        if !metadata.is_file()
            || metadata.nlink() != 1
            || metadata.uid() != rustix::process::geteuid().as_raw()
        {
            return Err(Error::UnsafeLockDirectory);
        }
        lock.set_permissions(fs::Permissions::from_mode(0o600))?;
        let started = std::time::Instant::now();
        loop {
            match fcntl_lock(&lock, FlockOperation::NonBlockingLockExclusive) {
                Ok(()) => return Ok(lock),
                Err(error)
                    if std::io::Error::from(error).kind() == std::io::ErrorKind::WouldBlock =>
                {
                    if started.elapsed() >= ENTRY_LOCK_TIMEOUT {
                        return Err(Error::TimedOut);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(error) => return Err(Error::Io(std::io::Error::from(error))),
            }
        }
    }

    pub(crate) fn write_entry(
        &self,
        entry: &Entry,
        ciphertext: &[u8],
        expected: Option<&EntryRevision>,
        policy: &RecipientPolicy,
    ) -> Result<(), Error> {
        #[cfg(unix)]
        {
            use rustix::{
                fs::{AtFlags, Mode, OFlags, linkat, mkdirat, openat, renameat, unlinkat},
                io::Errno,
            };

            let parts: Vec<_> = entry.relative_file().components().collect();
            if parts.len() > MAX_DEPTH + 1 || parts.is_empty() {
                return Err(Error::InvalidEntry);
            }
            let (parent_parts, file_name) = parts.split_at(parts.len() - 1);
            let Component::Normal(file_name) = &file_name[0] else {
                return Err(Error::InvalidEntry);
            };
            let mut parent = self.directory.try_clone()?;
            for component in parent_parts {
                let Component::Normal(name) = component else {
                    return Err(Error::InvalidEntry);
                };
                let directory = match openat(
                    &parent,
                    *name,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                ) {
                    Ok(directory) => directory,
                    Err(error) if error == Errno::NOENT => {
                        match mkdirat(&parent, *name, Mode::RWXU) {
                            Ok(()) => {}
                            Err(error) if error == Errno::EXIST => {}
                            Err(_) => return Err(Error::Io(std::io::Error::from(error))),
                        }
                        openat(
                            &parent,
                            *name,
                            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                            Mode::empty(),
                        )
                        .map_err(|_| Error::InvalidEntry)?
                    }
                    Err(_) => return Err(Error::InvalidEntry),
                };
                parent = File::from(directory);
            }

            let _process_lock = ENTRY_WRITE_SERIALIZER
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let _interprocess_lock = self.acquire_entry_write_lock(entry)?;

            let mut random = [0u8; 16];
            getrandom::fill(&mut random).map_err(|_| Error::RandomUnavailable)?;
            let temporary = format!(
                ".gpui-pass-{}.tmp",
                random
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            );
            let temporary_os = std::ffi::OsStr::new(&temporary);
            let file = File::from(
                openat(
                    &parent,
                    temporary_os,
                    OFlags::WRONLY
                        | OFlags::CREATE
                        | OFlags::EXCL
                        | OFlags::CLOEXEC
                        | OFlags::NOFOLLOW,
                    Mode::RUSR | Mode::WUSR,
                )
                .map_err(std::io::Error::from)?,
            );
            let write_result = (|| {
                let mut file = file;
                file.write_all(ciphertext)?;
                file.sync_all()?;
                let current_policy = self.recipients_for_entry(entry).map_err(|error| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
                })?;
                if current_policy != *policy {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "recipient policy changed",
                    ));
                }
                if let Some(expected) = expected {
                    // Check the current destination after taking the process
                    // write mutex; a concurrent app save may have replaced it.
                    let destination = File::from(
                        openat(
                            &parent,
                            *file_name,
                            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                            Mode::empty(),
                        )
                        .map_err(std::io::Error::from)?,
                    );
                    if EntryRevision::from_file(&destination)
                        .map_err(|_| std::io::Error::other("entry conflict"))?
                        != *expected
                    {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::AlreadyExists,
                            "entry conflict",
                        ));
                    }
                    renameat(&parent, temporary_os, &parent, *file_name)
                        .map_err(std::io::Error::from)?;
                } else {
                    linkat(&parent, temporary_os, &parent, *file_name, AtFlags::empty())
                        .map_err(std::io::Error::from)?;
                }
                if expected.is_none() {
                    unlinkat(&parent, temporary_os, AtFlags::empty())
                        .map_err(std::io::Error::from)?;
                }
                parent.sync_all()?;
                Ok::<(), std::io::Error>(())
            })();
            if let Err(error) = write_result {
                let _ = unlinkat(&parent, temporary_os, AtFlags::empty());
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    return Err(if expected.is_some() {
                        Error::EntryConflict
                    } else {
                        Error::EntryAlreadyExists
                    });
                }
                if error.kind() == std::io::ErrorKind::InvalidData {
                    return Err(Error::RecipientPolicyChanged);
                }
                return Err(Error::Io(error));
            }
            Ok(())
        }
        #[cfg(not(unix))]
        {
            let _ = (entry, ciphertext, expected);
            Err(Error::UnsupportedPlatform)
        }
    }
}
