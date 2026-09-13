use super::Record;
use crate::interface::Reaction;
use crate::journal::{JournalProjection, JournalRecord};

impl Record {
    /// Applies one already-preflighted head mutation and stages its replacement projection.
    pub(crate) fn apply_tree_navigation(
        &mut self,
        record: JournalRecord,
        projection: JournalProjection,
        reaction: &mut Reaction,
    ) -> Result<(), crate::JournalError> {
        self.journal.apply(record.clone())?;
        reaction.records.push(record);
        self.next_event = projection.events().last().map_or(1, |event| {
            event
                .sequence
                .get()
                .checked_add(1)
                .unwrap_or_else(|| unreachable!("projected event sequence cannot be extended"))
        });
        reaction.projection_reset = Some(projection.events().to_vec());
        Ok(())
    }
}
