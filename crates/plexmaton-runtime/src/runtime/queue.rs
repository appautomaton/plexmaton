//! Reading out the user input that has been submitted but not sent yet (IQU-1).
//!
//! Reading only, apart from `withdraw_queued`. The agent owns its queues (LOOP-6) and this runtime
//! holds the inputs it has not handed over yet (CPL-9). Copying either into a second collection
//! here would mean keeping that copy in step every time one of them is sent.

use plexmaton_agent::{Input, UndeliveredInput, UndeliveredReason};
use plexmaton_core::AgentId;

use super::LiveRuntime;
use crate::{DispatchReport, RuntimeError};

/// When one waiting input will be sent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueuedBoundary {
    /// With the current turn's next step, once the running tool calls have returned (LOOP-6).
    Step,
    /// As a new turn, once this one ends (LOOP-6).
    Turn,
    /// Not yet given to the agent, because an operation this runtime owns is running (CPL-9).
    ///
    /// The one message a skill file read is being performed *for* is not here: it is held apart
    /// until the read settles, and only the messages queued behind it wait in this queue.
    Admission,
}

/// One message the user submitted that has not been sent to the model yet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueuedInput<'a> {
    /// The text the user typed, unchanged.
    pub text: &'a str,
    /// When it will be sent.
    pub boundary: QueuedBoundary,
}

impl LiveRuntime {
    /// Everything waiting to be sent, soonest first.
    ///
    /// Read on demand rather than published as an event: a waiting message is not in the session
    /// journal, so it has no entry and no sequence number, and once it is sent the ordinary turn
    /// events already report it.
    pub fn queued_input(&self) -> impl Iterator<Item = QueuedInput<'_>> {
        let step = self.agent.queued_for_next_step().map(|text| QueuedInput {
            text,
            boundary: QueuedBoundary::Step,
        });
        let turn = self.agent.queued_for_next_turn().map(|text| QueuedInput {
            text,
            boundary: QueuedBoundary::Turn,
        });
        // Last, because the agent has not been given these yet: anything it already holds was
        // submitted earlier and will be sent first.
        let admission = self
            .pending_inputs
            .iter()
            .filter_map(|pending| submitted_text(&pending.input))
            .map(|text| QueuedInput {
                text,
                boundary: QueuedBoundary::Admission,
            });
        step.chain(turn).chain(admission)
    }
}

impl LiveRuntime {
    /// Takes back the message the user submitted most recently, text and skill unchanged (IQU-4).
    ///
    /// The only thing undone is its place in the queue. It was never put in a model request, so
    /// there is no journal entry to amend and no running work to cancel. It needs no event either:
    /// the band reads the queue again every frame, so the message is simply not in the next one.
    pub fn withdraw_queued(&mut self, to: &AgentId) -> Result<DispatchReport, RuntimeError> {
        if to != &self.agent_id {
            return Err(RuntimeError::WrongAgent {
                expected: self.agent_id.clone(),
                received: to.clone(),
            });
        }
        if self.shutdown_state != super::ShutdownState::Open {
            return Err(RuntimeError::ShuttingDown);
        }
        // Anything this runtime is still holding arrived after everything the agent has, so take
        // from the back of that first.
        let withdrawn = self.withdraw_held().or_else(|| {
            let mut reaction = self.agent.withdraw_last_queued();
            reaction.undelivered.pop()
        });
        if let Some(input) = withdrawn {
            self.report.undelivered.push(input);
        }
        Ok(self.take_report())
    }

    /// Removes the newest message this runtime is still holding, if it is one the user typed.
    fn withdraw_held(&mut self) -> Option<UndeliveredInput> {
        let index = self
            .pending_inputs
            .iter()
            .rposition(|pending| submitted_text(&pending.input).is_some())?;
        let pending = self.pending_inputs.remove(index)?;
        let text = submitted_text(&pending.input)?.to_owned();
        let skill = match &pending.input {
            Input::SkillSubmitted { skill, .. } | Input::SkillSteered { skill, .. } => {
                Some(skill.name().to_owned())
            }
            _ => pending.selected_skill.clone(),
        };
        Some(UndeliveredInput::with_skill(
            text,
            skill,
            UndeliveredReason::Withdrawn,
        ))
    }
}

/// The text of a message the user typed. Control inputs such as an interrupt carry none.
fn submitted_text(input: &Input) -> Option<&str> {
    match input {
        Input::Submitted { text }
        | Input::Steered { text }
        | Input::SkillSubmitted { text, .. }
        | Input::SkillSteered { text, .. } => Some(text.as_str()),
        _ => None,
    }
}
