//! The bounded record of conditions the projection refused to silently ignore.

use std::collections::VecDeque;

use plexmaton_core::EventSequence;

use super::ReduceError;

/// Upper bound on retained notices.
///
/// Notices originate from producers the projection does not control, so the log discards its
/// oldest entries and reports the discarded count rather than growing without limit.
const CAPACITY: usize = 32;

/// A runtime notice surfaced without interrupting the user's work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NoticeView {
    /// Warning reported by the event producer.
    RuntimeWarning { message: String },
    /// Events were lost before the received sequence; the projection resynchronized forward.
    SequenceGap { expected: u64, received: u64 },
    /// One event violated the projection contract and was dropped.
    Rejected {
        sequence: EventSequence,
        error: ReduceError,
    },
}

/// A bounded log that reports what it had to throw away.
///
/// Dropping silently would turn a flood of producer defects into the appearance of a healthy
/// workspace, which is the opposite of the visible degradation this log exists to give.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct NoticeLog {
    entries: VecDeque<NoticeView>,
    dropped: u64,
}

impl NoticeLog {
    /// Records one notice, discarding the oldest if the log is full.
    pub(super) fn push(&mut self, notice: NoticeView) {
        if self.entries.len() == CAPACITY {
            self.entries.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
        self.entries.push_back(notice);
    }

    /// Iterates retained notices from oldest to newest.
    pub(super) fn iter(&self) -> impl Iterator<Item = &NoticeView> {
        self.entries.iter()
    }

    /// Number of notices discarded because the log was full.
    pub(super) const fn dropped(&self) -> u64 {
        self.dropped
    }
}

#[cfg(test)]
mod tests {
    use super::{CAPACITY, NoticeLog, NoticeView};

    #[test]
    fn the_log_is_bounded_and_reports_what_it_discarded() {
        let mut log = NoticeLog::default();
        for index in 0..CAPACITY + 8 {
            log.push(NoticeView::RuntimeWarning {
                message: format!("warning {index}"),
            });
        }

        assert_eq!(log.iter().count(), CAPACITY);
        assert_eq!(log.dropped(), 8);
        assert!(
            matches!(log.iter().next(), Some(NoticeView::RuntimeWarning { message }) if message == "warning 8"),
            "the oldest survivor must be the one after the last discard"
        );
    }
}
