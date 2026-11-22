//! Access to standard pass stores. No UI, key management, or network code.
#![forbid(unsafe_code)]

mod decrypt;
mod generator;
mod gpg_status;
mod hardening;
mod operation;
mod session;
mod store;

pub use decrypt::{Cancellation, Gpg, Secret};
pub use generator::{
    GeneratorOptions, MAX_MEMORABLE_WORDS, MIN_MEMORABLE_WORDS, generate_memorable_password,
    generate_pass_default_password,
};
pub use hardening::harden_process;
pub use operation::{CardEvent, CardObservation, OperationProgress, OperationStatus, StatusKind};
pub use session::{REVEAL_DURATION, RevealRequest, Session};
pub use store::{
    ENTRY_SUFFIX, Entry, EntryRevision, Store, configured_signing_keys, default_store_path,
};

use std::{fmt, io};

/// Deliberately excludes subprocess output and plaintext from diagnostics.
#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    InvalidEntry,
    StoreTooLarge,
    UnsupportedPlatform,
    InvalidExecutable,
    DecryptionFailed,
    Cancelled,
    TimedOut,
    SecretTooLarge,
    InvalidText,
    InvalidGenerationLength,
    RandomUnavailable,
    InvalidWordList,
    MissingRecipients,
    InvalidRecipients,
    InvalidSigningKey,
    UnsafeLockDirectory,
    RecipientSignatureRequired,
    UnsignedRecipients,
    InvalidRecipientSignature,
    RecipientPolicyChanged,
    EntryAlreadyExists,
    EntryNotFound,
    EntryConflict,
    EncryptionFailed,
    SigningFailed,
    StoreNotEmpty,
    StoreAlreadyInitialized,
    MissingHome,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Io(_) => "Could not access the store. Check its path and permissions.",
            Self::InvalidEntry => {
                "The entry changed or contains an unsupported path or link. Refresh the store."
            }
            Self::StoreTooLarge => "The store exceeds the prototype's directory or entry limits.",
            Self::UnsupportedPlatform => {
                "Secure file access is not implemented for this platform yet."
            }
            Self::InvalidExecutable => {
                "GnuPG was not found. Supply an absolute executable path with --gpg."
            }
            Self::DecryptionFailed => {
                "GnuPG could not decrypt this entry. Check your key or YubiKey, and your system PIN prompt."
            }
            Self::Cancelled => "Operation cancelled.",
            Self::TimedOut => "The operation timed out. Check the PIN prompt or YubiKey touch request, then try again.",
            Self::SecretTooLarge => "This entry exceeds the 1 MiB reveal limit.",
            Self::InvalidText => "This entry is not UTF-8 text and cannot be displayed safely.",
            Self::InvalidGenerationLength => "Use 2 to 4 words, or a random length of 1 to 1024 with a supported character set.",
            Self::RandomUnavailable => "The operating system could not provide secure randomness.",
            Self::InvalidWordList => "The bundled password word list is invalid.",
            Self::MissingRecipients => "No GnuPG recipients are configured for this folder.",
            Self::InvalidRecipients => "The nearest .gpg-id file is invalid or unsafe.",
            Self::InvalidSigningKey => "PASSWORD_STORE_SIGNING_KEY must contain full uppercase 40-character GPG fingerprints.",
            Self::UnsafeLockDirectory => "The per-user write-lock directory is unsafe or has incorrect ownership or permissions.",
            Self::RecipientSignatureRequired => "This .gpg-id is signed. Set PASSWORD_STORE_SIGNING_KEY to trust its signing fingerprint before writing.",
            Self::UnsignedRecipients => "PASSWORD_STORE_SIGNING_KEY is set, but the selected .gpg-id has no detached signature.",
            Self::InvalidRecipientSignature => "The selected .gpg-id signature is invalid or was not made by a trusted signing key.",
            Self::RecipientPolicyChanged => "The recipient policy changed while saving. Your entry was not written; retry after refreshing.",
            Self::EntryAlreadyExists => "An entry already exists at that name. Choose another name.",
            Self::EntryConflict => "This entry changed on disk. Your edits were not saved.",
            Self::EntryNotFound => "This entry no longer exists. Refresh the store.",
            Self::EncryptionFailed => "GnuPG could not encrypt the entry.",
            Self::SigningFailed => "GnuPG could not sign the store recipient policy.",
            Self::StoreNotEmpty => "The selected directory is not empty. Choose an empty directory or open the existing store.",
            Self::StoreAlreadyInitialized => "The selected directory already has a recipient policy.",
            Self::MissingHome => "Set PASSWORD_STORE_DIR or supply a store path with --store.",
        })
    }
}

impl std::error::Error for Error {}
impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
