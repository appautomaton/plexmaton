use plexmaton_core::{AgentId, ConversationEntryId, HeadName, TurnId};

use super::{ConversationJournal, JournalEntryPayload, JournalError, JournalRecord};
use crate::{TurnFinished, TurnFinishedAt, TurnOutcome};

impl ConversationJournal {
    pub(super) fn validate_new_turn(
        &self,
        turn: &TurnId,
        open: Option<&TurnId>,
    ) -> Result<(), JournalError> {
        if self.turn_starts.contains_key(turn) {
            return Err(JournalError::DuplicateTurn(turn.clone()));
        }
        if let Some(open) = open {
            return Err(JournalError::UnstableTurnTarget(open.clone()));
        }
        Ok(())
    }

    pub(super) fn validate_skill_activation(
        &self,
        agent_id: &AgentId,
        turn_id: &TurnId,
        open_turn: Option<&TurnId>,
    ) -> Result<(), JournalError> {
        self.validate_steering(agent_id, turn_id, open_turn)?;
        let follows_matching_input = self.records.last().is_some_and(|record| {
            let JournalRecord::AppendEntry { entry, .. } = record else {
                return false;
            };
            matches!(
                &entry.payload,
                JournalEntryPayload::TurnStarted {
                    agent_id: input_agent,
                    turn_id: input_turn,
                    ..
                }
                | JournalEntryPayload::SteeringAccepted {
                    agent_id: input_agent,
                    turn_id: input_turn,
                    ..
                } if input_agent == agent_id && input_turn == turn_id
            )
        });
        if !follows_matching_input {
            return Err(JournalError::InvalidSkillActivationOrder(turn_id.clone()));
        }
        Ok(())
    }

    pub(super) fn validate_turn_finished(
        &self,
        fact: &TurnFinished,
        open_turn: Option<&TurnId>,
    ) -> Result<(), JournalError> {
        let start = self
            .turn_starts
            .get(&fact.turn_id)
            .ok_or_else(|| JournalError::MissingTurn(fact.turn_id.clone()))?;
        if self.turn_finishes.contains_key(&fact.turn_id) {
            return Err(JournalError::DuplicateTurnFinish(fact.turn_id.clone()));
        }
        if start.agent_id != fact.agent_id {
            return Err(JournalError::WrongTurnAgent {
                turn_id: fact.turn_id.clone(),
                expected: start.agent_id.clone(),
                actual: fact.agent_id.clone(),
            });
        }
        if open_turn != Some(&fact.turn_id) {
            return Err(JournalError::InvalidTurnBoundary {
                turn_id: fact.turn_id.clone(),
                boundary: fact.semantic_boundary.clone(),
            });
        }
        let recovery = matches!(fact.at, TurnFinishedAt::Recovered { .. });
        if recovery != matches!(fact.outcome, TurnOutcome::ProcessDied) {
            return Err(JournalError::InvalidTurnFinishTime(fact.turn_id.clone()));
        }
        Ok(())
    }

    pub(super) fn validate_steering(
        &self,
        agent_id: &AgentId,
        turn_id: &TurnId,
        open_turn: Option<&TurnId>,
    ) -> Result<(), JournalError> {
        let start = self
            .turn_starts
            .get(turn_id)
            .ok_or_else(|| JournalError::MissingTurn(turn_id.clone()))?;
        if &start.agent_id != agent_id {
            return Err(JournalError::WrongTurnAgent {
                turn_id: turn_id.clone(),
                expected: start.agent_id.clone(),
                actual: agent_id.clone(),
            });
        }
        if self.turn_finishes.contains_key(turn_id) {
            return Err(JournalError::ClosedTurnInput(turn_id.clone()));
        }
        if open_turn != Some(turn_id) {
            return Err(JournalError::InvalidTurnBoundary {
                turn_id: turn_id.clone(),
                boundary: start.entry_id.clone(),
            });
        }
        Ok(())
    }

    pub(super) fn validate_turn_status(
        &self,
        agent_id: &AgentId,
        turn_id: &TurnId,
        open_turn: Option<&TurnId>,
    ) -> Result<(), JournalError> {
        self.validate_steering(agent_id, turn_id, open_turn)
    }

    pub(super) fn validate_stable_target(
        &self,
        target: Option<&ConversationEntryId>,
    ) -> Result<(), JournalError> {
        let Some(target) = target else {
            return Ok(());
        };
        if !self.stable_entries.contains(target) {
            let turn_id = self
                .unstable_entry_turns
                .get(target)
                .cloned()
                .unwrap_or_else(|| unreachable!("every entry has one stability classification"));
            return Err(JournalError::UnstableTurnTarget(turn_id));
        }
        Ok(())
    }

    pub(crate) fn open_turn_on_path(&self, head: &HeadName) -> Option<TurnId> {
        self.head(head)
            .unwrap_or_else(|error| unreachable!("selected head remains valid: {error:?}"))
            .open_turn
            .clone()
    }
}
