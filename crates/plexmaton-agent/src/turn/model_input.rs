//! Correlated model input, streamed step assembly, and provider-reported usage.

use plexmaton_core::{SessionEvent, TokenUsage, TurnId};

use super::{Agent, Turn, usage::UsageAccumulator};
use crate::TurnOutcome;
use crate::interface::{ModelDeliveryRefusal, Reaction, UndeliveredModelInput};
use crate::journal::JournalEntryPayload;
use crate::model::{ModelEvent, ModelStepId, StopReason};
use crate::tools::ToolCall;

impl Agent {
    pub(super) fn accepts_model_input(
        &self,
        step_id: ModelStepId,
        reaction: &mut Reaction,
    ) -> bool {
        let expected = match &self.turn {
            Turn::Streaming { turn_id, step, .. } => {
                Some(ModelStepId::new(turn_id.clone(), step.index()))
            }
            Turn::Idle | Turn::Working { .. } => None,
        };
        if expected.as_ref() == Some(&step_id) {
            return true;
        }
        let reason = expected.map_or(ModelDeliveryRefusal::NoActiveStep, |expected| {
            ModelDeliveryRefusal::WrongStep { expected }
        });
        reaction
            .undelivered_model
            .push(UndeliveredModelInput { step_id, reason });
        false
    }

    pub(super) fn stream(&mut self, event: ModelEvent, reaction: &mut Reaction) {
        match event {
            // Empty deltas are wire artefacts. Opening a message on one would paint a blank row
            // that the record then declines to keep.
            ModelEvent::TextDelta { delta, .. } if delta.is_empty() => {}
            ModelEvent::TextDelta { position, delta } => {
                let result = match &mut self.turn {
                    Turn::Streaming { step, .. } => {
                        step.append(&mut self.record, reaction, position, delta)
                    }
                    Turn::Idle | Turn::Working { .. } => return,
                };
                if let Err(error) = result {
                    self.fail(
                        &crate::ModelError::Malformed {
                            message: error.message().to_owned(),
                        },
                        reaction,
                    );
                }
            }
            ModelEvent::ReasoningDelta { delta, .. } if delta.is_empty() => {}
            ModelEvent::ReasoningDelta { position, delta } => {
                let result = match &mut self.turn {
                    Turn::Streaming { step, .. } => {
                        step.append_reasoning(&mut self.record, reaction, position, delta)
                    }
                    Turn::Idle | Turn::Working { .. } => return,
                };
                if let Err(error) = result {
                    self.fail(
                        &crate::ModelError::Malformed {
                            message: error.message().to_owned(),
                        },
                        reaction,
                    );
                }
            }
            ModelEvent::Replay { position, replay } => {
                let result = match &mut self.turn {
                    Turn::Streaming { step, .. } => step.retain_replay(position, replay),
                    Turn::Idle | Turn::Working { .. } => return,
                };
                if let Err(error) = result {
                    self.fail(
                        &crate::ModelError::Malformed {
                            message: error.message().to_owned(),
                        },
                        reaction,
                    );
                }
            }
            ModelEvent::Called { position, call } => {
                let reused = self.record.contains_tool_call(&call.call_id)
                    || matches!(&self.turn, Turn::Streaming { step, .. } if step.contains_call(&call.call_id));
                if reused {
                    self.fail(
                        &crate::ModelError::Malformed {
                            message: "a tool call identity was reused".to_owned(),
                        },
                        reaction,
                    );
                } else if let Turn::Streaming { step, .. } = &mut self.turn
                    && let Err(error) = step.collect(&self.record, position, call)
                {
                    self.fail(
                        &crate::ModelError::Malformed {
                            message: error.message().to_owned(),
                        },
                        reaction,
                    );
                }
            }
            ModelEvent::Usage(usage) => self.report_usage(usage, reaction),
            ModelEvent::Stopped(reason) => self.stop(reason, reaction),
        }
    }

    fn report_usage(&mut self, report: TokenUsage, reaction: &mut Reaction) {
        let result = match &mut self.turn {
            Turn::Streaming {
                turn_id,
                step,
                usage,
            } => {
                if !step.mark_usage_reported() {
                    step.defer_warning("the provider reported usage more than once for one step");
                    return;
                }
                usage
                    .add(report)
                    .map(|aggregate| (turn_id.clone(), aggregate))
            }
            Turn::Idle | Turn::Working { .. } => return,
        };
        let Ok((turn_id, usage)) = result else {
            if let Turn::Streaming { step, .. } = &mut self.turn {
                step.defer_warning("the provider's turn usage overflowed its counter");
            }
            return;
        };
        self.record.commit(
            JournalEntryPayload::TurnUsageUpdated {
                agent_id: self.record.agent_id().clone(),
                turn_id: turn_id.clone(),
                usage: usage.clone(),
            },
            reaction,
        );
        self.record.emit(
            reaction,
            SessionEvent::TurnUsageUpdated {
                agent_id: self.record.agent_id().clone(),
                turn_id,
                usage,
            },
        );
    }

    fn stop(&mut self, reason: StopReason, reaction: &mut Reaction) {
        let diagnostic = match reason {
            StopReason::EndOfTurn | StopReason::ToolCalls => None,
            StopReason::OutputLimit => Some("the model reached its output limit mid-answer"),
            StopReason::Refused => Some("the model declined to answer"),
            StopReason::Unspecified => Some("the model stopped without saying why"),
        };
        let Some((turn_id, calls, index, usage)) = self.close_step(reaction) else {
            return;
        };
        if let Some(message) = diagnostic {
            self.warn(reaction, message);
        }
        if calls.is_empty() {
            if matches!(reason, StopReason::ToolCalls) {
                self.warn(
                    reaction,
                    "the model stopped for tools without asking for any",
                );
            }
            self.finish_turn(turn_id, TurnOutcome::Completed, reaction);
            return;
        }
        self.dispatch(turn_id, calls, index, usage, reaction);
    }

    /// Ends the streaming half of the step, and says what it asked for.
    pub(super) fn close_step(
        &mut self,
        reaction: &mut Reaction,
    ) -> Option<(TurnId, Vec<ToolCall>, u16, UsageAccumulator)> {
        let Turn::Streaming {
            turn_id,
            step,
            usage,
        } = std::mem::replace(&mut self.turn, Turn::Idle)
        else {
            return None;
        };
        let index = step.index();
        let (calls, warnings) = step.close(&mut self.record, reaction);
        for warning in warnings {
            self.warn(reaction, &warning);
        }
        Some((turn_id, calls, index, usage))
    }

    pub(super) fn abort_step(&mut self, reaction: &mut Reaction) {
        let Turn::Streaming { step, .. } = std::mem::replace(&mut self.turn, Turn::Idle) else {
            return;
        };
        for warning in step.abort(&mut self.record, reaction) {
            self.warn(reaction, &warning);
        }
    }
}
