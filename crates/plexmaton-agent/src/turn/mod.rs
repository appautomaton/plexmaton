//! The turn machine: what the loop decides, expressed as a value.
//!
//! Nothing here awaits, spawns, or reads a clock. One method takes a typed input and returns the
//! events the projection should see and the effects someone else must perform, so the whole of a
//! turn is inspectable between any two of them: what it is doing, and what it still owes.
//!
//! A turn is one or more steps. A step is one request to the model and the tool calls it comes
//! back with; the turn ends at the first step that stops for anything else, when its tools are
//! answered and the budget is spent, or when the user interrupts it.

use plexmaton_core::{AgentId, AgentStatus, SessionEvent, TranscriptRole, TurnId};

use crate::admission::ApprovalPolicy;
use crate::interface::{Effect, Input, Reaction, UndeliveredInput, UndeliveredReason};
use crate::model::{ModelEvent, RequestItem, StopReason};
use crate::record::Record;
use crate::step::Step;
use crate::tools::{Batch, PendingApproval, ToolCall};

mod batch;
mod input;
mod lifecycle;

use input::{DeliveryBoundary, InputQueue};

/// How many steps one turn may take before the loop stops it.
///
/// A model that answers its own tool results with more tool calls will do so until something says
/// otherwise, and "something" must not be the user noticing. The number is a guess until it is
/// measured against real work; what matters is that exhausting it is visible rather than silent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TurnBudget {
    /// Steps one turn may take, counting the first.
    pub max_steps: u16,
}

impl Default for TurnBudget {
    fn default() -> Self {
        Self { max_steps: 12 }
    }
}

/// Whether a turn is running, and where it has got to.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Turn {
    /// No turn is open; a submission starts one.
    Idle,
    /// A step is streaming into one assistant message and collecting the calls it asks for.
    Streaming {
        /// Stable owner identity retained through every step.
        turn_id: TurnId,
        /// Step currently receiving model events.
        step: Step,
    },
    /// The step is over and the calls it made are out being run.
    Working {
        /// Stable owner identity retained while tools run or await approval.
        turn_id: TurnId,
        /// What was dispatched, and what has answered.
        batch: Batch,
        /// Which step dispatched them.
        step: u16,
    },
}

/// One agent's session and the turn it is running.
///
/// The record is authoritative: what the model is shown next is assembled from it, and what the
/// screen shows is a projection of the events emitted here. There is no second copy to reconcile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Agent {
    record: Record,
    turn: Turn,
    input: InputQueue,
    budget: TurnBudget,
    policy: ApprovalPolicy,
    announced: bool,
}

impl Agent {
    /// Starts an idle agent with the default budget.
    #[must_use]
    pub fn new(agent_id: AgentId) -> Self {
        Self::with_budget(agent_id, TurnBudget::default())
    }

    /// Starts an idle agent whose turns may take `budget.max_steps` steps.
    #[must_use]
    pub fn with_budget(agent_id: AgentId, budget: TurnBudget) -> Self {
        Self::with_policy(agent_id, budget, ApprovalPolicy::default())
    }

    /// Starts an idle agent with an explicit stateless tool policy.
    #[must_use]
    pub fn with_policy(agent_id: AgentId, budget: TurnBudget, policy: ApprovalPolicy) -> Self {
        Self {
            record: Record::new(agent_id),
            turn: Turn::Idle,
            input: InputQueue::default(),
            budget,
            policy,
            announced: false,
        }
    }

    /// Announces the agent to the workspace, on the agent's own sequence.
    ///
    /// Numbered here rather than by whoever constructed the agent, because a stream numbered in
    /// two places is not a stream: the projection refuses a gap, and a creation event carrying a
    /// supervisor's number would be the first one.
    ///
    /// Announcing twice says nothing. The projection refuses a repeated identity and would show a
    /// producer-defect notice for what is a caller's slip — a reconnect, a resumed session — so
    /// the slip is made unrepresentable here instead of reported there.
    pub fn announce(&mut self, label: impl Into<String>) -> Reaction {
        let mut reaction = Reaction::default();
        if std::mem::replace(&mut self.announced, true) {
            return reaction;
        }
        let event = SessionEvent::AgentCreated {
            agent_id: self.record.agent_id().clone(),
            label: label.into(),
            status: AgentStatus::Idle,
        };
        self.record.emit(&mut reaction, event);
        reaction
    }

    /// Whether a turn is open, whether it is streaming or waiting on its tools.
    #[must_use]
    pub fn is_running(&self) -> bool {
        !matches!(self.turn, Turn::Idle)
    }

    /// The conversation as the model would be shown it right now.
    #[must_use]
    pub fn record(&self) -> &[RequestItem] {
        self.record.items()
    }

    /// Messages waiting for the next turn boundary, in arrival order (LOOP-6).
    pub fn queued_for_next_turn(&self) -> impl Iterator<Item = &str> {
        self.input.pending(DeliveryBoundary::NextTurn)
    }

    /// Steering waiting for the current turn's next step, in arrival order (LOOP-6).
    pub fn queued_for_next_step(&self) -> impl Iterator<Item = &str> {
        self.input.pending(DeliveryBoundary::NextStep)
    }

