//! Shared, bounded status reporting for background operations.
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusKind {
    Working,
    Attention,
}

/// Only application-owned static copy can enter the operation status UI.
/// Secret text, raw subprocess diagnostics, key IDs, and filenames cannot be passed here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OperationStatus {
    pub kind: StatusKind,
    pub title: &'static str,
    pub detail: &'static str,
}

impl OperationStatus {
    pub const fn working(title: &'static str, detail: &'static str) -> Self {
        Self {
            kind: StatusKind::Working,
            title,
            detail,
        }
    }

    pub const fn attention(title: &'static str, detail: &'static str) -> Self {
        Self {
            kind: StatusKind::Attention,
            title,
            detail,
        }
    }
}

/// A latest-value slot: status floods cannot build an unbounded event queue.
/// Create a fresh slot per operation so obsolete workers cannot update a newer operation's UI.
#[derive(Clone)]
pub struct OperationProgress(Arc<Mutex<OperationStatus>>);

impl OperationProgress {
    pub fn new(initial: OperationStatus) -> Self {
        Self(Arc::new(Mutex::new(initial)))
    }

    pub fn update(&self, status: OperationStatus) {
        *self.0.lock().unwrap_or_else(|error| error.into_inner()) = status;
    }

    pub fn snapshot(&self) -> OperationStatus {
        *self.0.lock().unwrap_or_else(|error| error.into_inner())
    }
}
