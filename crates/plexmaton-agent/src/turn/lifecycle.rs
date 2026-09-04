//! Turn completion, cancellation, status, and visible runtime degradation.

use plexmaton_core::{AgentStatus, ApprovalDecision, ApprovalId, SessionEvent};

use super::{Agent, DeliveryBoundary, Turn};
use crate::{
    interface::{
        ApprovalDecisionRefusal, Reaction, ReleasedInput, UndeliveredInput, UndeliveredReason,
        UnresolvedApprovalDecision,
    },
    journal::JournalEntryPayload,
    model::ModelError,
    tools::ToolCancellationReason,
};

impl Agent {
    pub(super) fn fail(&mut self, error: &ModelError, reaction: &mut Reaction) {
        if self.is_running() {
            self.abort_turn(
                UndeliveredReason::StepFailed,
                ToolCancellationReason::StepFailed,
                reaction,
            );
        }
        self.error(reaction, &error.message());
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
        let released = self.input.drain_all();
        self.return_queued(released, reason, reaction);
        self.status(reaction, AgentStatus::Idle);
    }

    /// Ends a turn that ran its course, and opens the next one if a message waited for it.
    pub(super) fn finish_turn(&mut self, reaction: &mut Reaction) {
        self.turn = Turn::Idle;
        let released = self.input.claim(DeliveryBoundary::NextStep);
        self.return_queued(released, UndeliveredReason::TurnEnded, reaction);
        let Some(next) = self.input.claim_one(DeliveryBoundary::NextTurn) else {
            self.status(reaction, AgentStatus::Idle);
            return;
        };
        reaction
            .released_inputs
            .push(ReleasedInput::new(next.order, next.text.clone()));
        self.open_turn(next.text, reaction);
    }

    /// Claims steering immediately before the request for the next step is assembled (LOOP-6).
    pub(super) fn claim_next_step_input(&mut self, reaction: &mut Reaction) {
        for input in self.input.claim(DeliveryBoundary::NextStep) {
            reaction
                .released_inputs
                .push(ReleasedInput::new(input.order, input.text.clone()));
            self.record_user(input.text, reaction);
        }
    }

    pub(super) fn reject_queued(
        &mut self,
        boundary: DeliveryBoundary,
        reason: UndeliveredReason,
        reaction: &mut Reaction,
    ) {
        let released = self.input.claim(boundary);
        self.return_queued(released, reason, reaction);
    }

    fn return_queued(
        &self,
        released: Vec<super::input::QueuedInput>,
        reason: UndeliveredReason,
        reaction: &mut Reaction,
    ) {
        for input in released {
            reaction
                .released_inputs
                .push(ReleasedInput::new(input.order, input.text.clone()));
            reaction
                .undelivered
                .push(UndeliveredInput::new(input.text, reason));
        }
    }

    pub(super) fn status(&mut self, reaction: &mut Reaction, status: AgentStatus) {
        self.record.commit(
            JournalEntryPayload::AgentStatusChanged {
                agent_id: self.record.agent_id().clone(),
                status,
            },
            reaction,
        );
        let event = SessionEvent::AgentStatusChanged {
            agent_id: self.record.agent_id().clone(),
            status,
        };
        self.record.emit(reaction, event);
    }

    pub(super) fn warn(&mut self, reaction: &mut Reaction, message: &str) {
        let item_id = self.record.next_item_id();
        self.record.commit(
            JournalEntryPayload::RuntimeWarning {
                agent_id: self.record.agent_id().clone(),
                item_id: item_id.clone(),
                message: message.to_owned(),
            },
            reaction,
        );
        let event = SessionEvent::RuntimeWarning {
            agent_id: self.record.agent_id().clone(),
            item_id,
            message: message.to_owned(),
        };
        self.record.emit(reaction, event);
    }

    fn error(&mut self, reaction: &mut Reaction, message: &str) {
        let item_id = self.record.next_item_id();
        self.record.commit(
            JournalEntryPayload::RuntimeError {
                agent_id: self.record.agent_id().clone(),
                item_id: item_id.clone(),
                message: message.to_owned(),
            },
            reaction,
        );
        let event = SessionEvent::RuntimeError {
            agent_id: self.record.agent_id().clone(),
            item_id,
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