    /// Approval records owned by the current turn (LOOP-5).
    pub fn pending_approvals(&self) -> impl Iterator<Item = &PendingApproval> {
        match &self.turn {
            Turn::Working { batch, .. } => Some(batch.pending_approvals()),
            Turn::Idle | Turn::Streaming { .. } => None,
        }
        .into_iter()
        .flatten()
    }

    /// Advances the machine by one input.
    pub fn handle(&mut self, input: Input) -> Reaction {
        let mut reaction = Reaction::default();
        match input {
            Input::Submitted { text } => self.submit(text, &mut reaction),
            Input::Steered { text } => self.steer(text, &mut reaction),
            Input::Streamed(event) => self.stream(event, &mut reaction),
            Input::Failed(error) => self.fail(&error, &mut reaction),
            Input::ToolAdmissionResolved(outcome) => {
                self.admission_resolved(outcome, &mut reaction);
            }
            Input::ToolFinished { call_id, outcome } => {
                self.tool_finished(&call_id, outcome, &mut reaction);
            }
            Input::ApprovalDecided {
                approval_id,
                decision,
            } => self.approval_decided(approval_id, decision, &mut reaction),
            Input::Interrupted => self.interrupt(&mut reaction),
            Input::ShuttingDown => self.shutdown(&mut reaction),
        }
        reaction
    }

    /// A submission starts an idle turn or waits for the next turn boundary (LOOP-6).
    fn submit(&mut self, text: String, reaction: &mut Reaction) {
        if self.is_running() {
            self.queue(DeliveryBoundary::NextTurn, text, reaction);
            return;
        }
        self.open_turn(text, reaction);
    }

    /// Steering belongs to the turn already in flight and can only enter at its next step.
    fn steer(&mut self, text: String, reaction: &mut Reaction) {
        if !self.is_running() {
            reaction
                .undelivered
                .push(UndeliveredInput::new(text, UndeliveredReason::NoActiveTurn));
            return;
        }
        self.queue(DeliveryBoundary::NextStep, text, reaction);
    }

    fn queue(&mut self, boundary: DeliveryBoundary, text: String, reaction: &mut Reaction) {
        if let Some(undelivered) = self.input.queue(boundary, text) {
            reaction.undelivered.push(undelivered);
        }
    }

    fn open_turn(&mut self, text: String, reaction: &mut Reaction) {
        self.record_user(text, reaction);
        let turn_id = self.record.next_turn_id();
        self.open_step(turn_id, 1, reaction);
    }

    /// Records user input in both views of the one record: semantic events and model history.
    fn record_user(&mut self, text: String, reaction: &mut Reaction) {
        let item = self.record.next_item_id();
        let agent_id = self.record.agent_id().clone();
        self.record.emit(
            reaction,
            SessionEvent::TranscriptItemStarted {
                agent_id: agent_id.clone(),
                item_id: item.clone(),
                role: TranscriptRole::User,
            },
        );
        self.record.emit(
            reaction,
            SessionEvent::TranscriptDelta {
                agent_id: agent_id.clone(),
                item_id: item.clone(),
                item_revision: 1,
                text: text.clone(),
            },
        );
        self.record.emit(
            reaction,
            SessionEvent::TranscriptItemFinalized {
                agent_id,
                item_id: item,
                item_revision: 2,
            },
        );
        self.record.push(RequestItem::User { text });
    }

    /// Asks the model, and says the agent is producing.
    fn open_step(&mut self, turn_id: TurnId, index: u16, reaction: &mut Reaction) {
        self.status(reaction, AgentStatus::Running);
        self.turn = Turn::Streaming {
            turn_id,
            step: Step::new(index),
        };
        reaction
            .effects
            .push(Effect::CallModel(self.record.request()));
    }

    fn stream(&mut self, event: ModelEvent, reaction: &mut Reaction) {
        if !matches!(self.turn, Turn::Streaming { .. }) {
            self.warn(reaction, "the model produced output with no step open");
            return;
        }
        match event {
            // An empty delta is a wire artefact, not something the reader or the record should
            // gain an item for: opening a message on it would paint a blank row that the record
            // then declines to keep.
            ModelEvent::TextDelta(delta) if delta.is_empty() => {}
            ModelEvent::TextDelta(delta) => {
                if let Turn::Streaming { step, .. } = &mut self.turn {
                    step.append(&mut self.record, reaction, delta);
                }
            }
            ModelEvent::Called(call) => {
                if let Turn::Streaming { step, .. } = &mut self.turn {
                    step.collect(call);
                }
            }
            ModelEvent::Stopped(reason) => self.stop(reason, reaction),
        }
    }

    fn stop(&mut self, reason: StopReason, reaction: &mut Reaction) {
        match reason {
            StopReason::EndOfTurn | StopReason::ToolCalls => {}
            StopReason::OutputLimit => {
                self.warn(reaction, "the model reached its output limit mid-answer");
            }
            StopReason::Refused => self.warn(reaction, "the model declined to answer"),
            StopReason::Unspecified => {
                self.warn(reaction, "the model stopped without saying why");
            }
        }
        let Some((turn_id, calls, index)) = self.close_step(reaction) else {
            return;
        };
        if calls.is_empty() {
            if matches!(reason, StopReason::ToolCalls) {
                self.warn(
                    reaction,
                    "the model stopped for tools without asking for any",
                );
            }
            self.finish_turn(reaction);
            return;
        }
        self.dispatch(turn_id, calls, index, reaction);
    }

