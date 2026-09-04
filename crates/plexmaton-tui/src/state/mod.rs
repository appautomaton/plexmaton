mod agent;
mod approval;
mod asking;
mod attention;
mod composer;
mod current_work;
mod disclosure;
mod entry;
mod focus;
mod ingest;
mod inspect;
mod inspector;
mod notices;
mod ordered;
mod roster;
mod scroll;
mod selection;
mod status;

use std::collections::BTreeMap;

use plexmaton_core::{AgentId, EventSequence};

pub use agent::AgentView;
pub use approval::{ApprovalSubmission, ApprovalView};
pub use attention::AttentionView;
pub use composer::Composer;
pub(crate) use composer::wrap_line;
pub(crate) use current_work::CurrentWork;
pub(crate) use disclosure::{DisclosureState, EntryAppearance, EntryTarget};
pub use entry::{
    ArtifactView, MailView, ToolCallView, TranscriptEntryView, TranscriptItemView,
    TranscriptTextKind,
};
pub use ingest::{ApplyOutcome, ReduceError};
pub use inspector::InspectorView;
pub use notices::{
    CleanupNotice, NoticeView, PersistenceNotice, SessionRecoveryNotice, TailRecoveryNotice,
};
pub use scroll::ScrollPosition;
pub use selection::{CopyRequest, Selection};
pub use status::{QuitPress, Status, StatusNote};

use crate::{
    intent::{Direction, ScrollDirection},
    surface::{KeyboardFocus, SurfaceId, SurfaceTree},
    transcript::{TranscriptMetrics, TranscriptPosition},
};
use approval::ApprovalSurface;
use attention::AttentionQueue;
use focus::Focus;
use inspector::Inspector;
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
    /// Ownership index for globally stable transcript identities (ENT-1).
    entry_owners: BTreeMap<plexmaton_core::TranscriptItemId, AgentId>,
    attention: AttentionQueue,
    approval: ApprovalSurface,
    notices: NoticeLog,
    focus: Focus,
    scroll: ScrollState,
    /// One draft per agent, keyed by who it is addressed to.
    ///
    /// A draft belongs to the conversation, not to the surface showing it: peeking another agent
    /// and coming back must find the half-written steer where it was left. This is also what makes
    /// "exactly one cursor" a claim that could fail — two inputs exist, and focus is what decides
    /// which of them has the cursor (COM-1).
    composers: BTreeMap<AgentId, Composer>,
    inspector: Inspector,
    /// Which semantic entries the user opened, plus the one under the pointer.
    disclosure: DisclosureState,
    /// What the user has selected for copying, expressed in entries rather than in cells.
    selection: Option<Selection>,
    status: Status,
}

/// A message the user submitted, and the agent it is addressed to.
///
/// The target travels with the text rather than being guessed by whoever receives it. With two
/// inputs on screen, a submission that did not name its target would be a mode error waiting to
/// happen — which is the failure COM-4 exists to prevent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Submission {
    /// Agent whose session receives the message.
    pub to: AgentId,
    /// Exactly what the user typed.
    pub text: String,
    /// Which loop boundary the visible input names.
    pub kind: SubmissionKind,
}

/// Whether submitted text starts a later turn or steers the current one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubmissionKind {
    /// Text from the primary composer, for the agent's next turn.
    Message,
    /// Text from a worker's entered window, for that turn's next step.
    Steering,
}

/// Borrowed when an agent has never been typed to, so a caller never has to handle absence.
static NO_DRAFT: Composer = Composer::new();

