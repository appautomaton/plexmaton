mod agent;
mod ordered;

use std::collections::VecDeque;

use plexmaton_core::{
    AgentId, AttentionId, AttentionKind, EventSequence, PrototypeEvent, PrototypeEventEnvelope,
    TranscriptItemId,
};
use thiserror::Error;

pub use agent::{AgentView, ArtifactView, MailView, ToolActivityView, TranscriptItemView};

use crate::{
    intent::Direction,
    surface::{KeyboardFocus, SurfaceId, SurfaceTree},
};
use ordered::OrderedById;

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
    /// Which surface the user last put focus on.
    ///
    /// A preference, not an assertion: the surfaces registered change with every frame, so this
    /// may name one that is not on screen right now. `focused` resolves it against the current
    /// tree instead of repairing it, which is what makes SURF-5 fall out — a surface that comes
    /// back gets its focus back, with nothing to keep in sync.
    focus: Option<SurfaceId>,
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

    /// Moves the agent selection one step in arrival order, clamped at both ends.
    ///
    /// Clamping rather than wrapping keeps a held key idempotent at the boundary: a list that
    /// wraps sends the user back to the first agent at the moment they stop reading the keys.
    pub fn move_selection(&mut self, direction: Direction) {
        let Some(current) = self.selected_agent.clone() else {
            // Nothing is selected yet, so either arrow lands on the first agent.
            let first = self.agents.iter().next().map(|agent| agent.id.clone());
            if let Some(first) = first {
                self.selected_agent = Some(first);
                self.touch();
            }
            return;
        };
        let Some(index) = self.agents.iter().position(|agent| agent.id == current) else {
            return;
        };
        let target = match direction {
            Direction::Forward => index.saturating_add(1),
            Direction::Backward => index.saturating_sub(1),
        };
        let Some(next) = self.agents.iter().nth(target).map(|agent| agent.id.clone()) else {
            return;
        };
        if next != current {
            self.selected_agent = Some(next);
            self.touch();
        }
    }

    /// Resolves which surface holds keyboard focus for the frame `surfaces` describes.
    ///
    /// Derived rather than stored, so a stale preference can never be delivered to. A stored focus
    /// that is no longer a stop falls back to the first one, and the preference is left alone so
    /// the surface reclaims focus when it returns.
    #[must_use]
    pub fn focused(&self, surfaces: &SurfaceTree) -> Option<SurfaceId> {
        self.focus
            .filter(|id| surfaces.get(*id).is_some_and(|s| s.kind.is_focusable()))
            .or_else(|| surfaces.focus_ring().next())
    }

    /// Resolves where typed text would go, from the focused surface's kind alone (SURF-3).
    #[must_use]
    pub fn keyboard_focus(&self, surfaces: &SurfaceTree) -> KeyboardFocus {
        self.focused(surfaces)
            .and_then(|id| surfaces.get(id))
            .map_or(KeyboardFocus::default(), |surface| {
                surface.kind.keyboard_focus()
            })
    }

    /// Moves focus one stop around the ring.
    pub fn cycle_focus(&mut self, surfaces: &SurfaceTree, direction: Direction) {
        let next = surfaces.next_focus(self.focused(surfaces), direction);
        self.set_focus(next);
    }

    /// Focuses the surface a press landed on, if that surface is a stop.
    ///
    /// A press on chrome routes but does not move focus, so clicking the hint strip does not strand
    /// the keyboard somewhere it cannot act.
    pub fn focus_surface(&mut self, surfaces: &SurfaceTree, surface_id: SurfaceId) {
        if surfaces
            .get(surface_id)
            .is_some_and(|surface| surface.kind.is_focusable())
        {
            self.set_focus(Some(surface_id));
        }
    }

    fn set_focus(&mut self, next: Option<SurfaceId>) {
        if next.is_some() && next != self.focus {
            self.focus = next;
            self.touch();
        }
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
            } => self.agent_mut(&agent_id)?.start_item(item_id, role)?,
            PrototypeEvent::TranscriptDelta {
                agent_id,
                item_id,
                item_revision,
                text,
            } => self
                .agent_mut(&agent_id)?
                .append_delta(&item_id, item_revision, &text)?,
            PrototypeEvent::TranscriptItemFinalized {
                agent_id,
                item_id,
                item_revision,
            } => self
                .agent_mut(&agent_id)?
                .finalize_item(&item_id, item_revision)?,
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
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{
        AgentId, AgentStatus, EventSequence, PrototypeEvent, PrototypeEventEnvelope,
    };
    use plexmaton_sim::Scenario;
    use ratatui::layout::Rect;

    use super::{ApplyOutcome, NoticeView, ReduceError, ViewState};
    use crate::{
        intent::Direction,
        layout,
        surface::{Surface, SurfaceId, SurfaceKind, SurfaceTree},
        test_support::canonical_state,
    };

    /// A tree holding exactly the named stops, so a surface can be taken away and given back.
    fn tree_of(ids: &[SurfaceId]) -> SurfaceTree {
        let mut tree = SurfaceTree::default();
        for id in ids {
            tree.insert(Surface {
                id: *id,
                bounds: Rect::new(0, 0, 10, 10),
                z_index: 0,
                kind: SurfaceKind::Panel,
            })
            .unwrap_or_else(|error| panic!("fixture must insert: {error}"));
        }
        tree
    }

    /// SURF-3: focus is a stop on the ring, and a press on chrome is not a way off it.
    #[test]
    fn focus_starts_on_the_ring_and_a_press_on_chrome_does_not_move_it() {
        let surfaces = layout::workspace(Rect::new(0, 0, 120, 24), true);
        let mut state = canonical_state();

        assert_eq!(state.focused(&surfaces), Some(SurfaceId::Agents));

        state.focus_surface(&surfaces, SurfaceId::Transcript);
        assert_eq!(state.focused(&surfaces), Some(SurfaceId::Transcript));

        let before = state.revision();
        state.focus_surface(&surfaces, SurfaceId::Footer);
        assert_eq!(
            state.focused(&surfaces),
            Some(SurfaceId::Transcript),
            "a hint strip is not a focus stop, so the press must leave focus where it was"
        );
        assert_eq!(
            state.revision(),
            before,
            "a press that changes nothing must not force a repaint"
        );
    }

    #[test]
    fn cycling_focus_walks_the_ring_and_wraps() {
        let surfaces = layout::workspace(Rect::new(0, 0, 120, 24), true);
        let mut state = canonical_state();
        let mut seen = Vec::new();

        for _ in 0..4 {
            seen.push(state.focused(&surfaces));
            state.cycle_focus(&surfaces, Direction::Forward);
        }

        assert_eq!(
            seen,
            [
                Some(SurfaceId::Agents),
                Some(SurfaceId::Transcript),
                Some(SurfaceId::Activity),
                Some(SurfaceId::Agents),
            ]
        );
    }

    /// SURF-5: focus belongs to the surface, not to the frame that happened to draw it.
    #[test]
    fn focus_returns_to_a_surface_that_comes_back() {
        let full = tree_of(&[SurfaceId::Agents, SurfaceId::Transcript]);
        let reduced = tree_of(&[SurfaceId::Agents]);
        let mut state = ViewState::default();
        state.focus_surface(&full, SurfaceId::Transcript);

        assert_eq!(
            state.focused(&reduced),
            Some(SurfaceId::Agents),
            "focus must never be delivered to a surface that is not on screen"
        );
        assert_eq!(
            state.focused(&full),
            Some(SurfaceId::Transcript),
            "and the preference must survive, or reopening loses where the user was"
        );
    }

    #[test]
    fn focus_resolves_to_nothing_when_no_surface_is_registered() {
        let state = ViewState::default();

        assert_eq!(state.focused(&SurfaceTree::default()), None);
    }

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
        let scenario =
            Scenario::canonical().unwrap_or_else(|error| panic!("invalid fixture: {error}"));
        let mut state = ViewState::default();
        for step in scenario.into_steps() {
            assert_eq!(state.apply(step.envelope), ApplyOutcome::Accepted);
        }
        assert_eq!(state.notices().count(), 0);
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

    fn selected(state: &ViewState) -> String {
        state
            .selected_agent()
            .map_or_else(|| "none".to_owned(), |agent| agent.id.to_string())
    }

    #[test]
    fn selection_moves_in_arrival_order_and_clamps_at_both_ends() {
        let mut state = canonical_state();
        assert_eq!(selected(&state), "agent-a");

        state.move_selection(Direction::Backward);
        assert_eq!(selected(&state), "agent-a", "clamped at the first agent");

        state.move_selection(Direction::Forward);
        assert_eq!(selected(&state), "agent-b");

        let at_end = state.revision();
        state.move_selection(Direction::Forward);
        assert_eq!(selected(&state), "agent-b", "clamped at the last agent");
        assert_eq!(
            state.revision(),
            at_end,
            "a clamped move changes nothing visible and must not force a repaint"
        );
    }

    #[test]
    fn moving_the_selection_with_no_agents_is_a_no_op() {
        let mut state = ViewState::default();

        state.move_selection(Direction::Forward);

        assert!(state.selected_agent().is_none());
        assert_eq!(state.revision().get(), 0);
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
