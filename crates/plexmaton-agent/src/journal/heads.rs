//! Head identity, ancestry and revision preconditions shared by journal transitions.
use super::*;

impl ConversationJournal {
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

    pub(super) fn validate_selected_origin(&self, expected: &HeadName) -> Result<(), JournalError> {
        if self.selected != *expected {
            return Err(JournalError::SelectedHeadMismatch {
                expected: expected.clone(),
                actual: self.selected.clone(),
            });
        }
        Ok(())
    }

    pub(super) fn validate_target(
        &self,
        target: Option<&ConversationEntryId>,
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

    pub(super) fn validate_named_head(&self, record: &JournalRecord) -> Result<(), JournalError> {
        match record {
            JournalRecord::CreateHead { head, at, .. } => {
                self.validate_available_head(head)?;
                self.validate_target(at.as_ref())?;
                self.validate_stable_target(at.as_ref())
            }
            JournalRecord::MoveHead {
                head,
                expected_head_revision,
                to,
                ..
            } => {
                let state = self.validate_head(head, *expected_head_revision)?;
                self.validate_target(to.as_ref())?;
                self.validate_stable_target(to.as_ref())?;
                self.validate_revision_increment(head, state.revision)
            }
            JournalRecord::RenameHead {
                head,
                expected_head_revision,
                renamed,
                ..
            } => {
                let state = self.validate_head(head, *expected_head_revision)?;
                self.validate_available_head(renamed)?;
                self.validate_revision_increment(head, state.revision)
            }
            JournalRecord::AbandonHead {
                head,
                expected_head_revision,
                ..
            } => {
                let state = self.validate_head(head, *expected_head_revision)?;
                if self.selected == *head {
                    return Err(JournalError::CannotAbandonSelectedHead(head.clone()));
                }
                self.validate_stable_target(state.target.as_ref())
            }
            JournalRecord::ForkAndSelectHead {
                source,
                expected_source_revision,
                destination,
                at,
                ..
            } => {
                self.validate_selected_origin(source)?;
                self.validate_head(source, *expected_source_revision)?;
                self.validate_available_head(destination)?;
                self.validate_target(at.as_ref())?;
                self.validate_stable_target(at.as_ref())
            }
            JournalRecord::SelectHead {
                expected_selected,
                destination,
                expected_destination_revision,
                ..
            } => {
                self.validate_selected_origin(expected_selected)?;
                self.validate_head(destination, *expected_destination_revision)
                    .map(|_| ())
            }
            _ => unreachable!("named-head validation is only called for head mutations"),
        }
    }

    pub(super) fn apply_named_head(&mut self, record: &JournalRecord) {
        match record {
            JournalRecord::CreateHead { head, at, .. } => {
                self.heads.insert(
                    head.clone(),
                    HeadState {
                        target: at.clone(),
                        revision: HeadRevision::new(0),
                        open_turn: None,
                    },
                );
            }
            JournalRecord::MoveHead { head, to, .. } => {
                let state = self
                    .heads
                    .get_mut(head)
                    .unwrap_or_else(|| unreachable!("validated head must remain present"));
                state.target = to.clone();
                state.revision = HeadRevision::new(state.revision.get() + 1);
                state.open_turn = None;
            }
            JournalRecord::RenameHead { head, renamed, .. } => {
                let mut state = self
                    .heads
                    .remove(head)
                    .unwrap_or_else(|| unreachable!("validated head must remain present"));
                state.revision = HeadRevision::new(state.revision.get() + 1);
                self.retired_heads.insert(head.clone());
                if self.selected == *head {
                    self.selected = renamed.clone();
                }
                self.heads.insert(renamed.clone(), state);
            }
            JournalRecord::AbandonHead { head, .. } => {
                self.heads.remove(head);
                self.retired_heads.insert(head.clone());
            }
            JournalRecord::ForkAndSelectHead {
                destination, at, ..
            } => {
                self.heads.insert(
                    destination.clone(),
                    HeadState {
                        target: at.clone(),
                        revision: HeadRevision::new(0),
                        open_turn: None,
                    },
                );
                self.selected = destination.clone();
            }
            JournalRecord::SelectHead { destination, .. } => {
                self.selected = destination.clone();
            }
            _ => unreachable!("named-head reduction is only called for head mutations"),
        }
    }
}
