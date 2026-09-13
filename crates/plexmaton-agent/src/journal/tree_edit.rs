use plexmaton_core::{ConversationEntryId, JournalRecordId, TreeEditAction, TreeLabel};

use super::{ConversationJournal, JournalError, JournalRecord};

impl ConversationJournal {
    /// Current annotation reduced from metadata records; source entries remain immutable.
    #[must_use]
    pub fn tree_label(&self, entry_id: &ConversationEntryId) -> Option<&TreeLabel> {
        self.tree_labels.get(entry_id)
    }

    pub(crate) fn prepare_tree_edit(
        &self,
        action: &TreeEditAction,
    ) -> Result<Option<JournalRecord>, JournalError> {
        let sequence = self.next_sequence;
        let record_id = JournalRecordId::new(format!(
            "{}-record-{}",
            self.conversation_id(),
            sequence.get()
        ))
        .unwrap_or_else(|_| unreachable!("a generated record identity is nonempty"));
        let record = match action {
            TreeEditAction::RenameHead { head, renamed } => {
                let expected_head_revision = self.head_revision(head)?;
                if head == renamed {
                    return Ok(None);
                }
                JournalRecord::RenameHead {
                    sequence,
                    record_id,
                    head: head.clone(),
                    expected_head_revision,
                    renamed: renamed.clone(),
                }
            }
            TreeEditAction::AbandonHead { head } => JournalRecord::AbandonHead {
                sequence,
                record_id,
                head: head.clone(),
                expected_head_revision: self.head_revision(head)?,
            },
            TreeEditAction::SetLabel { entry_id, label } => {
                self.validate_target(Some(entry_id))?;
                if self.tree_label(entry_id) == label.as_ref() {
                    return Ok(None);
                }
                JournalRecord::SetEntryLabel {
                    sequence,
                    record_id,
                    entry_id: entry_id.clone(),
                    label: label.clone(),
                }
            }
        };
        self.validate_record(&record)?;
        Ok(Some(record))
    }
}
