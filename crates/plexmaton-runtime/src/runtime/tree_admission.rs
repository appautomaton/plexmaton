//! One admission fence shared by context navigation and metadata-only edits (TRE-4/TRE-8).

use plexmaton_agent::{Reaction, TreeNavigationRefusal as AgentRefusal};
use plexmaton_core::TreeOrigin;

use super::{LiveRuntime, ShutdownState, transition::AfterCommit};
use crate::{RuntimeError, TreeAdmission, TreeRequestRefusal as Refusal};

impl LiveRuntime {
    /// Reads bounded exact source only from the acknowledged conversation and addressed agent.
    /// This performs no filesystem, provider or tool operation and publishes no semantic event.
    pub fn read_tree_source(
        &self,
        request: &plexmaton_core::TreeSourceRequest,
    ) -> Result<String, plexmaton_core::TreeSourceError> {
        let (journal, _) = self
            .acknowledged_conversation()
            .ok_or(plexmaton_core::TreeSourceError::HistoryUnavailable)?;
        if request.origin.agent_id != self.agent_id {
            return Err(plexmaton_core::TreeSourceError::StaleOrigin);
        }
        journal.tree_source(request)
    }

    /// Returns no staged or failed in-memory journal state as acknowledged history.
    #[must_use]
    pub fn acknowledged_tree_origin(&self) -> Option<TreeOrigin> {
        self.acknowledged_conversation()?;
        Some(self.agent.tree_origin())
    }

    pub(super) fn validate_tree_request(&self, origin: &TreeOrigin) -> Result<(), Refusal> {
        if origin.agent_id != self.agent_id {
            return Err(Refusal::Navigation(AgentRefusal::ForeignAgent {
                expected: self.agent_id.clone(),
                actual: origin.agent_id.clone(),
            }));
        }
        if self.shutdown_state != ShutdownState::Open {
            return Err(Refusal::ShuttingDown);
        }
        if self.journal_failed {
            return Err(Refusal::PersistenceFailed);
        }
        if self.journal.is_none() {
            return Err(Refusal::PersistenceUnavailable);
        }
        // A completed admission worker can leave the agent waiting for approval with no active
        // task. Queues and turn ownership therefore remain explicit alongside runtime work.
        if self.agent.is_running()
            || self.agent.pending_approvals().next().is_some()
            || self.agent.queued_for_next_step().next().is_some()
            || self.agent.queued_for_next_turn().next().is_some()
            || !self.pending_inputs.is_empty()
            || self.preparing_input.is_some()
            || self.has_active_work()
        {
            return Err(Refusal::Busy);
        }
        if self.report.tree_navigation.is_some()
            || self.report.tree_edit.is_some()
            || self.report.projection_reset.is_some()
        {
            return Err(Refusal::PendingReport);
        }
        let actual = self.acknowledged_tree_origin().ok_or(Refusal::Busy)?;
        if origin.conversation_id != actual.conversation_id {
            return Err(Refusal::Navigation(AgentRefusal::ForeignConversation {
                expected: actual.conversation_id,
                actual: origin.conversation_id.clone(),
            }));
        }
        if origin.revision != actual.revision {
            return Err(Refusal::StaleOrigin {
                expected: Box::new(origin.clone()),
                actual: Box::new(actual),
            });
        }
        if origin.selected_head != actual.selected_head {
            return Err(Refusal::Navigation(AgentRefusal::SourceHeadChanged {
                expected: origin.selected_head.clone(),
                actual: actual.selected_head,
            }));
        }
        Ok(())
    }

    pub(super) fn begin_tree_transition(
        &mut self,
        reaction: Reaction,
    ) -> Result<TreeAdmission, RuntimeError> {
        match self.begin_transition(reaction, Vec::new(), AfterCommit::None) {
            Ok(()) => Ok(if self.journal_failed {
                TreeAdmission::Refused(Refusal::PersistenceFailed)
            } else {
                TreeAdmission::Started
            }),
            Err(_) if self.journal_failed => Ok(TreeAdmission::Refused(Refusal::PersistenceFailed)),
            Err(error) => Err(error),
        }
    }
}
