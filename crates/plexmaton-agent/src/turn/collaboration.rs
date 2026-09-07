use plexmaton_core::{AgentStatus, ConversationEvent, TurnId};

use super::{Agent, DeliveryBoundary};
use crate::collaboration::{CollaborationError, ResolvedTurnAdmission, TurnBoundary};
use crate::{JournalEntryPayload, Reaction, UnixMillis};

impl Agent {
    /// Proposes an idle turn boundary; queued ordinary inputs must remain with their own owner.
    pub fn collaboration_boundary(&self, turn: TurnId) -> Result<TurnBoundary, CollaborationError> {
        if self.is_running()
            || !self.record.is_announced()
            || self
                .input
                .pending(DeliveryBoundary::NextTurn)
                .next()
                .is_some()
            || self
                .input
                .pending(DeliveryBoundary::NextStep)
                .next()
                .is_some()
        {
            return Err(CollaborationError::InvalidTurnBoundary);
        }
        self.journal()
            .collaboration_boundary(self.selected_head(), self.record.agent_id(), turn)
            .map_err(|_| CollaborationError::InvalidTurnBoundary)
    }

    /// Settles a preparation failure before request authorization, preserving a typed cause at
    /// the runtime boundary rather than classifying missing local context as a transport error.
    pub fn fail_collaboration_request(
        &mut self,
        step: &crate::ModelStepId,
        reason: &CollaborationError,
        at: UnixMillis,
    ) -> Result<Reaction, CollaborationError> {
        if self.active_model_step().as_ref() != Some(step)
            || self.journal().request_attempts().any(|attempt| {
                attempt.terminal().is_none()
                    && attempt.authorization().owner().agent_step() == Some(step)
            })
        {
            return Err(CollaborationError::InvalidTurnBoundary);
        }
        let mut reaction = Reaction::at(at);
        self.abort_turn(
            crate::UndeliveredReason::StepFailed,
            crate::ToolCancellationReason::StepFailed,
            &mut reaction,
        );
        self.error(
            &mut reaction,
            &format!("collaboration context preparation failed: {reason}"),
        );
        Ok(reaction.into_output())
    }

    /// Starts a typed collaboration turn without creating a user message (CIN-2).
    pub fn start_collaboration_turn(
        &mut self,
        resolved: &ResolvedTurnAdmission,
        at: UnixMillis,
    ) -> Result<Reaction, CollaborationError> {
        let admission = resolved.admission();
        let boundary = &admission.boundary;
        if self.collaboration_boundary(boundary.turn.clone())? != *boundary
            || self
                .journal()
                .previous_collaboration_inclusion(self.selected_head())
                .map_err(|_| CollaborationError::InvalidTurnBoundary)?
                != admission.previous
        {
            return Err(CollaborationError::InvalidTurnBoundary);
        }
        self.journal()
            .validate_collaboration_start(resolved.reference(), &boundary.turn, None)
            .map_err(|_| CollaborationError::InvalidTurnBoundary)?;
        let mut reaction = Reaction::at(at);
        self.record.commit(
            JournalEntryPayload::CollaborationTurnStarted {
                agent_id: boundary.recipient.agent.clone(),
                turn_id: boundary.turn.clone(),
                reference: resolved.reference().clone(),
                opened_at: at,
            },
            &mut reaction,
        );
        self.record.emit(
            &mut reaction,
            ConversationEvent::AgentStatusChanged {
                agent_id: boundary.recipient.agent.clone(),
                status: AgentStatus::Running,
            },
        );
        self.open_step(boundary.turn.clone(), 1, &mut reaction);
        Ok(reaction.into_output())
    }
}
