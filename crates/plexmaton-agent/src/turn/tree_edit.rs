use plexmaton_core::{TreeEdit, TreeEditAction, TreeLabel};

use super::Agent;
use crate::{Reaction, TreeEditRefusal, TreeEditResult};

impl Agent {
    /// Stages metadata only: no projection reset, model input, tool action or new turn (TRE-8).
    pub fn edit_tree(&mut self, edit: &TreeEdit) -> Result<Reaction, TreeEditRefusal> {
        if self.is_running()
            || self.queued_for_next_step().next().is_some()
            || self.queued_for_next_turn().next().is_some()
        {
            return Err(TreeEditRefusal::Busy);
        }
        if self.tree_origin() != edit.origin {
            return Err(TreeEditRefusal::StaleOrigin);
        }
        if let TreeEditAction::RenameHead { renamed, .. } = &edit.action
            && TreeLabel::new(renamed.as_str().to_owned()).is_err()
        {
            return Err(TreeEditRefusal::InvalidHeadName);
        }
        // Reuse bounded semantic membership and preserve automatic-session lazy materialization.
        // Blank history admits no metadata writes, including renaming its initial main pointer.
        let snapshot = self
            .journal()
            .tree_snapshot(&edit.origin.agent_id)
            .map_err(|_| TreeEditRefusal::EntryUnavailable)?;
        if snapshot.rows.is_empty() {
            return Err(TreeEditRefusal::EmptyTree);
        }
        if let TreeEditAction::SetLabel { entry_id, .. } = &edit.action
            && !snapshot.rows.iter().any(|row| &row.entry_id == entry_id)
        {
            return Err(TreeEditRefusal::EntryUnavailable);
        }
        let record = self
            .journal()
            .prepare_tree_edit(&edit.action)
            .map_err(TreeEditRefusal::Journal)?;
        let sequence = record.as_ref().map(crate::JournalRecord::sequence);
        let mut reaction = Reaction::default();
        if let Some(record) = record {
            self.record
                .apply_tree_edit(record, &mut reaction)
                .map_err(TreeEditRefusal::Journal)?;
        }
        reaction.tree_edit = Some(TreeEditResult {
            origin: self.tree_origin(),
            mutation_sequence: sequence,
        });
        Ok(reaction.into_output())
    }
}

#[cfg(test)]
mod tests;
