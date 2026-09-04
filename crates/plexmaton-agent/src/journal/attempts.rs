use plexmaton_core::{HeadName, SessionEntryId};

use super::{HeadRevision, JournalError, SessionJournal};
use crate::{ModelStepId, RequestAttemptAuthorized, RequestAttemptOwner};

impl SessionJournal {
    pub(super) fn validate_request_authorization(
        &self,
        head: &HeadName,
        expected_head_revision: HeadRevision,
        fact: &RequestAttemptAuthorized,
    ) -> Result<(), JournalError> {
        if self.request_attempts.contains_key(fact.attempt_id()) {
            return Err(JournalError::DuplicateRequestAttempt(
                fact.attempt_id().clone(),
            ));
        }
        if let Some(active) = self.active_request_owners.get(fact.owner()) {
            return Err(JournalError::RequestAttemptOwnerActive(active.clone()));
        }
        let state = self.validate_head(head, expected_head_revision)?;
        if state.target.as_ref() != Some(fact.semantic_boundary()) {
            return Err(JournalError::InvalidRequestAttemptBoundary {
                attempt_id: fact.attempt_id().clone(),
                boundary: fact.semantic_boundary().clone(),
            });
        }
        if let RequestAttemptOwner::AgentStep { step_id } = fact.owner() {
            self.validate_attempt_step(step_id, state.open_turn.as_ref())?;
        }
        Ok(())
    }

    fn validate_attempt_step(
        &self,
        step_id: &ModelStepId,
        open_turn: Option<&plexmaton_core::TurnId>,
    ) -> Result<(), JournalError> {
        let start = self
            .turn_starts
            .get(step_id.turn_id())
            .ok_or_else(|| JournalError::MissingTurn(step_id.turn_id().clone()))?;
        if open_turn != Some(step_id.turn_id()) {
            return Err(JournalError::InvalidTurnBoundary {
                turn_id: step_id.turn_id().clone(),
                boundary: start.entry_id.clone(),
            });
        }
        let expected = self.expected_model_step_index(step_id.turn_id())?;
        if step_id.index() != expected {
            return Err(JournalError::UnexpectedModelStep {
                turn_id: step_id.turn_id().clone(),
                expected,
                actual: step_id.index(),
            });
        }
        Ok(())
    }

    pub(super) fn expected_model_step_index(
        &self,
        turn_id: &plexmaton_core::TurnId,
    ) -> Result<u16, JournalError> {
        match self.last_model_step_indexes.get(turn_id) {
            Some(prior) => prior
                .checked_add(1)
                .ok_or_else(|| JournalError::ModelStepSequenceExhausted(turn_id.clone())),
            None => Ok(1),
        }
    }

    pub(super) fn validate_request_terminal(
        &self,
        fact: &crate::RequestAttemptTerminal,
    ) -> Result<(), JournalError> {
        let Some(attempt) = self.request_attempts.get(fact.attempt_id()) else {
            return Err(JournalError::MissingRequestAttempt(
                fact.attempt_id().clone(),
            ));
        };
        if attempt.terminal().is_some() {
            return Err(JournalError::DuplicateRequestAttemptTerminal(
                fact.attempt_id().clone(),
            ));
        }
        fact.validate()
            .map_err(|error| JournalError::InvalidRequestAttemptTerminal {
                attempt_id: fact.attempt_id().clone(),
                error,
            })
    }

    pub(super) fn boundary_is_selected(
        selected: &std::collections::BTreeSet<SessionEntryId>,
        fact: &RequestAttemptAuthorized,
    ) -> bool {
        selected.contains(fact.semantic_boundary())
    }
}
