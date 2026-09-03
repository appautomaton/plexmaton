//! The agent's dealings with one step's batch of tool calls.
//!
//! Admission, approval, execution, settling and cancellation live together because each is one
//! state of the same per-call slot. No future or presentation queue owns the transition (LOOP-5),
//! and that slot advances the entry revision paired with the call (ENT-2).

use plexmaton_core::{
    AgentStatus, ApprovalDecision, ApprovalId, AttentionRequest, SessionEvent, ToolCallId,
    ToolCallStatus, ToolPresentation, TurnId,
};

use super::usage::UsageAccumulator;
use super::{Agent, Turn};
use crate::admission::{AdmissionOutcome, AdmissionRequest, PolicyDecision};
use crate::interface::{Effect, Reaction, UndeliveredReason};
use crate::model::RequestItem;
use crate::tools::{
    ApprovalResolution, Batch, PendingApproval, ToolCall, ToolCancellationReason, ToolOutcome,
};

impl Agent {
    /// Records raw calls and asks the trusted catalog to admit each one (APV-1).
    pub(super) fn dispatch(
        &mut self,
        turn_id: TurnId,
        calls: Vec<ToolCall>,
        step: u16,
        usage: UsageAccumulator,
        reaction: &mut Reaction,
    ) {
        let calls_with_entries: Vec<_> = calls
            .into_iter()
            .map(|call| (call, self.record.next_item_id()))
            .collect();
        let dispatched: Vec<_> = calls_with_entries
            .iter()
            .map(|(call, _)| call.clone())
            .collect();
        self.turn = Turn::Working {
            turn_id,
            batch: Batch::new(calls_with_entries),
            step,
            usage,
        };
        for call in &dispatched {
            self.record.push(RequestItem::ToolCall(call.clone()));
            self.emit_tool_status(call.call_id.clone(), ToolCallStatus::Queued, reaction);
            reaction
                .effects
                .push(Effect::AdmitTool(AdmissionRequest::new(call.clone())));
        }
        self.status(reaction, AgentStatus::Waiting);
    }

    /// Applies one catalog result to the exact call still awaiting admission.
    pub(super) fn admission_resolved(
        &mut self,
        outcome: AdmissionOutcome,
        reaction: &mut Reaction,
    ) {
        match outcome {
            AdmissionOutcome::Admitted(admitted) => {
                let call_id = admitted.requested().call_id.clone();
                let Some((turn_id, expected)) = self.requested_call(&call_id) else {
                    self.warn(
                        reaction,
                        "admission answered for a call this turn is not awaiting",
                    );
                    return;
                };
                if expected != *admitted.requested() {
                    self.warn(
                        reaction,
                        "admission changed the model call it claimed to answer",
                    );
                    return;
                }

                match self.policy.decide(&admitted) {
                    PolicyDecision::Allow => {
                        let accepted = match &mut self.turn {
                            Turn::Working { batch, .. } => batch.run(admitted.clone()),
                            Turn::Idle | Turn::Streaming { .. } => false,
                        };
                        if !accepted {
                            self.warn(reaction, "admitted call no longer awaits admission");
                            return;
                        }
                        self.emit_tool_status(call_id, ToolCallStatus::Running, reaction);
                        reaction.effects.push(Effect::RunTool(admitted));
                    }
                    PolicyDecision::RequireApproval => {
                        let (approval_id, attention_id) = self.record.next_approval_ids();
                        let pending = PendingApproval::new(
                            approval_id.clone(),
                            attention_id.clone(),
                            turn_id,
                            admitted.clone(),
                        );
                        let accepted = match &mut self.turn {
                            Turn::Working { batch, .. } => batch.await_approval(pending),
                            Turn::Idle | Turn::Streaming { .. } => false,
                        };
                        if !accepted {
                            self.warn(reaction, "admitted call no longer awaits approval");
                            return;
                        }
                        self.emit_tool_status(
                            call_id.clone(),
                            ToolCallStatus::AwaitingApproval,
                            reaction,
                        );
                        let request = AttentionRequest::Approval {
                            approval_id,
                            call_id,
                            tool: admitted.requested().name.clone(),
                            capabilities: admitted.capabilities().to_vec(),
                            detail: admitted.detail().to_owned(),
                        };
                        self.record.emit(
                            reaction,
                            SessionEvent::AttentionRequested {
                                agent_id: self.record.agent_id().clone(),
                                attention_id,
                                request,
                            },
                        );
                    }
                    PolicyDecision::Forbidden => {
                        let accepted = match &mut self.turn {
                            Turn::Working { batch, .. } => {
                                batch.finish_before_run(&call_id, ToolOutcome::Forbidden)
                            }
                            Turn::Idle | Turn::Streaming { .. } => false,
                        };
                        if !accepted {
                            self.warn(reaction, "forbidden call no longer awaits policy");
                            return;
                        }
                        self.emit_tool_status(call_id, ToolCallStatus::Failed, reaction);
                        self.continue_if_batch_complete(reaction);
                    }
                }
            }
            AdmissionOutcome::Refused { call_id, reason } => {
                let accepted = match &mut self.turn {
                    Turn::Working { batch, .. } => {
                        batch.finish_before_run(&call_id, ToolOutcome::AdmissionRefused { reason })
                    }
                    Turn::Idle | Turn::Streaming { .. } => false,
                };
                if !accepted {
                    self.warn(
                        reaction,
                        "admission refused a call this turn is not awaiting",
                    );
                    return;
                }
                self.emit_tool_status(call_id, ToolCallStatus::Failed, reaction);
                self.continue_if_batch_complete(reaction);
            }
        }
    }

