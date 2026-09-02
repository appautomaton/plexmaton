//! The turn machine: what the loop decides, expressed as a value.
//!
//! Nothing here awaits, spawns, or reads a clock. One method takes a typed input and returns the
//! events the projection should see and the effects someone else must perform, so the whole of a
//! turn is inspectable between any two of them: what it is doing, and what it still owes.

use plexmaton_core::{
    AgentId, AgentStatus, EventSequence, SessionEvent, SessionEventEnvelope, TranscriptItemId,
    TranscriptRole,
};

use crate::model::{ModelError, ModelEvent, ModelRequest, RequestItem, StopReason};

/// Something the loop is told.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Input {
    /// The user submitted a message to this agent.
    Submitted {
        /// Exact text the user submitted.
        text: String,
    },
    /// The model produced something.
    Streamed(ModelEvent),
    /// The step failed before it could finish.
    Failed(ModelError),
    /// The user asked the current turn to stop.
    Interrupted,
}

/// Something the loop needs performed, and cannot perform itself.
///
/// An effect is a value. This crate has no way to carry one out, which is why the arrow from the
/// loop to the network cannot be drawn by accident.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Effect {
    /// Ask the model, and feed what it says back in as [`Input::Streamed`].
    CallModel(ModelRequest),
}

/// What one input produced.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Reaction {
    /// Events for the projection, numbered on this agent's one sequence.
    pub events: Vec<SessionEventEnvelope>,
    /// Work for whoever owns the outside world.
    pub effects: Vec<Effect>,
}

/// Whether a turn is running, and what it has assembled so far.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Turn {
    /// No turn is open; a submission starts one.
    Idle,
    /// A step is streaming into one assistant message.
    Streaming {
        /// The transcript item, once a delta has opened it.
        item: Option<TranscriptItemId>,
        /// Revisions issued for that item so far.
        revision: u64,
        /// Text assembled from the deltas, which is what the record keeps.
        text: String,
    },
}

/// One agent's session and the turn it is running.
///
/// The record is authoritative: what the model is shown next is assembled from `items`, and what
/// the screen shows is a projection of the events this type emitted. There is no second copy to
/// reconcile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Agent {
    agent_id: AgentId,
    items: Vec<RequestItem>,
    turn: Turn,
    next_sequence: u64,
    next_item: u64,
    queued: Vec<String>,
}

impl Agent {
    /// Starts an idle agent whose first event will be numbered one.
    #[must_use]
    pub fn new(agent_id: AgentId) -> Self {
        Self {
            agent_id,
            items: Vec::new(),
            turn: Turn::Idle,
            next_sequence: 1,
            next_item: 0,
            queued: Vec::new(),
        }
    }

    /// Announces the agent to the workspace, on the agent's own sequence.
    ///
    /// Numbered here rather than by whoever constructed the agent, because a stream numbered in
    /// two places is not a stream: the projection refuses a gap, and a creation event carrying a
    /// supervisor's number would be the first one.
    pub fn announce(&mut self, label: impl Into<String>) -> Reaction {
        let mut reaction = Reaction::default();
        self.emit(
            &mut reaction,
            SessionEvent::AgentCreated {
                agent_id: self.agent_id.clone(),
                label: label.into(),
                status: AgentStatus::Idle,
            },
        );
        reaction
    }

    /// Whether a turn is open.
    #[must_use]
    pub fn is_running(&self) -> bool {
        matches!(self.turn, Turn::Streaming { .. })
    }

    /// The conversation as the model would be shown it right now.
    #[must_use]
    pub fn record(&self) -> &[RequestItem] {
        &self.items
    }

    /// Messages submitted while a turn was running, waiting for the next turn boundary.
    #[must_use]
    pub fn queued(&self) -> &[String] {
        &self.queued
    }

