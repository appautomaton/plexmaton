//! Deciding whether one semantic event belongs in the projection, and where it goes.
//!
//! The rest of this module owns what the user is looking at. This half owns the producer contract:
//! stream ordering, the typed reasons an event is refused, and the dispatch from an event kind to
//! the collection that holds it. Keeping them apart is what stops "is this event well formed" and
//! "where is the reader" from sharing one file and one set of reasons to change.

use plexmaton_core::{
    AgentId, AttentionId, AttentionRequest, ConversationEvent, ConversationEventEnvelope,
    ToolCallId, ToolCallStatus, TranscriptItemId,
};
use thiserror::Error;

use super::{AgentView, AttentionView, NoticeView, ViewState};

/// Why the projection rejected one semantic event.
///
/// The reducer validates before it writes, so a rejected event never leaves partially applied
/// state behind.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ReduceError {
    #[error("stale event sequence: expected {expected}, received {received}")]
    StaleSequence { expected: u64, received: u64 },
    #[error("agent already exists: {0}")]
    DuplicateAgent(AgentId),
    #[error("unknown agent: {0}")]
    UnknownAgent(AgentId),
    #[error("attention {attention_id} belongs to {expected}, not {received}")]
    AttentionOwnerMismatch {
        attention_id: AttentionId,
        expected: AgentId,
        received: AgentId,
    },
    #[error("transcript item already exists: {0}")]
    DuplicateTranscriptItem(TranscriptItemId),
    #[error("unknown transcript item: {0}")]
    UnknownTranscriptItem(TranscriptItemId),
    #[error("transcript item already finalized: {0}")]
    ItemAlreadyFinalized(TranscriptItemId),
    #[error("item revision gap for {item_id}: expected {expected}, received {received}")]
    ItemRevisionGap {
        item_id: TranscriptItemId,
        expected: u64,
        received: u64,
    },
    #[error("transcript entry changed kind: {0}")]
    EntryKindChanged(TranscriptItemId),
    #[error("transcript entry {item_id} belongs to {expected}, not {received}")]
    EntryOwnerMismatch {
        item_id: TranscriptItemId,
        expected: AgentId,
        received: AgentId,
    },
    #[error("tool correlation changed for transcript entry: {0}")]
    ToolCorrelationChanged(TranscriptItemId),
    #[error("tool call already has a transcript entry: {0}")]
    DuplicateToolCall(ToolCallId),
    #[error("invalid tool transition for {call_id}: {from:?} -> {to:?}")]
    InvalidToolTransition {
        call_id: ToolCallId,
        from: ToolCallStatus,
        to: ToolCallStatus,
    },
}

/// Result of offering one semantic event to the projection.
///
/// Callers may ignore this value: every rejection is also recorded as a visible notice, so a
/// producer defect degrades the display instead of terminating the workspace.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApplyOutcome {
    /// The event satisfied the projection contract and was applied.
    Accepted,
    /// The event was dropped; the reason is retained in the notice log.
    Rejected(ReduceError),
}

impl ViewState {
    /// Offers one ordered semantic event to the projection.
    ///
    /// Ordering defects are recoverable rather than fatal: a forward gap resynchronizes and
    /// records what was lost, a stale sequence is dropped without rewinding, and a contract
    /// violation drops exactly one event. The offered sequence is consumed even when its content
    /// is rejected, so one defective event cannot make every later event look like a gap. This
    /// keeps the workspace interactive when a producer misbehaves, which returning an error to a
    /// caller that exits would not.
    pub fn apply(&mut self, envelope: ConversationEventEnvelope) -> ApplyOutcome {
        let sequence = envelope.sequence;
        let expected = self.last_sequence.map_or(1, |last| last.get() + 1);
        let received = sequence.get();

        if received < expected {
            let error = ReduceError::StaleSequence { expected, received };
            self.push_notice(NoticeView::Rejected {
                sequence,
                error: error.clone(),
            });
            return ApplyOutcome::Rejected(error);
        }
        if received > expected {
            self.push_notice(NoticeView::SequenceGap { expected, received });
        }

        let outcome = match self.apply_event(envelope.event) {
            Ok(changed) => {
                // Accepted is not the same as changed. Re-sending an agent's current status is a
                // report rather than a transition, and FR-1 says it costs no frame at all. Entry
                // updates instead carry exact revisions and reject repeats (ENT-2).
                if changed {
                    self.reconcile_command_inspection();
                    self.touch();
                }
                ApplyOutcome::Accepted
            }
            Err(error) => {
                self.push_notice(NoticeView::Rejected {
                    sequence,
                    error: error.clone(),
                });
                ApplyOutcome::Rejected(error)
            }
        };
        self.last_sequence = Some(sequence);
        outcome
    }

