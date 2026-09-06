//! The bounded record of conditions the projection refused to silently ignore.

use std::collections::VecDeque;

use plexmaton_core::EventSequence;

use super::{ReduceError, ViewState};

/// Upper bound on retained notices.
///
/// Notices originate from producers the projection does not control, so the log discards its
/// oldest entries and reports the discarded count rather than growing without limit.
const CAPACITY: usize = 32;
const MAX_SKILL_DIAGNOSTIC_BYTES: usize = 1024;

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
    /// A project or user skill could not be discovered, loaded, or activated.
    SkillDiagnostic { message: String },
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
    /// Conversation journal writer.
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

    /// Records bounded display-only skill diagnostics outside semantic model context (SKL-5).
    pub(crate) fn report_skill_diagnostic(&mut self, mut message: String) {
        if message.len() > MAX_SKILL_DIAGNOSTIC_BYTES {
            let mut end = MAX_SKILL_DIAGNOSTIC_BYTES;
            while !message.is_char_boundary(end) {
                end = end.saturating_sub(1);
            }
            message.truncate(end);
        }
        self.notices.push(NoticeView::SkillDiagnostic { message });
        self.touch();
    }
}

#[cfg(test)]
mod tests {
    use super::{CAPACITY, MAX_SKILL_DIAGNOSTIC_BYTES, NoticeLog, NoticeView};

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

    #[test]
    fn skill_diagnostics_are_bounded_without_splitting_utf8() {
        let mut state = super::ViewState::default();
        state.report_skill_diagnostic("🦀".repeat(300));

        let Some(NoticeView::SkillDiagnostic { message }) = state.notices().next() else {
            panic!("skill diagnostic notice");
        };
        assert_eq!(message.len(), MAX_SKILL_DIAGNOSTIC_BYTES);
        assert_eq!(message.chars().count(), MAX_SKILL_DIAGNOSTIC_BYTES / 4);
    }
}
