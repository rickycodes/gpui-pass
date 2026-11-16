//! One timed clipboard lease shared by entry, generated, and editor copies.
use gpui::ClipboardItem;
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

pub const DEFAULT_CLIPBOARD_DURATION: Duration = Duration::from_secs(45);

#[derive(Default)]
pub struct ClipboardState {
    lease: Option<Lease>,
}

struct Lease {
    expected: Zeroizing<String>,
    previous: Option<ClipboardItem>,
    source: String,
    deadline: Instant,
}

pub fn clipboard_deadline(now: Instant, seconds: Option<&str>) -> Instant {
    seconds
        .and_then(|value| value.parse::<u64>().ok())
        .and_then(|seconds| now.checked_add(Duration::from_secs(seconds)))
        .unwrap_or(now + DEFAULT_CLIPBOARD_DURATION)
}

impl ClipboardState {
    pub fn active(&self) -> bool {
        self.lease.is_some()
    }
    pub fn source(&self) -> Option<&str> {
        self.lease.as_ref().map(|lease| lease.source.as_str())
    }
    pub fn remaining_seconds(&self, now: Instant) -> Option<u64> {
        self.lease.as_ref().map(|lease| {
            let remaining = lease.deadline.saturating_duration_since(now);
            remaining.as_secs() + u64::from(remaining.subsec_nanos() != 0)
        })
    }

    pub fn copy(
        &mut self,
        content: String,
        source: String,
        current: Option<ClipboardItem>,
        deadline: Instant,
    ) -> ClipboardItem {
        let previous = match self.lease.take() {
            Some(lease) if owns(&lease, current.as_ref()) => lease.previous,
            _ => current,
        };
        let item = ClipboardItem::new_string(content.clone());
        self.lease = Some(Lease {
            expected: Zeroizing::new(content),
            previous,
            source,
            deadline,
        });
        item
    }

    /// Forget a replaced clipboard immediately; restore only our own expired value.
    pub fn poll(&mut self, current: Option<ClipboardItem>, now: Instant) -> Option<ClipboardItem> {
        let lease = self.lease.as_ref()?;
        if !owns(lease, current.as_ref()) {
            self.lease = None;
            None
        } else if now >= lease.deadline {
            self.restore(current)
        } else {
            None
        }
    }

    pub fn restore(&mut self, current: Option<ClipboardItem>) -> Option<ClipboardItem> {
        let lease = self.lease.take()?;
        owns(&lease, current.as_ref()).then(|| {
            lease
                .previous
                .unwrap_or_else(|| ClipboardItem::new_string(String::new()))
        })
    }
}

fn owns(lease: &Lease, current: Option<&ClipboardItem>) -> bool {
    current
        .and_then(|item| item.text())
        .map(Zeroizing::new)
        .is_some_and(|text| text.as_str() == lease.expected.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_copies_restore_original_and_keep_source() {
        let now = Instant::now();
        let mut state = ClipboardState::default();
        let first = state.copy(
            "secret A".into(),
            "A".into(),
            Some(ClipboardItem::new_string("original".into())),
            now + Duration::from_secs(5),
        );
        let second = state.copy(
            "secret B".into(),
            "B".into(),
            Some(first),
            now + Duration::from_secs(10),
        );
        assert_eq!(state.source(), Some("B"));
        assert_eq!(state.remaining_seconds(now), Some(10));
        assert_eq!(
            state
                .poll(Some(second), now + Duration::from_secs(10))
                .unwrap()
                .text()
                .as_deref(),
            Some("original")
        );
        assert!(!state.active());
    }

    #[test]
    fn external_copy_is_preserved_and_indicator_disappears() {
        let now = Instant::now();
        let mut state = ClipboardState::default();
        state.copy(
            "secret".into(),
            "A".into(),
            None,
            now + Duration::from_secs(5),
        );
        assert!(
            state
                .poll(Some(ClipboardItem::new_string("new content".into())), now)
                .is_none()
        );
        assert!(!state.active());
    }

    #[test]
    fn invalid_or_overflowing_timeouts_fall_back_and_zero_expires_now() {
        let now = Instant::now();
        for value in [None, Some("invalid"), Some("18446744073709551615")] {
            assert_eq!(
                clipboard_deadline(now, value),
                now + DEFAULT_CLIPBOARD_DURATION
            );
        }
        assert_eq!(clipboard_deadline(now, Some("0")), now);
    }
}
