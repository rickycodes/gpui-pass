//! Shared, bounded status reporting for background operations.
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardEvent {
    PinentryOpened,
    CardDetected,
    InsertionRequested,
    NoCardAvailable,
    NoReaderAvailable,
    CardBusy,
    AgentBusy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CardObservation {
    pub event: CardEvent,
    pub observed_at: Instant,
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
struct ProgressState {
    status: OperationStatus,
    card_observation: Option<CardObservation>,
}

#[derive(Clone)]
pub struct OperationProgress(Arc<Mutex<ProgressState>>);

impl OperationProgress {
    pub fn new(initial: OperationStatus) -> Self {
        Self(Arc::new(Mutex::new(ProgressState {
            status: initial,
            card_observation: None,
        })))
    }

    pub fn update(&self, status: OperationStatus) {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .status = status;
    }

    pub fn snapshot(&self) -> OperationStatus {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .status
    }

    pub(crate) fn observe_card(&self, event: CardEvent) {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .card_observation = Some(CardObservation {
            event,
            observed_at: Instant::now(),
        });
    }

    pub fn card_observation(&self) -> Option<CardObservation> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .card_observation
    }
}
