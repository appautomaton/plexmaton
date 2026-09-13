//! Acknowledgement-gated conversation-tree navigation (TRE-4).

use plexmaton_core::TreeNavigation;

use super::LiveRuntime;
use crate::{RuntimeError, TreeAdmission, TreeRequestRefusal};

impl LiveRuntime {
    /// Admits one tree action without waiting for the writer; its result is reported after JRN-7
    /// acknowledges the mutation. This method does not drain input queues or cancel owned work.
    pub fn request_tree_navigation(
        &mut self,
        navigation: TreeNavigation,
    ) -> Result<TreeAdmission, RuntimeError> {
        if let Err(refusal) = self.validate_tree_request(&navigation.origin) {
            return Ok(TreeAdmission::Refused(refusal));
        }

        let reaction = match self.agent.navigate(&navigation) {
            Ok(reaction) => reaction,
            Err(refusal) => {
                return Ok(TreeAdmission::Refused(TreeRequestRefusal::Navigation(
                    refusal,
                )));
            }
        };
        let Some(receipt) = reaction.tree_navigation.as_ref() else {
            return Err(self.invalid_navigation_reaction());
        };
        let Some(sequence) = receipt.mutation_sequence else {
            if reaction.records.is_empty()
                && reaction.projection_reset.is_none()
                && reaction.events.is_empty()
                && reaction.effects.is_empty()
                && reaction.undelivered.is_empty()
                && reaction.unresolved_approvals.is_empty()
                && reaction.undelivered_model.is_empty()
                && reaction.tree_edit.is_none()
            {
                return Ok(TreeAdmission::NoOp);
            }
            return Err(self.invalid_navigation_reaction());
        };
        if reaction.records.len() != 1
            || reaction.records[0].sequence() != sequence
            || reaction.projection_reset.is_none()
            || !reaction.events.is_empty()
            || !reaction.effects.is_empty()
            || !reaction.undelivered.is_empty()
            || !reaction.unresolved_approvals.is_empty()
            || !reaction.undelivered_model.is_empty()
            || &receipt.selected_head != self.agent.selected_head()
            || reaction.tree_edit.is_some()
        {
            return Err(self.invalid_navigation_reaction());
        }

        self.begin_tree_transition(reaction)
    }

    fn invalid_navigation_reaction(&mut self) -> RuntimeError {
        self.journal_failed = true;
        self.report.persistence_failure = Some(crate::PersistenceFailure::NotWritten);
        RuntimeError::TreeNavigationTransitionInvalid
    }
}
