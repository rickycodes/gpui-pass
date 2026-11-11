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
    GeneratorOptions, generate_memorable_password, generate_pass_default_password,
};
pub use hardening::harden_process;
pub use operation::{CardEvent, CardObservation, OperationProgress, OperationStatus, StatusKind};
pub use session::{RevealRequest, Session};
pub use store::{Entry, Store, default_store_path};

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
    SignedRecipientsUnsupported,
    EntryAlreadyExists,
    EntryNotFound,
    EncryptionFailed,
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
            Self::Cancelled => "Reveal cancelled.",
            Self::TimedOut => "Unlock timed out. Check the PIN prompt or YubiKey touch request, then reveal again.",
            Self::SecretTooLarge => "This entry exceeds the 1 MiB reveal limit.",
            Self::InvalidText => "This entry is not UTF-8 text and cannot be displayed safely.",
            Self::InvalidGenerationLength => "Choose a phrase with 4 to 8 words.",
            Self::RandomUnavailable => "The operating system could not provide secure randomness.",
            Self::InvalidWordList => "The bundled password word list is invalid.",
            Self::MissingRecipients => "No GnuPG recipients are configured for this folder.",
            Self::InvalidRecipients => "The nearest .gpg-id file is invalid or unsafe.",
            Self::SignedRecipientsUnsupported => {
                "This store requires signed .gpg-id verification, which is not supported for writes yet."
            }
            Self::EntryAlreadyExists => "An entry already exists at that name. Choose another name.",
            Self::EntryNotFound => "This entry no longer exists. Refresh the store.",
            Self::EncryptionFailed => "GnuPG could not encrypt the new entry.",
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
