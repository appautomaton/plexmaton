//! The turn machine: what the loop decides, expressed as a value.
//!
//! Nothing here awaits, spawns, or reads a clock. One method takes a typed input and returns the
//! events the projection should see and the effects someone else must perform, so the whole of a
//! turn is inspectable between any two of them: what it is doing, and what it still owes.
//!
//! A turn is one or more steps. A step is one request to the model and the tool calls it comes
//! back with; the turn ends at the first step that stops for anything else, when its tools are
//! answered and the budget is spent, or when the user interrupts it.

use plexmaton_core::{AgentId, AgentStatus, ConversationEvent, TurnId};

use crate::UnixMillis;
use crate::admission::ApprovalPolicy;
use crate::interface::{Effect, Input, Reaction};
use crate::journal::{ConversationJournal, JournalEntryPayload};
use crate::model::{ModelCall, ModelStepId};
use crate::record::Record;
use crate::step::Step;
use crate::tools::{Batch, PendingApproval};

mod batch;
mod input;
mod lifecycle;
mod model_input;
mod permission;
mod request_attempt;
mod retry;
#[cfg(test)]
mod skill_tests;
mod tool_projection;
mod user_input;

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

/// Why the settled screen projection cannot be rebuilt at this boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectionRebuildError {
    /// Provider or tool work still has transient state absent from the completed journal.
    ActiveTurn,
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
        step: Box<Step>,
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
/// The journal is authoritative: what the model is shown next is rebuilt from its selected path,
/// and completed screen state can be rebuilt from the same facts (JRN-5, JRN-6).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Agent {
    record: Record,
    turn: Turn,
    input: InputQueue,
    budget: TurnBudget,
    policy: ApprovalPolicy,
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
        Self::with_record(Record::new(agent_id), budget, policy)
    }

    /// Starts an idle agent for one explicit durable session identity.
    #[must_use]
    pub fn for_conversation(
        agent_id: AgentId,
        metadata: crate::ConversationMetadata,
        budget: TurnBudget,
        policy: ApprovalPolicy,
    ) -> Self {
        Self::with_record(Record::for_conversation(agent_id, metadata), budget, policy)
    }

    /// Rehydrates an idle owner from one already-validated canonical journal.
    pub fn from_journal(
        agent_id: AgentId,
        journal: ConversationJournal,
        budget: TurnBudget,
        policy: ApprovalPolicy,
    ) -> Result<Self, crate::JournalProjectionError> {
        Ok(Self::with_record(
            Record::from_journal(agent_id, journal)?,
            budget,
            policy,
        ))
    }

    fn with_record(record: Record, budget: TurnBudget, policy: ApprovalPolicy) -> Self {
        Self {
            record,
            turn: Turn::Idle,
            input: InputQueue::default(),
            budget,
            policy,
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
        self.announce_into(label.into(), &mut reaction);
        reaction.into_output()
    }

    fn announce_into(&mut self, label: String, reaction: &mut Reaction) {
        if self.record.is_announced() {
            return;
        }
        self.record.commit(
            JournalEntryPayload::AgentCreated {
                agent_id: self.record.agent_id().clone(),
                label: label.clone(),
                status: AgentStatus::Idle,
            },
            reaction,
        );
        let event = ConversationEvent::AgentCreated {
            agent_id: self.record.agent_id().clone(),
            label,
            status: AgentStatus::Idle,
        };
        self.record.emit(reaction, event);
    }

    /// Whether a turn is open, whether it is streaming or waiting on its tools.
    #[must_use]
    pub fn is_running(&self) -> bool {
        !matches!(self.turn, Turn::Idle)
    }

    /// Exact model step currently accepting provider output, if one is open (LIVE-2).
    #[must_use]
    pub fn active_model_step(&self) -> Option<ModelStepId> {
        match &self.turn {
            Turn::Streaming { turn_id, step, .. } => {
                Some(ModelStepId::new(turn_id.clone(), step.index()))
            }
            Turn::Idle | Turn::Working { .. } => None,
        }
    }

    /// The conversation as the model would be shown it right now.
    #[must_use]
    pub fn record(&self) -> Vec<crate::ContextAtom> {
        self.record.atoms()
    }

    /// Canonical in-memory journal from which the model and settled screen are rebuilt (JRN-5).
    #[must_use]
    pub fn journal(&self) -> &ConversationJournal {
        self.record.journal()
    }

    /// Named ancestry currently used by this agent's model requests.
    #[must_use]
    pub fn selected_head(&self) -> &plexmaton_core::HeadName {
        self.record.selected_head()
    }

    /// Rebuilds the settled visible projection and rebases its live delivery cursor (JRN-6).
    pub fn rebuild_projection(
        &mut self,
    ) -> Result<crate::JournalProjection, ProjectionRebuildError> {
        if self.is_running() {
            return Err(ProjectionRebuildError::ActiveTurn);
        }
        Ok(self.record.rebuild_projection())
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

    /// Accepts a read-only view from the coding Session owner; never restores grants from history.
    pub fn use_permission_snapshot(&mut self, snapshot: std::sync::Arc<crate::PermissionSnapshot>) {
        self.policy.use_snapshot(snapshot);
    }

    /// Advances the machine with a wall observation supplied by its runtime owner (TIM-1).
    pub fn handle_at(&mut self, input: Input, observed_at: UnixMillis) -> Reaction {
        let mut reaction = Reaction::at(observed_at);
        self.announce_into(self.record.agent_id().to_string(), &mut reaction);
        match input {
            Input::Submitted { text } => self.submit(text, &mut reaction),
            Input::SkillSubmitted { text, skill } => {
                self.submit_with_skill(text, skill, &mut reaction);
            }
            Input::Steered { text } => self.steer(text, &mut reaction),
            Input::SkillSteered { text, skill } => {
                self.steer_with_skill(text, skill, &mut reaction);
            }
            Input::Streamed { step_id, event } => {
                if self.accepts_model_input(step_id, &mut reaction) {
                    self.stream(event, &mut reaction);
                }
            }
            Input::Failed { step_id, error } => {
                if self.accepts_model_input(step_id, &mut reaction) {
                    self.fail(&error, &mut reaction);
                }
            }
            Input::ToolAdmissionResolved(outcome) => {
                self.admission_resolved(outcome, &mut reaction);
            }
            Input::ToolFinished { call_id, result } => {
                self.tool_finished(&call_id, result, &mut reaction);
            }
            Input::ApprovalDecided {
                approval_id,
                decision,
            } => self.approval_decided(approval_id, decision, &mut reaction),
            Input::PermissionPrepared(outcome) => self.permission_prepared(outcome, &mut reaction),
            Input::PermissionsChanged => self.release_allowed_approvals(&mut reaction),
            Input::Interrupted => self.interrupt(&mut reaction),
            Input::ShuttingDown => self.shutdown(&mut reaction),
        }
        reaction.into_output()
    }

    #[cfg(test)]
    pub(crate) fn handle(&mut self, input: Input) -> Reaction {
        self.handle_at(input, UnixMillis::EPOCH)
    }

    /// Asks the model, and says the agent is producing.
    fn open_step(&mut self, turn_id: TurnId, index: u16, reaction: &mut Reaction) {
        if index > 1 {
            self.status(turn_id.clone(), reaction, crate::ActiveTurnStatus::Running);
        }
        let step_id = ModelStepId::new(turn_id.clone(), index);
        self.turn = Turn::Streaming {
            turn_id,
            step: Box::new(Step::new(step_id.turn_id().clone(), index)),
        };
        reaction.effects.push(Effect::CallModel(ModelCall {
            step_id,
            request: self.record.request(),
        }));
    }
}

#[cfg(test)]
mod tests {
    mod permissions;
    use plexmaton_core::{
        AgentId, AgentStatus, ApprovalDecision, AttentionRequest, ConversationEvent,
        ConversationId, HeadName, ToolCallId, ToolCallStatus, ToolCapability, ToolDefinitionId,
        ToolDetail, TranscriptRole,
    };

    use super::{Agent, Effect, Input, ProjectionRebuildError, Reaction, Turn, TurnBudget};
    use crate::interface::UndeliveredReason;
    use crate::model::{
        AssistantBlock, ContextAtom, ContextAtomValue, MAX_ASSISTANT_TEXT_BYTES, ModelError,
        ModelEvent, ModelOutputPosition, ModelStepId, StopReason, ToolBatchResult,
    };
    use crate::test_support::replay;
    use crate::tools::{ToolCall, ToolCancellationReason, ToolExecutionResult, ToolOutcome};
    use crate::{
        AdmissionOutcome, AdmissionRefusal, AdmittedToolCall, ApprovalDecisionRefusal,
        ApprovalPolicy, CapabilitySet, ConversationJournal, JournalEntryPayload, JournalRecord,
        ModelDeliveryRefusal, ToolDefinitionRevision, TurnFinishedAt, TurnOutcome, UnixMillis,
    };

    fn bare_agent() -> Agent {
        Agent::new(AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}")))
    }

    fn agent() -> Agent {
        let mut agent = bare_agent();
        let _announced = agent.announce("Agent A");
        agent
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

    fn active_step(agent: &Agent) -> ModelStepId {
        let Turn::Streaming { turn_id, step, .. } = &agent.turn else {
            panic!("fixture expected an open model step");
        };
        ModelStepId::new(turn_id.clone(), step.index())
    }

    fn streamed(agent: &mut Agent, event: ModelEvent) -> Reaction {
        let step_id = active_step(agent);
        agent.handle(Input::Streamed { step_id, event })
    }

    fn fail_step(agent: &mut Agent, error: ModelError) -> Reaction {
        let step_id = active_step(agent);
        agent.handle(Input::Failed { step_id, error })
    }

    fn delta(agent: &mut Agent, text: &str) -> Reaction {
        streamed(
            agent,
            ModelEvent::TextDelta {
                position: ModelOutputPosition::new(0, 1),
                delta: text.to_owned(),
            },
        )
    }

    fn call(agent: &mut Agent, call_id: &str) -> Reaction {
        call_named(agent, call_id, "read")
    }

    fn call_named(agent: &mut Agent, call_id: &str, name: &str) -> Reaction {
        let ordinal = call_id
            .rsplit_once('-')
            .map_or(call_id, |(_, suffix)| suffix);
        let part = ordinal.parse::<u16>().unwrap_or_else(|_| match ordinal {
            "one" => 1,
            "two" => 2,
            "three" => 3,
            value if value.len() == 1 && value.as_bytes()[0].is_ascii_lowercase() => {
                u16::from(value.as_bytes()[0] - b'a' + 1)
            }
            _ => 1,
        });
        streamed(
            agent,
            ModelEvent::Called {
                position: ModelOutputPosition::new(1, part),
                call: ToolCall {
                    call_id: id(call_id),
                    name: name.to_owned(),
                    arguments: "{}".to_owned(),
                },
            },
        )
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
            Some(ToolDetail::Text {
                source: format!("{name} invocation"),
                omitted_bytes: 0,
            }),
        )
        .unwrap_or_else(|error| panic!("fixture: {error:?}"))
    }

    fn finish(agent: &mut Agent, call_id: &str, output: &str) -> Reaction {
        agent.handle(Input::ToolFinished {
            call_id: id(call_id),
            result: ToolExecutionResult::new(
                ToolOutcome::Succeeded {
                    output: output.to_owned(),
                },
                None,
            ),
        })
    }

    fn stop(agent: &mut Agent, reason: StopReason) -> Reaction {
        let mut reaction = stop_before_admission(agent, reason);
        let mut calls = Vec::new();
        for effect in std::mem::take(&mut reaction.effects) {
            match effect {
                Effect::AdmitTool(call) => calls.push(call),
                other @ (Effect::CallModel(_)
                | Effect::RunTool { .. }
                | Effect::PreparePermission(_)) => {
                    reaction.effects.push(other);
                }
            }
        }
        for request in calls {
            let admitted = request
                .admit(
                    ToolDefinitionId::new("read-v1")
                        .unwrap_or_else(|error| panic!("fixture: {error}")),
                    ToolDefinitionRevision::new(1).unwrap_or_else(|| panic!("fixture revision")),
                    [ToolCapability::FileRead],
                    "{}".to_owned(),
                    "read fixture".to_owned(),
                    None,
                )
                .unwrap_or_else(|error| panic!("fixture: {error:?}"));
            merge(
                &mut reaction,
                agent.handle(Input::ToolAdmissionResolved(admitted)),
            );
        }
        reaction
    }

    fn stop_before_admission(agent: &mut Agent, reason: StopReason) -> Reaction {
        streamed(agent, ModelEvent::Stopped(reason))
    }

    fn merge(target: &mut Reaction, mut source: Reaction) {
        target.records.append(&mut source.records);
        target.events.append(&mut source.events);
        target.effects.append(&mut source.effects);
        target.undelivered.append(&mut source.undelivered);
        target
            .unresolved_approvals
            .append(&mut source.unresolved_approvals);
        target
            .undelivered_model
            .append(&mut source.undelivered_model);
    }

    fn events(reaction: &Reaction) -> Vec<ConversationEvent> {
        reaction
            .events
            .iter()
            .map(|envelope| envelope.event.clone())
            .collect()
    }

    fn runtime_messages(reaction: &Reaction) -> Vec<String> {
        events(reaction)
            .into_iter()
            .filter_map(|event| match event {
                ConversationEvent::RuntimeWarning { message, .. }
                | ConversationEvent::RuntimeError { message, .. } => Some(message),
                _ => None,
            })
            .collect()
    }

    fn dispatched(agent: &Agent) -> Vec<String> {
        let head = HeadName::new("main").unwrap_or_else(|error| panic!("fixture: {error}"));
        agent
            .journal()
            .path(&head)
            .unwrap_or_else(|error| panic!("live journal path: {error:?}"))
            .iter()
            .filter_map(|entry| match &entry.payload {
                JournalEntryPayload::ToolCallRequested { call_id, .. } => Some(call_id.to_string()),
                _ => None,
            })
            .collect()
    }

    fn answered(agent: &Agent) -> Vec<String> {
        agent
            .record()
            .iter()
            .flat_map(|atom| match atom.value() {
                ContextAtomValue::ToolBatch(batch) => batch.results(),
                ContextAtomValue::User { .. }
                | ContextAtomValue::Skill(_)
                | ContextAtomValue::Assistant(_) => &[],
            })
            .map(|result| result.call_id().to_string())
            .collect()
    }

    fn tool_results(agent: &Agent) -> Vec<ToolBatchResult> {
        context_results(&agent.record())
            .into_iter()
            .cloned()
            .collect()
    }

    fn context_results(atoms: &[ContextAtom]) -> Vec<&ToolBatchResult> {
        atoms
            .iter()
            .flat_map(|atom| match atom.value() {
                ContextAtomValue::ToolBatch(batch) => batch.results(),
                ContextAtomValue::User { .. }
                | ContextAtomValue::Skill(_)
                | ContextAtomValue::Assistant(_) => &[],
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
                ConversationEvent::TranscriptItemStarted { role: TranscriptRole::User, .. },
                ConversationEvent::TranscriptDelta { item_revision: 1, text, .. },
                ConversationEvent::TranscriptItemFinalized { item_revision: 2, .. },
                ConversationEvent::AgentStatusChanged { status: AgentStatus::Running, .. },
            ] if text == "hello"
        ));
        let [Effect::CallModel(request)] = reaction.effects.as_slice() else {
            panic!("one submission asks the model once: {:?}", reaction.effects);
        };
        assert_eq!(request.request.atoms.len(), 1);
        assert!(matches!(
            request.request.atoms[0].value(),
            ContextAtomValue::User { text } if text == "hello"
        ));
        assert!(agent.is_running());
    }

    /// TIM-1/TIM-4: chronology survives in the journal without changing provider context or the
    /// semantic head when the turn becomes terminal.
    #[test]
    fn tim_1_turn_boundaries_are_durable_and_terminal_time_does_not_advance_the_head() {
        let mut agent = agent();
        let opened = agent.handle_at(
            Input::Submitted {
                text: "hello".to_owned(),
            },
            UnixMillis::new(100),
        );
        let started = opened.records.iter().find_map(|record| match record {
            JournalRecord::AppendEntry { entry, .. }
                if matches!(entry.payload, JournalEntryPayload::TurnStarted { .. }) =>
            {
                Some(entry.as_ref())
            }
            _ => None,
        });
        let Some(crate::ConversationEntry {
            payload:
                JournalEntryPayload::TurnStarted {
                    turn_id,
                    accepted_at,
                    opened_at,
                    ..
                },
            ..
        }) = started
        else {
            panic!("submission did not append an atomic turn start")
        };
        assert_eq!(*accepted_at, UnixMillis::new(100));
        assert_eq!(*opened_at, UnixMillis::new(100));
        let head = HeadName::new("main").unwrap_or_else(|error| panic!("fixture: {error}"));
        let target_before = agent
            .journal()
            .head_target(&head)
            .map(|target| target.cloned());
        let revision_before = agent.journal().head_revision(&head);
        let request_before = agent.record();
        let step_id = active_step(&agent);

        let finished = agent.handle_at(
            Input::Streamed {
                step_id,
                event: ModelEvent::Stopped(StopReason::EndOfTurn),
            },
            UnixMillis::new(250),
        );
        assert!(finished.records.iter().any(|record| matches!(
            record,
            JournalRecord::TurnFinished { fact, .. }
                if &fact.turn_id == turn_id
                    && fact.outcome == TurnOutcome::Completed
                    && fact.at == TurnFinishedAt::Observed {
                        completed_at: UnixMillis::new(250)
                    }
        )));
        assert_eq!(
            agent
                .journal()
                .head_target(&head)
                .map(|target| target.cloned()),
            target_before
        );
        assert_eq!(agent.journal().head_revision(&head), revision_before);
        assert_eq!(agent.record(), request_before);
    }

    /// TIM-1/LOOP-6: accepted time follows queued input until its actual semantic boundary opens.
    #[test]
    fn tim_1_queued_turn_and_steering_keep_their_original_accepted_time() {
        let mut agent = agent();
        agent.handle_at(
            Input::Submitted {
                text: "first".to_owned(),
            },
            UnixMillis::new(10),
        );
        agent.handle_at(
            Input::Submitted {
                text: "second".to_owned(),
            },
            UnixMillis::new(20),
        );
        let step_id = active_step(&agent);
        let boundary = agent.handle_at(
            Input::Streamed {
                step_id,
                event: ModelEvent::Stopped(StopReason::EndOfTurn),
            },
            UnixMillis::new(30),
        );
        assert!(boundary.records.iter().any(|record| matches!(
            record,
            JournalRecord::AppendEntry { entry, .. }
                if matches!(
                    entry.payload,
                    JournalEntryPayload::TurnStarted {
                        accepted_at,
                        opened_at,
                        ref text,
                        ..
                    } if text == "second"
                        && accepted_at == UnixMillis::new(20)
                        && opened_at == UnixMillis::new(30)
                )
        )));

        agent.handle_at(
            Input::Steered {
                text: "also inspect tests".to_owned(),
            },
            UnixMillis::new(40),
        );
        call(&mut agent, "queued-steering");
        stop(&mut agent, StopReason::ToolCalls);
        let claimed = agent.handle_at(
            Input::ToolFinished {
                call_id: id("queued-steering"),
                result: ToolExecutionResult::new(
                    ToolOutcome::Succeeded {
                        output: "done".to_owned(),
                    },
                    None,
                ),
            },
            UnixMillis::new(70),
        );
        assert!(claimed.records.iter().any(|record| matches!(
            record,
            JournalRecord::AppendEntry { entry, .. }
                if matches!(
                    entry.payload,
                    JournalEntryPayload::SteeringAccepted {
                        accepted_at,
                        ref text,
                        ..
                    } if text == "also inspect tests" && accepted_at == UnixMillis::new(40)
                )
        )));
    }

    /// TIM-1: every live terminal route records its semantic outcome instead of inferring it later.
    #[test]
    fn tim_1_every_live_turn_terminal_path_has_a_typed_outcome() {
        fn outcome(reaction: &Reaction) -> TurnOutcome {
            reaction
                .records
                .iter()
                .find_map(|record| match record {
                    JournalRecord::TurnFinished { fact, .. } => Some(fact.outcome),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("terminal transition omitted TurnFinished"))
        }

        let mut completed = agent();
        submit(&mut completed, "complete");
        assert_eq!(
            outcome(&stop(&mut completed, StopReason::EndOfTurn)),
            TurnOutcome::Completed
        );

        let mut interrupted = agent();
        submit(&mut interrupted, "interrupt");
        assert_eq!(
            outcome(&interrupted.handle(Input::Interrupted)),
            TurnOutcome::Interrupted
        );

        let mut failed = agent();
        submit(&mut failed, "fail");
        assert_eq!(
            outcome(&fail_step(
                &mut failed,
                ModelError::Transport {
                    message: "offline".to_owned(),
                },
            )),
            TurnOutcome::Failed
        );

        let mut shutdown = agent();
        submit(&mut shutdown, "shutdown");
        assert_eq!(
            outcome(&shutdown.handle(Input::ShuttingDown)),
            TurnOutcome::Shutdown
        );

        let mut budgeted = Agent::with_budget(
            AgentId::new("agent-budget").unwrap_or_else(|error| panic!("fixture: {error}")),
            TurnBudget { max_steps: 1 },
        );
        let _announcement = budgeted.announce("Budgeted");
        submit(&mut budgeted, "use a tool");
        call(&mut budgeted, "budget-call");
        stop(&mut budgeted, StopReason::ToolCalls);
        assert_eq!(
            outcome(&finish(&mut budgeted, "budget-call", "done")),
            TurnOutcome::StepBudgetReached
        );
    }

    /// JRN-6: a projection rebuild cannot erase provider state that has not become canonical.
    #[test]
    fn jrn_6_active_projection_rebuild_is_refused() {
        let mut agent = agent();
        submit(&mut agent, "hello");

        assert_eq!(
            agent.rebuild_projection(),
            Err(ProjectionRebuildError::ActiveTurn)
        );
    }

    /// LIVE-2: output is accepted only for the exact open step, so a cancelled task cannot append
    /// to a later turn even when its delta arrives after that turn opened.
    #[test]
    fn stale_and_post_cancellation_model_output_is_a_typed_non_delivery() {
        let mut agent = agent();
        let first = submit(&mut agent, "first");
        let [Effect::CallModel(first_call)] = first.effects.as_slice() else {
            panic!("first turn opens one step");
        };
        let first_step = first_call.step_id.clone();
        agent.handle(Input::Interrupted);

        let second = submit(&mut agent, "second");
        let [Effect::CallModel(second_call)] = second.effects.as_slice() else {
            panic!("second turn opens one step");
        };
        let second_step = second_call.step_id.clone();
        let stale = agent.handle(Input::Streamed {
            step_id: first_step.clone(),
            event: ModelEvent::TextDelta {
                position: ModelOutputPosition::new(0, 1),
                delta: "too late".to_owned(),
            },
        });

        assert!(stale.events.is_empty());
        assert!(matches!(
            stale.undelivered_model.as_slice(),
            [undelivered]
                if undelivered.step_id == first_step
                    && undelivered.reason
                        == ModelDeliveryRefusal::WrongStep {
                            expected: second_step
                        }
        ));
        assert_eq!(agent.record().len(), 2, "the stale text entered no record");

        agent.handle(Input::Interrupted);
        let after_cancel = agent.handle(Input::Failed {
            step_id: first_step,
            error: ModelError::Transport {
                message: "late failure".to_owned(),
            },
        });
        assert!(matches!(
            after_cancel.undelivered_model.as_slice(),
            [undelivered]
                if undelivered.reason == ModelDeliveryRefusal::NoActiveStep
        ));
        assert!(after_cancel.events.is_empty());
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
                ConversationEvent::TranscriptItemStarted {
                    role: TranscriptRole::Assistant,
                    ..
                },
                ConversationEvent::TranscriptDelta {
                    item_revision: 1,
                    ..
                },
            ]
        ));
        assert!(matches!(
            events(&second).as_slice(),
            [ConversationEvent::TranscriptDelta {
                item_revision: 2,
                ..
            }]
        ));
        assert!(matches!(
            events(&ended).as_slice(),
            [
                ConversationEvent::TranscriptItemFinalized {
                    item_revision: 3,
                    ..
                },
                ConversationEvent::AgentStatusChanged {
                    status: AgentStatus::Idle,
                    ..
                },
            ]
        ));
        assert!(
            matches!(
                agent.record().last().map(|atom| atom.value()),
                Some(ContextAtomValue::Assistant(output))
                    if matches!(output.blocks(), [AssistantBlock::Text { text, .. }] if text == "partial")
            ),
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
            [ConversationEvent::AgentStatusChanged {
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
                .filter(|effect| matches!(effect, Effect::RunTool { .. }))
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
            ConversationEvent::AgentStatusChanged {
                status: AgentStatus::Waiting,
                ..
            }
        )));
        assert_eq!(dispatched(&agent), ["one", "two"]);
        let tool_events: Vec<_> = events(&dispatching)
            .into_iter()
            .filter_map(|event| match event {
                ConversationEvent::ToolCallChanged {
                    item_id,
                    item_revision,
                    call_id,
                    status,
                    ..
                } => Some((
                    call_id.to_string(),
                    item_id.to_string(),
                    item_revision,
                    status,
                )),
                _ => None,
            })
            .collect();
        assert_eq!(
            tool_events
                .iter()
                .map(|(call, _, revision, status)| (call.as_str(), *revision, *status))
                .collect::<Vec<_>>(),
            [
                ("one", 0, ToolCallStatus::Queued),
                ("two", 0, ToolCallStatus::Queued),
                ("one", 1, ToolCallStatus::Running),
                ("two", 1, ToolCallStatus::Running),
            ]
        );
        assert_eq!(tool_events[0].1, tool_events[2].1);
        assert_eq!(tool_events[1].1, tool_events[3].1);
        assert_ne!(tool_events[0].1, tool_events[1].1);
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
            request.request.atoms.len(),
            2,
            "the user's message and one indivisible tool batch"
        );
        assert!(matches!(
            events(&opened).last(),
            Some(ConversationEvent::AgentStatusChanged {
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

    /// The same debt when the runtime shuts down while tools are still out.
    #[test]
    fn shutdown_pays_what_dispatched_calls_owe() {
        let mut agent = agent();
        submit(&mut agent, "read one file");
        call(&mut agent, "one");
        stop(&mut agent, StopReason::ToolCalls);

        let stopped = agent.handle(Input::ShuttingDown);

        assert!(runtime_messages(&stopped).is_empty());
        assert_eq!(dispatched(&agent), answered(&agent));
        assert!(matches!(
            tool_results(&agent).last(),
            Some(result)
                if result.call_id() == &id("one")
                    && result.outcome() == &ToolOutcome::Cancelled {
                        reason: ToolCancellationReason::Shutdown,
                    }
        ));
    }

    /// A model that answers its own tool results with more tool calls is stopped by the budget,
    /// and the user is told why rather than watching it go quiet.
    #[test]
    fn a_turn_stops_at_its_step_budget_and_says_so() {
        let mut agent = Agent::with_budget(
            AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}")),
            TurnBudget { max_steps: 2 },
        );
        let _announced = agent.announce("Agent A");
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
                .any(|effect| matches!(effect, Effect::RunTool { .. }))
            {
                panic!("the step asked for a tool");
            }
            asked = finish(&mut agent, &call_id, "done");
            if !agent.is_running() {
                assert_eq!(
                    runtime_messages(&asked).len(),
                    1,
                    "the budget is spent out loud"
                );
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
        let values = request
            .request
            .atoms
            .iter()
            .map(|atom| atom.value())
            .collect::<Vec<_>>();
        assert!(matches!(
            values.as_slice(),
            [
                ContextAtomValue::User { text: first },
                ContextAtomValue::Assistant(output),
                ContextAtomValue::User { text: second },
            ] if first == "first"
                && second == "second"
                && matches!(output.blocks(), [AssistantBlock::Text { text, .. }] if text == "answer")
        ));
        assert_eq!(agent.queued_for_next_turn().count(), 0);
        assert_eq!(
            ended
                .released_inputs
                .iter()
                .map(crate::ReleasedInput::text)
                .collect::<Vec<_>>(),
            ["second"]
        );
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
            request.request.atoms.last().map(|atom| atom.value()),
            Some(&ContextAtomValue::User {
                text: "check the cache too".to_owned(),
            })
        );
        assert_eq!(agent.queued_for_next_step().count(), 0);
        assert_eq!(
            claimed
                .released_inputs
                .iter()
                .map(crate::ReleasedInput::text)
                .collect::<Vec<_>>(),
            ["check the cache too"]
        );
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
        assert!(
            matches!(
                ended.record().as_slice(),
                [atom]
                    if atom.value() == &ContextAtomValue::User { text: "first".to_owned() }
            ),
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
        let failed_reaction = fail_step(
            &mut failed,
            ModelError::Transport {
                message: "offline".to_owned(),
            },
        );
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
        let _announced = budgeted.announce("Agent A");
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
                ConversationEvent::TranscriptItemFinalized { .. },
                ConversationEvent::AgentStatusChanged {
                    status: AgentStatus::Idle,
                    ..
                },
            ]
        ));
        assert!(matches!(
            agent.record().last().map(|atom| atom.value()),
            Some(ContextAtomValue::Assistant(output))
                if matches!(output.blocks(), [AssistantBlock::Text { text, .. }] if text == "half an ans")
        ));
        assert!(!agent.is_running());
        assert_eq!(
            agent.handle(Input::Interrupted),
            Reaction::default(),
            "and interrupting an idle agent is not an event"
        );
    }

    /// JRN-5/JRN-6: aborting a stream cannot publish a call that never reached dispatch, because
    /// the immutable terminal would otherwise leave provider replay with an unmatched tool debt.
    #[test]
    fn abort_discards_complete_but_undispatched_stream_calls() {
        for fail in [false, true] {
            let mut agent = agent();
            submit(&mut agent, "inspect");
            delta(&mut agent, "partial");
            call(&mut agent, "never-dispatched");

            if fail {
                fail_step(
                    &mut agent,
                    ModelError::Transport {
                        message: "disconnected".to_owned(),
                    },
                );
            } else {
                agent.handle(Input::Interrupted);
            }

            let projection = agent
                .journal()
                .project(&HeadName::new("main").unwrap_or_else(|error| panic!("head: {error}")))
                .unwrap_or_else(|error| panic!("aborted journal must project: {error:?}"));
            assert!(projection.recovery().is_none());
            assert!(
                projection
                    .request()
                    .atoms
                    .iter()
                    .all(|atom| match atom.value() {
                        ContextAtomValue::User { .. } => true,
                        ContextAtomValue::Skill(_) => true,
                        ContextAtomValue::Assistant(output) => output.tool_calls().next().is_none(),
                        ContextAtomValue::ToolBatch(_) => false,
                    })
            );
        }
    }

    /// JRN-6: the live transcript cannot open blocks in an order reload would later reverse.
    #[test]
    fn decreasing_provider_output_positions_fail_before_entering_canonical_order() {
        let mut agent = agent();
        submit(&mut agent, "ordered output");
        streamed(
            &mut agent,
            ModelEvent::TextDelta {
                position: ModelOutputPosition::new(1, 0),
                delta: "arrived first".to_owned(),
            },
        );

        let failed = streamed(
            &mut agent,
            ModelEvent::ReasoningDelta {
                position: ModelOutputPosition::new(0, 0),
                delta: "claims to precede it".to_owned(),
            },
        );

        assert_eq!(runtime_messages(&failed).len(), 1);
        assert!(!agent.is_running());
        assert!(
            agent
                .journal()
                .project(&HeadName::new("main").unwrap_or_else(|error| panic!("head: {error}")))
                .is_ok()
        );
    }

    /// PRV-2/JRN-6: parallel calls may complete out of order because they remain buffered until
    /// the declaration-order batch is dispatched.
    #[test]
    fn reverse_parallel_call_completion_is_sorted_before_dispatch() {
        let mut agent = agent();
        submit(&mut agent, "parallel calls");
        streamed(
            &mut agent,
            ModelEvent::Called {
                position: ModelOutputPosition::new(1, 0),
                call: ToolCall {
                    call_id: id("second"),
                    name: "read".to_owned(),
                    arguments: "{}".to_owned(),
                },
            },
        );
        streamed(
            &mut agent,
            ModelEvent::Called {
                position: ModelOutputPosition::new(0, 0),
                call: ToolCall {
                    call_id: id("first"),
                    name: "read".to_owned(),
                    arguments: "{}".to_owned(),
                },
            },
        );

        let stopped = stop_before_admission(&mut agent, StopReason::ToolCalls);
        let call_ids: Vec<_> = stopped
            .effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::AdmitTool(request) => Some(request.requested().call_id.as_str()),
                Effect::CallModel(_) | Effect::RunTool { .. } | Effect::PreparePermission(_) => {
                    None
                }
            })
            .collect();

        assert_eq!(call_ids, ["first", "second"]);
        assert!(runtime_messages(&stopped).is_empty());
    }

    /// PRV-2/JRN-3: a non-provider model driver cannot grow canonical text past its bound.
    #[test]
    fn oversized_semantic_text_fails_before_canonical_commit() {
        let mut agent = agent();
        submit(&mut agent, "bounded output");

        let failed = streamed(
            &mut agent,
            ModelEvent::TextDelta {
                position: ModelOutputPosition::new(0, 0),
                delta: "x".repeat(MAX_ASSISTANT_TEXT_BYTES + 1),
            },
        );

        assert_eq!(runtime_messages(&failed).len(), 1);
        assert!(!agent.is_running());
        let projection = agent
            .journal()
            .project(&HeadName::new("main").unwrap_or_else(|error| panic!("head: {error}")))
            .unwrap_or_else(|error| panic!("bounded failure must project: {error:?}"));
        assert_eq!(projection.request().atoms.len(), 1);
    }

    /// PRV-3: plaintext reasoning is projected as its own semantic item, while opaque replay is
    /// retained only in the authoritative request record. Both survive an abnormal step boundary.
    #[test]
    fn reasoning_and_opaque_replay_survive_interrupt_without_sharing_presentation() {
        let mut agent = agent();
        submit(&mut agent, "hello");
        let reasoning = streamed(
            &mut agent,
            ModelEvent::ReasoningDelta {
                position: ModelOutputPosition::new(0, 0),
                delta: "bounded thought".to_owned(),
            },
        );
        let replay = replay(r#"{"type":"reasoning","encrypted_content":"ciphertext"}"#);
        let replay_reaction = streamed(
            &mut agent,
            ModelEvent::Replay {
                position: ModelOutputPosition::new(0, 0),
                replay: replay.clone(),
            },
        );
        delta(&mut agent, "partial answer");

        let interrupted = agent.handle(Input::Interrupted);

        assert!(events(&reasoning).iter().any(|event| matches!(
            event,
            ConversationEvent::TranscriptItemStarted {
                role: TranscriptRole::Reasoning,
                ..
            }
        )));
        assert_eq!(
            replay_reaction,
            Reaction::default(),
            "opaque replay is record state, never a transcript or notice"
        );
        let recorded = agent.record();
        let Some(ContextAtomValue::Assistant(output)) = recorded.get(1).map(|atom| atom.value())
        else {
            panic!("one assistant output follows the user atom")
        };
        assert!(matches!(
            output.blocks(),
            [
                AssistantBlock::Reasoning { text: thought, .. },
                AssistantBlock::Text { text: answer, .. },
            ] if thought == "bounded thought" && answer == "partial answer"
        ));
        assert_eq!(
            output
                .replay()
                .and_then(|replay| replay.attachments().first())
                .map(|attachment| attachment.payload()),
            Some(replay.payload())
        );
        assert_eq!(
            events(&interrupted)
                .iter()
                .filter(|event| matches!(event, ConversationEvent::TranscriptItemFinalized { .. }))
                .count(),
            2
        );
    }

    /// A failed step is an error the user can see, and the turn ends rather than hanging.
    #[test]
    fn a_failed_step_is_a_visible_error_and_ends_the_turn() {
        let mut agent = agent();
        submit(&mut agent, "hello");
        delta(&mut agent, "start");

        let failed = fail_step(
            &mut agent,
            ModelError::RateLimited {
                retry_after: Some(30),
            },
        );

        assert_eq!(
            runtime_messages(&failed).len(),
            1,
            "one error, not none and not two"
        );
        assert!(!agent.is_running());
        assert!(
            events(&failed)
                .iter()
                .any(|event| matches!(event, ConversationEvent::TranscriptItemFinalized { .. })),
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
                runtime_messages(&stop(&mut agent, reason)).len(),
                1,
                "{reason:?} ended a turn without saying so"
            );
        }

        let mut answered = agent();
        submit(&mut answered, "hello");
        assert!(runtime_messages(&stop(&mut answered, StopReason::EndOfTurn)).is_empty());
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

        assert_eq!(runtime_messages(&stray).len(), 1);
        assert!(answered(&agent).is_empty());
        assert!(agent.is_running(), "and the batch is still waiting");
    }

    /// JRN-6: a provider cannot make the canonical history ambiguous by reusing a call identity.
    #[test]
    fn jrn_6_reused_tool_call_identity_fails_the_turn_before_commit() {
        let mut agent = agent();
        submit(&mut agent, "read twice");
        call(&mut agent, "one");
        stop(&mut agent, StopReason::ToolCalls);
        finish(&mut agent, "one", "first");

        let refused = call(&mut agent, "one");

        assert_eq!(runtime_messages(&refused).len(), 1);
        assert!(!agent.is_running());
        assert_eq!(dispatched(&agent), ["one"]);
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
            [Effect::AdmitTool(call)] if call.requested().call_id == id("write-1")
        ));
        assert!(events(&dispatched).iter().any(|event| matches!(
            event,
            ConversationEvent::ToolCallChanged {
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
            ConversationEvent::AttentionRequested {
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
            [Effect::RunTool { call, .. }] if call.requested().call_id == id("write-1")
        ));
        assert!(matches!(
            events(&allowed).as_slice(),
            [
                ConversationEvent::AttentionResolved { attention_id, .. },
                ConversationEvent::ToolCallChanged {
                    status: ToolCallStatus::Running,
                    ..
                }
            ] if attention_id == pending.attention_id()
        ));
    }

    /// ENT-2/ENT-4: later lifecycle updates advance one entry without erasing presentation facts
    /// already produced at admission or execution.
    #[test]
    fn tool_status_updates_accumulate_invocation_and_outcome_presentation() {
        let mut agent = agent();
        submit(&mut agent, "change the file");
        call_named(&mut agent, "write-1", "edit");
        stop_before_admission(&mut agent, StopReason::ToolCalls);

        let waiting = agent.handle(Input::ToolAdmissionResolved(AdmissionOutcome::Admitted(
            admitted("write-1", "edit", [ToolCapability::FileWrite]),
        )));
        let waiting_presentation = events(&waiting)
            .into_iter()
            .find_map(|event| match event {
                ConversationEvent::ToolCallChanged {
                    status: ToolCallStatus::AwaitingApproval,
                    presentation,
                    ..
                } => Some(presentation),
                _ => None,
            })
            .unwrap_or_else(|| panic!("awaiting-approval presentation"));
        assert!(waiting_presentation.invocation.is_some());
        assert!(waiting_presentation.outcome.is_none());

        let approval_id = agent
            .pending_approvals()
            .next()
            .map(|pending| pending.approval_id().clone())
            .unwrap_or_else(|| panic!("pending approval"));
        let running = agent.handle(Input::ApprovalDecided {
            approval_id,
            decision: ApprovalDecision::AllowOnce,
        });
        let running_presentation = events(&running)
            .into_iter()
            .find_map(|event| match event {
                ConversationEvent::ToolCallChanged {
                    status: ToolCallStatus::Running,
                    presentation,
                    ..
                } => Some(presentation),
                _ => None,
            })
            .unwrap_or_else(|| panic!("running presentation"));
        assert_eq!(running_presentation, waiting_presentation);

        let outcome = ToolDetail::Diff {
            patch: "*** Begin Patch\n*** End Patch\n".to_owned(),
        };
        let finished = agent.handle(Input::ToolFinished {
            call_id: id("write-1"),
            result: ToolExecutionResult::new(
                ToolOutcome::Succeeded {
                    output: "unchanged model result".to_owned(),
                },
                Some(outcome.clone()),
            ),
        });
        let terminal_presentation = events(&finished)
            .into_iter()
            .find_map(|event| match event {
                ConversationEvent::ToolCallChanged {
                    status: ToolCallStatus::Succeeded,
                    presentation,
                    ..
                } => Some(presentation),
                _ => None,
            })
            .unwrap_or_else(|| panic!("terminal presentation"));
        assert_eq!(
            terminal_presentation.invocation,
            waiting_presentation.invocation
        );
        assert_eq!(terminal_presentation.outcome, Some(outcome));
        assert!(matches!(
            tool_results(&agent).first().map(|result| result.outcome()),
            Some(ToolOutcome::Succeeded { output }) if output == "unchanged model result"
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
                .all(|effect| !matches!(effect, Effect::RunTool { .. }))
        );
        let request = denied
            .effects
            .iter()
            .find_map(|effect| match effect {
                Effect::CallModel(request) => Some(request),
                Effect::AdmitTool(_) | Effect::RunTool { .. } | Effect::PreparePermission(_) => {
                    None
                }
            })
            .unwrap_or_else(|| panic!("denial completes the batch and opens the next step"));
        assert!(matches!(
            context_results(&request.request.atoms).last(),
            Some(result)
                if result.call_id() == &id("write-1")
                    && result.outcome() == &ToolOutcome::Denied
        ));
        assert!(events(&denied).iter().any(|event| matches!(
            event,
            ConversationEvent::ToolCallChanged {
                status: ToolCallStatus::Denied,
                presentation,
                ..
            } if presentation.invocation.is_some()
                && matches!(
                    &presentation.outcome,
                    Some(ToolDetail::Text { source, omitted_bytes: 0 })
                        if source == "denied by user"
                )
        )));

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
            [Effect::RunTool { call, .. }] if call.requested().call_id == id("read-2")
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
                Effect::AdmitTool(_) | Effect::RunTool { .. } | Effect::PreparePermission(_) => {
                    None
                }
            })
            .unwrap_or_else(|| panic!("settled batch opens the next step"));
        let result_ids: Vec<_> = context_results(&request.request.atoms)
            .into_iter()
            .map(|result| result.call_id().to_string())
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
        let _announced = forbidden.announce("Agent A");
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
                .all(|effect| !matches!(effect, Effect::RunTool { .. }))
        );
        assert!(
            !events(&forbidden_result)
                .iter()
                .any(|event| matches!(event, ConversationEvent::AttentionRequested { .. }))
        );
        assert!(matches!(
            tool_results(&forbidden)
                .first()
                .map(|result| result.outcome()),
            Some(ToolOutcome::Forbidden)
        ));
        assert!(events(&forbidden_result).iter().any(|event| matches!(
            event,
            ConversationEvent::ToolCallChanged {
                status: ToolCallStatus::Failed,
                presentation,
                ..
            } if presentation.invocation.is_some()
                && matches!(
                    &presentation.outcome,
                    Some(ToolDetail::Text { source, omitted_bytes: 0 })
                        if source == "forbidden by policy"
                )
        )));

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
                .all(|effect| !matches!(effect, Effect::RunTool { .. }))
        );
        assert!(matches!(
            tool_results(&refused)
                .first()
                .map(|result| result.outcome()),
            Some(ToolOutcome::AdmissionRefused {
                reason: AdmissionRefusal::UnknownTool
            })
        ));
        assert!(events(&refused_result).iter().any(|event| matches!(
            event,
            ConversationEvent::ToolCallChanged {
                status: ToolCallStatus::Failed,
                presentation,
                ..
            } if presentation.invocation.is_none()
                && matches!(
                    &presentation.outcome,
                    Some(ToolDetail::Text { source, omitted_bytes: 0 })
                        if source == "admission refused: unknown_tool"
                )
        )));
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
                ConversationEvent::AttentionResolved { attention_id: resolved, .. }
                    if resolved == &attention_id
            )));
            assert!(matches!(
                tool_results(&agent).last(),
                Some(result)
                    if result.outcome() == &ToolOutcome::Cancelled { reason: expected }
            ));
            assert!(events(&cancelled).iter().any(|event| matches!(
                event,
                ConversationEvent::ToolCallChanged {
                    status: ToolCallStatus::Cancelled,
                    presentation,
                    ..
                } if presentation.invocation.is_some()
                    && matches!(
                        &presentation.outcome,
                        Some(ToolDetail::Text { source, omitted_bytes: 0 })
                            if source.starts_with("cancelled: ")
                    )
            )));
        }
    }

    /// ENT-4: cancellation before admission still records a typed terminal explanation; there is
    /// no fabricated invocation because canonical admission never completed.
    #[test]
    fn cancellation_before_admission_has_outcome_without_invocation() {
        let mut agent = agent();
        submit(&mut agent, "read one file");
        call(&mut agent, "read-1");
        stop_before_admission(&mut agent, StopReason::ToolCalls);

        let cancelled = agent.handle(Input::Interrupted);

        assert!(events(&cancelled).iter().any(|event| matches!(
            event,
            ConversationEvent::ToolCallChanged {
                status: ToolCallStatus::Cancelled,
                presentation,
                ..
            } if presentation.invocation.is_none()
                && matches!(
                    &presentation.outcome,
                    Some(ToolDetail::Text { source, omitted_bytes: 0 })
                        if source == "cancelled: interrupted"
                )
        )));
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
            Some(ConversationEvent::AgentStatusChanged {
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
                .any(|event| matches!(event, ConversationEvent::TranscriptItemFinalized { .. })),
            "an item nothing opened cannot be finalized"
        );
    }

    /// Announcing twice would put a repeated identity on the agent's own sequence, which the
    /// projection refuses — reporting a producer defect for what is a caller's slip.
    #[test]
    fn an_agent_announces_itself_once_however_often_it_is_asked() {
        let mut agent = bare_agent();

        let first = agent.announce("Agent A");
        let again = agent.announce("Agent A");

        assert_eq!(first.events.len(), 1);
        assert_eq!(again, Reaction::default());
    }

    /// The projection refuses a gap or a repeat, and this is the only thing numbering the stream.
    #[test]
    fn one_agent_numbers_one_stream_with_no_gap_or_repeat() {
        let mut agent = bare_agent();
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

    /// JRN-5: an explicitly finished turn needs no recovery just because it has no answer.
    #[test]
    fn failed_or_cancelled_unanswered_turns_do_not_become_process_recovery() {
        for interrupted in [false, true] {
            let mut live = agent();
            submit(&mut live, "keep the original question");
            if interrupted {
                live.handle(Input::Interrupted);
            } else {
                fail_step(&mut live, ModelError::RateLimited { retry_after: None });
            }
            let before = live.journal().clone();
            let mut resumed = Agent::from_journal(
                AgentId::new("agent-a").expect("agent"),
                before.clone(),
                TurnBudget::default(),
                ApprovalPolicy::default(),
            )
            .expect("restore completed turn");
            assert!(resumed.recover_after_process_death().is_none());
            assert_eq!(resumed.journal(), &before);
            let projection = resumed.rebuild_projection().expect("context");
            assert_eq!(projection.request().atoms.len(), 1);
            assert!(matches!(
                projection.request().atoms[0].value(),
                ContextAtomValue::User { .. }
            ));
        }
    }

    /// JRN-5/JRN-7: process recovery settles canonical debt without rerunning its tool effect.
    #[test]
    fn an_unfinished_restored_turn_becomes_idle_with_a_stable_cancelled_tool_result() {
        let mut live = agent();
        submit(&mut live, "change a file");
        call_named(&mut live, "write-1", "edit");
        let _requested = stop_before_admission(&mut live, StopReason::ToolCalls);
        let waiting = live.handle(Input::ToolAdmissionResolved(AdmissionOutcome::Admitted(
            admitted("write-1", "edit", [ToolCapability::FileWrite]),
        )));
        assert!(waiting.effects.is_empty(), "approval had not run the tool");

        let mut resumed = Agent::from_journal(
            AgentId::new("agent-a").unwrap_or_else(|error| panic!("agent: {error}")),
            live.journal().clone(),
            TurnBudget::default(),
            ApprovalPolicy::default(),
        )
        .unwrap_or_else(|error| panic!("restore agent: {error:?}"));
        let recovered = resumed
            .recover_after_process_death_at(UnixMillis::new(999))
            .unwrap_or_else(|| panic!("unfinished turn was not recovered"));

        assert!(recovered.effects.is_empty());
        assert!(
            recovered
                .events
                .iter()
                .any(|event| matches!(event.event, ConversationEvent::AttentionResolved { .. }))
        );
        assert!(recovered.records.iter().any(|record| matches!(
            record,
            JournalRecord::TurnFinished { fact, .. }
                if fact.outcome == TurnOutcome::ProcessDied
                    && fact.at == TurnFinishedAt::Recovered {
                        recovery_observed_at: UnixMillis::new(999)
                    }
        )));
        assert!(recovered.events.iter().any(|event| matches!(
            event.event,
            ConversationEvent::ToolCallChanged {
                status: ToolCallStatus::Cancelled,
                ..
            }
        )));
        assert!(recovered.events.iter().any(|event| matches!(
            event.event,
            ConversationEvent::AgentStatusChanged {
                status: AgentStatus::Idle,
                ..
            }
        )));
        assert!(
            recovered
                .events
                .iter()
                .any(|event| matches!(event.event, ConversationEvent::RuntimeWarning { .. }))
        );
        let projection = resumed
            .rebuild_projection()
            .unwrap_or_else(|error| panic!("project recovered agent: {error:?}"));
        assert!(projection.recovery().is_none());
        assert!(matches!(
            context_results(&projection.request().atoms).last(),
            Some(result)
                if result.outcome() == &ToolOutcome::Cancelled {
                    reason: ToolCancellationReason::ProcessDied,
                }
        ));
        assert!(resumed.recover_after_process_death().is_none());
    }

    /// TIM-1/JRN-5: a durable atomic turn start is enough to identify process-orphaned work.
    #[test]
    fn an_atomic_turn_start_is_recovered_as_interrupted() {
        let agent_id = AgentId::new("agent-a").unwrap_or_else(|error| panic!("agent: {error}"));
        let session_id = ConversationId::new("partial-transition")
            .unwrap_or_else(|error| panic!("session: {error}"));
        let mut source = Agent::for_conversation(
            agent_id.clone(),
            crate::ConversationMetadata::new(session_id.clone(), UnixMillis::EPOCH),
            TurnBudget::default(),
            ApprovalPolicy::default(),
        );
        let announcement = source.announce("Agent A");
        let submission = submit(&mut source, "persisted before process death");
        let mut journal = ConversationJournal::new(session_id);
        journal
            .apply(announcement.records[0].clone())
            .unwrap_or_else(|error| panic!("apply announcement: {error:?}"));
        journal
            .apply(submission.records[0].clone())
            .unwrap_or_else(|error| panic!("apply user message: {error:?}"));

        let mut resumed = Agent::from_journal(
            agent_id,
            journal,
            TurnBudget::default(),
            ApprovalPolicy::default(),
        )
        .unwrap_or_else(|error| panic!("resume partial transition: {error:?}"));
        let recovered = resumed
            .recover_after_process_death()
            .unwrap_or_else(|| panic!("partial transition was mistaken for a clean session"));

        assert!(recovered.events.iter().any(|event| matches!(
            event.event,
            ConversationEvent::AgentStatusChanged {
                status: AgentStatus::Idle,
                ..
            }
        )));
        assert!(
            recovered
                .events
                .iter()
                .any(|event| matches!(event.event, ConversationEvent::RuntimeWarning { .. }))
        );
        assert!(resumed.recover_after_process_death().is_none());
    }

    /// JRN-5/JRN-7: every recovery prefix completes requested and not-yet-requested calls once,
    /// preserving the model's declaration order across repeated process deaths.
    #[test]
    fn recovery_completes_calls_declared_before_their_request_record() {
        let agent_id = AgentId::new("agent-a").unwrap_or_else(|error| panic!("agent: {error}"));
        let session_id = ConversationId::new("agent-a-session")
            .unwrap_or_else(|error| panic!("session: {error}"));
        let mut source = bare_agent();
        let mut base_records = source.announce("Agent A").records;
        base_records.extend(submit(&mut source, "inspect").records);
        call(&mut source, "crash-one");
        call(&mut source, "crash-two");
        let stopped = stop_before_admission(&mut source, StopReason::ToolCalls);
        let output_index = stopped
            .records
            .iter()
            .position(|record| {
                matches!(
                    record,
                    JournalRecord::AppendEntry { entry, .. }
                        if matches!(entry.payload, JournalEntryPayload::AssistantOutput { .. })
                )
            })
            .unwrap_or_else(|| panic!("stopped step omitted assistant output"));
        let first_requested_index = stopped
            .records
            .iter()
            .enumerate()
            .skip(output_index + 1)
            .find_map(|(index, record)| match record {
                JournalRecord::AppendEntry { entry, .. }
                    if matches!(entry.payload, JournalEntryPayload::ToolCallRequested { .. }) =>
                {
                    Some(index)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("stopped step omitted its first requested call"));

        for durable_stop in [output_index, first_requested_index] {
            let mut crash_records = base_records.clone();
            crash_records.extend(stopped.records[..=durable_stop].iter().cloned());
            let build_journal = || {
                let mut journal = ConversationJournal::new(session_id.clone());
                for record in &crash_records {
                    journal
                        .apply(record.clone())
                        .unwrap_or_else(|error| panic!("apply crash prefix: {error:?}"));
                }
                journal
            };
            let mut planned = Agent::from_journal(
                agent_id.clone(),
                build_journal(),
                TurnBudget::default(),
                ApprovalPolicy::default(),
            )
            .unwrap_or_else(|error| panic!("plan recovery: {error:?}"));
            let expected = planned
                .recover_after_process_death()
                .unwrap_or_else(|| panic!("declared calls were mistaken for clean state"))
                .records;

            for recovered_prefix in 0..=expected.len() {
                let mut journal = build_journal();
                for record in &expected[..recovered_prefix] {
                    journal.apply(record.clone()).unwrap_or_else(|error| {
                        panic!("apply recovery prefix {recovered_prefix}: {error:?}")
                    });
                }
                let mut resumed = Agent::from_journal(
                    agent_id.clone(),
                    journal,
                    TurnBudget::default(),
                    ApprovalPolicy::default(),
                )
                .unwrap_or_else(|error| panic!("resume recovery prefix: {error:?}"));
                let continued = resumed
                    .recover_after_process_death()
                    .map_or_else(Vec::new, |reaction| reaction.records);
                assert_eq!(continued, expected[recovered_prefix..]);
                let projection = resumed
                    .rebuild_projection()
                    .unwrap_or_else(|error| panic!("recovered calls must project: {error:?}"));
                assert!(projection.recovery().is_none());
                let results = context_results(&projection.request().atoms);
                assert_eq!(
                    results
                        .iter()
                        .map(|result| result.call_id().as_str())
                        .collect::<Vec<_>>(),
                    ["crash-one", "crash-two"]
                );
                assert!(results.iter().all(|result| {
                    result.outcome()
                        == &ToolOutcome::Cancelled {
                            reason: ToolCancellationReason::ProcessDied,
                        }
                }));
            }
        }
    }

    /// JRN-5: every durable prefix of recovery either resumes the same suffix or is complete.
    #[test]
    fn process_recovery_is_idempotent_across_every_record_prefix() {
        let agent_id = AgentId::new("agent-a").unwrap_or_else(|error| panic!("agent: {error}"));
        let mut live = agent();
        submit(&mut live, "change a file");
        call_named(&mut live, "write-1", "edit");
        let _requested = stop_before_admission(&mut live, StopReason::ToolCalls);
        let _waiting = live.handle(Input::ToolAdmissionResolved(AdmissionOutcome::Admitted(
            admitted("write-1", "edit", [ToolCapability::FileWrite]),
        )));
        let before_recovery = live.journal().clone();
        let mut planned = Agent::from_journal(
            agent_id.clone(),
            before_recovery.clone(),
            TurnBudget::default(),
            ApprovalPolicy::default(),
        )
        .unwrap_or_else(|error| panic!("plan recovery: {error:?}"));
        let recovery = planned
            .recover_after_process_death()
            .unwrap_or_else(|| panic!("unfinished turn was not recovered"));
        assert!(matches!(
            recovery.records.first(),
            Some(JournalRecord::AppendEntry { entry, .. })
                if matches!(entry.payload, JournalEntryPayload::TurnInterruptedByRecovery { .. })
        ));

        for prefix_len in 0..=recovery.records.len() {
            let mut prefix = before_recovery.clone();
            for record in &recovery.records[..prefix_len] {
                prefix
                    .apply(record.clone())
                    .unwrap_or_else(|error| panic!("apply recovery prefix: {error:?}"));
            }
            let mut resumed = Agent::from_journal(
                agent_id.clone(),
                prefix,
                TurnBudget::default(),
                ApprovalPolicy::default(),
            )
            .unwrap_or_else(|error| panic!("resume recovery prefix: {error:?}"));

            if prefix_len == recovery.records.len() {
                assert!(resumed.recover_after_process_death().is_none());
            } else {
                let suffix = resumed
                    .recover_after_process_death()
                    .unwrap_or_else(|| panic!("prefix {prefix_len} was mistaken for complete"));
                assert_eq!(suffix.records, recovery.records[prefix_len..]);
                assert!(resumed.recover_after_process_death().is_none());
            }

            let projection = resumed
                .rebuild_projection()
                .unwrap_or_else(|error| panic!("project recovered prefix: {error:?}"));
            assert_eq!(
                projection
                    .events()
                    .iter()
                    .filter(|event| matches!(event.event, ConversationEvent::RuntimeWarning { .. }))
                    .count(),
                1
            );
            assert!(projection.recovery().is_none());
            assert!(matches!(
                context_results(&projection.request().atoms).last(),
                Some(result)
                    if result.outcome() == &ToolOutcome::Cancelled {
                        reason: ToolCancellationReason::ProcessDied,
                    }
            ));
        }
    }
}
