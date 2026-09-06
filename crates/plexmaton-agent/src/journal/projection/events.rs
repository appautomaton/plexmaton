use plexmaton_core::{AgentId, AttentionId, SessionEvent, TranscriptItemId};

use super::super::JournalEntryPayload;
use super::{JournalProjectionError, Projector};

pub(super) fn visible_event(payload: JournalEntryPayload) -> SessionEvent {
    match payload {
        JournalEntryPayload::AgentCreated {
            agent_id,
            label,
            status,
        } => SessionEvent::AgentCreated {
            agent_id,
            label,
            status,
        },
        JournalEntryPayload::AttentionRequested {
            agent_id,
            attention_id,
            request,
        } => SessionEvent::AttentionRequested {
            agent_id,
            attention_id,
            request,
        },
        JournalEntryPayload::AttentionResolved {
            agent_id,
            attention_id,
        } => SessionEvent::AttentionResolved {
            agent_id,
            attention_id,
        },
        JournalEntryPayload::MailDelivered {
            item_id,
            mail_id,
            from,
            to,
            summary,
        } => SessionEvent::MailDelivered {
            item_id,
            mail_id,
            from,
            to,
            summary,
        },
        JournalEntryPayload::ArtifactAnnounced {
            agent_id,
            item_id,
            artifact_id,
            label,
            pointer,
        } => SessionEvent::ArtifactAnnounced {
            agent_id,
            item_id,
            artifact_id,
            label,
            pointer,
        },
        JournalEntryPayload::RuntimeWarning {
            agent_id,
            item_id,
            message,
        } => SessionEvent::RuntimeWarning {
            agent_id,
            item_id,
            message,
        },
        JournalEntryPayload::RuntimeError {
            agent_id,
            item_id,
            message,
        } => SessionEvent::RuntimeError {
            agent_id,
            item_id,
            message,
        },
        JournalEntryPayload::TurnInterruptedByRecovery { agent_id, item_id } => {
            SessionEvent::RuntimeWarning {
                agent_id,
                item_id,
                message: super::super::PROCESS_RECOVERY_MESSAGE.to_owned(),
            }
        }
        JournalEntryPayload::AssistantOutput { .. }
        | JournalEntryPayload::CompactionCheckpoint { .. }
        | JournalEntryPayload::TurnStatusChanged { .. }
        | JournalEntryPayload::TurnStarted { .. }
        | JournalEntryPayload::TurnRetried { .. }
        | JournalEntryPayload::SteeringAccepted { .. }
        | JournalEntryPayload::ToolCallRequested { .. }
        | JournalEntryPayload::ToolCallChanged { .. } => {
            unreachable!("model-bearing payloads are projected separately")
        }
    }
}

impl Projector {
    pub(super) fn compaction_attempt_finished(
        &mut self,
        fact: &crate::CompactionAttemptFinished,
        agent_id: &AgentId,
    ) -> Result<(), JournalProjectionError> {
        let Some(failure) = fact.outcome().failure() else {
            return Ok(());
        };
        self.require_agent(agent_id)?;
        let item_id = TranscriptItemId::new(format!("compaction-error-{}", fact.attempt_id()))
            .unwrap_or_else(|error| unreachable!("a formatted identity is valid: {error}"));
        self.claim_entry(&item_id, agent_id)?;
        self.emit(SessionEvent::RuntimeError {
            agent_id: agent_id.clone(),
            item_id,
            message: failure.to_string(),
        })
    }

    pub(super) fn require_agent(&self, agent_id: &AgentId) -> Result<(), JournalProjectionError> {
        if self.agents.contains(agent_id) {
            Ok(())
        } else {
            Err(JournalProjectionError::MissingAgent(agent_id.clone()))
        }
    }

    pub(super) fn claim_entry(
        &mut self,
        item_id: &TranscriptItemId,
        agent_id: &AgentId,
    ) -> Result<(), JournalProjectionError> {
        if self.entries.contains_key(item_id) {
            return Err(JournalProjectionError::DuplicateTranscriptItem(
                item_id.clone(),
            ));
        }
        self.entries.insert(item_id.clone(), agent_id.clone());
        Ok(())
    }

    pub(super) fn visible(
        &mut self,
        payload: JournalEntryPayload,
    ) -> Result<(), JournalProjectionError> {
        match &payload {
            JournalEntryPayload::AgentCreated { agent_id, .. } => {
                if !self.agents.insert(agent_id.clone()) {
                    return Err(JournalProjectionError::DuplicateAgent(agent_id.clone()));
                }
            }
            JournalEntryPayload::AttentionRequested {
                agent_id,
                attention_id,
                ..
            } => {
                self.require_agent(agent_id)?;
                self.validate_attention_owner(attention_id, agent_id)?;
                self.attention
                    .insert(attention_id.clone(), agent_id.clone());
            }
            JournalEntryPayload::AttentionResolved {
                agent_id,
                attention_id,
            } => {
                self.require_agent(agent_id)?;
                self.validate_attention_owner(attention_id, agent_id)?;
                self.attention.remove(attention_id);
            }
            JournalEntryPayload::MailDelivered {
                item_id, from, to, ..
            } => {
                self.require_agent(from)?;
                self.require_agent(to)?;
                self.claim_entry(item_id, from)?;
            }
            JournalEntryPayload::ArtifactAnnounced {
                agent_id, item_id, ..
            }
            | JournalEntryPayload::RuntimeWarning {
                agent_id, item_id, ..
            }
            | JournalEntryPayload::RuntimeError {
                agent_id, item_id, ..
            }
            | JournalEntryPayload::TurnInterruptedByRecovery {
                agent_id, item_id, ..
            } => {
                self.require_agent(agent_id)?;
                self.claim_entry(item_id, agent_id)?;
            }
            JournalEntryPayload::TurnStatusChanged { .. }
            | JournalEntryPayload::TurnStarted { .. }
            | JournalEntryPayload::TurnRetried { .. }
            | JournalEntryPayload::SteeringAccepted { .. }
            | JournalEntryPayload::AssistantOutput { .. }
            | JournalEntryPayload::CompactionCheckpoint { .. }
            | JournalEntryPayload::ToolCallRequested { .. }
            | JournalEntryPayload::ToolCallChanged { .. } => {
                unreachable!("model-bearing payloads are projected separately")
            }
        }
        self.emit(visible_event(payload))
    }

    fn validate_attention_owner(
        &self,
        attention_id: &AttentionId,
        agent_id: &AgentId,
    ) -> Result<(), JournalProjectionError> {
        if let Some(expected) = self.attention.get(attention_id)
            && expected != agent_id
        {
            return Err(JournalProjectionError::AttentionOwnerMismatch {
                attention_id: attention_id.clone(),
                expected: expected.clone(),
                actual: agent_id.clone(),
            });
        }
        Ok(())
    }
}
