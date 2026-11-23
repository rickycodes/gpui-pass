use super::*;

const MAX_ENTRIES: usize = 50_000;
const MAX_NODES: usize = 100_000;

#[derive(Clone)]
pub struct Store {
    pub(crate) root: PathBuf,
    pub(crate) directory: Arc<File>,
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

    #[cfg(unix)]
    pub fn initialize(
        root: impl AsRef<Path>,
        recipients: &[String],
        signature: Option<&[u8]>,
    ) -> Result<Self, Error> {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

        if recipients.is_empty()
            || recipients.iter().any(|recipient| {
                recipient.is_empty()
                    || recipient.len() > 256
                    || recipient.chars().any(char::is_control)
            })
        {
            return Err(Error::InvalidRecipients);
        }
        let root = root.as_ref();
        let created_root = match fs::symlink_metadata(root) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(Error::InvalidEntry);
                }
                if fs::read_dir(root)?.next().transpose()?.is_some() {
                    return Err(Error::StoreNotEmpty);
                }
                false
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir_all(root)?;
                true
            }
            Err(error) => return Err(Error::Io(error)),
        };
        fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;

        let id_path = root.join(GPG_ID_FILE);
        let signature_path = root.join(GPG_ID_SIGNATURE_FILE);
        let result = (|| {
            let mut id = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&id_path)?;
            for recipient in recipients {
                writeln!(id, "{recipient}")?;
            }
            id.sync_all()?;
            if let Some(signature) = signature {
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&signature_path)?;
                file.write_all(signature)?;
                file.sync_all()?;
            }
            Self::open(root)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&signature_path);
            let _ = fs::remove_file(&id_path);
            if created_root {
                let _ = fs::remove_dir(root);
            }
        }
        result
    }

    #[cfg(not(unix))]
    pub fn initialize(_: impl AsRef<Path>, _: &[String], _: Option<&[u8]>) -> Result<Self, Error> {
        Err(Error::UnsupportedPlatform)
    }

    pub fn has_root_recipient_policy(&self) -> Result<bool, Error> {
        match fs::symlink_metadata(self.root.join(GPG_ID_FILE)) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                Err(Error::InvalidRecipients)
            }
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(Error::Io(error)),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Listing reads filenames only. Decryption always reopens through pinned directory handles.
    pub fn entries(&self) -> Result<Vec<Entry>, Error> {
        let mut entries = Vec::new();
        let mut nodes = 0;
        self.visit(&self.directory, Path::new(""), 0, &mut nodes, &mut entries)?;
        entries.sort_by_cached_key(|entry| (entry.name().to_lowercase(), entry.name().to_owned()));
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
}
