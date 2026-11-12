use std::{
    cell::Cell,
    sync::{atomic::{AtomicU64, Ordering}, mpsc::{Receiver, TryRecvError}},
    time::{Duration, Instant},
};
use crate::diagnostics::trace;
static NEXT_PROBE: AtomicU64 = AtomicU64::new(1);

/// Physical connection identity. Refresh requests do not change this identity.
#[derive(Default)]
pub(crate) struct CardRefresh {
    presence: Option<bool>,
    generation: u64,
    misses: u8,
}

impl CardRefresh {
    pub fn observe(&mut self, present: bool) {
        if self.presence != Some(present) {
            self.presence = Some(present);
            self.generation = self.generation.wrapping_add(1);
            self.misses = 0;
        }
    }

    pub fn generation(&self) -> u64 { self.generation }

    pub fn retry_delay(&mut self) -> Duration {
        let delay = match self.misses {
            0 => Duration::from_millis(250),
            1 => Duration::from_millis(500),
            2 => Duration::from_secs(1),
            _ => Duration::from_secs(3),
        };
        self.misses = self.misses.saturating_add(1);
        delay
    }

    pub fn completed(&mut self) { self.misses = 0; }
}

pub(crate) enum ProbeResult<T> {
    Current(T),
    Stale,
    Disconnected,
}

/// Keep the receiver until completion, even after unplugging: dropping a
/// receiver does not stop its worker from accessing the card.
pub(crate) struct CardProbe<T> {
    generation: u64,
    receiver: Receiver<T>,
    id: u64,
    started: Instant,
    last_report: Cell<Instant>,
}

impl<T> CardProbe<T> {
    pub fn new(kind: &'static str, generation: u64, receiver: Receiver<T>) -> Self {
        let id = NEXT_PROBE.fetch_add(1, Ordering::Relaxed);
        let started = Instant::now();
        trace!("probe.start id={id} kind={kind} generation={generation}");
        Self { generation, receiver, id, started, last_report: Cell::new(started) }
    }

    pub fn id(&self) -> u64 { self.id }

    pub fn poll(&self, refresh: &CardRefresh) -> Option<ProbeResult<T>> {
        match self.receiver.try_recv() {
            Ok(value) => {
                let current = self.generation == refresh.generation();
                trace!("probe.received id={} generation={} current_generation={} accepted={} elapsed_ms={}",
                    self.id, self.generation, refresh.generation(), current, self.started.elapsed().as_millis());
                Some(if current { ProbeResult::Current(value) } else { ProbeResult::Stale })
            },
            Err(TryRecvError::Empty) => {
                if self.last_report.get().elapsed() >= Duration::from_secs(5) {
                    trace!("probe.waiting id={} elapsed_ms={}", self.id, self.started.elapsed().as_millis());
                    self.last_report.set(Instant::now());
                }
                None
            },
            Err(TryRecvError::Disconnected) => {
                trace!("probe.disconnected id={} elapsed_ms={}", self.id, self.started.elapsed().as_millis());
                Some(ProbeResult::Disconnected)
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn late_result_from_before_reinsertion_is_discarded() {
        let mut refresh = CardRefresh::default();
        refresh.observe(true);
        let (sender, receiver) = mpsc::channel();
        let old = CardProbe::new("fixture", refresh.generation(), receiver);
        refresh.observe(false);
        refresh.observe(true);
        sender.send("old card details").unwrap();
        assert!(matches!(old.poll(&refresh), Some(ProbeResult::Stale)));
        let (sender, receiver) = mpsc::channel();
        let current = CardProbe::new("fixture", refresh.generation(), receiver);
        sender.send("current card details").unwrap();
        assert!(matches!(current.poll(&refresh), Some(ProbeResult::Current("current card details"))));
    }

    #[test]
    fn duplicate_presence_events_do_not_discard_good_details() {
        let mut refresh = CardRefresh::default();
        refresh.observe(true);
        let (sender, receiver) = mpsc::channel();
        let probe = CardProbe::new("fixture", refresh.generation(), receiver);
        for _ in 0..10 { refresh.observe(true); }
        sender.send("details").unwrap();
        assert!(matches!(probe.poll(&refresh), Some(ProbeResult::Current("details"))));
    }

    #[test]
    fn slow_enumeration_keeps_retrying_with_bounded_frequency() {
        let mut refresh = CardRefresh::default();
        refresh.observe(true);
        assert_eq!(refresh.retry_delay(), Duration::from_millis(250));
        assert_eq!(refresh.retry_delay(), Duration::from_millis(500));
        assert_eq!(refresh.retry_delay(), Duration::from_secs(1));
        for _ in 0..300 { assert_eq!(refresh.retry_delay(), Duration::from_secs(3)); }
        refresh.observe(false);
        refresh.observe(true);
        assert_eq!(refresh.retry_delay(), Duration::from_millis(250));
    }

    #[test]
    fn crashed_worker_is_distinct_from_one_still_running() {
        let refresh = CardRefresh::default();
        let (sender, receiver) = mpsc::channel::<()>();
        let probe = CardProbe::new("fixture", refresh.generation(), receiver);
        assert!(probe.poll(&refresh).is_none());
        drop(sender);
        assert!(matches!(probe.poll(&refresh), Some(ProbeResult::Disconnected)));
    }
}
