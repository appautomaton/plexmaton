//! Deciding whether one semantic event belongs in the projection, and where it goes.
//!
//! The rest of this module owns what the user is looking at. This half owns the producer contract:
//! stream ordering, the typed reasons an event is refused, and the dispatch from an event kind to
//! the collection that holds it. Keeping them apart is what stops "is this event well formed" and
//! "where is the reader" from sharing one file and one set of reasons to change.

use plexmaton_core::{AgentId, PrototypeEvent, PrototypeEventEnvelope, TranscriptItemId};
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
    pub fn apply(&mut self, envelope: PrototypeEventEnvelope) -> ApplyOutcome {
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
                // Accepted is not the same as changed. A producer that re-sends an agent's current
                // status or a tool's current state is reporting rather than transitioning, and
                // FR-1 says traffic that alters nothing visible costs no frame at all.
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
    fn apply_event(&mut self, event: PrototypeEvent) -> Result<bool, ReduceError> {
        let changed = match event {
            PrototypeEvent::AgentCreated {
                agent_id,
                label,
                status,
            } => {
                self.agents.add(agent_id, label, status)?;
                true
            }
            PrototypeEvent::AgentStatusChanged { agent_id, status } => {
                let agent = self.agent_mut(&agent_id)?;
                let moved = agent.status != status;
                agent.status = status;
                moved
            }
            PrototypeEvent::TranscriptItemStarted {
                agent_id,
                item_id,
                role,
            } => {
                self.agent_mut(&agent_id)?.start_item(item_id, role)?;
                true
            }
            PrototypeEvent::TranscriptDelta {
                agent_id,
                item_id,
                item_revision,
                text,
            } => {
                self.agent_mut(&agent_id)?
                    .append_delta(&item_id, item_revision, &text)?;
                true
            }
            PrototypeEvent::TranscriptItemFinalized {
                agent_id,
                item_id,
                item_revision,
            } => {
                self.agent_mut(&agent_id)?
                    .finalize_item(&item_id, item_revision)?;
                true
            }
            PrototypeEvent::ToolActivityChanged {
                agent_id,
                activity_id,
                label,
                status,
            } => self
                .agent_mut(&agent_id)?
                .set_tool_activity(activity_id, label, status),
            PrototypeEvent::AttentionRequested {
                agent_id,
                attention_id,
                kind,
                summary,
            } => {
                if !self.agents.contains(&agent_id) {
                    return Err(ReduceError::UnknownAgent(agent_id));
                }
                self.attention.request(AttentionView {
                    id: attention_id,
                    agent_id,
                    kind,
                    summary,
                    // A producer cannot deliver an already-seen request, and a repeat of one the
                    // user had seen is a fresh ask (ATT-3).
                    acknowledged: false,
                })
            }
            PrototypeEvent::MailDelivered {
                mail_id,
                from,
                to,
                summary,
            } => self.agent_mut(&to)?.deliver_mail(mail_id, from, summary),
            PrototypeEvent::ArtifactAnnounced {
                agent_id,
                artifact_id,
                label,
                pointer,
            } => self
                .agent_mut(&agent_id)?
                .announce_artifact(artifact_id, label, pointer),
            PrototypeEvent::RuntimeWarning { message } => {
                // A notice is visible, and pushing one repaints on its own account.
                self.push_notice(NoticeView::RuntimeWarning { message });
                false
            }
        };
        Ok(changed)
    }

    fn push_notice(&mut self, notice: NoticeView) {
        self.notices.push(notice);
        // A notice is visible, so recording one is a change the renderer has to repaint for.
        self.touch();
    }

    fn agent_mut(&mut self, agent_id: &AgentId) -> Result<&mut AgentView, ReduceError> {
        self.agents.get_mut(agent_id)
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{
        AgentId, AgentStatus, EventSequence, PrototypeEvent, PrototypeEventEnvelope,
        ToolActivityId, ToolActivityStatus,
    };

    use super::{ApplyOutcome, ReduceError};
    use crate::{
        NoticeView, ViewState,
        test_support::{canonical_runtime, canonical_state},
    };

    fn agent_id(value: &str) -> AgentId {
        AgentId::new(value).unwrap_or_else(|error| panic!("invalid fixture: {error}"))
    }

    fn envelope(sequence: u64, event: PrototypeEvent) -> PrototypeEventEnvelope {
        PrototypeEventEnvelope {
            sequence: EventSequence::new(sequence),
            event,
        }
    }

    fn created(id: &str) -> PrototypeEvent {
        PrototypeEvent::AgentCreated {
            agent_id: agent_id(id),
            label: id.to_owned(),
            status: AgentStatus::Running,
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
    fn mail_retains_sender_identity() {
        let state = canonical_state();
        let primary = state
            .selected_agent()
            .unwrap_or_else(|| panic!("canonical scenario selects a primary agent"));
        let mail: Vec<_> = primary.inbox().collect();

        assert_eq!(mail.len(), 1);
        assert_eq!(mail[0].from.as_str(), "agent-b");
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
    ///
    /// A real runtime polls. It re-reports an agent that is still running and a tool that is still
    /// executing, and until now every one of those forced a repaint — so a workspace watching four
    /// busy agents redrew continuously while saying exactly the same thing.
    #[test]
    fn a_repeated_status_or_tool_state_costs_no_frame() {
        let mut state = ViewState::default();
        state.apply(envelope(1, created("agent-a")));
        let tool = |status| PrototypeEvent::ToolActivityChanged {
            agent_id: agent_id("agent-a"),
            activity_id: ToolActivityId::new("tool-1")
                .unwrap_or_else(|error| panic!("invalid fixture: {error}")),
            label: "read".to_owned(),
            status,
        };
        state.apply(envelope(2, tool(ToolActivityStatus::Running)));
        let quiet = state.revision();

        assert_eq!(
            state.apply(envelope(
                3,
                PrototypeEvent::AgentStatusChanged {
                    agent_id: agent_id("agent-a"),
                    status: AgentStatus::Running,
                }
            )),
            ApplyOutcome::Accepted,
            "the event is well formed, so it is accepted; what it is not is a change"
        );
        state.apply(envelope(4, tool(ToolActivityStatus::Running)));
        assert_eq!(state.revision(), quiet);

        // The same two events carrying an actual transition must still repaint.
        state.apply(envelope(
            5,
            PrototypeEvent::AgentStatusChanged {
                agent_id: agent_id("agent-a"),
                status: AgentStatus::Waiting,
            },
        ));
        assert!(state.revision() > quiet);
        let waiting = state.revision();
        state.apply(envelope(6, tool(ToolActivityStatus::Succeeded)));
        assert!(state.revision() > waiting);
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
