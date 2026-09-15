//! Acknowledgement-gated projection of facts whose canonical source is outside the session journal.

use plexmaton_core::ConversationEvent;

use super::LiveRuntime;

/// Why one transient collaboration fact cannot enter the root projection yet.
///
/// The caller retains the fact on either refusal. A pending commit may be retried after its journal
/// acknowledgement; a failed journal requires a new runtime, whose durable collaboration sources
/// rebuild the projection (JRN-7/ENT-1).
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum DelegatedProjectionRefusal {
    /// Earlier conversation facts are still waiting for their journal acknowledgement.
    #[error("the conversation projection is waiting for a journal acknowledgement")]
    PendingCommit,
    /// An unwritten or uncertain append froze this runtime until reopen.
    #[error("the conversation projection requires reopen after a journal failure")]
    PersistenceFailed,
}

impl LiveRuntime {
    /// Puts one fact about a delegated child on this conversation's roster.
    ///
    /// The child is a separate Conversation; only its existence and lifecycle belong to the root's
    /// projection, and the collaboration log already holds both durably. The borrowed event remains
    /// caller-owned when JRN-7 refuses projection, so it can be retried after acknowledgement.
    ///
    /// Queued rather than returned, because the sequence it takes belongs to this conversation and
    /// the queue is where that order is kept: a caller that published the envelope itself would
    /// step in front of events numbered earlier and still waiting — behind a projection reset, for
    /// one — and the projection drops whatever arrives after a number it has already applied.
    pub fn project_delegated(
        &mut self,
        event: &ConversationEvent,
    ) -> Result<(), DelegatedProjectionRefusal> {
        if let Some(refusal) = self.delegated_projection_refusal() {
            return Err(refusal);
        }
        self.pending
            .extend(self.agent.project_delegated(event.clone()).events);
        Ok(())
    }

    /// Reports the JRN-7 publication barrier without numbering or retaining another event.
    #[must_use]
    pub const fn delegated_projection_refusal(&self) -> Option<DelegatedProjectionRefusal> {
        if self.journal_failed {
            Some(DelegatedProjectionRefusal::PersistenceFailed)
        } else if self.pending_commit.is_some() {
            Some(DelegatedProjectionRefusal::PendingCommit)
        } else {
            None
        }
    }
}
