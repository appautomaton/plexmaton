//! Which agent the user opened, and how they asked for it to be shown.
//!
//! Inspection is a separate axis from selection. The conversation underneath keeps showing whatever
//! is selected, so with one agent selected and another inspected there are two agents on screen —
//! which is the only arrangement in which a shelf shows something the workspace does not already.

use plexmaton_core::AgentId;
use ratatui::layout::Rect;

use crate::layout::InspectorRequest;

/// The open inspector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InspectorView {
    /// The agent whose detail is on screen.
    pub agent: AgentId,
    /// Whether it survives the selection moving on.
    pub pinned: bool,
    /// Whether the user asked for the whole conversation region.
    pub maximized: bool,
    /// Rows the user chose for the shelf, if they ever changed it.
    pub rows: Option<u16>,
}

/// The inspector, open or not, and the drag that may be resizing it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct Inspector {
    open: Option<InspectorView>,
    /// Whether the pointer went down on the resize edge rather than in the body.
    ///
    /// Kept here rather than in the router, which owns *that a gesture is in progress* and has no
    /// business knowing what the gesture means. Without it, dragging anywhere inside the inspector
    /// would resize it, and a drag that started on the text would move the edge under the pointer.
    grabbed: bool,
}

impl Inspector {
    pub(super) const fn open(&self) -> Option<&InspectorView> {
        self.open.as_ref()
    }

    /// What layout needs in order to place it.
    pub(super) fn request(&self) -> Option<InspectorRequest> {
        self.open.as_ref().map(|view| InspectorRequest {
            maximized: view.maximized,
            rows: view.rows,
        })
    }

    /// Opens `agent`'s inspector, or re-points an open one.
    ///
    /// Re-pointing keeps the presentation the user chose. Pin, maximize, and height belong to the
    /// surface rather than to what it happens to be showing, so looking at a different agent must
    /// not silently undo three of the user's decisions.
    pub(super) fn show(&mut self, agent: AgentId) -> bool {
        match &mut self.open {
            Some(view) if view.agent == agent => false,
            Some(view) => {
                view.agent = agent;
                true
            }
            None => {
                self.open = Some(InspectorView {
                    agent,
                    pinned: false,
                    maximized: false,
                    rows: None,
                });
                true
            }
        }
    }

    pub(super) fn dismiss(&mut self) -> bool {
        self.grabbed = false;
        self.open.take().is_some()
    }

    pub(super) fn toggle_pin(&mut self) -> bool {
        let Some(view) = &mut self.open else {
            return false;
        };
        view.pinned = !view.pinned;
        true
    }

    pub(super) fn toggle_maximized(&mut self) -> bool {
        let Some(view) = &mut self.open else {
            return false;
        };
        view.maximized = !view.maximized;
        true
    }

    /// Sets the shelf's height from the rows it currently occupies.
    ///
    /// Measured rather than remembered, because layout clamps what it is given and only the drawn
    /// rectangle knows the result. Storing an unclamped number instead would let repeated presses
    /// at the boundary accumulate, and the first press back would then do nothing.
    pub(super) fn set_rows(&mut self, rows: u16) -> bool {
        let Some(view) = &mut self.open else {
            return false;
        };
        if view.rows == Some(rows) {
            return false;
        }
        view.rows = Some(rows);
        true
    }

    /// Records whether a press landed on the resize edge.
    ///
    /// `bounds` is `None` when the press was not on the inspector at all, which clears any grab
    /// rather than leaving a stale one for the next drag to act on.
    pub(super) fn grab(&mut self, bounds: Option<Rect>, at_row: u16) {
        self.grabbed = bounds.is_some_and(|bounds| at_row.saturating_add(1) == bounds.bottom());
    }

    pub(super) const fn release(&mut self) {
        self.grabbed = false;
    }

    pub(super) const fn is_grabbed(&self) -> bool {
        self.grabbed
    }

