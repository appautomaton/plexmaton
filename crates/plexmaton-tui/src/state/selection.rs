//! What the user has selected, and what copying it returns.
//!
//! Keyboard ranges name entries; pointer ranges name visible text within them, never terminal
//! cells. Scrolling and reflow preserve those content endpoints. The contract is
//! [`specs/selection-and-copy.md`](../../../../.agents/specs/selection-and-copy.md).

use plexmaton_core::{AgentId, ToolDetail};

use super::{AgentView, TranscriptEntryView, ViewState};
use crate::{
    intent::Direction,
    surface::{SurfaceId, SurfaceTree},
};
mod text;
pub(crate) use text::TextPoint;
use text::TextRange;

#[derive(Clone, Debug, Eq, PartialEq)]
enum Range {
    Entries { anchor: usize, focus: usize },
    Text(TextRange),
}

/// A range over one surface's entries, in that surface's own order.
///
/// `anchor` is where the selection started and `focus` is the end the user is moving; either may be
/// the larger. Normalizing on write would lose which end moves next, and normalizing on read is one
/// comparison.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Selection {
    /// Which surface's list this indexes. A selection never spans two.
    pub surface: SurfaceId,
    /// Whose content it is, so a selection cannot survive onto a different agent's list.
    pub agent: AgentId,
    range: Range,
}

impl Selection {
    pub(super) const fn at(surface: SurfaceId, agent: AgentId, index: usize) -> Self {
        Self {
            surface,
            agent,
            range: Range::Entries {
                anchor: index,
                focus: index,
            },
        }
    }

    /// First and last selected entry, in list order.
    #[must_use]
    pub const fn bounds(&self) -> (usize, usize) {
        let (anchor, focus) = self.indices();
        if anchor <= focus {
            (anchor, focus)
        } else {
            (focus, anchor)
        }
    }

    /// How many entries are selected.
    ///
    /// Named `entries` rather than `len` because a selection is never empty — one with no entries
    /// is `None` — so the `is_empty` a length would imply could only ever answer "no".
    #[must_use]
    pub const fn entries(&self) -> usize {
        let (first, last) = self.bounds();
        last.saturating_sub(first).saturating_add(1)
    }

    pub(crate) const fn is_text(&self) -> bool {
        matches!(self.range, Range::Text(_))
    }

    /// The moving end of the range, which is the entry a disclosure command addresses.
    pub(super) const fn focus_index(&self) -> usize {
        self.indices().1
    }

    const fn indices(&self) -> (usize, usize) {
        match &self.range {
            Range::Entries { anchor, focus } => (*anchor, *focus),
            Range::Text(range) => (range.anchor.index, range.focus.index),
        }
    }
}

/// Which entries of one list a frame should paint as selected.
///
/// Handed to content functions instead of the selection itself, so a content function cannot ask
/// which surface it is drawing or which agent owns it — it only asks whether entry `n` is in.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Selected(Option<(usize, usize)>);

impl Selected {
    pub(crate) const fn contains(self, index: usize) -> bool {
        match self.0 {
            Some((first, last)) => index >= first && index <= last,
            None => false,
        }
    }
}

/// Text the user asked to put on the clipboard.
///
/// Leaves the workspace as a value for the same reason a submission does (COM-3): nothing in this
/// crate may touch the terminal or the host, so the composition root is what owns the sink.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CopyRequest {
    /// Original source for entry/input selections, visible plain text for pointer ranges; never terminal cells.
    pub text: String,
    /// How many transcript entries it came from; zero for an editable input selection.
    pub entries: usize,
}

/// Local conversation feedback for a selected-text request; never a delivery acknowledgement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CopyNote {
    Preparing,
    Unavailable,
    Capacity,
    Changed,
}

impl ViewState {
    pub(crate) fn set_copy_note(&mut self, note: Option<CopyNote>) {
        let next = note.and_then(|note| self.selection.clone().map(|selection| (selection, note)));
        if self.copy_note != next {
            self.copy_note = next;
            self.touch();
        }
    }

    pub(crate) fn copy_note(&self, surface: SurfaceId) -> Option<CopyNote> {
        let (selection, note) = self.copy_note.as_ref()?;
        (selection.surface == surface && self.selection.as_ref() == Some(selection))
            .then_some(*note)
    }
}

impl ViewState {
    /// The current selection, if there is one.
    #[must_use]
    pub const fn selection(&self) -> Option<&Selection> {
        self.selection.as_ref()
    }

    /// Which entries of one surface's list are selected, for the frame drawing it.
    pub(crate) fn selected_in(&self, surface: SurfaceId, agent_id: &AgentId) -> Selected {
        Selected(self.selection.as_ref().and_then(|selection| {
            (selection.surface == surface
                && &selection.agent == agent_id
                && matches!(selection.range, Range::Entries { .. }))
            .then(|| selection.bounds())
        }))
    }

