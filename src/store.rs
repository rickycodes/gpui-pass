use crate::Error;
use std::{
    env,
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::Arc,
};

pub(crate) const MAX_DEPTH: usize = 32;
#[cfg(unix)]
pub(crate) static ENTRY_WRITE_SERIALIZER: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[path = "store_access.rs"]
mod store_access;
#[path = "store_entry.rs"]
mod store_entry;
#[path = "store_policy.rs"]
mod store_policy;
#[path = "store_policy_access.rs"]
mod store_policy_access;
#[path = "store_remove.rs"]
mod store_remove;
#[path = "store_write.rs"]
mod store_write;

pub use store_access::Store;
pub use store_entry::{ENTRY_SUFFIX, Entry, EntryRevision};
pub(crate) use store_policy::{
    GPG_ID_FILE, GPG_ID_SIGNATURE_FILE, MAX_RECIPIENTS_BYTES, RecipientPolicy, parse_recipients,
};
pub use store_policy::{configured_signing_keys, default_store_path};