    /// Advances the machine by one input.
    pub fn handle(&mut self, input: Input) -> Reaction {
        let mut reaction = Reaction::default();
        match input {
            Input::Submitted { text } => self.submit(text, &mut reaction),
            Input::Streamed(event) => self.stream(event, &mut reaction),
            Input::Failed(error) => self.fail(&error, &mut reaction),
            Input::Interrupted => self.interrupt(&mut reaction),
        }
        reaction
    }

    /// A submission during a turn joins the next one rather than this one.
    ///
    /// The model is mid-answer and the request for this step has already been sent, so there is
    /// nowhere for the text to go except the next boundary. Holding it is not the whole of input
    /// routing — steering a running turn is its own queue — but losing it is a defect either way.
    fn submit(&mut self, text: String, reaction: &mut Reaction) {
        if self.is_running() {
            self.queued.push(text);
            return;
        }
        self.open_turn(text, reaction);
    }

    fn open_turn(&mut self, text: String, reaction: &mut Reaction) {
        let item = self.next_item_id();
        self.emit(
            reaction,
            SessionEvent::TranscriptItemStarted {
                agent_id: self.agent_id.clone(),
                item_id: item.clone(),
                role: TranscriptRole::User,
            },
        );
        self.emit(
            reaction,
            SessionEvent::TranscriptDelta {
                agent_id: self.agent_id.clone(),
                item_id: item.clone(),
                item_revision: 1,
                text: text.clone(),
            },
        );
        self.emit(
            reaction,
            SessionEvent::TranscriptItemFinalized {
                agent_id: self.agent_id.clone(),
                item_id: item,
                item_revision: 2,
            },
        );
        self.items.push(RequestItem::User { text });
        self.status(reaction, AgentStatus::Running);
        self.turn = Turn::Streaming {
            item: None,
            revision: 0,
            text: String::new(),
        };
        reaction.effects.push(Effect::CallModel(ModelRequest {
            items: self.items.clone(),
        }));
    }

    fn stream(&mut self, event: ModelEvent, reaction: &mut Reaction) {
        if !self.is_running() {
            self.warn(reaction, "the model produced output with no turn open");
            return;
        }
        match event {
            ModelEvent::TextDelta(delta) => self.append(delta, reaction),
            ModelEvent::Stopped(reason) => self.stop(reason, reaction),
        }
    }

    fn append(&mut self, delta: String, reaction: &mut Reaction) {
        let Turn::Streaming {
            item,
            revision,
            text,
        } = &mut self.turn
        else {
            return;
        };
        let opened = match item {
            Some(open) => open.clone(),
            none => {
                let opened = next_item_id(&self.agent_id, &mut self.next_item);
                *none = Some(opened.clone());
                reaction.events.push(envelope(
                    &mut self.next_sequence,
                    SessionEvent::TranscriptItemStarted {
                        agent_id: self.agent_id.clone(),
                        item_id: opened.clone(),
                        role: TranscriptRole::Assistant,
                    },
                ));
                opened
            }
        };
        text.push_str(&delta);
        *revision = revision.saturating_add(1);
        let item_revision = *revision;
        reaction.events.push(envelope(
            &mut self.next_sequence,
            SessionEvent::TranscriptDelta {
                agent_id: self.agent_id.clone(),
                item_id: opened,
                item_revision,
                text: delta,
            },
        ));
    }

    /// Ends the step, and with it the turn: with no tools yet, one turn is one step.
    fn stop(&mut self, reason: StopReason, reaction: &mut Reaction) {
        match reason {
            StopReason::EndOfTurn => {}
            StopReason::ToolCalls => {
                self.warn(
                    reaction,
                    "the model asked for a tool, and none are offered yet",
                );
            }
            StopReason::OutputLimit => {
                self.warn(reaction, "the model reached its output limit mid-answer");
            }
            StopReason::Refused => self.warn(reaction, "the model declined to answer"),
            StopReason::Unspecified => {
                self.warn(reaction, "the model stopped without saying why");
            }
        }
        self.close_turn(reaction);
    }