    /// Extends the selection by one entry, starting one if there is none.
    ///
    /// With nothing selected this takes the newest entry, because everything in this workspace is
    /// append-ordered and the newest is what the surface is showing. Starting at the end the arrow
    /// came from would make the same conversation select differently in its two presentations.
    pub fn select(&mut self, surfaces: &SurfaceTree, direction: Direction) {
        let Some((surface, agent_id)) = self.content_target(surfaces) else {
            return;
        };
        let count = self.entry_count(surface, &agent_id);
        let Some(last) = count.checked_sub(1) else {
            return;
        };
        // A selection clamped at either end of the list does not move, and an unchanged highlight
        // must not cost a frame — the same rule `move_selection` follows on the agent rail (FR-1).
        let changed = match self.selection.as_mut() {
            Some(selection) if selection.surface == surface && selection.agent == agent_id => {
                let (anchor, focus) = selection.indices();
                let next = match direction {
                    Direction::Forward => focus.saturating_add(1).min(last),
                    Direction::Backward => focus.saturating_sub(1),
                };
                let moved = next != focus || matches!(selection.range, Range::Text(_));
                selection.range = Range::Entries {
                    anchor,
                    focus: next,
                };
                moved
            }
            _ => {
                self.selection = Some(Selection::at(surface, agent_id, last));
                true
            }
        };
        if changed {
            self.touch();
        }
    }

    /// Establishes a keyboard-style entry selection for component fixtures.
    #[cfg(test)]
    pub(crate) fn begin_selection(&mut self, surface: SurfaceId, agent: AgentId, index: usize) {
        let next = Selection::at(surface, agent, index);
        if self.selection.as_ref() != Some(&next) {
            self.selection = Some(next);
            self.touch();
        }
    }

    /// Drops the selection, reporting whether there was one. A rung on the `Escape` ladder (INV-6).
    pub fn clear_selection(&mut self) -> bool {
        let had = self.selection.take().is_some();
        if had {
            self.touch();
        }
        had
    }

    /// Drops a selection whose surface has stopped showing the agent it indexes (SEL-3).
    ///
    /// A selection names an agent so that it cannot be read against a different one, and
    /// [`Self::selected_in`] honours that by not highlighting it — but a highlight that has quietly
    /// gone is not the same as a selection that has gone, and [`Self::copy`] reads through the
    /// stored agent. Left alone, moving the agent selection and pressing the copy key returns the
    /// text of a conversation that is no longer on the screen.
    ///
    /// Dropped rather than rebound onto the new agent: index three of A's messages is a different
    /// message in B's, so carrying the range across would silently select something the user never
    /// pointed at. Losing the selection is visible; the alternative is not.
    pub(super) fn prune_selection(&mut self) -> bool {
        let stale = self.selection.as_ref().is_some_and(|selection| {
            self.agent_shown_by(selection.surface).as_ref() != Some(&selection.agent)
        });
        if stale {
            self.selection = None;
        }
        stale
    }

    /// Whose content a surface is currently drawing, for the surfaces that draw one agent's.
    ///
    /// The single answer to "what is this panel showing", so that what a key selects, what a frame
    /// highlights, what a wheel scrolls, and what a copy returns cannot come to four different
    /// conclusions. Two surfaces draw a conversation and they never draw the same agent: the
    /// conversation is the primary's, and the second window is the selected agent's when that is
    /// someone else (INS-1).
    pub(crate) fn agent_shown_by(&self, surface: SurfaceId) -> Option<AgentId> {
        match surface {
            SurfaceId::Inspector => self.agents.peeked().map(|agent| agent.id.clone()),
            SurfaceId::Transcript => self.agents.primary().map(|agent| agent.id.clone()),
            _ => None,
        }
    }

    /// The semantic source of everything selected, ready for a clipboard.
    ///
    /// Reads from the projection, which holds what the producer sent, and never from the cells the
    /// renderer painted. That is what makes the answer independent of width, scroll position, and
    /// decoration — and it is why an artifact copies as its pointer rather than as its label.
    #[must_use]
    pub(crate) fn copy_entries(&self) -> Option<CopyRequest> {
        let selection = self.selection.as_ref()?;
        let agent = self.agents.get(&selection.agent)?;
        if selection.is_text() {
            return None;
        }
        let (first, last) = selection.bounds();
        let sources = self.sources(
            selection.surface,
            agent,
            first,
            last.saturating_sub(first).saturating_add(1),
        );
        (!sources.is_empty()).then(|| CopyRequest {
            entries: sources.len(),
            text: sources.join("\n"),
        })
    }