    /// Applies one accepted event, reporting whether it changed anything the user can see.
    ///
    /// Three arms always report a change even though the change may be invisible in the frame that
    /// follows. A transcript delta bumps the item's revision, which is the wrapping cache's key, so
    /// even an empty one invalidates a measured height; finalizing closes the item to further
    /// deltas; and starting one adds an item. Each is state a later frame reads, which is the test
    /// FR-1 actually asks — not whether a glyph moved.
    fn apply_event(&mut self, event: ConversationEvent) -> Result<bool, ReduceError> {
        let changed = match event {
            ConversationEvent::AgentCreated {
                agent_id,
                label,
                status,
            } => {
                self.agents.add(agent_id, label, status)?;
                true
            }
            ConversationEvent::AgentStatusChanged { agent_id, status } => {
                let agent = self.agent_mut(&agent_id)?;
                let moved = agent.status != status;
                agent.status = status;
                moved
            }
            ConversationEvent::TurnUsageUpdated {
                agent_id,
                turn_id,
                usage,
            } => {
                self.agent_mut(&agent_id)?.set_usage(turn_id, usage);
                // Usage is retained for the on-demand diagnostics surface. Until that surface
                // exists it paints no cell, so FR-1 charges it no revision or frame.
                false
            }
            ConversationEvent::TranscriptItemStarted {
                agent_id,
                item_id,
                role,
            } => {
                self.validate_entry_owner(&agent_id, &item_id)?;
                self.agent_mut(&agent_id)?
                    .start_item(item_id.clone(), role)?;
                self.remember_entry_owner(item_id, agent_id);
                true
            }
            ConversationEvent::TranscriptDelta {
                agent_id,
                item_id,
                item_revision,
                text,
            } => {
                self.validate_entry_owner(&agent_id, &item_id)?;
                self.agent_mut(&agent_id)?
                    .append_delta(&item_id, item_revision, &text)?;
                true
            }
            ConversationEvent::TranscriptItemFinalized {
                agent_id,
                item_id,
                item_revision,
            } => {
                self.validate_entry_owner(&agent_id, &item_id)?;
                self.agent_mut(&agent_id)?
                    .finalize_item(&item_id, item_revision)?;
                true
            }
            ConversationEvent::ToolCallChanged {
                agent_id,
                item_id,
                item_revision,
                call_id,
                label,
                status,
                presentation,
            } => self.apply_entry(agent_id, item_id, |agent, item_id| {
                agent.set_tool_entry(item_id, item_revision, call_id, label, status, presentation)
            })?,
            ConversationEvent::ServerToolCalled {
                agent_id,
                item_id,
                call,
            } => self.apply_entry(agent_id, item_id, |agent, item_id| {
                agent.note_server_tool(item_id, call)
            })?,
            ConversationEvent::AttentionRequested {
                agent_id,
                attention_id,
                request,
            } => {
                if !self.agents.contains(&agent_id) {
                    return Err(ReduceError::UnknownAgent(agent_id));
                }
                // The queue is what the user is not looking at. A request from the agent whose
                // conversation fills the screen is not a background interruption to be announced
                // and then travelled to: it opens where the composer is, in the box of the
                // conversation that asked (ui-ux §input). The record still enters the queue,
                // because that is where its resolution finds it (ATT-3).
                let answer_here = matches!(request, AttentionRequest::Approval { .. })
                    && self
                        .agents
                        .primary()
                        .is_some_and(|primary| primary.id == agent_id);
                let queued = self.attention.request(AttentionView {
                    id: attention_id,
                    agent_id,
                    request,
                    // A producer cannot deliver an already-seen request, and a repeat of one the
                    // user had seen is a fresh ask (ATT-3).
                    acknowledged: false,
                });
                let opened = answer_here && self.open_next_primary_approval();
                queued || opened
            }
            ConversationEvent::AttentionResolved {
                agent_id,
                attention_id,
            } => {
                if !self.agents.contains(&agent_id) {
                    return Err(ReduceError::UnknownAgent(agent_id));
                }
                if let Some(item) = self.attention.get(&attention_id)
                    && item.agent_id != agent_id
                {
                    return Err(ReduceError::AttentionOwnerMismatch {
                        attention_id,
                        expected: item.agent_id.clone(),
                        received: agent_id,
                    });
                }
                let return_focus = self.approval.resolved(&attention_id);
                let removed = self.attention.resolve(&attention_id);
                let advanced = self.open_next_primary_approval();
                let restored =
                    !advanced && return_focus.is_some_and(|surface| self.focus.prefer(surface));
                removed || restored || advanced
            }
            event @ (ConversationEvent::TaskAssigned { .. }
            | ConversationEvent::MailDelivered { .. }) => self.apply_addressed(event)?,
            ConversationEvent::HandoffCompleted {
                agent_id,
                item_id,
                child,
            } => self.apply_handoff(agent_id, item_id, child)?,
            ConversationEvent::ArtifactAnnounced {
                agent_id,
                item_id,
                artifact_id,
                label,
                pointer,
            } => self.apply_artifact(agent_id, item_id, artifact_id, label, pointer)?,
            ConversationEvent::RuntimeWarning {
                agent_id,
                item_id,
                message,
            } => self.apply_runtime_message(agent_id, item_id, message, false)?,
            ConversationEvent::RuntimeError {
                agent_id,
                item_id,
                message,
            } => self.apply_runtime_message(agent_id, item_id, message, true)?,
        };
        Ok(changed)
    }

