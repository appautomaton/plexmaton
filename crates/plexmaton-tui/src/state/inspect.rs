//! What the user can ask of the second window: look at an agent, enter it, maximize it, resize it,
//! close it.
//!
//! Separated from the rest of the projection because it is a command surface rather than a fact
//! about the workspace. Opening is selecting (INS-1): the window shows the selected agent whenever
//! that is not the primary, so the roster owns whether it is open and moving the selection lives
//! here beside what closing means. Every command answers the same way — change something and say
//! so, or change nothing and say that.

use plexmaton_core::AgentId;

use crate::{
    intent::{Direction, InspectorIntent, PointerIntent},
    layout::{self, InspectorRequest, SteerSplit},
    surface::{SurfaceId, SurfaceTree},
};

use super::{InspectorView, ReduceError, ViewState, inner_width};

impl ViewState {
    /// The second window, if one is open: the selected agent when that is not the primary.
    #[must_use]
    pub fn inspector(&self) -> Option<InspectorView> {
        self.agents
            .peeked()
            .map(|agent| self.inspector.view(agent.id.clone()))
    }

    /// The inspector's steer input and who it addresses, if it has one on screen right now.
    ///
    /// `None` in three cases that mean the same thing to everyone downstream: nothing is open,
    /// something else holds focus (INS-5), or the rectangle the user dragged to is too short
    /// to hold a conversation and an input at once (INS-7). The renderer draws from this, the caret
    /// follows it, and [`Self::text_target`] refuses without it, so a draft can never be typed into
    /// somewhere the user cannot see.
    #[must_use]
    pub(crate) fn steer_input(&self, surfaces: &SurfaceTree) -> Option<(SteerSplit, AgentId)> {
        if self.focus.resolve(surfaces)? != SurfaceId::Inspector {
            return None;
        }
        let bounds = surfaces.get(SurfaceId::Inspector)?.bounds;
        let agent = self.inspector()?.agent;
        let wanted = self.draft(&agent).requested_rows(inner_width(bounds.width));
        layout::steer_split(bounds, wanted).map(|split| (split, agent))
    }

    /// What layout needs in order to place the inspector.
    #[must_use]
    pub fn inspector_request(&self) -> Option<InspectorRequest> {
        self.agents.peeked().map(|_| self.inspector.request())
    }

    /// Resolves exactly one layer, innermost first. Returns whether anything was there to resolve.
    ///
    /// The order is transience: a selection is the most recent thing the user made and the cheapest
    /// to remake, so it goes before the surface it was made in. One layer per press is the whole of
    /// INV-6 — `Escape` is the key people press to back out of one mistake at a time.
    ///
    /// Closing the window is selecting the primary again, because the window is the selection
    /// (INS-1). Focus returns to the conversation only when the window was holding it: moving it
    /// unconditionally would take the cursor out of the composer for a user who pressed `Escape`
    /// while typing, which is not what closing an overlay somewhere else asked for.
    pub fn dismiss(&mut self, surfaces: &SurfaceTree) -> bool {
        if self.clear_selection() {
            return true;
        }
        if self.agents.peeked().is_none() {
            return false;
        }
        let held_focus = self.focus.resolve(surfaces) == Some(SurfaceId::Inspector);
        let closed = self.agents.clear_selection();
        self.settle_window(held_focus);
        if closed {
            self.touch();
        }
        closed
    }

    /// Applies one command to the second window.
    ///
    /// A command with nothing open is a no-op rather than a refusal: the router translates what the
    /// user pressed, and whether there is anything to act on is this side's question.
    pub fn inspect(&mut self, surfaces: &SurfaceTree, intent: InspectorIntent) {
        if self.agents.peeked().is_none() {
            return;
        }
        let changed = match intent {
            // Entering focuses it, so its input is usable without a second step (INS-4). The
            // surface is already registered, but a preference is what focus keeps across frames.
            InspectorIntent::Open => self.focus.prefer(SurfaceId::Inspector),
            InspectorIntent::ToggleMaximize => {
                self.inspector.toggle_maximized();
                true
            }
            InspectorIntent::Grow => self.nudge(surfaces, 1),
            InspectorIntent::Shrink => self.nudge(surfaces, -1),
        };
        if changed {
            self.touch();
        }
    }

    /// Moves the inspector's bottom edge by one row, from where it was actually drawn.
    fn nudge(&mut self, surfaces: &SurfaceTree, rows: i16) -> bool {
        let Some(bounds) = surfaces
            .get(SurfaceId::Inspector)
            .map(|surface| surface.bounds)
        else {
            return false;
        };
        let next = if rows.is_negative() {
            bounds.height.saturating_sub(1)
        } else {
            bounds.height.saturating_add(1)
        };
        self.inspector.set_rows(next)
    }

    /// Routes one step of a pointer gesture that may be resizing the inspector.
    ///
    /// Only a press that landed on the bottom edge starts a resize; a drag that began on the text
    /// moves nothing. Capture is the router's (INV-4), so a drag reaches here even after the
    /// pointer has left the rectangle, which is exactly what makes the edge followable.
    pub fn drag(&mut self, surfaces: &SurfaceTree, intent: PointerIntent) {
        let bounds = surfaces
            .get(SurfaceId::Inspector)
            .map(|surface| surface.bounds);
        let on_inspector = |surface| {
            (surface == SurfaceId::Inspector)
                .then_some(bounds)
                .flatten()
        };
        let changed = match intent {
            PointerIntent::Press { surface, at } => {
                self.inspector.grab(on_inspector(surface), at.y);
                false
            }
            PointerIntent::Drag { surface, at } => match on_inspector(surface) {
                // The edge follows the pointer: rows are the distance from the top of the surface
                // to where the pointer is now, and layout clamps whatever that comes to.
                Some(bounds) if self.inspector.is_grabbed() => self
                    .inspector
                    .set_rows(at.y.saturating_add(1).saturating_sub(bounds.y).max(1)),
                _ => false,
            },
            PointerIntent::Release { .. } | PointerIntent::Cancel { .. } => {
                self.inspector.release();
                false
            }
        };
        if changed {
            self.touch();
        }
    }

    /// Selects an existing agent without changing semantic runtime state.
    pub fn select_agent(&mut self, agent_id: &AgentId) -> Result<(), ReduceError> {
        if self.agents.select(agent_id)? {
            self.after_selection_moved();
            self.touch();
        }
        Ok(())
    }

    /// Moves the agent selection one step in arrival order, clamped at both ends.
    pub fn move_selection(&mut self, direction: Direction) {
        if self.agents.move_selection(direction) {
            self.after_selection_moved();
            self.touch();
        }
    }

    /// What a moved selection changes besides the rail: the second window (INS-1).
    ///
    /// Landing on another agent opens or re-points the window, so a selection made in it is judged
    /// against the agent it now shows (SEL-3). Landing on the primary closes it.
    fn after_selection_moved(&mut self) {
        if self.agents.peeked().is_some() {
            let _pruned = self.prune_selection();
        } else {
            let held_focus = self.focus.prefers(SurfaceId::Inspector);
            self.settle_window(held_focus);
        }
    }

    /// What closing the second window means besides the selection: its presentation is forgotten,
    /// a keyboard it was holding goes back to the conversation, and a selection made in it goes.
    fn settle_window(&mut self, held_focus: bool) {
        self.inspector.reset();
        if held_focus {
            self.focus.prefer(SurfaceId::Transcript);
        }
        let _pruned = self.prune_selection();
    }
}
