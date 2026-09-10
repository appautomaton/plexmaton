//! The workspace's half of the waiting-input band: what it is told, and what it asks for.

use super::{Outcome, Workspace};

impl Workspace {
    /// Replaces what the producer says is still waiting to be sent.
    ///
    /// A snapshot, not an event: the producer owns these queues, so the projection is replaced
    /// whole rather than accumulated here and reconciled when a boundary claims one (IQU-1).
    pub fn set_queued_input(&mut self, queued: Vec<crate::QueuedInput>) {
        self.state.set_queued_input(queued);
    }

    /// Asks the runtime to take the most recent waiting message back (IQU-4).
    ///
    /// The workspace removes nothing itself: the queue belongs to the producer, and a projection
    /// that dropped an entry locally would be describing a queue that still had it. Nothing
    /// waiting resolves to no request at all, so the key is inert rather than an error, the way
    /// `Escape` with an empty ladder is (INV-6).
    pub(super) fn withdraw_queued(&self) -> Outcome {
        Outcome {
            withdrawn: self.state.withdraw_target(&self.surfaces),
            ..Outcome::default()
        }
    }
}
