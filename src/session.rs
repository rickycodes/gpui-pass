use crate::{Cancellation, Entry, OperationProgress, OperationStatus, Secret};
use std::time::{Duration, Instant};

pub const REVEAL_DURATION: Duration = Duration::from_secs(30);

/// Request identity prevents late decryptions from repopulating a hidden or changed entry.
pub struct RevealRequest {
    id: u64,
    pub entry: Entry,
    pub cancellation: Cancellation,
    pub progress: OperationProgress,
}

struct PendingReveal {
    cancellation: Cancellation,
    progress: OperationProgress,
}

#[derive(Default)]
pub struct Session {
    selected: Option<Entry>,
    generation: u64,
    pending: Option<PendingReveal>,
    visible: Option<(Secret, Instant)>,
}

impl Session {
    pub fn selected(&self) -> Option<&Entry> {
        self.selected.as_ref()
    }
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }
    pub fn pending_status(&self) -> Option<OperationStatus> {
        self.pending
            .as_ref()
            .map(|pending| pending.progress.snapshot())
    }
    pub fn select(&mut self, entry: Option<Entry>) {
        self.hide();
        self.selected = entry;
    }
    pub fn hide(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        if let Some(pending) = self.pending.take() {
            pending.cancellation.cancel();
        }
        self.visible = None;
    }
    pub fn begin_reveal(&mut self) -> Option<RevealRequest> {
        let entry = self.selected.clone()?;
        self.hide();
        let cancellation = Cancellation::default();
        let progress = OperationProgress::new(crate::gpg_status::UNLOCKING);
        self.pending = Some(PendingReveal {
            cancellation: cancellation.clone(),
            progress: progress.clone(),
        });
        Some(RevealRequest {
            id: self.generation,
            entry,
            cancellation,
            progress,
        })
    }
    pub fn finish_reveal(
        &mut self,
        request: &RevealRequest,
        secret: Secret,
        active: bool,
        now: Instant,
    ) -> bool {
        if !self.is_current(request) {
            return false;
        }
        self.pending = None;
        if !active {
            return false;
        }
        self.visible = Some((secret, now + REVEAL_DURATION));
        true
    }
    pub fn fail_reveal(&mut self, request: &RevealRequest) -> bool {
        if !self.is_current(request) {
            return false;
        }
        self.pending = None;
        true
    }
    fn is_current(&self, request: &RevealRequest) -> bool {
        self.generation == request.id
            && self.pending.is_some()
            && !request.cancellation.is_cancelled()
            && self.selected.as_ref() == Some(&request.entry)
    }
    pub fn deactivate(&mut self) {
        // Pinentry may take focus during an ongoing request. Let it finish, but only
        // publish its result if the app has regained focus at completion.
        if self.visible.is_some() {
            self.hide();
        }
    }
    pub fn expire(&mut self, now: Instant) -> bool {
        if self
            .visible
            .as_ref()
            .is_some_and(|(_, deadline)| now >= *deadline)
        {
            self.hide();
            true
        } else {
            false
        }
    }
    pub fn revealed(&self, now: Instant) -> Option<&Secret> {
        self.visible
            .as_ref()
            .filter(|(_, deadline)| now < *deadline)
            .map(|(secret, _)| secret)
    }
    pub fn remaining_seconds(&self, now: Instant) -> u64 {
        self.visible.as_ref().map_or(0, |(_, deadline)| {
            deadline
                .saturating_duration_since(now)
                .as_secs()
                .saturating_add(1)
                .min(REVEAL_DURATION.as_secs())
        })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.hide();
    }
}
