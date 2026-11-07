use crate::Error;
use std::{
    env,
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::Arc,
};

const MAX_ENTRIES: usize = 50_000;
const MAX_NODES: usize = 100_000;
const MAX_DEPTH: usize = 32;
const MAX_RECIPIENTS_BYTES: usize = 65_536;

fn parse_recipients(text: &str) -> Result<Vec<String>, Error> {
    let mut recipients = Vec::new();
    for line in text.lines() {
        for recipient in line.split_whitespace() {
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
        Ok(Self {
            relative_file: path.to_owned(),
            name: text[..text.len() - 4].to_owned(),
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
        let name = self.name.to_lowercase();
        query
            .split_whitespace()
            .all(|part| name.contains(&part.to_lowercase()))
    }
}

fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
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
        entries.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.name.cmp(&b.name))
        });
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

    pub(crate) fn recipients_for_entry(&self, entry: &Entry) -> Result<Vec<String>, Error> {
        if env::var_os("PASSWORD_STORE_SIGNING_KEY").is_some_and(|value| !value.is_empty()) {
            return Err(Error::SignedRecipientsUnsupported);
        }
        if let Some(keys) = env::var_os("PASSWORD_STORE_KEY").filter(|value| !value.is_empty()) {
            return parse_recipients(&keys.to_string_lossy());
        }
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
                let contents =
                    std::str::from_utf8(&contents).map_err(|_| Error::InvalidRecipients)?;
                return parse_recipients(contents);
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
    pub(crate) fn create_entry(&self, entry: &Entry, ciphertext: &[u8]) -> Result<(), Error> {
        self.write_entry(entry, ciphertext, false)
    }

    /// Atomically replace an existing encrypted entry within the pinned store.
    pub(crate) fn replace_entry(&self, entry: &Entry, ciphertext: &[u8]) -> Result<(), Error> {
        self.open_entry(entry)?;
        self.write_entry(entry, ciphertext, true)
    }

    fn write_entry(&self, entry: &Entry, ciphertext: &[u8], replace: bool) -> Result<(), Error> {
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
                if replace {
                    renameat(&parent, temporary_os, &parent, *file_name)
                        .map_err(std::io::Error::from)?;
                } else {
                    linkat(&parent, temporary_os, &parent, *file_name, AtFlags::empty())
                        .map_err(std::io::Error::from)?;
                }
                if !replace {
                    unlinkat(&parent, temporary_os, AtFlags::empty())
                        .map_err(std::io::Error::from)?;
                }
                parent.sync_all()?;
                Ok::<(), std::io::Error>(())
            })();
            if let Err(error) = write_result {
                let _ = unlinkat(&parent, temporary_os, AtFlags::empty());
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    return Err(Error::EntryAlreadyExists);
                }
                return Err(Error::Io(error));
            }
            Ok(())
        }
        #[cfg(not(unix))]
        {
            let _ = (entry, ciphertext, replace);
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