    /// Which list a selection key addresses: the focused surface's, for the agent it is showing.
    ///
    /// The same derivation as [`Self::text_target`](Self::text_target) and for the same reason: two
    /// answers to "what am I acting on" is how a copy returns the wrong agent's conversation.
    fn content_target(&self, surfaces: &SurfaceTree) -> Option<(SurfaceId, AgentId)> {
        let surface = self.focus.resolve(surfaces)?;
        Some((surface, self.agent_shown_by(surface)?))
    }

    /// How many entries a surface offers, without building any of their text.
    fn entry_count(&self, surface: SurfaceId, agent_id: &AgentId) -> usize {
        let Some(agent) = self.agents.get(agent_id) else {
            return 0;
        };
        match surface {
            SurfaceId::Transcript | SurfaceId::Inspector => agent.entries().count(),
            _ => 0,
        }
    }

    /// The semantic strings of `count` entries starting at `first`, in the order the surface lists
    /// them.
    ///
    /// The order here *is* the order `content` draws, because an index that meant different entries
    /// in the two places would select one thing and copy another.
    ///
    /// The range is applied to the iterators rather than to a finished vector, so copying one
    /// message out of five thousand allocates one string: `map` is lazy, and a skipped entry is
    /// never cloned. Cloning a whole history in order to throw most of it away is exactly what
    /// `standards/rust.md` asks not to do.
    fn sources(
        &self,
        surface: SurfaceId,
        agent: &AgentView,
        first: usize,
        count: usize,
    ) -> Vec<String> {
        match surface {
            SurfaceId::Transcript | SurfaceId::Inspector => agent
                .entries()
                .skip(first)
                .take(count)
                .filter_map(entry_source)
                .collect(),
            _ => Vec::new(),
        }
    }
}

fn entry_source(entry: &TranscriptEntryView) -> Option<String> {
    match entry {
        TranscriptEntryView::Text(item) => Some(item.source.clone()),
        TranscriptEntryView::Tool(tool) => tool_source(&tool.presentation),
        // The pointer, not the human label: the pointer is the stable artifact value (SEL-2).
        TranscriptEntryView::Artifact(artifact) => Some(artifact.pointer.clone()),
        // Recipient travels with the summary because the entry belongs to its producer.
        TranscriptEntryView::Mail(mail) => Some(format!("{}: {}", mail.to, mail.summary)),
    }
}

/// Invocation then outcome, exactly as retained and with only the selection's neutral separator.
fn tool_source(presentation: &plexmaton_core::ToolPresentation) -> Option<String> {
    let parts: Vec<_> = [
        presentation.invocation.as_ref(),
        presentation.outcome.as_ref(),
    ]
    .into_iter()
    .flatten()
    .map(detail_source)
    .collect();
    (!parts.is_empty()).then(|| parts.join("\n"))
}

