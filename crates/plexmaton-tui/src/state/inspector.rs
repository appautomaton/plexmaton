//! How the user asked the second window to be shown.
//!
//! Which agent it shows is not stored here. The second window shows whichever agent the user
//! selected other than the primary, so its agent *is* the roster's selection, and it is open
//! exactly when that selection is not the primary (INS-1). Holding a copy would be a second source
//! of truth with a synchronization path between them, which is how the first implementation came
//! to show one conversation twice. What lives here is the presentation the user chose for the
//! window — maximized, dragged to a height — and the drag that may be resizing it.

use plexmaton_core::AgentId;
use ratatui::layout::Rect;

use crate::layout::InspectorRequest;

/// The open second window: the agent it shows and how the user asked for it to be shown.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InspectorView {
    /// The agent whose conversation is in the window.
    pub agent: AgentId,
    /// Whether the user asked for the whole conversation region.
    pub maximized: bool,
    /// Rows the user chose for the shelf, if they ever changed it.
    pub rows: Option<u16>,
}

/// The second window's presentation, and the drag that may be resizing it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct Inspector {
    maximized: bool,
    rows: Option<u16>,
    /// Whether the pointer went down on the resize edge rather than in the body.
    ///
    /// Kept here rather than in the router, which owns *that a gesture is in progress* and has no
    /// business knowing what the gesture means. Without it, dragging anywhere inside the window
    /// would resize it, and a drag that started on the text would move the edge under the pointer.
    grabbed: bool,
}

impl Inspector {
    /// The window as it shows `agent`, with the presentation the user chose.
    ///
    /// Presentation belongs to the window rather than to what it happens to be showing, so
    /// selecting a different agent must not silently undo the user's maximize or their height.
    pub(super) fn view(&self, agent: AgentId) -> InspectorView {
        InspectorView {
            agent,
            maximized: self.maximized,
            rows: self.rows,
        }
    }

    /// What layout needs in order to place it.
    pub(super) const fn request(&self) -> InspectorRequest {
        InspectorRequest {
            maximized: self.maximized,
            rows: self.rows,
        }
    }

    /// Forgets the presentation and any grab, for when the window closes.
    ///
    /// A window that reopens later starts at the default height and un-maximized, the same as it
    /// did the first time; a maximize left over from a session the user ended would replace the
    /// conversation without being asked.
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) const fn toggle_maximized(&mut self) {
        self.maximized = !self.maximized;
    }

    /// Sets the shelf's height from the rows it currently occupies.
    ///
    /// Measured rather than remembered, because layout clamps what it is given and only the drawn
    /// rectangle knows the result. Storing an unclamped number instead would let repeated presses
    /// at the boundary accumulate, and the first press back would then do nothing.
    pub(super) fn set_rows(&mut self, rows: u16) -> bool {
        if self.rows == Some(rows) {
            return false;
        }
        self.rows = Some(rows);
        true
    }

    /// Records whether a press landed on the resize edge.
    ///
    /// `bounds` is `None` when the press was not on the window at all, which clears any grab
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
}

#[cfg(test)]
mod tests {
    use plexmaton_core::AgentId;
    use ratatui::layout::Rect;

    use super::Inspector;

    fn agent(value: &str) -> AgentId {
        AgentId::new(value).unwrap_or_else(|error| panic!("invalid fixture: {error}"))
    }

    /// Presentation belongs to the window, not to what it is showing.
    #[test]
    fn the_presentation_survives_the_window_showing_another_agent() {
        let mut inspector = Inspector::default();
        inspector.toggle_maximized();
        inspector.set_rows(14);

        let view = inspector.view(agent("agent-b"));
        assert_eq!(view.agent, agent("agent-b"));
        assert!(view.maximized, "the presentation survives");
        assert_eq!(
            view.rows,
            Some(14),
            "and so does the height they dragged to"
        );
        assert!(
            !inspector.set_rows(14),
            "setting the height it already has changes nothing and must not force a repaint"
        );
    }

    /// Closing forgets everything, so a reopened window is the default one.
    #[test]
    fn resetting_forgets_the_presentation_and_the_grab() {
        let mut inspector = Inspector::default();
        inspector.toggle_maximized();
        inspector.set_rows(9);
        inspector.grab(Some(Rect::new(0, 4, 40, 10)), 13);
        assert!(inspector.is_grabbed());

        inspector.reset();

        assert_eq!(inspector, Inspector::default());
    }

    /// A drag resizes only when it began on the edge, or dragging the text would move the edge.
    #[test]
    fn only_a_press_on_the_bottom_edge_starts_a_resize() {
        let mut inspector = Inspector::default();
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
}
