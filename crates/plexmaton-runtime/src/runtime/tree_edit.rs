use plexmaton_core::TreeEdit;

use super::LiveRuntime;
use crate::{PersistenceFailure, RuntimeError, TreeAdmission, TreeRequestRefusal};

impl LiveRuntime {
    /// Admits an idle metadata change without waiting for disk or replacing conversation content.
    pub fn request_tree_edit(&mut self, edit: TreeEdit) -> Result<TreeAdmission, RuntimeError> {
        if let Err(refusal) = self.validate_tree_request(&edit.origin) {
            return Ok(TreeAdmission::Refused(refusal));
        }
        let reaction = match self.agent.edit_tree(&edit) {
            Ok(reaction) => reaction,
            Err(refusal) => return Ok(TreeAdmission::Refused(TreeRequestRefusal::Edit(refusal))),
        };
        let Some(receipt) = reaction.tree_edit.as_ref() else {
            return Err(self.invalid_tree_edit_reaction());
        };
        if reaction.projection_reset.is_some()
            || reaction.tree_navigation.is_some()
            || !reaction.events.is_empty()
            || !reaction.effects.is_empty()
            || !reaction.released_inputs.is_empty()
            || !reaction.undelivered.is_empty()
            || !reaction.unresolved_approvals.is_empty()
            || !reaction.undelivered_model.is_empty()
            || receipt.origin != self.agent.tree_origin()
        {
            return Err(self.invalid_tree_edit_reaction());
        }
        match receipt.mutation_sequence {
            None if reaction.records.is_empty() => return Ok(TreeAdmission::NoOp),
            Some(sequence)
                if reaction.records.len() == 1 && reaction.records[0].sequence() == sequence => {}
            _ => return Err(self.invalid_tree_edit_reaction()),
        }
        self.begin_tree_transition(reaction)
    }

    fn invalid_tree_edit_reaction(&mut self) -> RuntimeError {
        self.journal_failed = true;
        self.report.persistence_failure = Some(PersistenceFailure::NotWritten);
        RuntimeError::TreeEditTransitionInvalid
    }
}