    /// Resolves one pending request exactly once (APV-4).
    pub(super) fn approval_decided(
        &mut self,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
        reaction: &mut Reaction,
    ) {
        let resolution = match &mut self.turn {
            Turn::Working { batch, .. } => batch.resolve_approval(&approval_id, decision),
            Turn::Idle | Turn::Streaming { .. } => None,
        };
        let Some(resolution) = resolution else {
            Self::refuse_approval_decision(reaction, approval_id, decision);
            return;
        };

        match resolution {
            ApprovalResolution::Run {
                attention_id,
                admitted,
            } => {
                self.resolve_attention(attention_id, reaction);
                self.emit_tool_status(
                    admitted.requested().call_id.clone(),
                    ToolCallStatus::Running,
                    reaction,
                );
                reaction.effects.push(Effect::RunTool(admitted));
            }
            ApprovalResolution::Denied {
                attention_id,
                call_id,
            } => {
                self.resolve_attention(attention_id, reaction);
                self.emit_tool_status(call_id, ToolCallStatus::Denied, reaction);
                self.continue_if_batch_complete(reaction);
            }
        }
    }

    pub(super) fn tool_finished(
        &mut self,
        call_id: &ToolCallId,
        outcome: ToolOutcome,
        reaction: &mut Reaction,
    ) {
        let status = outcome.status();
        let settled = match &mut self.turn {
            Turn::Working { batch, .. } => {
                if batch.settle(call_id, outcome) {
                    Some(batch.is_settled())
                } else {
                    None
                }
            }
            Turn::Idle | Turn::Streaming { .. } => None,
        };
        let Some(complete) = settled else {
            self.warn(
                reaction,
                "a tool answered for a call this turn is not running",
            );
            return;
        };
        self.emit_tool_status(call_id.clone(), status, reaction);
        if complete {
            self.continue_if_batch_complete(reaction);
        }
    }

    fn requested_call(&self, call_id: &ToolCallId) -> Option<(TurnId, ToolCall)> {
        let Turn::Working { turn_id, batch, .. } = &self.turn else {
            return None;
        };
        Some((turn_id.clone(), batch.requested(call_id)?.clone()))
    }

    fn emit_tool_status(
        &mut self,
        call_id: ToolCallId,
        status: ToolCallStatus,
        reaction: &mut Reaction,
    ) {
        let entry = match &self.turn {
            Turn::Working { batch, .. } => batch
                .entry(&call_id)
                .map(|(item_id, revision)| (item_id.clone(), revision)),
            Turn::Idle | Turn::Streaming { .. } => None,
        };
        let Some((item_id, item_revision)) = entry else {
            self.warn(reaction, "tool state changed without its transcript entry");
            return;
        };
        let changed = SessionEvent::ToolCallChanged {
            agent_id: self.record.agent_id().clone(),
            item_id,
            item_revision,
            label: self.record.label_of(&call_id),
            call_id,
            status,
            presentation: ToolPresentation::default(),
        };
        self.record.emit(reaction, changed);
    }

    fn resolve_attention(
        &mut self,
        attention_id: plexmaton_core::AttentionId,
        reaction: &mut Reaction,
    ) {
        self.record.emit(
            reaction,
            SessionEvent::AttentionResolved {
                agent_id: self.record.agent_id().clone(),
                attention_id,
            },
        );
    }

    fn continue_if_batch_complete(&mut self, reaction: &mut Reaction) {
        let complete = matches!(
            &self.turn,
            Turn::Working { batch, .. } if batch.is_settled()
        );
        if complete && let Some((turn_id, step, usage)) = self.settle_batch() {
            self.next_step(turn_id, step, usage, reaction);
        }
    }

    /// Moves the batch's results into the record in model order.
    pub(super) fn settle_batch(&mut self) -> Option<(TurnId, u16, UsageAccumulator)> {
        let Turn::Working {
            turn_id,
            batch,
            step,
            usage,
        } = std::mem::replace(&mut self.turn, Turn::Idle)
        else {
            return None;
        };
        for (call, outcome) in batch.into_results() {
            self.record.push(RequestItem::ToolResult {
                call_id: call.call_id,
                outcome,
            });
        }
        Some((turn_id, step, usage))
    }

    /// Takes the next step, or ends the turn because there is no budget for one.
    pub(super) fn next_step(
        &mut self,
        turn_id: TurnId,
        step: u16,
        usage: UsageAccumulator,
        reaction: &mut Reaction,
    ) {
        if step >= self.budget.max_steps {
            self.warn(
                reaction,
                "the turn reached its step budget with the model still asking for tools",
            );
            reaction.undelivered.extend(self.input.reject(
                super::input::DeliveryBoundary::NextStep,
                UndeliveredReason::StepBudgetReached,
            ));
            self.finish_turn(reaction);
            return;
        }
        self.claim_next_step_input(reaction);
        self.open_step(turn_id, step.saturating_add(1), usage, reaction);
    }

    /// Pays every unfinished slot, resolving Attention projections on the way (APV-6).
    pub(super) fn abandon(&mut self, reason: ToolCancellationReason, reaction: &mut Reaction) {
        let abandoned = match &mut self.turn {
            Turn::Working { batch, .. } => batch.abandon(reason),
            Turn::Idle | Turn::Streaming { .. } => return,
        };
        for call in abandoned {
            if let Some(attention_id) = call.attention_id {
                self.resolve_attention(attention_id, reaction);
            }
            self.emit_tool_status(call.call_id, ToolCallStatus::Cancelled, reaction);
        }
        self.settle_batch();
    }
}
