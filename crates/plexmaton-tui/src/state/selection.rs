//! What the user has selected, and what copying it returns.
//!
//! A selection is a range over a surface's *entries* — messages in a conversation, tools, artifacts
//! and mail in a detail panel — and never a rectangle of cells. That is the whole design: because a
//! range names content, scrolling it out of view, resizing the terminal, or re-wrapping the text
//! cannot change what is selected or what copying it returns. The contract is
//! [`specs/selection-and-copy.md`](../../../../.agents/specs/selection-and-copy.md).

use plexmaton_core::AgentId;

use super::{AgentView, ViewState};
use crate::{
    intent::Direction,
    surface::{SurfaceId, SurfaceTree},
};

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
    anchor: usize,
    focus: usize,
}

impl Selection {
    const fn at(surface: SurfaceId, agent: AgentId, index: usize) -> Self {
        Self {
            surface,
            agent,
            anchor: index,
            focus: index,
        }
    }

    /// First and last selected entry, in list order.
    #[must_use]
    pub const fn bounds(&self) -> (usize, usize) {
        if self.anchor <= self.focus {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
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
    /// Exactly what producers sent, joined by newlines. Never a painted cell, never a border glyph.
    pub text: String,
    /// How many entries it came from, for a caller that wants to say so.
    pub entries: usize,
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
            (selection.surface == surface && &selection.agent == agent_id)
                .then(|| selection.bounds())
        }))
    }

    /// Extends the selection by one entry, starting one if there is none.
    ///
    /// With nothing selected this takes the newest entry, because everything in this workspace is
    /// append-ordered and the newest is what the surface is showing. One rule for both kinds of
    /// list; the alternative — start at the end the arrow came from — reads sensibly in a
    /// conversation and absurdly in a detail panel.
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
                let next = match direction {
                    Direction::Forward => selection.focus.saturating_add(1).min(last),
                    Direction::Backward => selection.focus.saturating_sub(1),
                };
                let moved = next != selection.focus;
                selection.focus = next;
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
            SurfaceId::Activity => self.activity_agent().map(|agent| agent.id.clone()),
            _ => None,
        }
    }

    /// The semantic source of everything selected, ready for a clipboard.
    ///
    /// Reads from the projection, which holds what the producer sent, and never from the cells the
    /// renderer painted. That is what makes the answer independent of width, scroll position, and
    /// decoration — and it is why an artifact copies as its pointer rather than as its label.
    #[must_use]
    pub fn copy(&self) -> Option<CopyRequest> {
        let selection = self.selection.as_ref()?;
        let agent = self.agents.get(&selection.agent)?;
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
            SurfaceId::Transcript | SurfaceId::Inspector => agent.transcript().count(),
            SurfaceId::Activity => agent
                .tool_activity()
                .count()
                .saturating_add(agent.artifacts().count())
                .saturating_add(agent.inbox().count()),
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
                .transcript()
                .skip(first)
                .take(count)
                .map(|item| item.source.clone())
                .collect(),
            SurfaceId::Activity => agent
                .tool_activity()
                .map(|tool| tool.label.clone())
                // The stable reference, not the label: the label is what got truncated on screen.
                .chain(agent.artifacts().map(|artifact| artifact.pointer.clone()))
                // Sender travels with the summary because a recipient must retain it, and a
                // summary alone would lose which agent said it.
                .chain(
                    agent
                        .inbox()
                        .map(|mail| format!("{}: {}", mail.from, mail.summary)),
                )
                .skip(first)
                .take(count)
                .collect(),
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{
        AgentId, AgentStatus, EventSequence, SessionEvent, SessionEventEnvelope, TranscriptItemId,
        TranscriptRole,
    };
    use proptest::{collection::vec, prelude::*};
    use ratatui::{
        Terminal,
        backend::TestBackend,
        crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers},
    };

    use crate::{Workspace, state::Selection, surface::SurfaceId};

    /// SEL-3: a selection cannot outlive the surface having stopped showing its agent.
    ///
    /// The failure this pins is invisible rather than loud. `selected_in` stops matching, so the
    /// highlight disappears and the workspace looks as though nothing is selected — while `copy`
    /// still reads through the stored agent and hands out the text of a conversation that is no
    /// longer on the screen.
    #[test]
    fn a_selection_does_not_survive_the_surface_changing_agents() {
        use plexmaton_core::{AgentStatus, SessionEvent};

        use crate::test_support::Conversation;

        // Three agents, so the window can change from one sub-agent to another.
        let mut conversation = Conversation::canonical();
        conversation.emit(SessionEvent::AgentCreated {
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
                .draw(&mut terminal)
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
            .state()
            .copy()
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
            workspace.state().copy(),
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

    /// One assistant message per string, on one agent, as the producer would send them.
    fn timeline(messages: &[String]) -> Vec<SessionEventEnvelope> {
        let agent_id = AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}"));
        let mut sequence = 0_u64;
        let mut envelopes = Vec::new();
        let mut emit = |event: SessionEvent| {
            sequence = sequence.saturating_add(1);
            envelopes.push(SessionEventEnvelope {
                sequence: EventSequence::new(sequence),
                event,
            });
        };
        emit(SessionEvent::AgentCreated {
            agent_id: agent_id.clone(),
            label: "Agent A".into(),
            status: AgentStatus::Running,
        });
        for (index, text) in messages.iter().enumerate() {
            let item_id = TranscriptItemId::new(format!("item-{index}"))
                .unwrap_or_else(|error| panic!("fixture: {error}"));
            emit(SessionEvent::TranscriptItemStarted {
                agent_id: agent_id.clone(),
                item_id: item_id.clone(),
                role: TranscriptRole::Assistant,
            });
            emit(SessionEvent::TranscriptDelta {
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
                .draw(&mut terminal)
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
        // A property here costs a full render, so the case count is chosen for a pre-commit hook.
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
                .state()
                .copy()
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
                    .draw(&mut terminal)
                    .unwrap_or_else(|error| panic!("test render: {error}"));
            }

            prop_assert_eq!(
                at_narrow.state().copy().map(|request| request.text),
                at_wide.state().copy().map(|request| request.text)
            );
        }
    }
}
