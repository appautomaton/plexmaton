use plexmaton_core::{ConversationEntryId, HeadName, TranscriptItemId, TurnId};

use super::{ConversationJournal, HeadRevision, JournalEntryPayload};
use crate::{RequestAttemptTerminalState, RequestDispatchedOutcome, TurnOutcome};

/// Compare-and-set identity of the exact failed conversation tail a user acted on.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetryTarget {
    pub turn_id: TurnId,
    pub head_revision: HeadRevision,
}

/// A rate-limited execution with no model output or tool effects to repeat.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetryCandidate {
    pub target: RetryTarget,
    pub question: String,
    /// Explicitly activated skill selected with this question, derived only from its typed fact.
    pub skill: Option<String>,
    pub question_item: TranscriptItemId,
    pub error_item: TranscriptItemId,
    pub before_question: Option<ConversationEntryId>,
}

impl ConversationJournal {
    pub(super) fn validate_retry(
        &self,
        head: &HeadName,
        agent_id: &plexmaton_core::AgentId,
        source_turn_id: &TurnId,
        turn_id: &TurnId,
    ) -> Result<(), super::JournalError> {
        let candidate = self
            .retry_candidate(head)
            .ok_or(super::JournalError::RetryUnavailable)?;
        if &candidate.target.turn_id != source_turn_id
            || self
                .turn_starts
                .get(source_turn_id)
                .is_none_or(|start| &start.agent_id != agent_id)
        {
            return Err(super::JournalError::RetryUnavailable);
        }
        if self.turn_starts.contains_key(turn_id) {
            return Err(super::JournalError::DuplicateTurn(turn_id.clone()));
        }
        Ok(())
    }

    /// Derives eligibility from typed attempts and the selected path, never diagnostic strings.
    pub fn retry_candidate(&self, head: &HeadName) -> Option<RetryCandidate> {
        let state = self.heads.get(head)?;
        if state.open_turn.is_some() {
            return None;
        }
        let path = self.path(head).ok()?;
        let latest = path.iter().rev().find(|entry| {
            matches!(
                entry.payload,
                JournalEntryPayload::TurnStarted { .. }
                    | JournalEntryPayload::TurnRetried { .. }
                    | JournalEntryPayload::CollaborationTurnStarted { .. }
            )
        })?;
        let turn_id = match &latest.payload {
            JournalEntryPayload::TurnStarted { turn_id, .. }
            | JournalEntryPayload::TurnRetried { turn_id, .. } => turn_id,
            _ => return None,
        };
        if self.turn_finishes.get(turn_id)?.fact.outcome != TurnOutcome::Failed {
            return None;
        }
        let attempt = self
            .request_attempts()
            .filter(|attempt| {
                attempt
                    .authorization()
                    .owner()
                    .agent_step()
                    .is_some_and(|step| step.turn_id() == turn_id)
            })
            .last()?;
        if !matches!(
            attempt.terminal()?.terminal(),
            RequestAttemptTerminalState::Dispatched {
                outcome: RequestDispatchedOutcome::RateLimited,
                ..
            }
        ) {
            return None;
        }
        let question_index = path
            .iter()
            .rposition(|entry| matches!(entry.payload, JournalEntryPayload::TurnStarted { .. }))?;
        let question_entry = path[question_index];
        let JournalEntryPayload::TurnStarted { item_id, text, .. } = &question_entry.payload else {
            return None;
        };
        let skill = path
            .get(question_index + 1)
            .and_then(|entry| match &entry.payload {
                JournalEntryPayload::SkillActivated {
                    turn_id: activated_turn,
                    activation,
                    ..
                } if activated_turn == turn_id => Some(activation.name().to_owned()),
                _ => None,
            });
        if path[question_index + 1..].iter().any(|entry| {
            matches!(
                entry.payload,
                JournalEntryPayload::AssistantOutput { .. }
                    | JournalEntryPayload::SteeringAccepted { .. }
                    | JournalEntryPayload::ToolCallRequested { .. }
                    | JournalEntryPayload::ToolCallChanged { .. }
            )
        }) {
            return None;
        }
        let error_item = path[question_index + 1..]
            .iter()
            .rev()
            .find_map(|entry| match &entry.payload {
                JournalEntryPayload::RuntimeError { item_id, .. } => Some(item_id.clone()),
                _ => None,
            })?;
        Some(RetryCandidate {
            target: RetryTarget {
                turn_id: turn_id.clone(),
                head_revision: state.revision,
            },
            question: text.clone(),
            skill,
            question_item: item_id.clone(),
            error_item,
            before_question: question_entry.parent_id.clone(),
        })
    }
}