    /// Re-points an unpinned inspector at the new selection. A pinned one keeps its agent.
    ///
    /// This is the whole of what pinned means, and both halves are load-bearing. An unpinned
    /// inspector is a peek at whatever the user is looking at, so it follows them — closing it
    /// instead would end the peek at the moment it became useful, which is when the user goes back
    /// to the conversation they were reading. Pinning is the user saying "keep this one", and it is
    /// what puts two different agents on the screen at once.
    pub(super) fn follow(&mut self, agent: AgentId) -> bool {
        match &mut self.open {
            Some(view) if !view.pinned && view.agent != agent => {
                view.agent = agent;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::AgentId;
    use ratatui::layout::Rect;

    use super::Inspector;

    fn agent(value: &str) -> AgentId {
        AgentId::new(value).unwrap_or_else(|error| panic!("invalid fixture: {error}"))
    }

    /// A peek follows the user; a pin is how they leave one behind.
    ///
    /// Both halves matter. Following is what stops the peek ending the moment the user goes back to
    /// the conversation they were reading, and not following is what puts a second agent on screen.
    #[test]
    fn an_unpinned_inspector_follows_the_selection_and_a_pinned_one_stays() {
        let mut inspector = Inspector::default();
        inspector.show(agent("agent-b"));

        assert!(inspector.follow(agent("agent-a")), "a peek follows");
        assert_eq!(
            inspector.open().map(|view| view.agent.clone()),
            Some(agent("agent-a"))
        );
        assert!(
            !inspector.follow(agent("agent-a")),
            "and following where it already is changes nothing"
        );

        assert!(inspector.toggle_pin());
        assert!(
            !inspector.follow(agent("agent-b")),
            "a pinned inspector keeps its own agent while the user works elsewhere"
        );
        assert_eq!(
            inspector.open().map(|view| view.agent.clone()),
            Some(agent("agent-a")),
            "which is what puts two different agents on the screen"
        );
    }

    /// Presentation belongs to the surface, not to what it is showing.
    #[test]
    fn re_pointing_at_another_agent_keeps_the_presentation_the_user_chose() {
        let mut inspector = Inspector::default();
        inspector.show(agent("agent-a"));
        inspector.toggle_pin();
        inspector.toggle_maximized();
        inspector.set_rows(14);

        assert!(inspector.show(agent("agent-b")));

        let view = inspector
            .open()
            .unwrap_or_else(|| panic!("it is still open"));
        assert_eq!(view.agent, agent("agent-b"));
        assert!(view.pinned, "pin survives");
        assert!(view.maximized, "so does the presentation");
        assert_eq!(view.rows, Some(14), "and the height they dragged to");
        assert!(
            !inspector.show(agent("agent-b")),
            "re-pointing at the same agent changes nothing and must not force a repaint"
        );
    }

    /// A drag resizes only when it began on the edge, or dragging the text would move the edge.
    #[test]
    fn only_a_press_on_the_bottom_edge_starts_a_resize() {
        let mut inspector = Inspector::default();
        inspector.show(agent("agent-a"));
        let bounds = Rect::new(0, 4, 40, 10);

        inspector.grab(Some(bounds), 8);
        assert!(!inspector.is_grabbed(), "a press in the body is not a grab");

        inspector.grab(Some(bounds), bounds.bottom().saturating_sub(1));
        assert!(inspector.is_grabbed(), "the last row is the resize edge");

        inspector.grab(None, bounds.bottom().saturating_sub(1));
        assert!(
            !inspector.is_grabbed(),
            "a press on another surface clears the grab rather than leaving a stale one"
        );

        inspector.grab(Some(bounds), bounds.bottom().saturating_sub(1));
        inspector.release();
        assert!(!inspector.is_grabbed());
    }

    #[test]
    fn a_closed_inspector_refuses_every_command_without_pretending_to_change() {
        let mut inspector = Inspector::default();

        assert!(!inspector.dismiss());
        assert!(!inspector.toggle_pin());
        assert!(!inspector.toggle_maximized());
        assert!(!inspector.set_rows(12));
        assert!(!inspector.follow(agent("agent-a")));
    }
}
