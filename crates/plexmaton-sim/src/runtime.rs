//! The deterministic stand-in for the real runtime.
//!
//! Two sources feed one monotonic event stream: a scripted timeline, and the user. Both are
//! numbered here, because a sequence assigned in two places is not a sequence.

use std::collections::VecDeque;

use plexmaton_core::{
    AgentId, EventSequence, IdError, PrototypeEvent, PrototypeEventEnvelope, TranscriptItemId,
    TranscriptRole,
};

use crate::{Scenario, ScenarioStep};

/// One thing the user asked the runtime to do.
///
/// A submitted message is a command, never a write. The projection shows it only once the runtime
/// emits it back, which is the boundary a real runtime will occupy — and the reason the transcript
/// has exactly one writer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeCommand {
    /// Deliver a user message to an agent's session.
    SendMessage {
        /// Agent whose transcript gains the message.
        to: AgentId,
        /// Exact text the user submitted.
        text: String,
    },
}

/// Deterministic runtime: a scripted timeline plus whatever the user asks for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Runtime {
    scheduled: VecDeque<ScenarioStep>,
    next_sequence: u64,
    submitted: u64,
}

impl Runtime {
    /// Starts a runtime that will replay `scenario` and accept user commands alongside it.
    #[must_use]
    pub fn new(scenario: Scenario) -> Self {
        Self {
            scheduled: scenario.into_steps().into(),
            // Producers conventionally begin at one, and the projection expects that.
            next_sequence: 1,
            submitted: 0,
        }
    }

    /// Emits every scheduled event whose logical tick has arrived.
    pub fn ready(&mut self, tick: u64) -> Vec<PrototypeEventEnvelope> {
        let mut emitted = Vec::new();
        while self
            .scheduled
            .front()
            .is_some_and(|step| step.at_tick <= tick)
        {
            let Some(step) = self.scheduled.pop_front() else {
                break;
            };
            emitted.push(self.envelope(step.event));
        }
        emitted
    }

    /// Emits the events one user command produces.
    ///
    /// A message becomes a complete transcript item rather than an open one: the user's text is not
    /// streamed, so leaving the item unfinalized would model a producer that never finishes.
    pub fn submit(
        &mut self,
        command: RuntimeCommand,
    ) -> Result<Vec<PrototypeEventEnvelope>, IdError> {
        match command {
            RuntimeCommand::SendMessage { to, text } => {
                self.submitted = self.submitted.saturating_add(1);
                let item_id = TranscriptItemId::new(format!("user-{}", self.submitted))?;
                Ok(vec![
                    self.envelope(PrototypeEvent::TranscriptItemStarted {
                        agent_id: to.clone(),
                        item_id: item_id.clone(),
                        role: TranscriptRole::User,
                    }),
                    self.envelope(PrototypeEvent::TranscriptDelta {
                        agent_id: to.clone(),
                        item_id: item_id.clone(),
                        item_revision: 1,
                        text,
                    }),
                    self.envelope(PrototypeEvent::TranscriptItemFinalized {
                        agent_id: to,
                        item_id,
                        item_revision: 2,
                    }),
                ])
            }
        }
    }

    /// Whether the scripted timeline still has events to emit.
    #[must_use]
    pub fn is_replaying(&self) -> bool {
        !self.scheduled.is_empty()
    }

    fn envelope(&mut self, event: PrototypeEvent) -> PrototypeEventEnvelope {
        let sequence = EventSequence::new(self.next_sequence);
        self.next_sequence = self.next_sequence.saturating_add(1);
        PrototypeEventEnvelope { sequence, event }
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{AgentId, PrototypeEvent, TranscriptRole};

    use super::{Runtime, RuntimeCommand};
    use crate::Scenario;

    fn runtime() -> Runtime {
        Runtime::new(Scenario::canonical().unwrap_or_else(|error| panic!("fixture: {error}")))
    }

    fn agent(value: &str) -> AgentId {
        AgentId::new(value).unwrap_or_else(|error| panic!("fixture: {error}"))
    }

    fn send(runtime: &mut Runtime, text: &str) -> Vec<PrototypeEvent> {
        runtime
            .submit(RuntimeCommand::SendMessage {
                to: agent("agent-a"),
                text: text.to_owned(),
            })
            .unwrap_or_else(|error| panic!("valid command: {error}"))
            .into_iter()
            .map(|envelope| envelope.event)
            .collect()
    }

    /// The projection rejects any gap or repeat, so interleaving the two sources must not create
    /// one. This is the whole reason numbering moved out of the scenario data.
    #[test]
    fn one_stream_is_numbered_across_both_sources() {
        let mut runtime = runtime();
        let mut sequences = Vec::new();

        sequences.extend(runtime.ready(2).into_iter().map(|e| e.sequence.get()));
        let mut submitted = runtime
            .submit(RuntimeCommand::SendMessage {
                to: agent("agent-a"),
                text: "mid-stream".to_owned(),
            })
            .unwrap_or_else(|error| panic!("valid command: {error}"));
        sequences.extend(submitted.drain(..).map(|e| e.sequence.get()));
        sequences.extend(runtime.ready(99).into_iter().map(|e| e.sequence.get()));

        let expected: Vec<u64> = (1..=sequences.len() as u64).collect();
        assert_eq!(sequences, expected, "the stream must have no gap or repeat");
        assert!(!runtime.is_replaying(), "tick 99 drains the timeline");
    }

    #[test]
    fn a_submitted_message_is_a_finished_user_item() {
        let mut runtime = runtime();

        let events = send(&mut runtime, "hello");

        assert!(matches!(
            events.as_slice(),
            [
                PrototypeEvent::TranscriptItemStarted { role: TranscriptRole::User, .. },
                PrototypeEvent::TranscriptDelta { text, item_revision: 1, .. },
                PrototypeEvent::TranscriptItemFinalized { item_revision: 2, .. },
            ] if text == "hello"
        ));
    }

    #[test]
    fn each_submission_gets_its_own_item_identity() {
        let mut runtime = runtime();

        let first = send(&mut runtime, "one");
        let second = send(&mut runtime, "two");

        let id = |events: &[PrototypeEvent]| match &events[0] {
            PrototypeEvent::TranscriptItemStarted { item_id, .. } => item_id.to_string(),
            other => panic!("expected a started item, got {other:?}"),
        };
        assert_ne!(
            id(&first),
            id(&second),
            "a repeated identity would make the projection reject the second message"
        );
    }

    #[test]
    fn nothing_is_emitted_before_its_tick() {
        let mut runtime = runtime();

        assert!(runtime.ready(0).len() == 1, "only the tick-zero event");
        assert!(runtime.is_replaying());
    }
}
