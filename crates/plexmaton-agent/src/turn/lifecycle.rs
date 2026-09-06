//! Turn completion, cancellation, status, and visible runtime degradation.

use plexmaton_core::{
    ApprovalDecision, ApprovalId, ConversationEvent, ToolCallStatus, ToolPresentation,
};

use super::{Agent, DeliveryBoundary, Turn};
use crate::{
    ActiveTurnStatus, TurnFinishedAt, TurnOutcome,
    interface::{
        ApprovalDecisionRefusal, Reaction, ReleasedInput, UndeliveredInput, UndeliveredReason,
        UnresolvedApprovalDecision,
    },
    journal::{JournalEntryPayload, PROCESS_RECOVERY_MESSAGE},
    model::ModelError,
    tools::{ToolCancellationReason, ToolOutcome, unexecuted_outcome},
};

impl Agent {
    #[cfg(test)]
    pub(crate) fn recover_after_process_death(&mut self) -> Option<Reaction> {
        self.recover_after_process_death_at(crate::UnixMillis::EPOCH)
    }

    /// Settles orphaned work at an externally observed recovery time (TIM-1, JRN-5).
    pub fn recover_after_process_death_at(
        &mut self,
        observed_at: crate::UnixMillis,
    ) -> Option<Reaction> {
        let recovery = self.record.interrupted_turn()?;
        let mut reaction = Reaction::at(observed_at);
        if recovery.needs_marker {
            let item_id = self.record.next_item_id();
            self.record.commit(
                JournalEntryPayload::TurnInterruptedByRecovery {
                    agent_id: self.record.agent_id().clone(),
                    item_id: item_id.clone(),
                },
                &mut reaction,
            );
            self.record.emit(
                &mut reaction,
                ConversationEvent::RuntimeWarning {
                    agent_id: self.record.agent_id().clone(),
                    item_id,
                    message: PROCESS_RECOVERY_MESSAGE.to_owned(),
                },
            );
        }
        for tool in recovery.tools {
            if !tool.requested {
                self.record.commit(
                    JournalEntryPayload::ToolCallRequested {
                        agent_id: self.record.agent_id().clone(),
                        call_id: tool.call_id.clone(),
                        presentation: tool.presentation.clone(),
                    },
                    &mut reaction,
                );
                self.record.emit(
                    &mut reaction,
                    ConversationEvent::ToolCallChanged {
                        agent_id: self.record.agent_id().clone(),
                        item_id: tool.item_id.clone(),
                        item_revision: 0,
                        call_id: tool.call_id.clone(),
                        label: tool.label.clone(),
                        status: ToolCallStatus::Queued,
                        presentation: tool.presentation.clone(),
                    },
                );
            }
            if let Some(attention_id) = tool.attention_id {
                self.resolve_attention(attention_id, &mut reaction);
            }
            let outcome = ToolOutcome::Cancelled {
                reason: ToolCancellationReason::ProcessDied,
            };
            let presentation = ToolPresentation {
                invocation: tool.presentation.invocation,
                outcome: unexecuted_outcome(&outcome),
            };
            let item_revision = tool.item_revision.saturating_add(1);
            self.record.commit(
                JournalEntryPayload::ToolCallChanged {
                    agent_id: self.record.agent_id().clone(),
                    call_id: tool.call_id.clone(),
                    item_revision,
                    status: ToolCallStatus::Cancelled,
                    presentation: presentation.clone(),
                    outcome: Some(outcome),
                },
                &mut reaction,
            );
            self.record.emit(
                &mut reaction,
                ConversationEvent::ToolCallChanged {
                    agent_id: self.record.agent_id().clone(),
                    item_id: tool.item_id,
                    item_revision,
                    call_id: tool.call_id,
                    label: tool.label,
                    status: ToolCallStatus::Cancelled,
                    presentation,
                },
            );
        }
        if let Some(turn_id) = recovery.turn_id {
            self.record.finish_turn(
                turn_id,
                TurnOutcome::ProcessDied,
                TurnFinishedAt::Recovered {
                    recovery_observed_at: observed_at,
                },
                &mut reaction,
            );
        }
        Some(reaction.into_output())
    }

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
        let turn_id = self
            .active_turn_id()
            .unwrap_or_else(|| unreachable!("abort was guarded by an active turn"));
        self.abandon(cancellation, reaction);
        self.abort_step(reaction);
        self.turn = Turn::Idle;
        let released = self.input.drain_all();
        self.return_queued(released, reason, reaction);
        let outcome = match reason {
            UndeliveredReason::Interrupted => TurnOutcome::Interrupted,
            UndeliveredReason::Shutdown => TurnOutcome::Shutdown,
            UndeliveredReason::StepFailed => TurnOutcome::Failed,
            UndeliveredReason::NoActiveTurn
            | UndeliveredReason::TurnEnded
            | UndeliveredReason::StepBudgetReached
            | UndeliveredReason::QueueFull
            | UndeliveredReason::SkillUnavailable
            | UndeliveredReason::PersistenceFailed
            | UndeliveredReason::Compacting => {
                unreachable!("only terminal abort reasons reach this transition")
            }
        };
        self.complete_turn(turn_id, outcome, reaction);
    }

    /// Ends a turn that ran its course, and opens the next one if a message waited for it.
    pub(super) fn finish_turn(
        &mut self,
        turn_id: plexmaton_core::TurnId,
        outcome: TurnOutcome,
        reaction: &mut Reaction,
    ) {
        self.turn = Turn::Idle;
        let released = self.input.claim(DeliveryBoundary::NextStep);
        self.return_queued(released, UndeliveredReason::TurnEnded, reaction);
        self.complete_turn(turn_id, outcome, reaction);
        let Some(next) = self.input.claim_one(DeliveryBoundary::NextTurn) else {
            return;
        };
        reaction.released_inputs.push(ReleasedInput::new(
            next.order,
            next.text.clone(),
            next.skill.as_ref().map(|skill| skill.name().to_owned()),
        ));
        self.open_turn(next.text, next.skill, next.accepted_at, reaction);
    }

    /// Claims steering immediately before the request for the next step is assembled (LOOP-6).
    pub(super) fn claim_next_step_input(
        &mut self,
        turn_id: &plexmaton_core::TurnId,
        reaction: &mut Reaction,
    ) {
        for input in self.input.claim(DeliveryBoundary::NextStep) {
            reaction.released_inputs.push(ReleasedInput::new(
                input.order,
                input.text.clone(),
                input.skill.as_ref().map(|skill| skill.name().to_owned()),
            ));
            self.record_steering(
                turn_id.clone(),
                input.text,
                input.skill,
                input.accepted_at,
                reaction,
            );
        }
    }

    fn active_turn_id(&self) -> Option<plexmaton_core::TurnId> {
        match &self.turn {
            Turn::Streaming { turn_id, .. } | Turn::Working { turn_id, .. } => {
                Some(turn_id.clone())
            }
            Turn::Idle => None,
        }
    }

    fn complete_turn(
        &mut self,
        turn_id: plexmaton_core::TurnId,
        outcome: TurnOutcome,
        reaction: &mut Reaction,
    ) {
        self.record.finish_turn(
            turn_id,
            outcome,
            TurnFinishedAt::Observed {
                completed_at: reaction.observed_at(),
            },
            reaction,
        );
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
            let selected = input.skill.as_ref().map(|skill| skill.name().to_owned());
            reaction.released_inputs.push(ReleasedInput::new(
                input.order,
                input.text.clone(),
                selected.clone(),
            ));
            reaction
                .undelivered
                .push(UndeliveredInput::with_skill(input.text, selected, reason));
        }
    }

    pub(super) fn status(
        &mut self,
        turn_id: plexmaton_core::TurnId,
        reaction: &mut Reaction,
        status: ActiveTurnStatus,
    ) {
        self.record.commit(
            JournalEntryPayload::TurnStatusChanged {
                agent_id: self.record.agent_id().clone(),
                turn_id,
                status,
            },
            reaction,
        );
        let event = ConversationEvent::AgentStatusChanged {
            agent_id: self.record.agent_id().clone(),
            status: status.agent_status(),
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
        let event = ConversationEvent::RuntimeWarning {
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
        let event = ConversationEvent::RuntimeError {
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
                current_offer: None,
            });
    }
}
