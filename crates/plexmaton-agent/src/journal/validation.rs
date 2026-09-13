//! Envelope and mutation validation shared by append and replay.

use super::{
    ConversationJournal, JournalEntryPayload, JournalError, JournalRecord, JournalSequence,
};

impl ConversationJournal {
    fn validate_envelope(&self, record: &JournalRecord) -> Result<JournalSequence, JournalError> {
        if record.sequence() != self.next_sequence {
            return Err(JournalError::UnexpectedSequence {
                expected: self.next_sequence,
                actual: record.sequence(),
            });
        }
        if self.record_ids.contains(record.record_id()) {
            return Err(JournalError::DuplicateRecord(record.record_id().clone()));
        }
        let next_sequence = self
            .next_sequence
            .get()
            .checked_add(1)
            .map(JournalSequence::new)
            .ok_or(JournalError::SequenceExhausted)?;
        Ok(next_sequence)
    }

    pub(super) fn validate(&self, record: &JournalRecord) -> Result<JournalSequence, JournalError> {
        let next_sequence = self.validate_envelope(record)?;
        match record {
            JournalRecord::AppendEntry {
                head,
                expected_head_revision,
                entry,
                ..
            } => {
                if self.entries.contains_key(&entry.id) {
                    return Err(JournalError::DuplicateEntry(entry.id.clone()));
                }
                self.validate_target(entry.parent_id.as_ref())?;
                let state = self.validate_head(head, *expected_head_revision)?;
                if entry.parent_id != state.target {
                    return Err(JournalError::ParentMismatch {
                        head: head.clone(),
                        expected: state.target.clone(),
                        actual: entry.parent_id.clone(),
                    });
                }
                match &entry.payload {
                    JournalEntryPayload::AgentCreated {
                        agent_id, status, ..
                    } if *status != plexmaton_core::AgentStatus::Idle => {
                        return Err(JournalError::InvalidInitialAgentStatus(agent_id.clone()));
                    }
                    JournalEntryPayload::CollaborationTurnStarted {
                        reference, turn_id, ..
                    } => {
                        self.validate_collaboration_start(
                            reference,
                            turn_id,
                            state.open_turn.as_ref(),
                        )?;
                    }
                    JournalEntryPayload::TurnStarted { turn_id, .. } => {
                        self.validate_new_turn(turn_id, state.open_turn.as_ref())?;
                    }
                    JournalEntryPayload::TurnRetried {
                        source_turn_id,
                        turn_id,
                        agent_id,
                        ..
                    } => self.validate_retry(head, agent_id, source_turn_id, turn_id)?,
                    JournalEntryPayload::SteeringAccepted {
                        agent_id, turn_id, ..
                    } => self.validate_steering(agent_id, turn_id, state.open_turn.as_ref())?,
                    JournalEntryPayload::SkillActivated {
                        agent_id, turn_id, ..
                    } => {
                        self.validate_skill_activation(agent_id, turn_id, state.open_turn.as_ref())?
                    }
                    JournalEntryPayload::TurnStatusChanged {
                        agent_id, turn_id, ..
                    } => self.validate_turn_status(agent_id, turn_id, state.open_turn.as_ref())?,
                    JournalEntryPayload::AssistantOutput {
                        agent_id, step_id, ..
                    } => {
                        if self.model_steps.contains(step_id) {
                            return Err(JournalError::DuplicateModelStep(step_id.clone()));
                        }
                        let expected = self.expected_model_step_index(step_id.turn_id())?;
                        if step_id.index() != expected {
                            return Err(JournalError::UnexpectedModelStep {
                                turn_id: step_id.turn_id().clone(),
                                expected,
                                actual: step_id.index(),
                            });
                        }
                        self.validate_steering(
                            agent_id,
                            step_id.turn_id(),
                            state.open_turn.as_ref(),
                        )?;
                    }
                    JournalEntryPayload::CompactionCheckpoint {
                        agent_id,
                        checkpoint,
                    } => self.validate_compaction_checkpoint(head, agent_id, checkpoint)?,
                    _ => {}
                }
                self.validate_revision_increment(head, state.revision)?;
            }
            JournalRecord::CreateHead { .. }
            | JournalRecord::MoveHead { .. }
            | JournalRecord::RenameHead { .. }
            | JournalRecord::AbandonHead { .. }
            | JournalRecord::ForkAndSelectHead { .. }
            | JournalRecord::SelectHead { .. } => self.validate_named_head(record)?,
            JournalRecord::SetEntryLabel { entry_id, .. } => {
                self.validate_target(Some(entry_id))?;
            }
            JournalRecord::TurnFinished {
                head,
                expected_head_revision,
                fact,
                ..
            } => {
                let state = self.validate_head(head, *expected_head_revision)?;
                if state.target.as_ref() != Some(&fact.semantic_boundary) {
                    return Err(JournalError::InvalidTurnBoundary {
                        turn_id: fact.turn_id.clone(),
                        boundary: fact.semantic_boundary.clone(),
                    });
                }
                self.validate_turn_finished(fact, state.open_turn.as_ref())?;
            }
            JournalRecord::RequestAttemptAuthorized {
                head,
                expected_head_revision,
                fact,
                ..
            } => self.validate_request_authorization(head, *expected_head_revision, fact)?,
            JournalRecord::RequestAttemptFinished { fact, .. } => {
                self.validate_request_terminal(fact)?
            }
            JournalRecord::CompactionAttemptFinished { fact, .. } => {
                self.validate_compaction_attempt_finished(fact)?
            }
        }
        Ok(next_sequence)
    }
}
