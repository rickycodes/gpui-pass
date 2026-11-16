use crate::Error;
use std::{
    env,
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::Arc,
};

#[cfg(unix)]
static ENTRY_WRITE_SERIALIZER: std::sync::Mutex<()> = std::sync::Mutex::new(());
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

const MAX_ENTRIES: usize = 50_000;
const MAX_NODES: usize = 100_000;
const MAX_DEPTH: usize = 32;
const MAX_RECIPIENTS_BYTES: usize = 65_536;

fn parse_recipients(text: &str) -> Result<Vec<String>, Error> {
    let mut recipients = Vec::new();
    for line in text.lines() {
        for recipient in line
            .split('#')
            .next()
            .unwrap_or_default()
            .split_whitespace()
        {
            if recipient.len() > 256 || recipient.chars().any(char::is_control) {
                return Err(Error::InvalidRecipients);
            }
            if !recipients.iter().any(|existing| existing == recipient) {
                recipients.push(recipient.to_owned());
                if recipients.len() > 256 {
                    return Err(Error::InvalidRecipients);
                }
            }
        }
    }
    if recipients.is_empty() {
        Err(Error::MissingRecipients)
    } else {
        Ok(recipients)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RecipientPolicy {
    pub recipients: Vec<String>,
    pub contents: Option<Vec<u8>>,
    pub signature: Option<Vec<u8>>,
    pub signing_keys: Vec<String>,
}

pub fn default_store_path() -> Result<PathBuf, Error> {
    if let Some(path) = env::var_os("PASSWORD_STORE_DIR").filter(|v| !v.is_empty()) {
        return Ok(path.into());
    }
    env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(|home| PathBuf::from(home).join(".password-store"))
        .ok_or(Error::MissingHome)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    relative_file: PathBuf,
    name: String,
    search_name: String,
}

impl Entry {
    /// Accept a relative name including its .gpg extension. Never normalizes traversal.
    pub fn from_relative_file(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref();
        let text = path.to_str().ok_or(Error::InvalidEntry)?;
        if text.is_empty()
            || text.chars().any(|c| c.is_control() || is_bidi_control(c))
            || text.contains('\\')
            || text
                .split('/')
                .any(|p| p.is_empty() || p == "." || p == "..")
            || path
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
            || path.extension().is_none_or(|e| e != "gpg")
            || path.file_stem().is_none_or(|s| s.is_empty())
        {
            return Err(Error::InvalidEntry);
        }
        let name = text[..text.len() - 4].to_owned();
        let search_name = name.to_lowercase();
        Ok(Self {
            relative_file: path.to_owned(),
            name,
            search_name,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn title(&self) -> &str {
        self.name.rsplit('/').next().unwrap_or(&self.name)
    }
    pub fn folder(&self) -> &str {
        self.name.rsplit_once('/').map_or("", |(folder, _)| folder)
    }
    pub fn relative_file(&self) -> &Path {
        &self.relative_file
    }
    pub fn matches(&self, query: &str) -> bool {
        let terms: Vec<_> = query.split_whitespace().map(str::to_lowercase).collect();
        self.matches_terms(&terms)
    }

    /// Match a query normalized once for an entire store search.
    pub fn matches_terms(&self, terms: &[String]) -> bool {
        terms.iter().all(|term| self.search_name.contains(term))
    }
}

fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// Identity and change timestamps for an encrypted entry; contains no plaintext.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntryRevision {
    #[cfg(unix)]
    identity: [i128; 7],
}

impl EntryRevision {
    pub(crate) fn from_file(file: &File) -> Result<Self, Error> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = file.metadata()?;
            Ok(Self {
                identity: [
                    metadata.dev() as i128,
                    metadata.ino() as i128,
                    metadata.len() as i128,
                    metadata.mtime() as i128,
                    metadata.mtime_nsec() as i128,
                    metadata.ctime() as i128,
                    metadata.ctime_nsec() as i128,
                ],
            })
        }
        #[cfg(not(unix))]
        {
            let _ = file;
            Err(Error::UnsupportedPlatform)
        }
    }
}

#[derive(Clone)]
pub struct Store {
    root: PathBuf,
    directory: Arc<File>,
}

impl Store {
    #[cfg(unix)]
    pub fn open(root: impl AsRef<Path>) -> Result<Self, Error> {
        let root = fs::canonicalize(root)?;
        let directory = {
            use rustix::fs::{Mode, OFlags, open};
            File::from(
                open(
                    &root,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                )
                .map_err(std::io::Error::from)?,
            )
        };
        Ok(Self {
            root,
            directory: Arc::new(directory),
        })
    }

    #[cfg(not(unix))]
    pub fn open(_: impl AsRef<Path>) -> Result<Self, Error> {
        Err(Error::UnsupportedPlatform)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Listing reads filenames only. Decryption always reopens through pinned directory handles.
    pub fn entries(&self) -> Result<Vec<Entry>, Error> {
        let mut entries = Vec::new();
        let mut nodes = 0;
        self.visit(&self.directory, Path::new(""), 0, &mut nodes, &mut entries)?;
        entries.sort_by_cached_key(|entry| (entry.name.to_lowercase(), entry.name.clone()));
        Ok(entries)
    }

    #[cfg(unix)]
    fn visit(
        &self,
        directory: &File,
        relative: &Path,
        depth: usize,
        nodes: &mut usize,
        entries: &mut Vec<Entry>,
    ) -> Result<(), Error> {
        use rustix::fs::{AtFlags, Dir, FileType, statat};
        use std::os::unix::ffi::OsStrExt;
        if depth > MAX_DEPTH {
            return Err(Error::StoreTooLarge);
        }
        for item in Dir::read_from(directory).map_err(std::io::Error::from)? {
            let item = item.map_err(std::io::Error::from)?;
            let name = std::ffi::OsStr::from_bytes(item.file_name().to_bytes());
            if name == "." || name == ".." || name == ".git" || name == ".extensions" {
                continue;
            }
            *nodes += 1;
            if *nodes > MAX_NODES {
                return Err(Error::StoreTooLarge);
            }
            let file_type = FileType::from_raw_mode(
                statat(directory, name, AtFlags::SYMLINK_NOFOLLOW)
                    .map_err(std::io::Error::from)?
                    .st_mode,
            );
            let path = relative.join(name);
            if file_type == FileType::Directory {
                use rustix::fs::{Mode, OFlags, openat};
                let child = File::from(
                    openat(
                        directory,
                        name,
                        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                        Mode::empty(),
                    )
                    .map_err(|_| Error::InvalidEntry)?,
                );
                self.visit(&child, &path, depth + 1, nodes, entries)?;
            } else if file_type == FileType::RegularFile
                && path.extension().is_some_and(|e| e == "gpg")
            {
                // Unsupported display names are omitted; raw paths never become markup or commands.
                if let Ok(entry) = Entry::from_relative_file(path) {
                    entries.push(entry);
                }
                if entries.len() > MAX_ENTRIES {
                    return Err(Error::StoreTooLarge);
                }
            }
        }
        Ok(())
    }

    #[cfg(not(unix))]
    fn visit(
        &self,
        _: &File,
        _: &Path,
        _: usize,
        _: &mut usize,
        _: &mut Vec<Entry>,
    ) -> Result<(), Error> {
        Err(Error::UnsupportedPlatform)
    }

    pub(crate) fn open_entry(&self, entry: &Entry) -> Result<File, Error> {
        let file = self.open_relative(entry.relative_file(), false)?;
        let metadata = file.metadata()?;
        // Reject hard links too: a store entry should not alias another user's file.
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if !metadata.is_file() || metadata.nlink() != 1 {
                return Err(Error::InvalidEntry);
            }
        }
        Ok(file)
    }

    pub(crate) fn recipients_for_entry(&self, entry: &Entry) -> Result<RecipientPolicy, Error> {
        if let Some(keys) = env::var_os("PASSWORD_STORE_KEY").filter(|value| !value.is_empty()) {
            return Ok(RecipientPolicy {
                recipients: parse_recipients(&keys.to_string_lossy())?,
                contents: None,
                signature: None,
                signing_keys: Vec::new(),
            });
        }
        let signing_keys =
            match env::var_os("PASSWORD_STORE_SIGNING_KEY").filter(|value| !value.is_empty()) {
                Some(value) => {
                    let keys: Vec<_> = value
                        .to_string_lossy()
                        .split_whitespace()
                        .map(str::to_owned)
                        .collect();
                    if keys.is_empty()
                        || keys.iter().any(|key| {
                            key.len() != 40
                                || !key.bytes().all(|byte| {
                                    byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte)
                                })
                        })
                    {
                        return Err(Error::InvalidSigningKey);
                    }
                    keys
                }
                None => Vec::new(),
            };
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
                    ".gpg-id",
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
                    ".gpg-id.sig",
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
    fn acquire_entry_write_lock(&self, entry: &Entry) -> Result<File, Error> {
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

    fn write_entry(
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

    /// Remove one regular encrypted entry without following links or traversing outside the store.
    pub fn remove_entry(&self, entry: &Entry) -> Result<(), Error> {
        #[cfg(unix)]
        {
            use rustix::{
                fs::{AtFlags, FileType, Mode, OFlags, openat, statat, unlinkat},
                io::Errno,
            };
            let _process_lock = ENTRY_WRITE_SERIALIZER
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let _interprocess_lock = self.acquire_entry_write_lock(entry)?;
            let parts: Vec<_> = entry.relative_file().components().collect();
            if parts.is_empty() || parts.len() > MAX_DEPTH + 1 {
                return Err(Error::InvalidEntry);
            }
            let (parent_parts, file_parts) = parts.split_at(parts.len() - 1);
            let Component::Normal(file_name) = &file_parts[0] else {
                return Err(Error::InvalidEntry);
            };
            let mut parent = self.directory.try_clone()?;
            for component in parent_parts {
                let Component::Normal(name) = component else {
                    return Err(Error::InvalidEntry);
                };
                parent = File::from(
                    openat(
                        &parent,
                        *name,
                        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                        Mode::empty(),
                    )
                    .map_err(|error| {
                        if error == Errno::NOENT {
                            Error::EntryNotFound
                        } else {
                            Error::InvalidEntry
                        }
                    })?,
                );
            }
            let metadata =
                statat(&parent, *file_name, AtFlags::SYMLINK_NOFOLLOW).map_err(|error| {
                    if error == Errno::NOENT {
                        Error::EntryNotFound
                    } else {
                        Error::InvalidEntry
                    }
                })?;
            if FileType::from_raw_mode(metadata.st_mode) != FileType::RegularFile {
                return Err(Error::InvalidEntry);
            }
            let file = File::from(
                openat(
                    &parent,
                    *file_name,
                    OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                    Mode::empty(),
                )
                .map_err(|_| Error::InvalidEntry)?,
            );
            let file_metadata = file.metadata()?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if !file_metadata.is_file()
                    || file_metadata.nlink() != 1
                    || file_metadata.ino() != metadata.st_ino as u64
                    || file_metadata.dev() != metadata.st_dev as u64
                {
                    return Err(Error::InvalidEntry);
                }
            }
            unlinkat(&parent, *file_name, AtFlags::empty()).map_err(|error| {
                if error == Errno::NOENT {
                    Error::EntryNotFound
                } else {
                    Error::InvalidEntry
                }
            })?;
            let _ = parent.sync_all();
            Ok(())
        }
        #[cfg(not(unix))]
        {
            let _ = entry;
            Err(Error::UnsupportedPlatform)
        }
    }

    #[cfg(unix)]
    fn open_relative(&self, path: &Path, directory: bool) -> Result<File, Error> {
        use rustix::fs::{Mode, OFlags, openat};
        let mut parent = self.directory.try_clone()?;
        let parts: Vec<_> = path.components().collect();
        if parts.is_empty() {
            return Err(Error::InvalidEntry);
        }
        for (i, part) in parts.iter().enumerate() {
            let Component::Normal(name) = part else {
                return Err(Error::InvalidEntry);
            };
            let mut flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK;
            if i + 1 < parts.len() || directory {
                flags |= OFlags::DIRECTORY;
            }
            parent = File::from(
                openat(&parent, *name, flags, Mode::empty()).map_err(|_| Error::InvalidEntry)?,
            );
        }
        Ok(parent)
    }

    #[cfg(not(unix))]
    fn open_relative(&self, _: &Path, _: bool) -> Result<File, Error> {
        Err(Error::UnsupportedPlatform)
    }
}
