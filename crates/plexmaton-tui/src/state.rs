use std::collections::{BTreeMap, VecDeque};

use plexmaton_core::{
    AgentId, AgentStatus, ArtifactId, AttentionId, AttentionKind, EventSequence, MailId,
    PrototypeEvent, PrototypeEventEnvelope, ToolActivityId, ToolActivityStatus, TranscriptItemId,
    TranscriptRole,
};
use thiserror::Error;

/// Upper bound on retained runtime notices.
///
/// Notices originate from producers the projection does not control, so the log discards its
/// oldest entries and reports the discarded count rather than growing without limit.
const NOTICE_CAPACITY: usize = 32;

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

/// Monotonic revision of the whole projection.
///
/// Renderers compare revisions to decide whether a repaint is required, so it must advance on
/// every user-visible change and must not advance on a no-op.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct ViewRevision(u64);

impl ViewRevision {
    /// Returns the numeric revision.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Identity-keyed collection that iterates in first-insertion order.
///
/// Display order must follow arrival, not identifier collation, so the order vector is the
/// authority and the map exists only to update an entry in place by identity.
#[derive(Clone, Debug, Eq, PartialEq)]
struct OrderedById<K, V> {
    order: Vec<K>,
    entries: BTreeMap<K, V>,
}

impl<K: Clone + Ord, V> OrderedById<K, V> {
    fn contains(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }

    /// Appends a new entry, or replaces an existing one without moving its position.
    fn upsert(&mut self, key: K, value: V) {
        if self.entries.insert(key.clone(), value).is_none() {
            self.order.push(key);
        }
    }

    fn get(&self, key: &K) -> Option<&V> {
        self.entries.get(key)
    }

    fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.entries.get_mut(key)
    }

    fn len(&self) -> usize {
        self.order.len()
    }

    fn iter(&self) -> impl Iterator<Item = &V> {
        self.order.iter().filter_map(|key| self.entries.get(key))
    }
}

impl<K, V> Default for OrderedById<K, V> {
    fn default() -> Self {
        Self {
            order: Vec::new(),
            entries: BTreeMap::new(),
        }
    }
}

/// Projected transcript content. Semantic source is retained separately from terminal cells.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TranscriptItemView {
    pub id: TranscriptItemId,
    pub role: TranscriptRole,
    pub source: String,
    pub revision: u64,
    pub finalized: bool,
}

/// One visible tool activity and its current lifecycle state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolActivityView {
    pub id: ToolActivityId,
    pub label: String,
    pub status: ToolActivityStatus,
}

/// Durable work product announced by an agent, referenced by pointer rather than copied inline.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactView {
    pub id: ArtifactId,
    pub label: String,
    pub pointer: String,
}

/// Typed mail delivered between sessions.
///
/// Sender identity is part of the product contract, so it is retained rather than reduced away.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MailView {
    pub id: MailId,
    pub from: AgentId,
    pub summary: String,
}

/// Per-agent projection consumed by the renderer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentView {
    pub id: AgentId,
    pub label: String,
    pub status: AgentStatus,
    items: OrderedById<TranscriptItemId, TranscriptItemView>,
    tools: OrderedById<ToolActivityId, ToolActivityView>,
    artifacts: OrderedById<ArtifactId, ArtifactView>,
    inbox: OrderedById<MailId, MailView>,
}

impl AgentView {
    fn new(id: AgentId, label: String, status: AgentStatus) -> Self {
        Self {
            id,
            label,
            status,
            items: OrderedById::default(),
            tools: OrderedById::default(),
            artifacts: OrderedById::default(),
            inbox: OrderedById::default(),
        }
    }

    /// Iterates transcript items in arrival order.
    pub fn transcript(&self) -> impl Iterator<Item = &TranscriptItemView> {
        self.items.iter()
    }

    /// Iterates tool activity in arrival order.
    pub fn tool_activity(&self) -> impl Iterator<Item = &ToolActivityView> {
        self.tools.iter()
    }

