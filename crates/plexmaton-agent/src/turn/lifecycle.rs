//! Turn completion, cancellation, status, and visible runtime degradation.

use plexmaton_core::{AgentStatus, ApprovalDecision, ApprovalId, SessionEvent};

use super::{Agent, DeliveryBoundary, Turn};
use crate::{
    interface::{ApprovalDecisionRefusal, Reaction, UndeliveredReason, UnresolvedApprovalDecision},
    model::ModelError,
    tools::ToolCancellationReason,
};

impl Agent {
    pub(super) fn fail(&mut self, error: &ModelError, reaction: &mut Reaction) {
        self.warn(reaction, &error.message());
        if self.is_running() {
            self.abort_turn(
                UndeliveredReason::StepFailed,
                ToolCancellationReason::StepFailed,
                reaction,
            );
        }
    }

    pub(super) fn interrupt(&mut self, reaction: &mut Reaction) {
        if self.is_running() {
            self.abort_turn(
                UndeliveredReason::Interrupted,
                ToolCancellationReason::Interrupted,
                reaction,
            );
        }
    }

    pub(super) fn shutdown(&mut self, reaction: &mut Reaction) {
        if self.is_running() {
            self.abort_turn(
                UndeliveredReason::Shutdown,
                ToolCancellationReason::Shutdown,
                reaction,
            );
        }
    }

    /// Stops the turn wherever it is, pays what it owes, and goes idle.
    ///
    /// A stopped turn does not roll into the next one: that would make cancellation start work.
    /// Pending input instead returns through [`Reaction::undelivered`] with its exact text and the
    /// transition that prevented its boundary from opening (LOOP-6).
    fn abort_turn(
        &mut self,
        reason: UndeliveredReason,
        cancellation: ToolCancellationReason,
        reaction: &mut Reaction,
    ) {
        self.abandon(cancellation, reaction);
        self.close_step(reaction);
        self.turn = Turn::Idle;
        reaction.undelivered.extend(self.input.reject_all(reason));
        self.status(reaction, AgentStatus::Idle);
    }

    /// Ends a turn that ran its course, and opens the next one if a message waited for it.
    pub(super) fn finish_turn(&mut self, reaction: &mut Reaction) {
        self.turn = Turn::Idle;
        reaction.undelivered.extend(
            self.input
                .reject(DeliveryBoundary::NextStep, UndeliveredReason::TurnEnded),
        );
        let Some(next) = self.input.claim_one(DeliveryBoundary::NextTurn) else {
            self.status(reaction, AgentStatus::Idle);
            return;
        };
        self.open_turn(next, reaction);
    }

    /// Claims steering immediately before the request for the next step is assembled (LOOP-6).
    pub(super) fn claim_next_step_input(&mut self, reaction: &mut Reaction) {
        for text in self.input.claim(DeliveryBoundary::NextStep) {
            self.record_user(text, reaction);
        }
    }

    pub(super) fn status(&mut self, reaction: &mut Reaction, status: AgentStatus) {
        let event = SessionEvent::AgentStatusChanged {
            agent_id: self.record.agent_id().clone(),
            status,
        };
        self.record.emit(reaction, event);
    }

    pub(super) fn warn(&mut self, reaction: &mut Reaction, message: &str) {
        let event = SessionEvent::RuntimeWarning {
            message: message.to_owned(),
        };
        self.record.emit(reaction, event);
    }

    pub(super) fn refuse_approval_decision(
        reaction: &mut Reaction,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
    ) {
        reaction
            .unresolved_approvals
            .push(UnresolvedApprovalDecision {
                approval_id,
                decision,
                reason: ApprovalDecisionRefusal::NotPending,
            });
    }
}