    /// Files one side of a letter into the conversation that owns that side.
    ///
    /// Both endpoints must exist, because a letter names two conversations and an item addressed to
    /// a conversation this projection has never heard of is a gap, not a delivery.
    fn apply_mail(
        &mut self,
        agent_id: AgentId,
        item_id: TranscriptItemId,
        mail_id: plexmaton_core::MailId,
        from: AgentId,
        to: AgentId,
        summary: String,
    ) -> Result<bool, ReduceError> {
        let counterpart = self.addressed_counterpart(&agent_id, &from, &to)?;
        self.validate_entry_owner(&agent_id, &item_id)?;
        let changed = self.agent_mut(&agent_id)?.deliver_mail(
            item_id.clone(),
            mail_id,
            from,
            to,
            counterpart,
            summary,
        )?;
        self.remember_entry_owner(item_id, agent_id);
        Ok(changed)
    }

    /// Files one side of whatever one session addressed to another.
    ///
    /// Mail and a task share this arm because they share every rule that matters here: both name
    /// two conversations, both are announced once per side, and both refuse an endpoint this
    /// projection has never heard of.
    fn apply_addressed(&mut self, event: ConversationEvent) -> Result<bool, ReduceError> {
        match event {
            ConversationEvent::TaskAssigned {
                agent_id,
                item_id,
                from,
                to,
                task,
            } => self.apply_task(agent_id, item_id, from, to, task),
            ConversationEvent::MailDelivered {
                agent_id,
                item_id,
                mail_id,
                from,
                to,
                summary,
            } => self.apply_mail(agent_id, item_id, mail_id, from, to, summary),
            _ => unreachable!("only addressed events reach this arm"),
        }
    }

    /// Files one side of an assigned task, under the same rule mail follows.
    fn apply_task(
        &mut self,
        agent_id: AgentId,
        item_id: TranscriptItemId,
        from: AgentId,
        to: AgentId,
        task: String,
    ) -> Result<bool, ReduceError> {
        let counterpart = self.addressed_counterpart(&agent_id, &from, &to)?;
        self.validate_entry_owner(&agent_id, &item_id)?;
        let changed =
            self.agent_mut(&agent_id)?
                .assign_task(item_id.clone(), from, to, counterpart, task)?;
        self.remember_entry_owner(item_id, agent_id);
        Ok(changed)
    }

    fn addressed_counterpart(
        &self,
        owner: &AgentId,
        from: &AgentId,
        to: &AgentId,
    ) -> Result<String, ReduceError> {
        for endpoint in [from, to] {
            if !self.agents.contains(endpoint) {
                return Err(ReduceError::UnknownAgent(endpoint.clone()));
            }
        }
        let counterpart = if owner == to { from } else { to };
        Ok(self
            .agents
            .get(counterpart)
            .expect("validated addressed endpoint remains present")
            .label
            .clone())
    }

