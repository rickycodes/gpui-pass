use super::*;

impl Store {
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
    pub(crate) fn open_relative(&self, path: &Path, directory: bool) -> Result<File, Error> {
        use rustix::{
            fs::{Mode, OFlags, openat},
            io::Errno,
        };
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
                openat(&parent, *name, flags, Mode::empty()).map_err(|error| {
                    if error == Errno::NOENT {
                        Error::EntryNotFound
                    } else {
                        Error::InvalidEntry
                    }
                })?,
            );
        }
        Ok(parent)
    }

    #[cfg(not(unix))]
    pub(crate) fn open_relative(&self, _: &Path, _: bool) -> Result<File, Error> {
        Err(Error::UnsupportedPlatform)
    }
}
