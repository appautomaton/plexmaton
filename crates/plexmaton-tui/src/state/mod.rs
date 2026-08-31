mod agent;
mod attention;
mod composer;
mod focus;
mod notices;
mod ordered;
mod roster;
mod scroll;

use plexmaton_core::{
    AgentId, EventSequence, PrototypeEvent, PrototypeEventEnvelope, TranscriptItemId,
};
use thiserror::Error;

pub use agent::{AgentView, ArtifactView, MailView, ToolActivityView, TranscriptItemView};
pub use attention::AttentionView;
pub use composer::Composer;
pub use notices::NoticeView;

use crate::{
    intent::{Direction, ScrollDirection, TextIntent},
    surface::{KeyboardFocus, SurfaceId, SurfaceTree},
};
use attention::AttentionQueue;
use focus::Focus;
use notices::NoticeLog;
use roster::Roster;
use scroll::ScrollState;

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

/// Revisioned, deterministic TUI projection.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ViewState {
    revision: ViewRevision,
    last_sequence: Option<EventSequence>,
    agents: Roster,
    attention: AttentionQueue,
    notices: NoticeLog,
    focus: Focus,
    scroll: ScrollState,
    composer: Composer,
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

    /// Returns the agent the composer addresses: the first to appear, never the selected one.
    #[must_use]
    pub fn primary_agent(&self) -> Option<&AgentView> {
        self.agents.primary()
    }

    /// Returns the draft the user is typing.
    #[must_use]
    pub const fn composer(&self) -> &Composer {
        &self.composer
    }

    /// Applies one edit, returning the text when the user asked to send it.
    ///
    /// The returned string is a *command* for the runtime, never something to write into the
    /// transcript here: the projection has one writer, and it is the event stream (COM-3).
    ///
    /// This trusts INV-2 rather than re-checking focus. The router only produces a text intent
    /// while a text input holds the cursor, and it reads that from this same state, so a second
    /// check here would be a second source of truth for the same fact.
    pub fn edit(&mut self, intent: TextIntent) -> Option<String> {
        let changed = match intent {
            TextIntent::Insert(character) => {
                self.composer.insert(character);
                true
            }
            TextIntent::Newline => {
                self.composer.newline();
                true
            }
            TextIntent::DeleteBackward => self.composer.delete_backward(),
            TextIntent::Submit => {
                let submitted = self.composer.take_draft();
                if submitted.is_some() {
                    self.touch();
                }
                return submitted;
            }
        };
        if changed {
            self.touch();
        }
        None
    }

    /// Returns the selected agent projection, when one exists.
    #[must_use]
    pub fn selected_agent(&self) -> Option<&AgentView> {
        self.agents.selected()
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
    pub const fn notices_dropped(&self) -> u64 {
        self.notices.dropped()
    }

    /// Selects an existing agent without changing semantic runtime state.
    pub fn select_agent(&mut self, agent_id: &AgentId) -> Result<(), ReduceError> {
        if self.agents.select(agent_id)? {
            self.touch();
        }
        Ok(())
    }

    /// Moves the agent selection one step in arrival order, clamped at both ends.
    pub fn move_selection(&mut self, direction: Direction) {
        if self.agents.move_selection(direction) {
            self.touch();
        }
    }

    /// Resolves which surface holds keyboard focus for the frame `surfaces` describes.
    #[must_use]
    pub fn focused(&self, surfaces: &SurfaceTree) -> Option<SurfaceId> {
        self.focus.resolve(surfaces)
    }

    /// Resolves where typed text would go, from the focused surface's kind alone (SURF-3).
    #[must_use]
    pub fn keyboard_focus(&self, surfaces: &SurfaceTree) -> KeyboardFocus {
        self.focus.keyboard(surfaces)
    }

    /// Returns where the user last put this surface, if they ever moved it.
    ///
    /// `None` means untouched, which is not the same as zero: the renderer then anchors the
    /// surface to its own kind of content, so a conversation opens at its newest line.
    #[must_use]
    pub fn scroll_offset(&self, surface_id: SurfaceId) -> Option<u16> {
        self.scroll.offset(surface_id)
    }

    /// Scrolls one surface's viewport by a wheel notch.
    ///
    /// The event is consumed here whether or not anything moved. An exhausted viewport stops the
    /// wheel rather than passing it to what is beneath (D-006); only a viewport that cannot move at
    /// all is skipped, and that decision was already made when the target was resolved.
    pub fn scroll(
        &mut self,
        surfaces: &SurfaceTree,
        surface_id: SurfaceId,
        direction: ScrollDirection,
    ) {
        let Some(viewport) = surfaces.viewport(surface_id) else {
            return;
        };
        if self.scroll.scroll(surface_id, viewport, direction) {
            self.touch();
        }
    }

    /// Moves focus one stop around the ring.
    pub fn cycle_focus(&mut self, surfaces: &SurfaceTree, direction: Direction) {
        if self.focus.cycle(surfaces, direction) {
            self.touch();
        }
    }

    /// Focuses the surface a press landed on, if that surface is a stop.
    pub fn focus_surface(&mut self, surfaces: &SurfaceTree, surface_id: SurfaceId) {
        if self.focus.point_at(surfaces, surface_id) {
            self.touch();
        }
    }

    fn apply_event(&mut self, event: PrototypeEvent) -> Result<(), ReduceError> {
        match event {
            PrototypeEvent::AgentCreated {
                agent_id,
                label,
                status,
            } => self.agents.add(agent_id, label, status)?,
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
                self.attention.request(AttentionView {
                    id: attention_id,
                    agent_id,
                    kind,
                    summary,
                });
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
        self.notices.push(notice);
        // A notice is visible, so recording one is a change the renderer has to repaint for.
        self.touch();
    }

    fn touch(&mut self) {
        self.revision = ViewRevision(self.revision.0.saturating_add(1));
    }

    fn agent_mut(&mut self, agent_id: &AgentId) -> Result<&mut AgentView, ReduceError> {
        self.agents.get_mut(agent_id)
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{
        AgentId, AgentStatus, EventSequence, PrototypeEvent, PrototypeEventEnvelope,
    };
    use ratatui::layout::Rect;

    use super::{ApplyOutcome, NoticeView, ReduceError, ViewState};
    use crate::{
        intent::Direction,
        layout::{self, WorkspaceInput},
        surface::SurfaceId,
        test_support::{canonical_runtime, canonical_state},
    };

    /// SURF-3: focus is a stop on the ring, and a press on chrome is not a way off it.
    #[test]
    fn focus_starts_on_the_ring_and_a_press_on_chrome_does_not_move_it() {
        let surfaces = layout::workspace(Rect::new(0, 0, 120, 24), WorkspaceInput::default());
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
        let surfaces = layout::workspace(Rect::new(0, 0, 120, 24), WorkspaceInput::default());
        let mut state = canonical_state();
        let mut seen = Vec::new();

        for _ in 0..5 {
            seen.push(state.focused(&surfaces));
            state.cycle_focus(&surfaces, Direction::Forward);
        }

        assert_eq!(
            seen,
            [
                Some(SurfaceId::Agents),
                Some(SurfaceId::Transcript),
                Some(SurfaceId::Activity),
                Some(SurfaceId::Composer),
                Some(SurfaceId::Agents),
            ],
            "the ring runs down the screen and wraps"
        );
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
        let mut state = ViewState::default();
        for envelope in canonical_runtime().ready(u64::MAX) {
            assert_eq!(state.apply(envelope), ApplyOutcome::Accepted);
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
}
