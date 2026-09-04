//! The bounded record of conditions the projection refused to silently ignore.

use std::collections::VecDeque;

use plexmaton_core::EventSequence;

use super::{ReduceError, ViewState};

/// Upper bound on retained notices.
///
/// Notices originate from producers the projection does not control, so the log discards its
/// oldest entries and reports the discarded count rather than growing without limit.
const CAPACITY: usize = 32;

/// A producer-contract defect surfaced without interrupting the user's work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NoticeView {
    /// Events were lost before the received sequence; the projection resynchronized forward.
    SequenceGap { expected: u64, received: u64 },
    /// One event violated the projection contract and was dropped.
    Rejected {
        sequence: EventSequence,
        error: ReduceError,
    },
    /// The session writer could not establish whether one submitted message reached disk.
    PersistenceFailed(PersistenceNotice),
    /// One owner could not be joined cleanly while the runtime froze after persistence failure.
    CleanupFailed(CleanupNotice),
    /// One summary of a syntactic file-tail repair while resuming a session.
    SessionRecovered(SessionRecoveryNotice),
}

/// Syntactic journal-tail repair reported by the storage adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TailRecoveryNotice {
    /// A complete final JSON record lacked only its newline.
    AddedFinalNewline,
    /// An incomplete final fragment was moved beside the canonical journal.
    IsolatedFinalTail { bytes: u64 },
}

/// Startup-only summary shown once after a journal tail is repaired.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionRecoveryNotice {
    /// File-tail repair performed before the session was loaded.
    pub tail: TailRecoveryNotice,
}

/// What the session writer knows about a failed submission append.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PersistenceNotice {
    /// No record was written, so the restored draft is safe to retry after reopen.
    NotWritten,
    /// Bytes may have reached disk, so recovery must reconcile before any retry.
    OutcomeUnknown,
}

/// Runtime owner whose cleanup failed after a durable transition stopped.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CleanupNotice {
    /// Provider operation.
    Provider,
    /// Native tool work.
    Tools,
    /// Session journal writer.
    JournalWriter,
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

impl ViewState {
    /// Records a session-writer failure outside the durable event stream it could not advance.
    pub(crate) fn report_persistence_failure(&mut self, failure: PersistenceNotice) {
        self.notices.push(NoticeView::PersistenceFailed(failure));
        self.touch();
    }

    /// Records one owner that could not be joined cleanly after the durable stream froze.
    pub(crate) fn report_cleanup_failure(&mut self, failure: CleanupNotice) {
        self.notices.push(NoticeView::CleanupFailed(failure));
        self.touch();
    }

    /// Records exactly one summary of a journal-tail repair performed during resume.
    pub(crate) fn report_session_recovery(&mut self, recovery: SessionRecoveryNotice) {
        self.notices.push(NoticeView::SessionRecovered(recovery));
        self.touch();
    }
}

#[cfg(test)]
mod tests {
    use super::{CAPACITY, NoticeLog, NoticeView};

    #[test]
    fn the_log_is_bounded_and_reports_what_it_discarded() {
        let mut log = NoticeLog::default();
        for index in 0..CAPACITY + 8 {
            log.push(NoticeView::SequenceGap {
                expected: index as u64,
                received: index as u64 + 2,
            });
        }

        assert_eq!(log.iter().count(), CAPACITY);
        assert_eq!(log.dropped(), 8);
        assert!(
            matches!(
                log.iter().next(),
                Some(NoticeView::SequenceGap {
                    expected: 8,
                    received: 10
                })
            ),
            "the oldest survivor must be the one after the last discard"
        );
    }
}