/// The text width inside a bordered panel that spans `width` cells.
///
/// One definition, because the height a draft asks for and the rows it is drawn into have to be
/// measured at the same width or the panel is the wrong size for what goes in it.
#[must_use]
pub const fn inner_width(width: u16) -> u16 {
    width.saturating_sub(2)
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

    /// Returns the primary agent's draft, which is what the composer shows (COM-4).
    #[must_use]
    pub fn composer(&self) -> &Composer {
        self.draft_for(self.agents.primary().map(|agent| &agent.id))
    }

    /// Returns the draft addressed to one agent.
    #[must_use]
    pub fn draft(&self, agent_id: &AgentId) -> &Composer {
        self.draft_for(Some(agent_id))
    }

    fn draft_for(&self, agent_id: Option<&AgentId>) -> &Composer {
        agent_id
            .and_then(|id| self.composers.get(id))
            .unwrap_or(&NO_DRAFT)
    }

    /// Which agent the workspace's one cursor is addressing, if any.
    ///
    /// Derived from the focused surface rather than stored, for the same reason `KeyboardFocus` is
    /// (SURF-3): two answers to "where does this keystroke go" is how a workspace ends up
    /// delivering a steer to the wrong worker.
    #[must_use]
    pub fn text_target(&self, surfaces: &SurfaceTree) -> Option<AgentId> {
        match self.focus.resolve(surfaces)? {
            // Not merely "an inspector is open": one with no room for its input has no cursor, so
            // it has nowhere for a keystroke to land either (INS-7).
            SurfaceId::Inspector => self.steer_input(surfaces).map(|(_, agent)| agent),
            SurfaceId::Composer => self.agents.primary().map(|agent| agent.id.clone()),
            _ => None,
        }
    }

    /// Whether the primary composer is collapsed to its single row (INS-5).
    ///
    /// Read from the stored preference rather than from resolved focus, because laying out the
    /// workspace is what needs the answer and there is no tree yet when it asks.
    ///
    /// `width` is the composer rectangle layout will register; the draft wraps inside its borders.
    /// A height asked for without that width is a height for a draft nobody paints.
    #[must_use]
    pub fn composer_rows(&self, width: u16) -> u16 {
        if self.agents.peeked().is_some() && self.focus.prefers(SurfaceId::Inspector) {
            // One row, not none. A composer that vanishes costs the affordance and jumps the tail
            // of the transcript by three rows; one row of jump is what INS-5 accepts.
            1
        } else {
            self.composer().requested_rows(inner_width(width))
        }
    }

    /// Returns the selected agent projection, when one exists.
    #[must_use]
    pub fn selected_agent(&self) -> Option<&AgentView> {
        self.agents.selected()
    }

    /// The sub-agents: every agent but the primary, which is what the list holds.
    pub fn sub_agents(&self) -> impl Iterator<Item = &AgentView> {
        self.agents.sub_agents()
    }

    /// Returns one agent by identity, for a surface showing an agent that is not selected.
    #[must_use]
    pub fn agent(&self, agent_id: &AgentId) -> Option<&AgentView> {
        self.agents.get(agent_id)
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

    /// Resolves which surface holds keyboard focus for the frame `surfaces` describes.
    #[must_use]
    pub fn focused(&self, surfaces: &SurfaceTree) -> Option<SurfaceId> {
        self.focus.resolve(surfaces)
    }

    /// Resolves where typed text would go (SURF-3), and whether that input is on screen (INS-7).
    #[must_use]
    pub fn keyboard_focus(&self, surfaces: &SurfaceTree) -> KeyboardFocus {
        self.focus
            .keyboard(surfaces, self.steer_input(surfaces).is_some())
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

    /// Returns where the reader of one conversation is, if they have ever moved.
    ///
    /// By agent rather than by surface, because two surfaces now draw conversations and the
    /// position belongs to the conversation rather than to the panel showing it (TR-5).
    pub(crate) fn conversation_position(&self, agent_id: &AgentId) -> Option<&TranscriptPosition> {
        self.scroll.conversation(agent_id)
    }

    /// Scrolls one surface's viewport by a wheel notch.
    ///
    /// The event is consumed here whether or not anything moved. An exhausted viewport stops the
    /// wheel rather than passing it to what is beneath (ui-ux §nested scrolling); only a viewport
    /// that cannot move at
    /// all is skipped, and that decision was already made when the target was resolved.
    ///
    /// A conversation takes a different path from every other surface because it is the one made
    /// of items: it parks against the message being read rather than against a row number. Two
    /// surfaces draw one, and each parks its own agent's reader — which is what "independently
    /// scrolling" means (TR-5, INS-1).
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
        let moved = match surface_id {
            SurfaceId::Transcript | SurfaceId::Inspector => {
                let Some(agent_id) = self.agent_shown_by(surface_id) else {
                    return;
                };
                self.scroll
                    .scroll_conversation(&agent_id, viewport, direction, metrics)
            }
            _ => self.scroll.scroll_panel(surface_id, viewport, direction),
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
        let surfaces = layout::workspace(
            Rect::new(0, 0, 120, 24),
            WorkspaceInput {
                rail: true,
                ..WorkspaceInput::default()
            },
        );
        let mut state = canonical_state();

        assert_eq!(state.focused(&surfaces), Some(SurfaceId::Agents));

        state.focus_surface(&surfaces, SurfaceId::Transcript);
        assert_eq!(state.focused(&surfaces), Some(SurfaceId::Transcript));

        let before = state.revision();
        state.focus_surface(&surfaces, SurfaceId::Status);
        assert_eq!(
            state.focused(&surfaces),
            Some(SurfaceId::Transcript),
            "the status line is not a focus stop, so the press must leave focus where it was"
        );
        assert_eq!(
            state.revision(),
            before,
            "a press that changes nothing must not force a repaint"
        );
    }

    #[test]
    fn cycling_focus_walks_the_ring_and_wraps() {
        let surfaces = layout::workspace(
            Rect::new(0, 0, 120, 24),
            WorkspaceInput {
                rail: true,
                ..WorkspaceInput::default()
            },
        );
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
                Some(SurfaceId::Composer),
                Some(SurfaceId::Agents),
            ],
            "the ring runs from the agent column through the conversation and its input, and wraps"
        );
    }

    fn agent_id(value: &str) -> AgentId {
        AgentId::new(value).unwrap_or_else(|error| panic!("invalid fixture: {error}"))
    }

    #[test]
    fn canonical_projection_selects_nobody_when_a_background_agent_appears() {
        let state = canonical_state();

        assert_eq!(
            state.selected_agent().map(|agent| agent.id.as_str()),
            None,
            "the primary is on screen without being selected, and B's arrival selects nothing"
        );
        assert_eq!(state.sub_agents().count(), 1);
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
        assert_eq!(selected(&state), "none");

        state.move_selection(Direction::Backward);
        assert_eq!(
            selected(&state),
            "agent-b",
            "from nothing, either arrow lands on the first sub-agent"
        );

        let at_end = state.revision();
        state.move_selection(Direction::Backward);
        assert_eq!(
            selected(&state),
            "agent-b",
            "clamped at the first sub-agent"
        );
        state.move_selection(Direction::Forward);
        assert_eq!(selected(&state), "agent-b", "clamped at the last sub-agent");
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