    fn apply_handoff(
        &mut self,
        agent_id: AgentId,
        item_id: TranscriptItemId,
        child: AgentId,
    ) -> Result<bool, ReduceError> {
        for endpoint in [&agent_id, &child] {
            if !self.agents.contains(endpoint) {
                return Err(ReduceError::UnknownAgent(endpoint.clone()));
            }
        }
        self.validate_entry_owner(&agent_id, &item_id)?;
        let changed = self.agent_mut(&agent_id)?.complete_handoff(
            item_id.clone(),
            agent_id.clone(),
            child,
        )?;
        self.remember_entry_owner(item_id, agent_id);
        Ok(changed)
    }

    /// One entry-bearing fact, in the shape every kind shares: the entry belongs to the agent, the
    /// agent's projection files it, and the owner is remembered for the revisions to come.
    fn apply_entry(
        &mut self,
        agent_id: AgentId,
        item_id: TranscriptItemId,
        file: impl FnOnce(&mut AgentView, TranscriptItemId) -> Result<bool, ReduceError>,
    ) -> Result<bool, ReduceError> {
        self.validate_entry_owner(&agent_id, &item_id)?;
        let changed = file(self.agent_mut(&agent_id)?, item_id.clone())?;
        self.remember_entry_owner(item_id, agent_id);
        Ok(changed)
    }

    fn apply_artifact(
        &mut self,
        agent_id: AgentId,
        item_id: TranscriptItemId,
        artifact_id: plexmaton_core::ArtifactId,
        label: String,
        pointer: String,
    ) -> Result<bool, ReduceError> {
        self.validate_entry_owner(&agent_id, &item_id)?;
        let changed = self.agent_mut(&agent_id)?.announce_artifact(
            item_id.clone(),
            artifact_id,
            label,
            pointer,
        )?;
        self.remember_entry_owner(item_id, agent_id);
        Ok(changed)
    }

    fn apply_runtime_message(
        &mut self,
        agent_id: AgentId,
        item_id: TranscriptItemId,
        message: String,
        error: bool,
    ) -> Result<bool, ReduceError> {
        self.validate_entry_owner(&agent_id, &item_id)?;
        let changed =
            self.agent_mut(&agent_id)?
                .runtime_message(item_id.clone(), message, error)?;
        self.remember_entry_owner(item_id, agent_id);
        Ok(changed)
    }

    fn push_notice(&mut self, notice: NoticeView) {
        self.notices.push(notice);
        // A notice is visible, so recording one is a change the renderer has to repaint for.
        self.touch();
    }

    fn validate_entry_owner(
        &self,
        received: &AgentId,
        item_id: &TranscriptItemId,
    ) -> Result<(), ReduceError> {
        if let Some(expected) = self.entry_owners.get(item_id)
            && expected != received
        {
            return Err(ReduceError::EntryOwnerMismatch {
                item_id: item_id.clone(),
                expected: expected.clone(),
                received: received.clone(),
            });
        }
        Ok(())
    }

    fn remember_entry_owner(&mut self, item_id: TranscriptItemId, agent_id: AgentId) {
        self.entry_owners.entry(item_id).or_insert(agent_id);
    }