    /// Ends the streaming half of the step, and says what it asked for.
    fn close_step(&mut self, reaction: &mut Reaction) -> Option<(TurnId, Vec<ToolCall>, u16)> {
        let Turn::Streaming { turn_id, step } = std::mem::replace(&mut self.turn, Turn::Idle)
        else {
            return None;
        };
        let index = step.index();
        Some((turn_id, step.close(&mut self.record, reaction), index))
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{
        AgentId, AgentStatus, ApprovalDecision, AttentionRequest, SessionEvent, ToolCallId,
        ToolCallStatus, ToolCapability, ToolDefinitionId, TranscriptRole,
    };

    use super::{Agent, Effect, Input, Reaction, TurnBudget};
    use crate::interface::UndeliveredReason;
    use crate::model::{ModelError, ModelEvent, RequestItem, StopReason};
    use crate::tools::{ToolCall, ToolCancellationReason, ToolOutcome};
    use crate::{
        AdmissionOutcome, AdmissionRefusal, AdmittedToolCall, ApprovalDecisionRefusal,
        ApprovalPolicy, CapabilitySet, ToolDefinitionRevision,
    };

    fn agent() -> Agent {
        Agent::new(AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}")))
    }

    fn id(value: &str) -> ToolCallId {
        ToolCallId::new(value).unwrap_or_else(|error| panic!("fixture: {error}"))
    }

    fn submit(agent: &mut Agent, text: &str) -> Reaction {
        agent.handle(Input::Submitted {
            text: text.to_owned(),
        })
    }

    fn steer(agent: &mut Agent, text: &str) -> Reaction {
        agent.handle(Input::Steered {
            text: text.to_owned(),
        })
    }

    fn delta(agent: &mut Agent, text: &str) -> Reaction {
        agent.handle(Input::Streamed(ModelEvent::TextDelta(text.to_owned())))
    }

    fn call(agent: &mut Agent, call_id: &str) -> Reaction {
        call_named(agent, call_id, "read")
    }

    fn call_named(agent: &mut Agent, call_id: &str, name: &str) -> Reaction {
        agent.handle(Input::Streamed(ModelEvent::Called(ToolCall {
            call_id: id(call_id),
            name: name.to_owned(),
            arguments: "{}".to_owned(),
        })))
    }

    fn admitted(
        call_id: &str,
        name: &str,
        capabilities: impl IntoIterator<Item = ToolCapability>,
    ) -> AdmittedToolCall {
        AdmittedToolCall::new(
            ToolCall {
                call_id: id(call_id),
                name: name.to_owned(),
                arguments: "{}".to_owned(),
            },
            ToolDefinitionId::new(format!("{name}-v1"))
                .unwrap_or_else(|error| panic!("fixture: {error}")),
            ToolDefinitionRevision::new(1).unwrap_or_else(|| panic!("fixture revision")),
            capabilities,
            "{}".to_owned(),
            format!("{name} fixture"),
        )
        .unwrap_or_else(|error| panic!("fixture: {error:?}"))
    }

    fn finish(agent: &mut Agent, call_id: &str, output: &str) -> Reaction {
        agent.handle(Input::ToolFinished {
            call_id: id(call_id),
            outcome: ToolOutcome::Succeeded {
                output: output.to_owned(),
            },
        })
    }

    fn stop(agent: &mut Agent, reason: StopReason) -> Reaction {
        let mut reaction = stop_before_admission(agent, reason);
        let calls: Vec<_> = reaction
            .effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::AdmitTool(call) => Some(call.clone()),
                Effect::CallModel(_) | Effect::RunTool(_) => None,
            })
            .collect();
        reaction
            .effects
            .retain(|effect| !matches!(effect, Effect::AdmitTool(_)));
        for call in calls {
            let admitted = AdmittedToolCall::new(
                call,
                ToolDefinitionId::new("read-v1").unwrap_or_else(|error| panic!("fixture: {error}")),
                ToolDefinitionRevision::new(1).unwrap_or_else(|| panic!("fixture revision")),
                [ToolCapability::FileRead],
                "{}".to_owned(),
                "read fixture".to_owned(),
            )
            .unwrap_or_else(|error| panic!("fixture: {error:?}"));
            merge(
                &mut reaction,
                agent.handle(Input::ToolAdmissionResolved(AdmissionOutcome::Admitted(
                    admitted,
                ))),
            );
        }
        reaction
    }

    fn stop_before_admission(agent: &mut Agent, reason: StopReason) -> Reaction {
        agent.handle(Input::Streamed(ModelEvent::Stopped(reason)))
    }

    fn merge(target: &mut Reaction, mut source: Reaction) {
        target.events.append(&mut source.events);
        target.effects.append(&mut source.effects);
        target.undelivered.append(&mut source.undelivered);
        target
            .unresolved_approvals
            .append(&mut source.unresolved_approvals);
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

    fn dispatched(agent: &Agent) -> Vec<String> {
        agent
            .record()
            .iter()
            .filter_map(|item| match item {
                RequestItem::ToolCall(call) => Some(call.call_id.to_string()),
                _ => None,
            })
            .collect()
    }

    fn answered(agent: &Agent) -> Vec<String> {
        agent
            .record()
            .iter()
            .filter_map(|item| match item {
                RequestItem::ToolResult { call_id, .. } => Some(call_id.to_string()),
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

    /// A step that asked for tools does not end the turn: it dispatches, and waits to be answered.
    #[test]
    fn a_step_that_asked_for_tools_dispatches_them_and_waits() {
        let mut agent = agent();
        submit(&mut agent, "read two files");
        delta(&mut agent, "on it");
        call(&mut agent, "one");
        call(&mut agent, "two");

        let dispatching = stop(&mut agent, StopReason::ToolCalls);

        assert_eq!(
            dispatching
                .effects
                .iter()
                .filter(|effect| matches!(effect, Effect::RunTool(_)))
                .count(),
            2,
            "both calls are asked for, and nothing else is"
        );
        assert!(
            !dispatching
                .effects
                .iter()
                .any(|effect| matches!(effect, Effect::CallModel(_))),
            "the model is not asked again until its calls are answered"
        );
        assert!(events(&dispatching).iter().any(|event| matches!(
            event,
            SessionEvent::AgentStatusChanged {
                status: AgentStatus::Waiting,
                ..
            }
        )));
        assert_eq!(dispatched(&agent), ["one", "two"]);
        assert!(agent.is_running(), "waiting on a tool is still a turn");
    }

    /// The model is answered in the order it asked, whatever order the machine finished in.
    #[test]
    fn answered_calls_become_the_next_step_with_results_in_model_order() {
        let mut agent = agent();
        submit(&mut agent, "read three files");
        for call_id in ["one", "two", "three"] {
            call(&mut agent, call_id);
        }
        stop(&mut agent, StopReason::ToolCalls);

        finish(&mut agent, "three", "third");
        finish(&mut agent, "one", "first");
        let opened = finish(&mut agent, "two", "second");

        assert_eq!(answered(&agent), ["one", "two", "three"]);
        let [Effect::CallModel(request)] = opened.effects.as_slice() else {
            panic!(
                "the settled batch takes the next step: {:?}",
                opened.effects
            );
        };
        assert_eq!(
            request.items.len(),
            7,
            "the user's message, three calls and three results"
        );
        assert!(matches!(
            events(&opened).last(),
            Some(SessionEvent::AgentStatusChanged {
                status: AgentStatus::Running,
                ..
            })
        ));
    }

    /// The debt rule, at every point an interrupt can land: a call the loop dispatched is
    /// answered, so what the turn leaves behind is a conversation the next request can be built
    /// from. A dispatched call with no result is a request no dialect will accept, which makes the
    /// defect appear one turn after the mistake.
    #[test]
    fn an_interrupt_leaves_a_result_for_every_call_it_dispatched() {
        for finished in 0..=3 {
            let mut agent = agent();
            submit(&mut agent, "read three files");
            for call_id in ["one", "two", "three"] {
                call(&mut agent, call_id);
            }
            stop(&mut agent, StopReason::ToolCalls);
            for call_id in ["one", "two", "three"].iter().take(finished) {
                finish(&mut agent, call_id, "done");
            }

            agent.handle(Input::Interrupted);

            assert_eq!(
                dispatched(&agent),
                answered(&agent),
                "interrupting with {finished} of three answered left a call unanswered"
            );
            assert!(!agent.is_running());
        }
    }

    /// The same debt, when the step fails rather than when the user stops it.
    #[test]
    fn a_failed_step_pays_what_its_calls_owe() {
        let mut agent = agent();
        submit(&mut agent, "read one file");
        call(&mut agent, "one");
        stop(&mut agent, StopReason::ToolCalls);

        let failed = agent.handle(Input::Failed(ModelError::Transport {
            message: "connection reset".to_owned(),
        }));

        assert_eq!(warnings(&failed).len(), 1);
        assert_eq!(dispatched(&agent), answered(&agent));
        assert_eq!(
            agent.record().last(),
            Some(&RequestItem::ToolResult {
                call_id: id("one"),
                outcome: ToolOutcome::Cancelled {
                    reason: ToolCancellationReason::StepFailed
                }
            })
        );
    }

    /// A model that answers its own tool results with more tool calls is stopped by the budget,
    /// and the user is told why rather than watching it go quiet.
    #[test]
    fn a_turn_stops_at_its_step_budget_and_says_so() {
        let mut agent = Agent::with_budget(
            AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}")),
            TurnBudget { max_steps: 2 },
        );
        let mut requests = 0;

        let mut asked = submit(&mut agent, "keep going");
        let mut round = 0;
        loop {
            requests += asked
                .effects
                .iter()
                .filter(|effect| matches!(effect, Effect::CallModel(_)))
                .count();
            round += 1;
            let call_id = format!("call-{round}");
            call(&mut agent, &call_id);
            let dispatching = stop(&mut agent, StopReason::ToolCalls);
            if !dispatching
                .effects
                .iter()
                .any(|effect| matches!(effect, Effect::RunTool(_)))
            {
                panic!("the step asked for a tool");
            }
            asked = finish(&mut agent, &call_id, "done");
            if !agent.is_running() {
                assert_eq!(warnings(&asked).len(), 1, "the budget is spent out loud");
                break;
            }
            assert!(round < 8, "the budget never stopped the turn");
        }

        assert_eq!(requests, 2, "two steps, because that is the budget");
        assert_eq!(dispatched(&agent), answered(&agent));
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
        assert_eq!(agent.queued_for_next_turn().collect::<Vec<_>>(), ["second"]);

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
        assert_eq!(agent.queued_for_next_turn().count(), 0);
        assert!(agent.is_running());
    }

    /// LOOP-6: steering names the current turn's next step, not merely the next model request that
    /// happens to be made. The tool-result boundary claims it; the later turn queue stays separate.
    #[test]
    fn steering_is_claimed_only_by_the_current_turns_next_step() {
        let mut agent = agent();
        submit(&mut agent, "read one file");
        steer(&mut agent, "check the cache too");
        submit(&mut agent, "then summarize");
        call(&mut agent, "one");
        stop(&mut agent, StopReason::ToolCalls);

        assert_eq!(
            agent.queued_for_next_step().collect::<Vec<_>>(),
            ["check the cache too"]
        );
        assert_eq!(
            agent.queued_for_next_turn().collect::<Vec<_>>(),
            ["then summarize"]
        );

        let claimed = finish(&mut agent, "one", "contents");
        let [Effect::CallModel(request)] = claimed.effects.as_slice() else {
            panic!(
                "settling the batch must open the next step: {:?}",
                claimed.effects
            );
        };
        assert_eq!(
            request.items.last(),
            Some(&RequestItem::User {
                text: "check the cache too".to_owned()
            })
        );
        assert_eq!(agent.queued_for_next_step().count(), 0);
        assert_eq!(
            agent.queued_for_next_turn().collect::<Vec<_>>(),
            ["then summarize"],
            "a step boundary must not claim the next turn"
        );
    }

    /// LOOP-6: a missing boundary returns ownership with a typed reason. The text is never moved
    /// to a different boundary just because that one still exists.
    #[test]
    fn input_without_its_boundary_is_returned_with_its_text_intact() {
        let mut idle = agent();
        let no_turn = steer(&mut idle, "do not lose this");
        assert!(matches!(
            no_turn.undelivered.as_slice(),
            [input]
                if input.text == "do not lose this"
                    && input.reason == UndeliveredReason::NoActiveTurn
        ));

        let mut ended = agent();
        submit(&mut ended, "first");
        steer(&mut ended, "amend the answer");
        let stopped = stop(&mut ended, StopReason::EndOfTurn);
        assert!(matches!(
            stopped.undelivered.as_slice(),
            [input]
                if input.text == "amend the answer"
                    && input.reason == UndeliveredReason::TurnEnded
        ));
        assert_eq!(
            ended.record(),
            [RequestItem::User {
                text: "first".to_owned()
            }],
            "steering must not be rewritten as a later user turn"
        );
    }

    /// LOOP-6 through the public boundary: queue overflow returns ownership from `handle` rather
    /// than relying on the internal queue's caller to remember its `Option`.
    #[test]
    fn agent_returns_the_exact_input_that_overflows_its_queue() {
        let mut agent = agent();
        submit(&mut agent, "first");
        let mut overflow = None;

        for index in 0..100 {
            let text = format!("steer {index}");
            let reaction = steer(&mut agent, &text);
            if !reaction.undelivered.is_empty() {
                overflow = Some((text, reaction));
                break;
            }
        }

        let (text, reaction) = overflow
            .unwrap_or_else(|| panic!("the bounded queue accepted one hundred pending inputs"));
        assert!(matches!(
            reaction.undelivered.as_slice(),
            [input]
                if input.text == text && input.reason == UndeliveredReason::QueueFull
        ));
    }

    /// LOOP-6 on abnormal boundaries: failure and budget exhaustion do not silently move steering
    /// into a later turn, and both return the exact payload with the transition that prevented it.
    #[test]
    fn failure_and_budget_return_pending_steering() {
        let mut failed = agent();
        submit(&mut failed, "first");
        steer(&mut failed, "still mine");
        let failed_reaction = failed.handle(Input::Failed(ModelError::Transport {
            message: "offline".to_owned(),
        }));
        assert!(matches!(
            failed_reaction.undelivered.as_slice(),
            [input]
                if input.text == "still mine"
                    && input.reason == UndeliveredReason::StepFailed
        ));

        let mut budgeted = Agent::with_budget(
            AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}")),
            TurnBudget { max_steps: 1 },
        );
        submit(&mut budgeted, "first");
        steer(&mut budgeted, "keep this too");
        call(&mut budgeted, "one");
        stop(&mut budgeted, StopReason::ToolCalls);
        let spent = finish(&mut budgeted, "one", "done");
        assert!(matches!(
            spent.undelivered.as_slice(),
            [input]
                if input.text == "keep this too"
                    && input.reason == UndeliveredReason::StepBudgetReached
        ));
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

    /// Output with no step open is a producer defect, and the projection refuses invented items,
    /// so the loop reports it instead of opening one.
    #[test]
    fn output_arriving_with_no_step_open_is_reported_and_writes_nothing() {
        let mut agent = agent();

        let stray = delta(&mut agent, "unasked for");

        assert_eq!(warnings(&stray).len(), 1);
        assert!(agent.record().is_empty());
        assert!(!agent.is_running());
    }

    /// An outcome for a call this turn never dispatched would put an identity in the conversation
    /// the model never used, so it is reported instead of recorded.
    #[test]
    fn a_tool_answering_for_a_call_no_step_made_is_reported() {
        let mut agent = agent();
        submit(&mut agent, "read one file");
        call(&mut agent, "one");
        stop(&mut agent, StopReason::ToolCalls);

        let stray = finish(&mut agent, "elsewhere", "stray");

        assert_eq!(warnings(&stray).len(), 1);
        assert!(answered(&agent).is_empty());
        assert!(agent.is_running(), "and the batch is still waiting");
    }

    /// APV-1, APV-2 and LOOP-5: raw model arguments produce only an admission effect. A protected
    /// admitted call becomes inspectable pending state and runs only after its exact ID is allowed.
    #[test]
    fn a_protected_call_waits_as_state_and_allow_once_resumes_that_exact_call() {
        let mut agent = agent();
        submit(&mut agent, "change the file");
        call_named(&mut agent, "write-1", "edit");

        let dispatched = stop_before_admission(&mut agent, StopReason::ToolCalls);
        assert!(matches!(
            dispatched.effects.as_slice(),
            [Effect::AdmitTool(call)] if call.call_id == id("write-1")
        ));
        assert!(events(&dispatched).iter().any(|event| matches!(
            event,
            SessionEvent::ToolCallChanged {
                status: ToolCallStatus::Queued,
                ..
            }
        )));

        let waiting = agent.handle(Input::ToolAdmissionResolved(AdmissionOutcome::Admitted(
            admitted("write-1", "edit", [ToolCapability::FileWrite]),
        )));
        assert!(
            waiting.effects.is_empty(),
            "approval is state, not execution"
        );
        let pending = agent
            .pending_approvals()
            .next()
            .cloned()
            .unwrap_or_else(|| panic!("the protected call must be pending"));
        assert_eq!(pending.admitted().requested().call_id, id("write-1"));
        assert_eq!(pending.admitted().definition_revision().get(), 1);
        assert!(events(&waiting).iter().any(|event| matches!(
            event,
            SessionEvent::AttentionRequested {
                request: AttentionRequest::Approval { approval_id, call_id, .. },
                ..
            } if approval_id == pending.approval_id() && call_id == &id("write-1")
        )));

        let allowed = agent.handle(Input::ApprovalDecided {
            approval_id: pending.approval_id().clone(),
            decision: ApprovalDecision::AllowOnce,
        });
        assert!(agent.pending_approvals().next().is_none());
        assert!(matches!(
            allowed.effects.as_slice(),
            [Effect::RunTool(call)] if call.requested().call_id == id("write-1")
        ));
        assert!(matches!(
            events(&allowed).as_slice(),
            [
                SessionEvent::AttentionResolved { attention_id, .. },
                SessionEvent::ToolCallChanged {
                    status: ToolCallStatus::Running,
                    ..
                }
            ] if attention_id == pending.attention_id()
        ));
    }

    /// APV-4 and LOOP-2: denial resolves Attention, never executes, and is still a tool result in
    /// the next model request. Reusing the same approval ID is a typed non-decision.
    #[test]
    fn deny_pays_the_call_debt_and_a_duplicate_decision_is_typed() {
        let mut agent = agent();
        submit(&mut agent, "change the file");
        call_named(&mut agent, "write-1", "edit");
        stop_before_admission(&mut agent, StopReason::ToolCalls);
        agent.handle(Input::ToolAdmissionResolved(AdmissionOutcome::Admitted(
            admitted("write-1", "edit", [ToolCapability::FileWrite]),
        )));
        let approval_id = agent
            .pending_approvals()
            .next()
            .map(|pending| pending.approval_id().clone())
            .unwrap_or_else(|| panic!("pending approval"));

        let denied = agent.handle(Input::ApprovalDecided {
            approval_id: approval_id.clone(),
            decision: ApprovalDecision::Deny,
        });
        assert!(
            denied
                .effects
                .iter()
                .all(|effect| !matches!(effect, Effect::RunTool(_)))
        );
        let request = denied
            .effects
            .iter()
            .find_map(|effect| match effect {
                Effect::CallModel(request) => Some(request),
                Effect::AdmitTool(_) | Effect::RunTool(_) => None,
            })
            .unwrap_or_else(|| panic!("denial completes the batch and opens the next step"));
        assert!(matches!(
            request.items.last(),
            Some(RequestItem::ToolResult {
                call_id,
                outcome: ToolOutcome::Denied
            }) if call_id == &id("write-1")
        ));

        let duplicate = agent.handle(Input::ApprovalDecided {
            approval_id: approval_id.clone(),
            decision: ApprovalDecision::Deny,
        });
        assert!(matches!(
            duplicate.unresolved_approvals.as_slice(),
            [unresolved]
                if unresolved.approval_id == approval_id
                    && unresolved.reason == ApprovalDecisionRefusal::NotPending
        ));
        assert!(duplicate.effects.is_empty());
        assert!(duplicate.events.is_empty());
    }

    /// APV-5: admission and execution are per slot. A safe sibling runs and may finish while a
    /// protected call waits, but the model receives neither result until the whole batch is paid.
    #[test]
    fn a_safe_sibling_runs_while_a_protected_call_waits_and_results_keep_model_order() {
        let mut agent = agent();
        submit(&mut agent, "read then edit");
        call_named(&mut agent, "write-1", "edit");
        call_named(&mut agent, "read-2", "read");
        stop_before_admission(&mut agent, StopReason::ToolCalls);

        agent.handle(Input::ToolAdmissionResolved(AdmissionOutcome::Admitted(
            admitted("write-1", "edit", [ToolCapability::FileWrite]),
        )));
        let read = agent.handle(Input::ToolAdmissionResolved(AdmissionOutcome::Admitted(
            admitted("read-2", "read", [ToolCapability::FileRead]),
        )));
        assert!(matches!(
            read.effects.as_slice(),
            [Effect::RunTool(call)] if call.requested().call_id == id("read-2")
        ));
        let read_done = finish(&mut agent, "read-2", "contents");
        assert!(
            !read_done
                .effects
                .iter()
                .any(|effect| matches!(effect, Effect::CallModel(_))),
            "one pending slot keeps the batch open"
        );

        let approval_id = agent
            .pending_approvals()
            .next()
            .map(|pending| pending.approval_id().clone())
            .unwrap_or_else(|| panic!("write waits"));
        agent.handle(Input::ApprovalDecided {
            approval_id,
            decision: ApprovalDecision::AllowOnce,
        });
        let write_done = finish(&mut agent, "write-1", "changed");
        let request = write_done
            .effects
            .iter()
            .find_map(|effect| match effect {
                Effect::CallModel(request) => Some(request),
                Effect::AdmitTool(_) | Effect::RunTool(_) => None,
            })
            .unwrap_or_else(|| panic!("settled batch opens the next step"));
        let result_ids: Vec<_> = request
            .items
            .iter()
            .filter_map(|item| match item {
                RequestItem::ToolResult { call_id, .. } => Some(call_id.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(result_ids, ["write-1", "read-2"]);
    }

    /// APV-2 and APV-3: forbidden is policy, not an approval option, and catalog refusal is a
    /// separate result. Neither path emits a run effect or an Attention request.
    #[test]
    fn forbidden_and_admission_refusal_finish_without_approval_or_execution() {
        let policy = ApprovalPolicy::new(
            CapabilitySet::default(),
            CapabilitySet::new([ToolCapability::FileWrite]),
        );
        let mut forbidden = Agent::with_policy(
            AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}")),
            TurnBudget::default(),
            policy,
        );
        submit(&mut forbidden, "change");
        call_named(&mut forbidden, "write-1", "edit");
        stop_before_admission(&mut forbidden, StopReason::ToolCalls);
        let forbidden_result = forbidden.handle(Input::ToolAdmissionResolved(
            AdmissionOutcome::Admitted(admitted("write-1", "edit", [ToolCapability::FileWrite])),
        ));
        assert!(
            forbidden_result
                .effects
                .iter()
                .all(|effect| !matches!(effect, Effect::RunTool(_)))
        );
        assert!(
            !events(&forbidden_result)
                .iter()
                .any(|event| matches!(event, SessionEvent::AttentionRequested { .. }))
        );
        assert!(matches!(
            forbidden.record().iter().find_map(|item| match item {
                RequestItem::ToolResult { outcome, .. } => Some(outcome),
                _ => None,
            }),
            Some(ToolOutcome::Forbidden)
        ));

        let mut refused = agent();
        submit(&mut refused, "unknown");
        call_named(&mut refused, "unknown-1", "missing");
        stop_before_admission(&mut refused, StopReason::ToolCalls);
        let refused_result =
            refused.handle(Input::ToolAdmissionResolved(AdmissionOutcome::Refused {
                call_id: id("unknown-1"),
                reason: AdmissionRefusal::UnknownTool,
            }));
        assert!(
            refused_result
                .effects
                .iter()
                .all(|effect| !matches!(effect, Effect::RunTool(_)))
        );
        assert!(matches!(
            refused.record().iter().find_map(|item| match item {
                RequestItem::ToolResult { outcome, .. } => Some(outcome),
                _ => None,
            }),
            Some(ToolOutcome::AdmissionRefused {
                reason: AdmissionRefusal::UnknownTool
            })
        ));
    }

    /// APV-6: no hidden waiter survives cancellation. Both interrupt and shutdown resolve the
    /// Attention item, pay the call with the typed cause, and leave the turn idle.
    #[test]
    fn interrupt_and_shutdown_cancel_pending_approval_as_explicit_state() {
        for (input, expected) in [
            (Input::Interrupted, ToolCancellationReason::Interrupted),
            (Input::ShuttingDown, ToolCancellationReason::Shutdown),
        ] {
            let mut agent = agent();
            submit(&mut agent, "change");
            call_named(&mut agent, "write-1", "edit");
            stop_before_admission(&mut agent, StopReason::ToolCalls);
            agent.handle(Input::ToolAdmissionResolved(AdmissionOutcome::Admitted(
                admitted("write-1", "edit", [ToolCapability::FileWrite]),
            )));
            let attention_id = agent
                .pending_approvals()
                .next()
                .map(|pending| pending.attention_id().clone())
                .unwrap_or_else(|| panic!("pending approval"));

            let cancelled = agent.handle(input);

            assert!(!agent.is_running());
            assert!(agent.pending_approvals().next().is_none());
            assert!(events(&cancelled).iter().any(|event| matches!(
                event,
                SessionEvent::AttentionResolved { attention_id: resolved, .. }
                    if resolved == &attention_id
            )));
            assert!(matches!(
                agent.record().last(),
                Some(RequestItem::ToolResult {
                    outcome: ToolOutcome::Cancelled { reason },
                    ..
                }) if *reason == expected
            ));
        }
    }

    /// An interrupt stops work; it does not start any.
    ///
    /// A cancelled turn that immediately opened the queued message's turn would fire a model
    /// request the user had just cancelled, and leave the agent running when they asked for it to
    /// stop. The text is neither sent nor dropped: ownership returns to the caller with its reason.
    #[test]
    fn an_interrupt_starts_no_new_work_and_returns_what_was_waiting() {
        let mut agent = agent();
        submit(&mut agent, "first");
        delta(&mut agent, "answering");
        submit(&mut agent, "second");
        steer(&mut agent, "steer the first");

        let stopped = agent.handle(Input::Interrupted);

        assert!(
            !stopped
                .effects
                .iter()
                .any(|effect| matches!(effect, Effect::CallModel(_))),
            "the cancelled turn asked the model again: {:?}",
            stopped.effects
        );
        assert!(!agent.is_running(), "and left the agent running");
        assert_eq!(agent.queued_for_next_turn().count(), 0);
        assert_eq!(agent.queued_for_next_step().count(), 0);
        assert_eq!(
            stopped
                .undelivered
                .iter()
                .map(|input| (input.text.as_str(), input.reason))
                .collect::<Vec<_>>(),
            [
                ("second", UndeliveredReason::Interrupted),
                ("steer the first", UndeliveredReason::Interrupted),
            ],
            "the boundary returned every held input, in arrival order"
        );
        assert!(matches!(
            events(&stopped).last(),
            Some(SessionEvent::AgentStatusChanged {
                status: AgentStatus::Idle,
                ..
            })
        ));
    }

    /// An empty delta is a wire artefact. Opening a message for it paints a blank row that the
    /// record then declines to keep, so the screen and the next request disagree about the turn.
    #[test]
    fn an_empty_delta_opens_nothing_and_says_nothing() {
        let mut agent = agent();
        submit(&mut agent, "hello");

        let empty = delta(&mut agent, "");

        assert_eq!(empty, Reaction::default());
        assert_eq!(
            agent.record().len(),
            1,
            "only the user's message is in the record"
        );

        let ended = stop(&mut agent, StopReason::EndOfTurn);
        assert!(
            !events(&ended)
                .iter()
                .any(|event| matches!(event, SessionEvent::TranscriptItemFinalized { .. })),
            "an item nothing opened cannot be finalized"
        );
    }

    /// Announcing twice would put a repeated identity on the agent's own sequence, which the
    /// projection refuses — reporting a producer defect for what is a caller's slip.
    #[test]
    fn an_agent_announces_itself_once_however_often_it_is_asked() {
        let mut agent = agent();

        let first = agent.announce("Agent A");
        let again = agent.announce("Agent A");

        assert_eq!(first.events.len(), 1);
        assert_eq!(again, Reaction::default());
    }

    /// The projection refuses a gap or a repeat, and this is the only thing numbering the stream.
    #[test]
    fn one_agent_numbers_one_stream_with_no_gap_or_repeat() {
        let mut agent = agent();
        let mut sequences = Vec::new();
        let mut collect = |reaction: Reaction| {
            sequences.extend(reaction.events.iter().map(|event| event.sequence.get()));
        };

        collect(agent.announce("Agent A"));
        collect(submit(&mut agent, "first"));
        collect(delta(&mut agent, "answering"));
        collect(call(&mut agent, "one"));
        collect(stop(&mut agent, StopReason::ToolCalls));
        collect(agent.handle(Input::Submitted {
            text: "second".into(),
        }));
        collect(finish(&mut agent, "one", "done"));
        collect(agent.handle(Input::Interrupted));

        let expected: Vec<u64> = (1..=sequences.len() as u64).collect();
        assert_eq!(sequences, expected);
    }
}