    fn fail(&mut self, error: &ModelError, reaction: &mut Reaction) {
        if !self.is_running() {
            self.warn(reaction, &error.message());
            return;
        }
        self.warn(reaction, &error.message());
        self.close_turn(reaction);
    }

    fn interrupt(&mut self, reaction: &mut Reaction) {
        if !self.is_running() {
            return;
        }
        self.close_turn(reaction);
    }

    /// Closes whatever is open, records what was assembled, and opens the next turn if one waits.
    ///
    /// A step that produced no text finalizes nothing and records nothing: an empty assistant
    /// message is not something the reader should see, and it is not something to send back.
    fn close_turn(&mut self, reaction: &mut Reaction) {
        let turn = std::mem::replace(&mut self.turn, Turn::Idle);
        if let Turn::Streaming {
            item: Some(item),
            revision,
            text,
        } = turn
        {
            self.emit(
                reaction,
                SessionEvent::TranscriptItemFinalized {
                    agent_id: self.agent_id.clone(),
                    item_id: item,
                    item_revision: revision.saturating_add(1),
                },
            );
            if !text.is_empty() {
                self.items.push(RequestItem::Assistant { text });
            }
        }
        if self.queued.is_empty() {
            self.status(reaction, AgentStatus::Idle);
            return;
        }
        let next = self.queued.remove(0);
        self.open_turn(next, reaction);
    }

    fn status(&mut self, reaction: &mut Reaction, status: AgentStatus) {
        self.emit(
            reaction,
            SessionEvent::AgentStatusChanged {
                agent_id: self.agent_id.clone(),
                status,
            },
        );
    }

    fn warn(&mut self, reaction: &mut Reaction, message: &str) {
        self.emit(
            reaction,
            SessionEvent::RuntimeWarning {
                message: message.to_owned(),
            },
        );
    }

    fn emit(&mut self, reaction: &mut Reaction, event: SessionEvent) {
        reaction
            .events
            .push(envelope(&mut self.next_sequence, event));
    }

    fn next_item_id(&mut self) -> TranscriptItemId {
        next_item_id(&self.agent_id, &mut self.next_item)
    }
}

fn envelope(next_sequence: &mut u64, event: SessionEvent) -> SessionEventEnvelope {
    let sequence = EventSequence::new(*next_sequence);
    *next_sequence = next_sequence.saturating_add(1);
    SessionEventEnvelope { sequence, event }
}

