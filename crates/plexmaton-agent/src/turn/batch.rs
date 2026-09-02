//! The agent's dealings with one step's batch of tool calls.
//!
//! Dispatching, settling and abandoning are one responsibility: everything between a step asking
//! for tools and the model being shown what they produced. The debt rule lives here, which is why
//! abandoning sits beside dispatching rather than beside cancellation.

use plexmaton_core::{AgentStatus, SessionEvent, ToolCallId, ToolCallStatus};

use super::{Agent, Turn};
use crate::interface::{Effect, Reaction};
use crate::model::RequestItem;
use crate::tools::{Batch, ToolCall, ToolOutcome};

impl Agent {
    /// Records the calls, announces them, and asks for them to be run.
    pub(super) fn dispatch(&mut self, calls: Vec<ToolCall>, step: u16, reaction: &mut Reaction) {
        for call in &calls {
            self.record.push(RequestItem::ToolCall(call.clone()));
            let announced = SessionEvent::ToolCallChanged {
                agent_id: self.record.agent_id().clone(),
                call_id: call.call_id.clone(),
                label: call.name.clone(),
                status: ToolCallStatus::Running,
            };
            self.record.emit(reaction, announced);
            reaction.effects.push(Effect::RunTool(call.clone()));
        }
        self.status(reaction, AgentStatus::Waiting);
        self.turn = Turn::Working {
            batch: Batch::new(calls),
            step,
        };
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
                "a tool answered for a call this turn is not waiting on",
            );
            return;
        };
        let label = self.record.label_of(call_id);
        let changed = SessionEvent::ToolCallChanged {
            agent_id: self.record.agent_id().clone(),
            call_id: call_id.clone(),
            label,
            status,
        };
        self.record.emit(reaction, changed);
        if complete && let Some(step) = self.settle_batch() {
            self.next_step(step, reaction);
        }
    }

    /// Moves the batch's results into the record in model order, and says which step made them.
    ///
    /// Completion order is whatever the machine did; the model is answered in the order it asked,
    /// because reordering a batch teaches it that its own ordering means nothing.
    pub(super) fn settle_batch(&mut self) -> Option<u16> {
        let Turn::Working { batch, step } = std::mem::replace(&mut self.turn, Turn::Idle) else {
            return None;
        };
        for (call, outcome) in batch.into_results() {
            self.record.push(RequestItem::ToolResult {
                call_id: call.call_id,
                outcome,
            });
        }
        Some(step)
    }

    /// Takes the next step, or ends the turn because there is no budget for one.
    pub(super) fn next_step(&mut self, step: u16, reaction: &mut Reaction) {
        if step >= self.budget.max_steps {
            self.warn(
                reaction,
                "the turn reached its step budget with the model still asking for tools",
            );
            self.finish_turn(reaction);
            return;
        }
        self.open_step(step.saturating_add(1), reaction);
    }

    /// Answers every dispatched call that had not answered, so the conversation stays usable.
    ///
    /// This is the debt rule at run time: a call the model made and the loop dispatched leaves a
    /// result behind whatever happens to the turn, because the next request is built from this
    /// record, and a call with no result is a request no dialect will accept.
    pub(super) fn abandon(&mut self, reaction: &mut Reaction) {
        let abandoned = match &mut self.turn {
            Turn::Working { batch, .. } => batch.abandon(),
            Turn::Idle | Turn::Streaming(_) => return,
        };
        for call_id in abandoned {
            let label = self.record.label_of(&call_id);
            let cancelled = SessionEvent::ToolCallChanged {
                agent_id: self.record.agent_id().clone(),
                call_id,
                label,
                status: ToolCallStatus::Cancelled,
            };
            self.record.emit(reaction, cancelled);
        }
        self.settle_batch();
    }
}