    /// Iterates announced artifacts in arrival order.
    pub fn artifacts(&self) -> impl Iterator<Item = &ArtifactView> {
        self.artifacts.iter()
    }

    /// Iterates delivered mail in arrival order.
    pub fn inbox(&self) -> impl Iterator<Item = &MailView> {
        self.inbox.iter()
    }
}

/// One queued background request. Adding it never changes keyboard focus.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttentionView {
    pub id: AttentionId,
    pub agent_id: AgentId,
    pub kind: AttentionKind,
    pub summary: String,
}

/// A runtime notice surfaced without interrupting the user's work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NoticeView {
    /// Warning reported by the event producer.
    RuntimeWarning { message: String },
    /// Events were lost before the received sequence; the projection resynchronized forward.
    SequenceGap { expected: u64, received: u64 },
    /// One event violated the projection contract and was dropped.
    Rejected {
        sequence: EventSequence,
        error: ReduceError,
    },
}

/// Revisioned, deterministic TUI projection.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ViewState {
    revision: ViewRevision,
    last_sequence: Option<EventSequence>,
    agents: OrderedById<AgentId, AgentView>,
    selected_agent: Option<AgentId>,
    attention: OrderedById<AttentionId, AttentionView>,
    notices: VecDeque<NoticeView>,
    notices_dropped: u64,
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
            Ok(()) => {
                self.touch();
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

    /// Returns the current projection revision.
    #[must_use]
    pub fn revision(&self) -> ViewRevision {
        self.revision
    }

    /// Returns agents in creation order.
    pub fn agents(&self) -> impl Iterator<Item = &AgentView> {
        self.agents.iter()
    }

    /// Returns the selected agent projection, when one exists.
    #[must_use]
    pub fn selected_agent(&self) -> Option<&AgentView> {
        self.selected_agent
            .as_ref()
            .and_then(|id| self.agents.get(id))
    }

    /// Number of background requests awaiting attention.
    #[must_use]
    pub fn attention_count(&self) -> usize {
        self.attention.len()
    }

    /// Returns queued attention items in arrival order.
    pub fn attention(&self) -> impl Iterator<Item = &AttentionView> {
        self.attention.iter()
    }

    /// Returns retained notices from oldest to newest.
    pub fn notices(&self) -> impl Iterator<Item = &NoticeView> {
        self.notices.iter()
    }

    /// Number of notices discarded because the bounded log was full.
    #[must_use]
    pub fn notices_dropped(&self) -> u64 {
        self.notices_dropped
    }

    /// Selects an existing agent without changing semantic runtime state.
    pub fn select_agent(&mut self, agent_id: &AgentId) -> Result<(), ReduceError> {
        if !self.agents.contains(agent_id) {
            return Err(ReduceError::UnknownAgent(agent_id.clone()));
        }
        if self.selected_agent.as_ref() == Some(agent_id) {
            return Ok(());
        }
        self.selected_agent = Some(agent_id.clone());
        self.touch();
        Ok(())
    }

    fn apply_event(&mut self, event: PrototypeEvent) -> Result<(), ReduceError> {
        match event {
            PrototypeEvent::AgentCreated {
                agent_id,
                label,
                status,
            } => {
                if self.agents.contains(&agent_id) {
                    return Err(ReduceError::DuplicateAgent(agent_id));
                }
                self.selected_agent.get_or_insert_with(|| agent_id.clone());
                self.agents
                    .upsert(agent_id.clone(), AgentView::new(agent_id, label, status));
            }
            PrototypeEvent::AgentStatusChanged { agent_id, status } => {
                self.agent_mut(&agent_id)?.status = status;
            }
            PrototypeEvent::TranscriptItemStarted {
                agent_id,
                item_id,
                role,
            } => {
                let agent = self.agent_mut(&agent_id)?;
                if agent.items.contains(&item_id) {
                    return Err(ReduceError::DuplicateTranscriptItem(item_id));
                }
                agent.items.upsert(
                    item_id.clone(),
                    TranscriptItemView {
                        id: item_id,
                        role,
                        source: String::new(),
                        revision: 0,
                        finalized: false,
                    },
                );
            }
            PrototypeEvent::TranscriptDelta {
                agent_id,
                item_id,
                item_revision,
                text,
            } => {
                let item = self.item_mut(&agent_id, &item_id)?;
                Self::advance_item_revision(item, item_revision)?;
                item.source.push_str(&text);
            }
            PrototypeEvent::TranscriptItemFinalized {
                agent_id,
                item_id,
                item_revision,
            } => {
                let item = self.item_mut(&agent_id, &item_id)?;
                Self::advance_item_revision(item, item_revision)?;
                item.finalized = true;
            }
            PrototypeEvent::ToolActivityChanged {
                agent_id,
                activity_id,
                label,
                status,
            } => {
                self.agent_mut(&agent_id)?.tools.upsert(
                    activity_id.clone(),
                    ToolActivityView {
                        id: activity_id,
                        label,
                        status,
                    },
                );
            }
            PrototypeEvent::AttentionRequested {
                agent_id,
                attention_id,
                kind,
                summary,
            } => {
                if !self.agents.contains(&agent_id) {
                    return Err(ReduceError::UnknownAgent(agent_id));
                }
                self.attention.upsert(
                    attention_id.clone(),
                    AttentionView {
                        id: attention_id,
                        agent_id,
                        kind,
                        summary,
                    },
                );
            }
            PrototypeEvent::MailDelivered {
                mail_id,
                from,
                to,
                summary,
            } => {
                self.agent_mut(&to)?.inbox.upsert(
                    mail_id.clone(),
                    MailView {
                        id: mail_id,
                        from,
                        summary,
                    },
                );
            }
            PrototypeEvent::ArtifactAnnounced {
                agent_id,
                artifact_id,
                label,
                pointer,
            } => {
                self.agent_mut(&agent_id)?.artifacts.upsert(
                    artifact_id.clone(),
                    ArtifactView {
                        id: artifact_id,
                        label,
                        pointer,
                    },
                );
            }
            PrototypeEvent::RuntimeWarning { message } => {
                self.push_notice(NoticeView::RuntimeWarning { message });
            }
        }
        Ok(())
    }

    fn push_notice(&mut self, notice: NoticeView) {
        if self.notices.len() == NOTICE_CAPACITY {
            self.notices.pop_front();
            self.notices_dropped = self.notices_dropped.saturating_add(1);
        }
        self.notices.push_back(notice);
        self.touch();
    }

    fn touch(&mut self) {
        self.revision = ViewRevision(self.revision.0.saturating_add(1));
    }

    fn agent_mut(&mut self, agent_id: &AgentId) -> Result<&mut AgentView, ReduceError> {
        self.agents
            .get_mut(agent_id)
            .ok_or_else(|| ReduceError::UnknownAgent(agent_id.clone()))
    }

    fn item_mut(
        &mut self,
        agent_id: &AgentId,
        item_id: &TranscriptItemId,
    ) -> Result<&mut TranscriptItemView, ReduceError> {
        self.agent_mut(agent_id)?
            .items
            .get_mut(item_id)
            .ok_or_else(|| ReduceError::UnknownTranscriptItem(item_id.clone()))
    }

    fn advance_item_revision(
        item: &mut TranscriptItemView,
        received: u64,
    ) -> Result<(), ReduceError> {
        let expected = item.revision + 1;
        if received != expected {
            return Err(ReduceError::ItemRevisionGap {
                item_id: item.id.clone(),
                expected,
                received,
            });
        }
        item.revision = received;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{
        AgentId, AgentStatus, EventSequence, PrototypeEvent, PrototypeEventEnvelope,
        ToolActivityId, ToolActivityStatus,
    };
    use plexmaton_sim::Scenario;

    use super::{ApplyOutcome, NoticeView, ReduceError, ViewState};

    fn agent_id(value: &str) -> AgentId {
        AgentId::new(value).unwrap_or_else(|error| panic!("invalid fixture: {error}"))
    }

    fn tool_id(value: &str) -> ToolActivityId {
        ToolActivityId::new(value).unwrap_or_else(|error| panic!("invalid fixture: {error}"))
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

    fn canonical_state() -> ViewState {
        let scenario =
            Scenario::canonical().unwrap_or_else(|error| panic!("invalid fixture: {error}"));
        let mut state = ViewState::default();
        for step in scenario.into_steps() {
            assert_eq!(state.apply(step.envelope), ApplyOutcome::Accepted);
        }
        state
    }

    #[test]
    fn canonical_projection_keeps_primary_selected_when_background_agent_appears() {
        let state = canonical_state();

        assert_eq!(
            state.selected_agent().map(|agent| agent.id.as_str()),
            Some("agent-a")
        );
        assert_eq!(state.attention_count(), 1);
        assert_eq!(state.agents().count(), 2);
        assert_eq!(state.notices().count(), 0);
    }

    #[test]
    fn selecting_unknown_agent_is_rejected() {
        let mut state = ViewState::default();

        assert!(state.select_agent(&agent_id("missing")).is_err());
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

    #[test]
    fn rejection_advances_the_revision_because_the_notice_is_visible() {
        let mut state = ViewState::default();
        state.apply(envelope(1, created("agent-a")));
        let before = state.revision();

        state.apply(envelope(2, created("agent-a")));

        assert!(state.revision() > before);
    }

    #[test]
    fn tool_activity_iterates_in_arrival_order_not_identifier_order() {
        let mut state = ViewState::default();
        state.apply(envelope(1, created("agent-a")));
        for (sequence, tool) in ["tool-z", "tool-a"].into_iter().enumerate() {
            state.apply(envelope(
                sequence as u64 + 2,
                PrototypeEvent::ToolActivityChanged {
                    agent_id: agent_id("agent-a"),
                    activity_id: tool_id(tool),
                    label: tool.to_owned(),
                    status: ToolActivityStatus::Running,
                },
            ));
        }

        let labels: Vec<_> = state
            .selected_agent()
            .unwrap_or_else(|| panic!("agent-a is selected"))
            .tool_activity()
            .map(|tool| tool.label.as_str())
            .collect();

        assert_eq!(labels, ["tool-z", "tool-a"]);
    }

    #[test]
    fn updating_a_tool_keeps_its_position_and_replaces_its_status() {
        let mut state = ViewState::default();
        state.apply(envelope(1, created("agent-a")));
        for (sequence, (tool, status)) in [
            ("tool-z", ToolActivityStatus::Running),
            ("tool-a", ToolActivityStatus::Running),
            ("tool-z", ToolActivityStatus::Succeeded),
        ]
        .into_iter()
        .enumerate()
        {
            state.apply(envelope(
                sequence as u64 + 2,
                PrototypeEvent::ToolActivityChanged {
                    agent_id: agent_id("agent-a"),
                    activity_id: tool_id(tool),
                    label: tool.to_owned(),
                    status,
                },
            ));
        }

        let tools: Vec<_> = state
            .selected_agent()
            .unwrap_or_else(|| panic!("agent-a is selected"))
            .tool_activity()
            .map(|tool| (tool.label.as_str(), tool.status))
            .collect();

        assert_eq!(
            tools,
            [
                ("tool-z", ToolActivityStatus::Succeeded),
                ("tool-a", ToolActivityStatus::Running),
            ]
        );
    }

    #[test]
    fn notice_log_is_bounded_and_reports_discarded_entries() {
        let mut state = ViewState::default();
        let total = super::NOTICE_CAPACITY + 8;
        for index in 0..total {
            state.apply(envelope(
                index as u64 + 1,
                PrototypeEvent::RuntimeWarning {
                    message: format!("warning {index}"),
                },
            ));
        }

        assert_eq!(state.notices().count(), super::NOTICE_CAPACITY);
        assert_eq!(state.notices_dropped(), 8);
        assert!(matches!(
            state.notices().next(),
            Some(NoticeView::RuntimeWarning { message }) if message == "warning 8"
        ));
    }
}