fn next_item_id(agent_id: &AgentId, next_item: &mut u64) -> TranscriptItemId {
    *next_item = next_item.saturating_add(1);
    TranscriptItemId::new(format!("{agent_id}-{next_item}"))
        .unwrap_or_else(|error| unreachable!("a formatted identity is never empty: {error}"))
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{AgentId, AgentStatus, SessionEvent, TranscriptRole};

    use super::{Agent, Effect, Input, Reaction};
    use crate::model::{ModelError, ModelEvent, RequestItem, StopReason};

    fn agent() -> Agent {
        Agent::new(AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}")))
    }

    fn submit(agent: &mut Agent, text: &str) -> Reaction {
        agent.handle(Input::Submitted {
            text: text.to_owned(),
        })
    }

    fn delta(agent: &mut Agent, text: &str) -> Reaction {
        agent.handle(Input::Streamed(ModelEvent::TextDelta(text.to_owned())))
    }

    fn stop(agent: &mut Agent, reason: StopReason) -> Reaction {
        agent.handle(Input::Streamed(ModelEvent::Stopped(reason)))
    }

    fn events(reaction: &Reaction) -> Vec<SessionEvent> {
        reaction
            .events
            .iter()
            .map(|envelope| envelope.event.clone())
            .collect()
    }

    fn warnings(reaction: &Reaction) -> Vec<String> {
        events(reaction)
            .into_iter()
            .filter_map(|event| match event {
                SessionEvent::RuntimeWarning { message } => Some(message),
                _ => None,
            })
            .collect()
    }

    /// The record is what the model is shown, and the events are what the screen is shown. One
    /// submission produces both, and the request carries the conversation rather than the message.
    #[test]
    fn a_submission_becomes_a_finished_user_item_and_a_request_for_the_whole_conversation() {
        let mut agent = agent();

        let reaction = submit(&mut agent, "hello");

        assert!(matches!(
            events(&reaction).as_slice(),
            [
                SessionEvent::TranscriptItemStarted { role: TranscriptRole::User, .. },
                SessionEvent::TranscriptDelta { item_revision: 1, text, .. },
                SessionEvent::TranscriptItemFinalized { item_revision: 2, .. },
                SessionEvent::AgentStatusChanged { status: AgentStatus::Running, .. },
            ] if text == "hello"
        ));
        let [Effect::CallModel(request)] = reaction.effects.as_slice() else {
            panic!("one submission asks the model once: {:?}", reaction.effects);
        };
        assert_eq!(
            request.items,
            [RequestItem::User {
                text: "hello".into()
            }]
        );
        assert!(agent.is_running());
    }

    /// One assistant message per step, opened by the first delta and numbered from there, so a
    /// projection can detect a lost delta without comparing text.
    #[test]
    fn a_streamed_answer_opens_one_item_and_numbers_every_revision() {
        let mut agent = agent();
        submit(&mut agent, "hello");

        let first = delta(&mut agent, "par");
        let second = delta(&mut agent, "tial");
        let ended = stop(&mut agent, StopReason::EndOfTurn);

        assert!(matches!(
            events(&first).as_slice(),
            [
                SessionEvent::TranscriptItemStarted {
                    role: TranscriptRole::Assistant,
                    ..
                },
                SessionEvent::TranscriptDelta {
                    item_revision: 1,
                    ..
                },
            ]
        ));
        assert!(matches!(
            events(&second).as_slice(),
            [SessionEvent::TranscriptDelta {
                item_revision: 2,
                ..
            }]
        ));
        assert!(matches!(
            events(&ended).as_slice(),
            [
                SessionEvent::TranscriptItemFinalized {
                    item_revision: 3,
                    ..
                },
                SessionEvent::AgentStatusChanged {
                    status: AgentStatus::Idle,
                    ..
                },
            ]
        ));
        assert_eq!(
            agent.record().last(),
            Some(&RequestItem::Assistant {
                text: "partial".into()
            }),
            "the record keeps what the deltas assembled, not the deltas"
        );
    }

    /// A step that produced nothing leaves nothing behind.
    ///
    /// An empty assistant item would be a blank message on screen and an empty turn in the next
    /// request, which is worse than the absence it is trying to record.
    #[test]
    fn a_step_that_produced_no_text_records_nothing_and_finalizes_nothing() {
        let mut agent = agent();
        submit(&mut agent, "hello");

        let ended = stop(&mut agent, StopReason::EndOfTurn);

        assert!(matches!(
            events(&ended).as_slice(),
            [SessionEvent::AgentStatusChanged {
                status: AgentStatus::Idle,
                ..
            }]
        ));
        assert_eq!(agent.record().len(), 1, "only the user's message");
    }

    /// Typing while the model answers must not lose the text, and must not join the request that
    /// was already sent. It waits for the boundary and opens the next turn there.
    #[test]
    fn a_message_typed_while_the_model_streams_opens_the_next_turn_at_the_boundary() {
        let mut agent = agent();
        submit(&mut agent, "first");
        delta(&mut agent, "answer");

        let held = submit(&mut agent, "second");
        assert_eq!(held, Reaction::default(), "nothing happens mid-step");
        assert_eq!(agent.queued(), ["second"]);

        let ended = stop(&mut agent, StopReason::EndOfTurn);

        let [Effect::CallModel(request)] = ended.effects.as_slice() else {
            panic!("the boundary opens the held turn: {:?}", ended.effects);
        };
        assert_eq!(
            request.items,
            [
                RequestItem::User {
                    text: "first".into()
                },
                RequestItem::Assistant {
                    text: "answer".into()
                },
                RequestItem::User {
                    text: "second".into()
                },
            ]
        );
        assert!(agent.queued().is_empty());
        assert!(agent.is_running());
    }

    /// Cancellation is a transition, not an error: what arrived is kept and read, and nothing is
    /// left open for a later delta to attach to.
    #[test]
    fn an_interrupted_turn_keeps_what_arrived_and_leaves_no_item_open() {
        let mut agent = agent();
        submit(&mut agent, "hello");
        delta(&mut agent, "half an ans");

        let stopped = agent.handle(Input::Interrupted);

        assert!(matches!(
            events(&stopped).as_slice(),
            [
                SessionEvent::TranscriptItemFinalized { .. },
                SessionEvent::AgentStatusChanged {
                    status: AgentStatus::Idle,
                    ..
                },
            ]
        ));
        assert_eq!(
            agent.record().last(),
            Some(&RequestItem::Assistant {
                text: "half an ans".into()
            })
        );
        assert!(!agent.is_running());
        assert_eq!(
            agent.handle(Input::Interrupted),
            Reaction::default(),
            "and interrupting an idle agent is not an event"
        );
    }

    /// A failed step is degradation the user can see, and the turn ends rather than hanging.
    #[test]
    fn a_failed_step_is_a_visible_notice_and_ends_the_turn() {
        let mut agent = agent();
        submit(&mut agent, "hello");
        delta(&mut agent, "start");

        let failed = agent.handle(Input::Failed(ModelError::RateLimited {
            retry_after: Some(30),
        }));

        assert_eq!(
            warnings(&failed).len(),
            1,
            "one notice, not none and not two"
        );
        assert!(!agent.is_running());
        assert!(
            events(&failed)
                .iter()
                .any(|event| matches!(event, SessionEvent::TranscriptItemFinalized { .. })),
            "the partial answer is closed rather than left waiting"
        );
    }

    /// Every reason a step can end other than answering is reported, because a turn that stops
    /// silently is indistinguishable from one that answered.
    #[test]
    fn every_stop_that_is_not_an_answer_is_reported() {
        for reason in [
            StopReason::Refused,
            StopReason::OutputLimit,
            StopReason::Unspecified,
            StopReason::ToolCalls,
        ] {
            let mut agent = agent();
            submit(&mut agent, "hello");

            assert_eq!(
                warnings(&stop(&mut agent, reason)).len(),
                1,
                "{reason:?} ended a turn without saying so"
            );
        }

        let mut answered = agent();
        submit(&mut answered, "hello");
        assert!(warnings(&stop(&mut answered, StopReason::EndOfTurn)).is_empty());
    }

    /// Output with no turn open is a producer defect, and the projection refuses invented items,
    /// so the loop reports it instead of opening one.
    #[test]
    fn output_arriving_with_no_turn_open_is_reported_and_writes_nothing() {
        let mut agent = agent();

        let stray = delta(&mut agent, "unasked for");

        assert_eq!(warnings(&stray).len(), 1);
        assert!(agent.record().is_empty());
        assert!(!agent.is_running());
    }

    /// The projection refuses a gap or a repeat, and this is the only thing numbering the stream.
    #[test]
    fn one_agent_numbers_one_stream_with_no_gap_or_repeat() {
        let mut agent = agent();
        let mut sequences = Vec::new();
        let mut collect = |reaction: Reaction| {
            sequences.extend(reaction.events.iter().map(|event| event.sequence.get()));
        };

        collect(submit(&mut agent, "first"));
        collect(delta(&mut agent, "answering"));
        collect(agent.handle(Input::Submitted {
            text: "second".into(),
        }));
        collect(stop(&mut agent, StopReason::EndOfTurn));
        collect(delta(&mut agent, "again"));
        collect(agent.handle(Input::Interrupted));

        let expected: Vec<u64> = (1..=sequences.len() as u64).collect();
        assert_eq!(sequences, expected);
    }
}
