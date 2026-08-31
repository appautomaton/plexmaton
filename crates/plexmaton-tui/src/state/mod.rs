mod agent;
mod attention;
mod composer;
mod focus;
mod ingest;
mod notices;
mod ordered;
mod roster;
mod scroll;

use plexmaton_core::{AgentId, EventSequence};

pub use agent::{AgentView, ArtifactView, MailView, ToolActivityView, TranscriptItemView};
pub use attention::AttentionView;
pub use composer::Composer;
pub use ingest::{ApplyOutcome, ReduceError};
pub use notices::NoticeView;
pub use scroll::ScrollPosition;

use crate::{
    intent::{Direction, ScrollDirection, TextIntent},
    surface::{KeyboardFocus, SurfaceId, SurfaceTree},
    transcript::{TranscriptMetrics, TranscriptPosition},
};
use attention::AttentionQueue;
use focus::Focus;
use notices::NoticeLog;
use roster::Roster;
use scroll::ScrollState;

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

    /// Returns where the user last put this panel, if they ever moved it.
    ///
    /// `None` means untouched, which is not the same as `Row(0)`: the renderer then anchors the
    /// surface to its own kind of content, so a bounded tail view opens at its newest line. The
    /// conversation is not a panel here — its position is semantic and per agent (TR-3, TR-5).
    #[must_use]
    pub fn scroll_position(&self, surface_id: SurfaceId) -> Option<ScrollPosition> {
        self.scroll.panel(surface_id)
    }

    /// Returns where the reader of the selected conversation is, if they have ever moved.
    pub(crate) fn conversation_position(&self) -> Option<&TranscriptPosition> {
        self.scroll.conversation(&self.agents.selected()?.id)
    }

    /// Scrolls one surface's viewport by a wheel notch.
    ///
    /// The event is consumed here whether or not anything moved. An exhausted viewport stops the
    /// wheel rather than passing it to what is beneath (D-006); only a viewport that cannot move at
    /// all is skipped, and that decision was already made when the target was resolved.
    ///
    /// The conversation takes a different path from every other surface because it is the one made
    /// of items: it parks against the message being read rather than against a row number.
    pub fn scroll(
        &mut self,
        surfaces: &SurfaceTree,
        metrics: &TranscriptMetrics,
        surface_id: SurfaceId,
        direction: ScrollDirection,
    ) {
        let Some(viewport) = surfaces.viewport(surface_id) else {
            return;
        };
        let moved = if surface_id == SurfaceId::Transcript {
            let Some(agent_id) = self.agents.selected().map(|agent| agent.id.clone()) else {
                return;
            };
            self.scroll
                .scroll_conversation(&agent_id, viewport, direction, metrics)
        } else {
            self.scroll.scroll_panel(surface_id, viewport, direction)
        };
        if moved {
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

    fn touch(&mut self) {
        self.revision = ViewRevision(self.revision.0.saturating_add(1));
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::AgentId;
    use ratatui::layout::Rect;

    use super::ViewState;
    use crate::{
        intent::Direction,
        layout::{self, WorkspaceInput},
        surface::SurfaceId,
        test_support::canonical_state,
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
}
