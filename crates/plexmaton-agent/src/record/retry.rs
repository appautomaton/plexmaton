use super::*;

impl Record {
    /// Preserve the failed branch, then move the active head before its question (JRN-1).
    pub(crate) fn branch_before_retry(
        &mut self,
        candidate: &crate::RetryCandidate,
        reaction: &mut Reaction,
    ) -> Result<(), crate::JournalError> {
        let sequence = self.journal.next_sequence();
        let archived = HeadName::new(format!("before-edit-{}", sequence.get()))
            .expect("generated branch name");
        let record_id = JournalRecordId::new(format!(
            "{}-record-{}",
            self.journal.conversation_id(),
            sequence.get()
        ))
        .expect("generated record id");
        let create = JournalRecord::CreateHead {
            sequence,
            record_id: record_id.clone(),
            head: archived,
            at: self
                .journal
                .head_target(self.selected_head())
                .expect("selected head")
                .cloned(),
        };
        self.journal.validate_record(&create)?;
        // Preflight movement before changing either head. User-defined archive names and
        // unstable targets are ordinary typed refusals, not internal invariants.
        self.journal.validate_record(&JournalRecord::MoveHead {
            sequence,
            record_id,
            head: self.selected_head().clone(),
            expected_head_revision: candidate.target.head_revision,
            to: candidate.before_question.clone(),
        })?;
        self.journal.apply(create.clone())?;
        reaction.records.push(create);
        let sequence = self.journal.next_sequence();
        let record_id = JournalRecordId::new(format!(
            "{}-record-{}",
            self.journal.conversation_id(),
            sequence.get()
        ))
        .expect("generated record id");
        let movement = JournalRecord::MoveHead {
            sequence,
            record_id,
            head: self.selected_head().clone(),
            expected_head_revision: candidate.target.head_revision,
            to: candidate.before_question.clone(),
        };
        self.journal
            .apply(movement.clone())
            .expect("question parent is a completed boundary");
        reaction.records.push(movement);
        let _ = self.rebuild_projection();
        Ok(())
    }
}
