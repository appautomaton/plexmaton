//! Status output and monotonic hints supplied by the composition root (STL-4, SEL-5).
use std::time::Instant;

use super::Workspace;

impl Workspace {
    /// Shows a session-writer failure that cannot itself enter the failed durable stream.
    pub fn report_persistence_failure(&mut self, failure: crate::PersistenceNotice) {
        self.state.report_persistence_failure(failure);
    }

    /// Shows an owner that could not be joined cleanly after session persistence failed.
    pub fn report_cleanup_failure(&mut self, failure: crate::CleanupNotice) {
        self.state.report_cleanup_failure(failure);
    }

    /// Replace one fully decoded script result. Equal output does not request another frame.
    pub fn set_status_line(&mut self, text: crate::StatusLineText, max_rows: u16) {
        self.state.set_status_line(text, max_rows);
    }

    /// A failed presentation command cannot take down the session or hide a quit question.
    pub fn set_status_line_error(&mut self, error: String) {
        self.state.set_status_line_error(error);
    }

    /// Withdraw the previous receipt when a new copy is admitted (SEL-5).
    pub fn clear_copy_receipt(&mut self) {
        self.state.clear_copy_receipt();
    }

    /// Report an observed transport completion (SEL-5), without changing selection or focus.
    pub fn report_copy(&mut self, receipt: crate::CopyReceipt, now: Instant) {
        self.state.report_copy(receipt, now);
    }

    /// The earliest monotonic deadline for a status hint.
    #[must_use]
    pub fn note_deadline(&self) -> Option<Instant> {
        self.state.status().deadline()
    }

    /// Clears status hints whose monotonic deadlines have passed.
    ///
    /// Returns whether the projection changed, so the event-loop owner can distinguish the one
    /// deadline transition from a stale wakeup (FR-1).
    pub fn expire_note(&mut self, now: Instant) -> bool {
        self.state.expire_note(now)
    }
}
