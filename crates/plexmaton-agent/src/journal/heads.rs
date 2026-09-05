//! Head identity, ancestry and revision preconditions shared by journal transitions.
use super::*;

impl SessionJournal {
    pub(super) fn head(&self, head: &HeadName) -> Result<&HeadState, JournalError> {
        self.heads
            .get(head)
            .ok_or_else(|| JournalError::MissingHead(head.clone()))
    }

    pub(super) fn validate_head(
        &self,
        head: &HeadName,
        expected: HeadRevision,
    ) -> Result<&HeadState, JournalError> {
        let state = self.head(head)?;
        if state.revision != expected {
            return Err(JournalError::StaleHead {
                head: head.clone(),
                expected,
                actual: state.revision,
            });
        }
        Ok(state)
    }

    pub(super) fn validate_available_head(&self, head: &HeadName) -> Result<(), JournalError> {
        if self.heads.contains_key(head) || self.retired_heads.contains(head) {
            return Err(JournalError::UnavailableHeadName(head.clone()));
        }
        Ok(())
    }

    pub(super) fn validate_target(
        &self,
        target: Option<&SessionEntryId>,
    ) -> Result<(), JournalError> {
        if let Some(target) = target
            && !self.entries.contains_key(target)
        {
            return Err(JournalError::MissingEntry(target.clone()));
        }
        Ok(())
    }

    pub(super) fn validate_revision_increment(
        &self,
        head: &HeadName,
        revision: HeadRevision,
    ) -> Result<(), JournalError> {
        revision
            .get()
            .checked_add(1)
            .map(|_| ())
            .ok_or_else(|| JournalError::RevisionExhausted(head.clone()))
    }
}
