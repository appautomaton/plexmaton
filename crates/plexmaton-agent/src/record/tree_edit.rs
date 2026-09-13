use super::Record;
use crate::{JournalError, JournalRecord, Reaction};

impl Record {
    /// Metadata changes leave the semantic delivery cursor and context projection untouched.
    pub(crate) fn apply_tree_edit(
        &mut self,
        record: JournalRecord,
        reaction: &mut Reaction,
    ) -> Result<(), JournalError> {
        self.journal.apply(record.clone())?;
        reaction.records.push(record);
        Ok(())
    }
}