    fn agent_mut(&mut self, agent_id: &AgentId) -> Result<&mut AgentView, ReduceError> {
        self.agents.get_mut(agent_id)
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{
        AgentId, AgentStatus, ArtifactId, ConversationEvent, ConversationEventEnvelope,
        EventSequence, MailId, TokenCounts, TokenUsage, ToolCallId, ToolCallStatus,
        ToolPresentation, TranscriptItemId, TranscriptRole, TurnId,
    };

    use super::{ApplyOutcome, ReduceError};
    use crate::{
        NoticeView, TranscriptEntryView, TranscriptTextKind, ViewState,
        state::CurrentWork,
        test_support::{canonical_runtime, canonical_state},
    };

    fn agent_id(value: &str) -> AgentId {
        AgentId::new(value).unwrap_or_else(|error| panic!("invalid fixture: {error}"))
    }

    fn envelope(sequence: u64, event: ConversationEvent) -> ConversationEventEnvelope {
        ConversationEventEnvelope {
            sequence: EventSequence::new(sequence),
            event,
        }
    }

    fn created(id: &str) -> ConversationEvent {
        ConversationEvent::AgentCreated {
            agent_id: agent_id(id),
            label: id.to_owned(),
            status: AgentStatus::Running,
        }
    }

    fn item_id(value: &str) -> TranscriptItemId {
        TranscriptItemId::new(value).unwrap_or_else(|error| panic!("invalid fixture: {error}"))
    }

    fn tool_event(
        entry: &str,
        call: &str,
        revision: u64,
        status: ToolCallStatus,
    ) -> ConversationEvent {
        ConversationEvent::ToolCallChanged {
            agent_id: agent_id("agent-a"),
            item_id: item_id(entry),
            item_revision: revision,
            call_id: ToolCallId::new(call)
                .unwrap_or_else(|error| panic!("invalid fixture: {error}")),
            label: call.to_owned(),
            status,
            presentation: ToolPresentation::default(),
        }
    }

    fn server_tool_event(agent: &AgentId, entry: &str, queries: &[&str]) -> ConversationEvent {
        ConversationEvent::ServerToolCalled {
            agent_id: agent.clone(),
            item_id: item_id(entry),
            call: plexmaton_core::ServerToolCall {
                tool: plexmaton_core::ServerTool::WebSearch,
                action: plexmaton_core::ServerToolAction::Search {
                    queries: queries.iter().map(|query| (*query).to_owned()).collect(),
                },
                status: plexmaton_core::ServerToolStatus::Completed,
            },
        }
    }

    /// ENT-2: a call the provider ran appears once, finished, and a second report of the same entry
    /// is a duplicate the notice log keeps, never an update to the row.
    #[test]
    fn a_server_tool_call_appears_finished_and_never_transitions() {
        let mut state = ViewState::default();
        let agent = agent_id("agent-a");
        state.apply(envelope(1, created("agent-a")));
        assert_eq!(
            state.apply(envelope(2, server_tool_event(&agent, "search", &["rust"]))),
            ApplyOutcome::Accepted
        );
        let outcome = state.apply(envelope(3, server_tool_event(&agent, "search", &["again"])));
        assert!(
            matches!(
                outcome,
                ApplyOutcome::Rejected(ReduceError::DuplicateTranscriptItem(ref item)) if item == &item_id("search")
            ),
            "{outcome:?}"
        );
        let entries: Vec<_> = state
            .primary_agent()
            .unwrap_or_else(|| panic!("agent was projected"))
            .entries()
            .collect();
        assert!(
            matches!(
                entries.as_slice(),
                [TranscriptEntryView::ServerTool(view)]
                    if view.revision == 0
                        && view.call.action == plexmaton_core::ServerToolAction::Search { queries: vec!["rust".to_owned()] }
            ),
            "{entries:?}"
        );
    }

    #[test]
    fn every_step_of_the_canonical_scenario_is_accepted() {
        // The shared fixture ignores the outcome so a degraded projection is still constructible.
        // Somebody has to assert that the canonical timeline itself has no producer defect in it,
        // or every test built on it would be testing against a silently broken baseline.
        let mut state = ViewState::default();
        for envelope in canonical_runtime().ready(u64::MAX) {
            assert_eq!(state.apply(envelope), ApplyOutcome::Accepted);
        }
        assert_eq!(state.notices().count(), 0);
    }

    /// One letter reaches two conversations, and both of them name the session that wrote it.
    ///
    /// The recipient's side is the whole point: without it a delegating agent answers a question
    /// the user can see no trace of having been asked.
    #[test]
    fn a_letter_reaches_both_conversations_attributed_to_its_sender() {
        let state = canonical_state();
        let sender = state
            .agent(&agent_id("agent-b"))
            .unwrap_or_else(|| panic!("canonical scenario creates the sender"));
        let recipient = state
            .agent(&agent_id("agent-a"))
            .unwrap_or_else(|| panic!("canonical scenario creates the recipient"));
        let sent: Vec<_> = sender.mail().collect();
        let arrived: Vec<_> = recipient.mail().collect();

        assert_eq!(sent.len(), 1);
        assert_eq!(arrived.len(), 1);
        for letter in [sent[0], arrived[0]] {
            assert_eq!(letter.from.as_str(), "agent-b");
            assert_eq!(letter.to.as_str(), "agent-a");
        }
        assert_eq!(sent[0].counterpart, "Agent A · primary");
        assert_eq!(arrived[0].counterpart, "Agent B · UI study");
        assert_eq!(sent[0].id, arrived[0].id, "one letter, one mail identity");
        assert_ne!(
            sent[0].entry_id, arrived[0].entry_id,
            "an item belongs to one conversation, so each side is its own item"
        );
        assert_eq!(
            recipient.mail().count(),
            1,
            "the recipient holds its own item, not the sender's"
        );
    }

    /// Stage 3 entry spine: domain facts share one order without losing their typed payloads.
    #[test]
    fn every_transcript_category_enters_one_ordered_projection() {
        let mut state = ViewState::default();
        let agent = agent_id("agent-a");
        state.apply(envelope(1, created("agent-a")));
        state.apply(envelope(2, created("agent-b")));
        state.apply(envelope(
            3,
            ConversationEvent::TranscriptItemStarted {
                agent_id: agent.clone(),
                item_id: item_id("text"),
                role: TranscriptRole::System,
            },
        ));
        state.apply(envelope(
            4,
            tool_event("tool", "tool-1", 0, ToolCallStatus::Queued),
        ));
        state.apply(envelope(
            5,
            ConversationEvent::ArtifactAnnounced {
                agent_id: agent.clone(),
                item_id: item_id("artifact"),
                artifact_id: ArtifactId::new("artifact-1")
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                label: "patch".to_owned(),
                pointer: "artifact://patch".to_owned(),
            },
        ));
        state.apply(envelope(
            6,
            ConversationEvent::MailDelivered {
                agent_id: agent.clone(),
                item_id: item_id("mail"),
                mail_id: MailId::new("mail-1").unwrap_or_else(|error| panic!("fixture: {error}")),
                from: agent.clone(),
                to: agent_id("agent-b"),
                summary: "findings".to_owned(),
            },
        ));
        state.apply(envelope(
            7,
            ConversationEvent::RuntimeWarning {
                agent_id: agent.clone(),
                item_id: item_id("warning"),
                message: "degraded".to_owned(),
            },
        ));
        state.apply(envelope(
            8,
            ConversationEvent::RuntimeError {
                agent_id: agent.clone(),
                item_id: item_id("error"),
                message: "failed".to_owned(),
            },
        ));
        state.apply(envelope(9, server_tool_event(&agent, "search", &["rust"])));

        let entries: Vec<_> = state
            .primary_agent()
            .unwrap_or_else(|| panic!("agent was projected"))
            .entries()
            .map(|entry| match entry {
                TranscriptEntryView::Text(item) => match item.kind {
                    TranscriptTextKind::Message => "text",
                    TranscriptTextKind::Warning => "warning",
                    TranscriptTextKind::Error => "error",
                },
                TranscriptEntryView::Tool(_) => "tool",
                TranscriptEntryView::ServerTool(_) => "server_tool",
                TranscriptEntryView::Artifact(_) => "artifact",
                TranscriptEntryView::Mail(_) => "mail",
                TranscriptEntryView::Task(_) => "task",
                TranscriptEntryView::Handoff(_) => "handoff",
            })
            .collect();
        assert_eq!(
            entries,
            [
                "text",
                "tool",
                "artifact",
                "mail",
                "warning",
                "error",
                "server_tool"
            ]
        );
        assert_eq!(
            state.notices().count(),
            0,
            "semantic entries are not defects"
        );
    }

    /// ENT-3: replaying the same envelopes is a pure reduction with no hidden projection state.
    #[test]
    fn two_fresh_projections_of_the_same_envelopes_are_equal() {
        let mut runtime = canonical_runtime();
        let envelopes = runtime.ready(u64::MAX);
        let mut first = ViewState::default();
        let mut second = ViewState::default();

        for envelope in envelopes {
            assert_eq!(first.apply(envelope.clone()), ApplyOutcome::Accepted);
            assert_eq!(second.apply(envelope), ApplyOutcome::Accepted);
        }

        assert_eq!(first, second);
    }

    /// Completion order changes state, never the stable positions established by model order.
    #[test]
    fn shuffled_tool_completions_update_their_original_entries() {
        let mut state = ViewState::default();
        state.apply(envelope(1, created("agent-a")));
        let events = [
            tool_event("entry-a", "call-a", 0, ToolCallStatus::Queued),
            tool_event("entry-b", "call-b", 0, ToolCallStatus::Queued),
            tool_event("entry-a", "call-a", 1, ToolCallStatus::Running),
            tool_event("entry-b", "call-b", 1, ToolCallStatus::Running),
            tool_event("entry-b", "call-b", 2, ToolCallStatus::Succeeded),
            tool_event("entry-a", "call-a", 2, ToolCallStatus::Succeeded),
        ];
        for (index, event) in events.into_iter().enumerate() {
            assert_eq!(
                state.apply(envelope(index as u64 + 2, event)),
                ApplyOutcome::Accepted
            );
        }

        let entries: Vec<_> = state
            .primary_agent()
            .unwrap_or_else(|| panic!("agent was projected"))
            .entries()
            .filter_map(|entry| match entry {
                TranscriptEntryView::Tool(tool) => {
                    Some((tool.entry_id.as_str(), tool.id.as_str(), tool.status))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            entries,
            [
                ("entry-a", "call-a", ToolCallStatus::Succeeded),
                ("entry-b", "call-b", ToolCallStatus::Succeeded),
            ]
        );
    }

    #[test]
    fn sequence_gap_resynchronizes_and_records_a_notice() {
        let mut state = ViewState::default();

        assert_eq!(
            state.apply(envelope(1, created("agent-a"))),
            ApplyOutcome::Accepted
        );
        assert_eq!(
            state.apply(envelope(4, created("agent-b"))),
            ApplyOutcome::Accepted
        );

        assert_eq!(state.agents().count(), 2);
        assert!(matches!(
            state.notices().next(),
            Some(NoticeView::SequenceGap {
                expected: 2,
                received: 4
            })
        ));
    }

    #[test]
    fn stale_sequence_is_rejected_without_rewinding() {
        let mut state = ViewState::default();
        state.apply(envelope(1, created("agent-a")));
        state.apply(envelope(2, created("agent-b")));

        let outcome = state.apply(envelope(2, created("agent-c")));

        assert_eq!(
            outcome,
            ApplyOutcome::Rejected(ReduceError::StaleSequence {
                expected: 3,
                received: 2
            })
        );
        assert_eq!(state.agents().count(), 2);
        // The next in-order event must still be accepted, proving the rewind did not happen.
        assert_eq!(
            state.apply(envelope(3, created("agent-c"))),
            ApplyOutcome::Accepted
        );
    }

    /// ENT-1: an entry identity fixes its owner as well as its position.
    #[test]
    fn an_entry_identity_cannot_move_between_agents() {
        let mut state = ViewState::default();
        state.apply(envelope(1, created("agent-a")));
        state.apply(envelope(2, created("agent-b")));
        state.apply(envelope(
            3,
            ConversationEvent::TranscriptItemStarted {
                agent_id: agent_id("agent-a"),
                item_id: item_id("shared"),
                role: TranscriptRole::Assistant,
            },
        ));

        assert!(matches!(
            state.apply(envelope(
                4,
                ConversationEvent::TranscriptDelta {
                    agent_id: agent_id("agent-b"),
                    item_id: item_id("shared"),
                    item_revision: 1,
                    text: "wrong owner".to_owned(),
                }
            )),
            ApplyOutcome::Rejected(ReduceError::EntryOwnerMismatch { .. })
        ));
        let source = state
            .agent(&agent_id("agent-a"))
            .and_then(|agent| agent.transcript().next())
            .map(|item| item.source.as_str());
        assert_eq!(source, Some(""));
    }

    #[test]
    fn rejected_event_does_not_block_the_rest_of_the_stream() {
        let mut state = ViewState::default();
        state.apply(envelope(1, created("agent-a")));

        let outcome = state.apply(envelope(2, created("agent-a")));

        assert!(matches!(
            outcome,
            ApplyOutcome::Rejected(ReduceError::DuplicateAgent(_))
        ));
        assert_eq!(
            state.apply(envelope(3, created("agent-b"))),
            ApplyOutcome::Accepted
        );
        assert_eq!(state.agents().count(), 2);
        assert_eq!(state.notices().count(), 1);
    }

    #[test]
    fn revision_advances_on_visible_change_and_holds_on_a_no_op() {
        let mut state = ViewState::default();
        let empty = state.revision();

        state.apply(envelope(1, created("agent-a")));
        let after_apply = state.revision();
        assert!(after_apply > empty);

        // Re-selecting the already selected agent changes nothing the user can see.
        state
            .select_agent(&agent_id("agent-a"))
            .unwrap_or_else(|error| panic!("agent exists: {error}"));
        assert_eq!(state.revision(), after_apply);

        state.apply(envelope(2, created("agent-b")));
        state
            .select_agent(&agent_id("agent-b"))
            .unwrap_or_else(|error| panic!("agent exists: {error}"));
        assert!(state.revision() > after_apply);
    }

    /// FR-1: producer traffic that alters nothing visible costs no frame at all.
    #[test]
    fn a_repeated_agent_status_costs_no_frame() {
        let mut state = ViewState::default();
        state.apply(envelope(1, created("agent-a")));
        let quiet = state.revision();
        assert_eq!(state.current_work(), Some(CurrentWork::Thinking));

        assert_eq!(
            state.apply(envelope(
                2,
                ConversationEvent::AgentStatusChanged {
                    agent_id: agent_id("agent-a"),
                    status: AgentStatus::Running,
                }
            )),
            ApplyOutcome::Accepted,
            "the event is well formed, so it is accepted; what it is not is a change"
        );
        assert_eq!(state.revision(), quiet);
        assert_eq!(
            state.current_work(),
            Some(CurrentWork::Thinking),
            "the fact painted in the composer did not change either"
        );

        state.apply(envelope(
            3,
            ConversationEvent::AgentStatusChanged {
                agent_id: agent_id("agent-a"),
                status: AgentStatus::Waiting,
            },
        ));
        assert!(state.revision() > quiet);
    }

    /// A replay can only advance a tool entry one revision along its declared lifecycle.
    #[test]
    fn tool_updates_refuse_revision_gaps_and_invalid_transitions() {
        let mut state = ViewState::default();
        state.apply(envelope(1, created("agent-a")));

        assert_eq!(
            state.apply(envelope(
                2,
                tool_event("entry-1", "tool-1", 0, ToolCallStatus::Queued)
            )),
            ApplyOutcome::Accepted
        );
        assert!(matches!(
            state.apply(envelope(
                3,
                tool_event("entry-1", "tool-1", 2, ToolCallStatus::Running)
            )),
            ApplyOutcome::Rejected(ReduceError::ItemRevisionGap { expected: 1, .. })
        ));
        assert!(matches!(
            state.apply(envelope(
                4,
                tool_event("entry-1", "tool-1", 1, ToolCallStatus::Succeeded)
            )),
            ApplyOutcome::Rejected(ReduceError::InvalidToolTransition { .. })
        ));
        assert!(matches!(
            state.apply(envelope(
                5,
                tool_event("entry-1", "tool-other", 1, ToolCallStatus::Running)
            )),
            ApplyOutcome::Rejected(ReduceError::ToolCorrelationChanged(_))
        ));
        assert!(matches!(
            state.apply(envelope(
                6,
                tool_event("entry-other", "tool-1", 0, ToolCallStatus::Queued)
            )),
            ApplyOutcome::Rejected(ReduceError::DuplicateToolCall(_))
        ));
        assert_eq!(
            state.apply(envelope(
                7,
                tool_event("entry-1", "tool-1", 1, ToolCallStatus::Running)
            )),
            ApplyOutcome::Accepted
        );
        assert_eq!(
            state.apply(envelope(
                8,
                tool_event("entry-1", "tool-1", 2, ToolCallStatus::Succeeded)
            )),
            ApplyOutcome::Accepted
        );
        let stored = state
            .primary_agent()
            .and_then(|agent| agent.tools().next())
            .unwrap_or_else(|| panic!("tool entry was projected"));
        assert_eq!(stored.revision, 2);
        assert_eq!(stored.status, ToolCallStatus::Succeeded);
    }

    /// LIVE-4 and FR-1: reported turn usage is available to a later diagnostics surface, but the
    /// user's choice to defer that surface means the update paints no permanent chrome today.
    #[test]
    fn usage_is_retained_without_charging_an_invisible_frame() {
        let mut state = ViewState::default();
        state.apply(envelope(1, created("agent-a")));
        let before = state.revision();
        let turn_id = TurnId::new("turn-1").unwrap_or_else(|error| panic!("fixture turn: {error}"));

        assert_eq!(
            state.apply(envelope(
                2,
                ConversationEvent::TurnUsageUpdated {
                    agent_id: agent_id("agent-a"),
                    turn_id: turn_id.clone(),
                    usage: TokenUsage::Partial(TokenCounts {
                        input: 10,
                        cached_input: Some(2),
                        cache_write_input: None,
                        output: 4,
                        reasoning_output: Some(3),
                        total: 14,
                    }),
                }
            )),
            ApplyOutcome::Accepted
        );
        assert_eq!(state.revision(), before);
        assert!(matches!(
            state
                .agent(&agent_id("agent-a"))
                .and_then(|agent| agent.usage()),
            Some((stored_turn, TokenUsage::Partial(counts)))
                if stored_turn == &turn_id && counts.total == 14
        ));
    }

    #[test]
    fn rejection_advances_the_revision_because_the_notice_is_visible() {
        let mut state = ViewState::default();
        state.apply(envelope(1, created("agent-a")));
        let before = state.revision();

        state.apply(envelope(2, created("agent-a")));

        assert!(state.revision() > before);
    }
}