fn detail_source(detail: &ToolDetail) -> std::borrow::Cow<'_, str> {
    match detail {
        ToolDetail::Text { source, .. } => source.as_str().into(),
        ToolDetail::Diff { patch } => patch.as_str().into(),
        ToolDetail::Command(command) => crate::content::command_transcript_source(
            &command.source,
            &command.workspace_root,
            command.timeout_ms,
        )
        .into(),
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{
        AgentId, AgentStatus, ConversationEvent, ConversationEventEnvelope, EventSequence,
        ToolDetail, ToolPresentation, TranscriptItemId, TranscriptRole,
    };
    use proptest::{collection::vec, prelude::*};
    use ratatui::{
        Terminal,
        backend::TestBackend,
        crossterm::event::{
            Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
        },
        layout::Rect,
    };
    use std::{collections::BTreeMap, time::Duration};

    use super::tool_source;
    use crate::{Workspace, state::Selection, surface::SurfaceId, test_support::canonical_state};

    /// ENT-1 and SEL-2: copy follows the unified first-appearance order across entry kinds.
    #[test]
    fn copying_a_conversation_preserves_interleaved_entry_sources() {
        let mut state = canonical_state();
        let agent = AgentId::new("agent-b").unwrap_or_else(|error| panic!("fixture: {error}"));
        let count = state.agent(&agent).map_or(0, |view| view.entries().count());
        state.selection = Some(Selection {
            surface: SurfaceId::Inspector,
            agent,
            range: super::Range::Entries {
                anchor: 0,
                focus: count.saturating_sub(1),
            },
        });

        let copied = state
            .copy_entries()
            .unwrap_or_else(|| panic!("the selected conversation has semantic source"));
        assert_eq!(
            copied.entries, 3,
            "a tool with no retained detail contributes no invented label"
        );
        assert_eq!(
            copied.text,
            "I found the surface-routing boundary and am checking overlap behavior.\n\
             artifact://agent-b/interaction-findings\n\
             agent-a: Routing stays centralized and z-ordered."
        );
    }

    /// ENT-4 and SEL-2: tool copy is the retained invocation then outcome, without UI labels.
    #[test]
    fn tool_copy_preserves_every_retained_source_in_producer_order() {
        let invocation = "path: crates/plexmaton-tui/src/content.rs";
        let outcome = "*** Begin Patch\n*** Update File: src/lib.rs\n@@ bytes 0..3; old_bytes=3; new_bytes=3 @@\n-old\n+new\n*** End Patch";
        let copied = tool_source(&ToolPresentation {
            invocation: Some(ToolDetail::Text {
                source: invocation.to_owned(),
                omitted_bytes: 0,
            }),
            outcome: Some(ToolDetail::Diff {
                patch: outcome.to_owned(),
            }),
        })
        .unwrap_or_else(|| panic!("retained tool detail has a copy source"));

        assert_eq!(copied, format!("{invocation}\n{outcome}"));
        assert!(!copied.contains("invocation:"));
        assert!(!copied.contains("outcome:"));
    }

    /// ENT-4: copying a whole tool entry retains cwd and timeout; only modal copy extracts shell source.
    #[test]
    fn command_tool_copy_keeps_the_complete_invocation_context() {
        let copied = tool_source(&ToolPresentation {
            invocation: Some(ToolDetail::Command(Box::new(
                plexmaton_core::CommandInvocation {
                    source: "echo one\necho \"two\"".into(),
                    workspace_root: "/workspace".into(),
                    timeout_ms: 42,
                },
            ))),
            outcome: None,
        })
        .expect("complete invocation");
        assert_eq!(
            copied,
            "Command \"echo one\\necho \\\"two\\\"\"\ncwd: \"/workspace\"\ntimeout_ms: 42"
        );
    }

    /// SEL-3: a selection cannot outlive the surface having stopped showing its agent.
    ///
    /// The failure this pins is invisible rather than loud. `selected_in` stops matching, so the
    /// highlight disappears and the workspace looks as though nothing is selected — while `copy`
    /// still reads through the stored agent and hands out the text of a conversation that is no
    /// longer on the screen.
    #[test]
    fn a_selection_does_not_survive_the_surface_changing_agents() {
        use plexmaton_core::{AgentStatus, ConversationEvent};

        use crate::test_support::Conversation;

        // Three agents, so the window can change from one sub-agent to another.
        let mut conversation = Conversation::canonical();
        conversation.emit(ConversationEvent::AgentCreated {
            agent_id: AgentId::new("agent-c").unwrap_or_else(|error| panic!("fixture: {error}")),
            label: "Agent C · review".into(),
            status: AgentStatus::Running,
        });
        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 30))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        workspace.emit(conversation.drain());

        let mut step = |workspace: &mut Workspace, event: Option<&Event>| {
            if let Some(event) = event {
                workspace.handle(event);
            }
            let _frame = workspace
                .settled_draw(&mut terminal)
                .unwrap_or_else(|error| panic!("test render: {error}"));
        };
        // Look at B, enter its window, and select something in it.
        step(&mut workspace, None);
        step(
            &mut workspace,
            Some(&key(KeyCode::Down, KeyModifiers::NONE)),
        );
        step(
            &mut workspace,
            Some(&key(KeyCode::Enter, KeyModifiers::NONE)),
        );
        step(&mut workspace, Some(&key(KeyCode::Up, KeyModifiers::SHIFT)));

        let selected = workspace
            .copy_selection()
            .unwrap_or_else(|| panic!("shift-up in the window must select something"));
        assert!(!selected.text.is_empty());

        // Back to the list, and on to C. The window now draws somebody else.
        for _ in 0..workspace.surfaces().len() {
            if workspace.state().focused(workspace.surfaces()) == Some(SurfaceId::Agents) {
                break;
            }
            step(
                &mut workspace,
                Some(&key(KeyCode::BackTab, KeyModifiers::NONE)),
            );
        }
        assert_eq!(
            workspace.state().focused(workspace.surfaces()),
            Some(SurfaceId::Agents)
        );
        let before = workspace.state().revision();
        step(
            &mut workspace,
            Some(&key(KeyCode::Down, KeyModifiers::NONE)),
        );

        assert_eq!(
            workspace
                .state()
                .selected_agent()
                .map(|agent| agent.id.as_str()),
            Some("agent-c"),
            "the fixture must actually have moved the window to another agent, or this proves nothing"
        );
        assert!(workspace.state().selection().is_none());
        assert_eq!(
            workspace.copy_selection(),
            None,
            "the copy key must not reach a conversation the user cannot see"
        );
        assert!(
            workspace.state().revision() > before,
            "the highlight has to be repainted away, so the change is a visible one"
        );
    }

    /// FR-1: a selection already at the end of the list does not move, so it costs no frame.
    ///
    /// The agent rail has had this rule and a test for it since step 2; the selection did not, and
    /// held `Shift-↑` at the oldest message repaints for as long as the key is down.
    #[test]
    fn extending_a_clamped_selection_costs_no_frame() {
        let messages: Vec<String> = ["one", "two"]
            .iter()
            .map(|text| (*text).to_owned())
            .collect();
        let mut workspace = selecting(&messages, 100, messages.len());
        let clamped = workspace.state().revision();

        assert_eq!(
            workspace.state().selection().map(Selection::bounds),
            Some((0, 1)),
            "the fixture must already be at the oldest message, or this proves nothing"
        );

        workspace.handle(&key(KeyCode::Up, KeyModifiers::SHIFT));

        assert_eq!(workspace.state().revision(), clamped);
    }

    /// SEL-1, and `ui-ux.md` §selection and copy: mouse capture never makes content uncopyable.
    ///
    /// The workspace owns the screen and the mouse, so what the terminal's own selection would
    /// have done it has to do itself. This drags across three messages and copies them, which is
    /// the whole gesture — before this, a press resolved only against foldable tool rows, so a
    /// drag over a conversation of messages selected nothing and copied nothing.
    #[test]
    fn dragging_across_a_conversation_selects_and_copies_what_it_crossed() {
        let messages: Vec<String> = ["alpha", "bravo", "charlie", "delta"]
            .iter()
            .map(|text| (*text).to_owned())
            .collect();
        let (mut workspace, terminal) = drawn(&messages, 100);
        let rows = message_rows(&workspace, &terminal, &messages);

        workspace.handle(&press(rows["alpha"]));
        assert_eq!(
            workspace.state().selection().map(Selection::bounds),
            None,
            "a press is pending; only a drag creates a selection"
        );

        workspace.handle(&drag(rows["charlie"]));
        assert_eq!(
            workspace.state().selection().map(Selection::bounds),
            Some((0, 2)),
            "the drag carries the moving end with the pointer"
        );
        // Releasing the button is the copy. On macOS the terminal keeps `Cmd-C` for its own
        // selection, which over an owned screen is empty, so a mouse selection that waited for a
        // key was a selection the habit could not copy.
        workspace.handle(&Event::Resize(100, 24));
        let copied = workspace
            .handle(&release(rows["charlie"]))
            .copied
            .unwrap_or_else(|| panic!("releasing a drag copies what it crossed"));
        assert_eq!(copied.text, "alpha\n\nbravo\n\ncharlie");

        // Pressing off the content is the gesture's own undo: without it the only way out of a
        // highlight the mouse made is a key.
        workspace.handle(&press(0));
        assert_eq!(workspace.state().selection(), None);
    }

    /// SEL-6: holding a captured drag at the viewport edge reaches entries beyond the frame.
    #[test]
    fn an_edge_drag_scrolls_and_copies_entries_that_started_off_screen() {
        for width in [60, 100, 160] {
            assert_edge_drag(width);
        }
    }

    /// SEL-6: chrome-centred activation costs only one row of conversation content.
    #[test]
    fn drag_autoscroll_activates_on_the_content_row_beside_chrome() {
        let messages: Vec<String> = (0..60).map(|index| format!("message-{index:02}")).collect();
        let (mut workspace, mut terminal) = drawn(&messages, 100);
        let bounds = workspace
            .surfaces()
            .get(SurfaceId::Transcript)
            .unwrap_or_else(|| panic!("conversation surface"))
            .bounds;
        let wheel_at = MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: bounds.x.saturating_add(3),
            row: bounds.y.saturating_add(bounds.height / 2),
            modifiers: KeyModifiers::NONE,
        };
        for _ in 0..4 {
            workspace.handle(&Event::Mouse(wheel_at));
            workspace
                .settled_draw(&mut terminal)
                .unwrap_or_else(|error| panic!("draw scrolled conversation: {error}"));
        }

        let visible = visible_message_rows(&workspace, &terminal, &messages);
        let (_, anchor_row) = visible
            .iter()
            .nth(visible.len() / 2)
            .unwrap_or_else(|| panic!("a long conversation has visible messages"));
        workspace.handle(&press(*anchor_row));

        workspace.handle(&drag(bounds.y.saturating_add(2)));
        assert_eq!(workspace.drag_autoscroll_deadline(), None);
        workspace.handle(&drag(bounds.y.saturating_add(1)));
        assert!(workspace.drag_autoscroll_deadline().is_some());

        workspace.handle(&drag(bounds.y.saturating_add(bounds.height / 2)));
        assert_eq!(workspace.drag_autoscroll_deadline(), None);
        workspace.handle(&drag(bounds.bottom().saturating_sub(3)));
        assert_eq!(workspace.drag_autoscroll_deadline(), None);
        workspace.handle(&drag(bounds.bottom().saturating_sub(2)));
        let slow_from = transcript_offset(&workspace);
        advance_edge_drag(&mut workspace, &mut terminal);
        assert_eq!(transcript_offset(&workspace), slow_from.saturating_add(1));

        workspace.handle(&drag(bounds.bottom().saturating_sub(1)));
        let medium_from = transcript_offset(&workspace);
        advance_edge_drag(&mut workspace, &mut terminal);
        assert_eq!(transcript_offset(&workspace), medium_from.saturating_add(2));

        workspace.handle(&drag(bounds.bottom()));
        let fast_from = transcript_offset(&workspace);
        advance_edge_drag(&mut workspace, &mut terminal);
        assert_eq!(transcript_offset(&workspace), fast_from.saturating_add(3));

        let selected = workspace.state().selection().cloned();
        workspace.handle(&Event::FocusLost);
        assert_eq!(workspace.drag_autoscroll_deadline(), None);
        assert_eq!(workspace.state().selection(), selected.as_ref());
        workspace.handle(&drag(bounds.bottom()));
        assert!(workspace.drag_autoscroll_deadline().is_some());
    }

    fn assert_edge_drag(width: u16) {
        let messages: Vec<String> = (0..60).map(|index| format!("message-{index:02}")).collect();
        let (mut workspace, mut terminal) = drawn(&messages, width);
        let bounds = workspace
            .surfaces()
            .get(SurfaceId::Transcript)
            .unwrap_or_else(|| panic!("conversation surface"))
            .bounds;
        let wheel_at = MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: bounds.x.saturating_add(3),
            row: bounds.y.saturating_add(bounds.height / 2),
            modifiers: KeyModifiers::NONE,
        };
        for _ in 0..8 {
            workspace.handle(&Event::Mouse(wheel_at));
            workspace
                .settled_draw(&mut terminal)
                .unwrap_or_else(|error| panic!("draw scrolled conversation: {error}"));
        }
        let before_offset = workspace
            .surfaces()
            .viewport(SurfaceId::Transcript)
            .unwrap_or_else(|| panic!("conversation viewport"))
            .offset;
        let visible = visible_message_rows(&workspace, &terminal, &messages);
        let (anchor, anchor_row) = visible
            .iter()
            .nth(visible.len() / 2)
            .unwrap_or_else(|| panic!("a long conversation has visible messages"));
        // The pointer has crossed the chrome into the neighbouring region. Capture keeps the
        // gesture addressed to this conversation and selects at the fast bounded rate.
        let below_chrome = bounds.bottom();

        workspace.handle(&press(*anchor_row));
        workspace.handle(&drag(below_chrome));
        let before_entries = workspace.state().selection().map_or(0, Selection::entries);
        for _ in 0..6 {
            let deadline = workspace
                .drag_autoscroll_deadline()
                .unwrap_or_else(|| panic!("edge drag owns a wakeup"));
            assert!(workspace.advance_drag_autoscroll(deadline));
            workspace
                .settled_draw(&mut terminal)
                .unwrap_or_else(|error| panic!("draw autoscrolled conversation: {error}"));
        }

        let after_offset = workspace
            .surfaces()
            .viewport(SurfaceId::Transcript)
            .unwrap_or_else(|| panic!("conversation viewport after drag"))
            .offset;
        assert!(
            after_offset > before_offset,
            "the held drag moved toward newer entries at width {width}"
        );
        assert!(
            workspace
                .state()
                .selection()
                .is_some_and(|selection| selection.entries() > before_entries),
            "the moving end followed content revealed by autoscroll at width {width}"
        );

        // Keep the pointer held until the tail. This catches a subtle boundary regression: if the
        // timer checks whether the viewport can move before resolving the newly revealed edge,
        // the final entry is visible but never joins the semantic selection.
        for _ in 0..256 {
            let Some(deadline) = workspace.drag_autoscroll_deadline() else {
                break;
            };
            let _changed = workspace.advance_drag_autoscroll(deadline);
            workspace
                .settled_draw(&mut terminal)
                .unwrap_or_else(|error| panic!("draw autoscroll boundary: {error}"));
        }
        assert_eq!(
            workspace.drag_autoscroll_deadline(),
            None,
            "the content boundary disarms the timer at width {width}"
        );

        let copied = workspace
            .handle(&release(below_chrome))
            .copied
            .unwrap_or_else(|| panic!("release copies the autoscrolled selection"));
        assert!(copied.text.contains(anchor));
        assert!(
            copied.text.contains("message-59"),
            "the last entry revealed at the boundary is selected at width {width}"
        );
        let settled = workspace.state().revision();
        assert!(
            !workspace.advance_drag_autoscroll(std::time::Instant::now() + Duration::from_secs(1))
        );
        assert_eq!(workspace.state().revision(), settled);
    }

    fn advance_edge_drag(workspace: &mut Workspace, terminal: &mut Terminal<TestBackend>) {
        let deadline = workspace
            .drag_autoscroll_deadline()
            .unwrap_or_else(|| panic!("edge drag owns a wakeup"));
        assert!(workspace.advance_drag_autoscroll(deadline));
        workspace
            .settled_draw(terminal)
            .unwrap_or_else(|error| panic!("draw autoscrolled conversation: {error}"));
    }

    fn transcript_offset(workspace: &Workspace) -> usize {
        workspace
            .surfaces()
            .viewport(SurfaceId::Transcript)
            .unwrap_or_else(|| panic!("conversation viewport"))
            .offset
    }

    fn mouse(kind: MouseEventKind, row: u16) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            column: 30,
            row,
            modifiers: KeyModifiers::NONE,
        })
    }

    fn press(row: u16) -> Event {
        let mut event = mouse(MouseEventKind::Down(MouseButton::Left), row);
        if let Event::Mouse(event) = &mut event {
            event.column = 1;
        }
        event
    }

    fn drag(row: u16) -> Event {
        mouse(MouseEventKind::Drag(MouseButton::Left), row)
    }

    fn release(row: u16) -> Event {
        mouse(MouseEventKind::Up(MouseButton::Left), row)
    }

    /// A drawn workspace with no selection, so a test can start the gesture itself.
    fn drawn(messages: &[String], width: u16) -> (Workspace, Terminal<TestBackend>) {
        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(width, 24))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        workspace.emit(timeline(messages));
        let _frame = workspace
            .settled_draw(&mut terminal)
            .unwrap_or_else(|error| panic!("test render: {error}"));
        (workspace, terminal)
    }

    /// The screen row each message was painted on, read off the frame rather than computed.
    ///
    /// A row number worked out from the fixture would be a second layout, and the gesture under
    /// test is the one that resolves a real cell against the frame that was really drawn (FR-3).
    fn message_rows(
        workspace: &Workspace,
        terminal: &Terminal<TestBackend>,
        messages: &[String],
    ) -> BTreeMap<String, u16> {
        let rows = visible_message_rows(workspace, terminal, messages);
        assert_eq!(
            rows.len(),
            messages.len(),
            "every fixture message has to be on screen or the gesture proves nothing"
        );
        rows
    }

    fn visible_message_rows(
        workspace: &Workspace,
        terminal: &Terminal<TestBackend>,
        messages: &[String],
    ) -> BTreeMap<String, u16> {
        let bounds = workspace
            .surfaces()
            .get(SurfaceId::Transcript)
            .unwrap_or_else(|| panic!("the conversation is registered at this width"))
            .bounds;
        let buffer = terminal.backend().buffer();
        let mut rows = BTreeMap::new();
        for row in bounds.y..bounds.bottom() {
            let text = crate::test_support::region_text(
                buffer,
                Rect {
                    y: row,
                    height: 1,
                    ..bounds
                },
            );
            for message in messages {
                if text.contains(message.as_str()) {
                    rows.entry(message.clone()).or_insert(row);
                }
            }
        }
        rows
    }

    /// One assistant message per string, on one agent, as the producer would send them.
    fn timeline(messages: &[String]) -> Vec<ConversationEventEnvelope> {
        let agent_id = AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}"));
        let mut sequence = 0_u64;
        let mut envelopes = Vec::new();
        let mut emit = |event: ConversationEvent| {
            sequence = sequence.saturating_add(1);
            envelopes.push(ConversationEventEnvelope {
                sequence: EventSequence::new(sequence),
                event,
            });
        };
        emit(ConversationEvent::AgentCreated {
            agent_id: agent_id.clone(),
            label: "Agent A".into(),
            status: AgentStatus::Running,
        });
        for (index, text) in messages.iter().enumerate() {
            let item_id = TranscriptItemId::new(format!("item-{index}"))
                .unwrap_or_else(|error| panic!("fixture: {error}"));
            emit(ConversationEvent::TranscriptItemStarted {
                agent_id: agent_id.clone(),
                item_id: item_id.clone(),
                role: TranscriptRole::Assistant,
            });
            emit(ConversationEvent::TranscriptDelta {
                agent_id: agent_id.clone(),
                item_id,
                item_revision: 1,
                text: text.clone(),
            });
        }
        envelopes
    }

    fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
        Event::Key(KeyEvent::new(code, modifiers))
    }

    /// A workspace showing `messages`, drawn at `width`, with the conversation focused and
    /// `extend` messages selected back from the newest.
    fn selecting(messages: &[String], width: u16, extend: usize) -> Workspace {
        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(width, 24))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        workspace.emit(timeline(messages));

        // A frame first: focus and hit testing resolve against the registry the last frame drew, so
        // a key pressed before anything was painted has no workspace to act on (FR-3).
        let mut step = |workspace: &mut Workspace, event: Option<&Event>| {
            if let Some(event) = event {
                workspace.handle(event);
            }
            let _frame = workspace
                .settled_draw(&mut terminal)
                .unwrap_or_else(|error| panic!("test render: {error}"));
        };
        step(&mut workspace, None);
        // Walk the ring to the conversation; how many stops precede it depends on the width.
        for _ in 0..workspace.surfaces().len() {
            if workspace.state().focused(workspace.surfaces()) == Some(SurfaceId::Transcript) {
                break;
            }
            step(&mut workspace, Some(&key(KeyCode::Tab, KeyModifiers::NONE)));
        }
        assert_eq!(
            workspace.state().focused(workspace.surfaces()),
            Some(SurfaceId::Transcript),
            "the conversation is on the ring at every width"
        );
        for _ in 0..=extend {
            step(&mut workspace, Some(&key(KeyCode::Up, KeyModifiers::SHIFT)));
        }
        workspace
    }

    fn messages() -> impl Strategy<Value = Vec<String>> {
        // Spaces so the text wraps at a narrow width, and no newline so a joined copy stays
        // unambiguous about where one message ends.
        vec("[a-z ]{1,80}", 1..12_usize)
    }

    proptest! {
        // A property here costs a full render, so the case count keeps the per-change suite fast.
        #![proptest_config(ProptestConfig::with_cases(48))]

        /// SEL-2: copying returns the source of exactly the entries between the two endpoints.
        ///
        /// Not "the text that was on screen": the expectation is built from the strings the
        /// producer sent, so a copy that read painted cells, dropped a scrolled-away message, or
        /// picked up a border glyph fails here rather than being noticed by whoever pasted it.
        #[test]
        fn copying_returns_the_source_between_the_endpoints(
            messages in messages(),
            extend in 0..14_usize,
        ) {
            let workspace = selecting(&messages, 100, extend);
            let selection = workspace
                .state()
                .selection()
                .unwrap_or_else(|| panic!("shift-up with messages present must select"));
            let (first, last) = selection.bounds();

            prop_assert_eq!(last, messages.len().saturating_sub(1), "extends from the newest");
            prop_assert_eq!(first, messages.len().saturating_sub(1).saturating_sub(extend.min(messages.len() - 1)));

            let copied = workspace
                .copy_selection()
                .unwrap_or_else(|| panic!("a selection must copy to something"));
            let expected: Vec<&str> = messages
                .get(first..=last)
                .unwrap_or_default()
                .iter()
                .map(String::as_str)
                .collect();
            prop_assert_eq!(copied.text, expected.join("\n"));
            prop_assert_eq!(copied.entries, selection.entries());
        }

        /// SEL-1: presentation cannot change what a selection means.
        ///
        /// The same conversation and the same keystrokes at two widths, with the reader scrolled to
        /// different places, must copy the same characters — which is the whole reason a selection
        /// is a range over content rather than over rows.
        #[test]
        fn copy_is_the_same_at_every_width_and_scroll_position(
            messages in messages(),
            extend in 0..14_usize,
            narrow in 50..70_u16,
            wide in 100..160_u16,
        ) {
            let at_narrow = selecting(&messages, narrow, extend);
            let mut at_wide = selecting(&messages, wide, extend);

            // Move the reader somewhere else entirely before copying. The wheel is hover-routed, so
            // this is the conversation being scrolled without focus or selection being touched.
            let mut terminal = Terminal::new(TestBackend::new(wide, 24))
                .unwrap_or_else(|error| panic!("test terminal: {error}"));
            for _ in 0..6 {
                at_wide.handle(&key(KeyCode::Up, KeyModifiers::NONE));
                let _frame = at_wide
                    .settled_draw(&mut terminal)
                    .unwrap_or_else(|error| panic!("test render: {error}"));
            }

            prop_assert_eq!(
                at_narrow.copy_selection().map(|request| request.text),
                at_wide.copy_selection().map(|request| request.text)
            );
        }
    }
}
