use super::*;

pub const ENTRY_SUFFIX: &str = ".gpg";

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
            || path
                .extension()
                .is_none_or(|e| e != ENTRY_SUFFIX.trim_start_matches('.'))
            || path.file_stem().is_none_or(|s| s.is_empty())
        {
            return Err(Error::InvalidEntry);
        }
        let name = text
            .strip_suffix(ENTRY_SUFFIX)
            .expect("validated entry suffix")
            .to_owned();
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
