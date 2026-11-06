use crate::Error;
use std::{
    env,
    fs::{self, File},
    path::{Component, Path, PathBuf},
    sync::Arc,
};

const MAX_ENTRIES: usize = 50_000;
const MAX_NODES: usize = 100_000;
const MAX_DEPTH: usize = 32;

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
