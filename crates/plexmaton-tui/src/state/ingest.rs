//! Deciding whether one semantic event belongs in the projection, and where it goes.
//!
//! The rest of this module owns what the user is looking at. This half owns the producer contract:
//! stream ordering, the typed reasons an event is refused, and the dispatch from an event kind to
//! the collection that holds it. Keeping them apart is what stops "is this event well formed" and
//! "where is the reader" from sharing one file and one set of reasons to change.

use plexmaton_core::{
    AgentId, AttentionId, AttentionRequest, SessionEvent, SessionEventEnvelope, ToolCallId,
    ToolCallStatus, TranscriptItemId,
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
    pub fn apply(&mut self, envelope: SessionEventEnvelope) -> ApplyOutcome {
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
    fn apply_event(&mut self, event: SessionEvent) -> Result<bool, ReduceError> {
        let changed = match event {
            SessionEvent::AgentCreated {
                agent_id,
                label,
                status,
            } => {
                self.agents.add(agent_id, label, status)?;
                true
            }
            SessionEvent::AgentStatusChanged { agent_id, status } => {
                let agent = self.agent_mut(&agent_id)?;
                let moved = agent.status != status;
                agent.status = status;
                moved
            }
            SessionEvent::TurnUsageUpdated {
                agent_id,
                turn_id,
                usage,
            } => {
                self.agent_mut(&agent_id)?.set_usage(turn_id, usage);
                // Usage is retained for the on-demand diagnostics surface. Until that surface
                // exists it paints no cell, so FR-1 charges it no revision or frame.
                false
            }
            SessionEvent::TranscriptItemStarted {
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
            SessionEvent::TranscriptDelta {
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
            SessionEvent::TranscriptItemFinalized {
                agent_id,
                item_id,
                item_revision,
            } => {
                self.validate_entry_owner(&agent_id, &item_id)?;
                self.agent_mut(&agent_id)?
                    .finalize_item(&item_id, item_revision)?;
                true
            }
            SessionEvent::ToolCallChanged {
                agent_id,
                item_id,
                item_revision,
                call_id,
                label,
                status,
                presentation,
            } => {
                self.validate_entry_owner(&agent_id, &item_id)?;
                let changed = self.agent_mut(&agent_id)?.set_tool_entry(
                    item_id.clone(),
                    item_revision,
                    call_id,
                    label,
                    status,
                    presentation,
                )?;
                self.remember_entry_owner(item_id, agent_id);
                changed
            }
            SessionEvent::AttentionRequested {
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
            SessionEvent::AttentionResolved {
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
                let advanced = return_focus.is_some() && self.open_next_primary_approval();
                let restored =
                    !advanced && return_focus.is_some_and(|surface| self.focus.prefer(surface));
                removed || restored || advanced
            }
            SessionEvent::MailDelivered {
                item_id,
                mail_id,
                from,
                to,
                summary,
            } => self.apply_mail(item_id, mail_id, from, to, summary)?,
            SessionEvent::ArtifactAnnounced {
                agent_id,
                item_id,
                artifact_id,
                label,
                pointer,
            } => self.apply_artifact(agent_id, item_id, artifact_id, label, pointer)?,
            SessionEvent::RuntimeWarning {
                agent_id,
                item_id,
                message,
            } => self.apply_runtime_message(agent_id, item_id, message, false)?,
            SessionEvent::RuntimeError {
                agent_id,
                item_id,
                message,
            } => self.apply_runtime_message(agent_id, item_id, message, true)?,
        };
        Ok(changed)
    }

    fn apply_mail(
        &mut self,
        item_id: TranscriptItemId,
        mail_id: plexmaton_core::MailId,
        from: AgentId,
        to: AgentId,
        summary: String,
    ) -> Result<bool, ReduceError> {
        if !self.agents.contains(&to) {
            return Err(ReduceError::UnknownAgent(to));
        }
        self.validate_entry_owner(&from, &item_id)?;
        let changed = self.agent_mut(&from)?.deliver_mail(
            item_id.clone(),
            mail_id,
            from.clone(),
            to,
            summary,
        )?;
        self.remember_entry_owner(item_id, from);
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
        AgentId, AgentStatus, ArtifactId, EventSequence, MailId, SessionEvent,
        SessionEventEnvelope, TokenCounts, TokenUsage, ToolCallId, ToolCallStatus,
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

    fn envelope(sequence: u64, event: SessionEvent) -> SessionEventEnvelope {
        SessionEventEnvelope {
            sequence: EventSequence::new(sequence),
            event,
        }
    }

    fn created(id: &str) -> SessionEvent {
        SessionEvent::AgentCreated {
            agent_id: agent_id(id),
            label: id.to_owned(),
            status: AgentStatus::Running,
        }
    }

    fn item_id(value: &str) -> TranscriptItemId {
        TranscriptItemId::new(value).unwrap_or_else(|error| panic!("invalid fixture: {error}"))
    }

    fn tool_event(entry: &str, call: &str, revision: u64, status: ToolCallStatus) -> SessionEvent {
        SessionEvent::ToolCallChanged {
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

    #[test]
    fn mail_retains_both_endpoints_and_lives_with_its_producer() {
        let state = canonical_state();
        let producer = state
            .agent(&agent_id("agent-b"))
            .unwrap_or_else(|| panic!("canonical scenario creates the sender"));
        let mail: Vec<_> = producer.mail().collect();

        assert_eq!(mail.len(), 1);
        assert_eq!(mail[0].from.as_str(), "agent-b");
        assert_eq!(mail[0].to.as_str(), "agent-a");
        assert_eq!(
            state
                .primary_agent()
                .map_or(0, |agent| agent.mail().count()),
            0,
            "delivery does not move the sender's entry into the recipient transcript"
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
            SessionEvent::TranscriptItemStarted {
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
            SessionEvent::ArtifactAnnounced {
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
            SessionEvent::MailDelivered {
                item_id: item_id("mail"),
                mail_id: MailId::new("mail-1").unwrap_or_else(|error| panic!("fixture: {error}")),
                from: agent.clone(),
                to: agent_id("agent-b"),
                summary: "findings".to_owned(),
            },
        ));
        state.apply(envelope(
            7,
            SessionEvent::RuntimeWarning {
                agent_id: agent.clone(),
                item_id: item_id("warning"),
                message: "degraded".to_owned(),
            },
        ));
        state.apply(envelope(
            8,
            SessionEvent::RuntimeError {
                agent_id: agent,
                item_id: item_id("error"),
                message: "failed".to_owned(),
            },
        ));

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
                TranscriptEntryView::Artifact(_) => "artifact",
                TranscriptEntryView::Mail(_) => "mail",
            })
            .collect();
        assert_eq!(
            entries,
            ["text", "tool", "artifact", "mail", "warning", "error"]
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
            SessionEvent::TranscriptItemStarted {
                agent_id: agent_id("agent-a"),
                item_id: item_id("shared"),
                role: TranscriptRole::Assistant,
            },
        ));

        assert!(matches!(
            state.apply(envelope(
                4,
                SessionEvent::TranscriptDelta {
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
                SessionEvent::AgentStatusChanged {
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
            SessionEvent::AgentStatusChanged {
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
                SessionEvent::TurnUsageUpdated {
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
