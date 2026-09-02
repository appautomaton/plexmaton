//! What the user can ask of the inspector: open it, pin it, resize it, close it.
//!
//! Separated from the rest of the projection because it is a command surface rather than a fact
//! about the workspace. What the inspector *is* lives in [`super::inspector`]; this is the set of
//! things a user gesture can do to it, and every one of them answers the same way — change
//! something and say so, or change nothing and say that.

use plexmaton_core::AgentId;

use crate::{
    intent::{InspectorIntent, PointerIntent},
    layout::{self, InspectorRequest, SteerSplit},
    surface::{SurfaceId, SurfaceTree},
};

use super::{InspectorView, ViewState, inner_width};

impl ViewState {
    /// Returns the open inspector, if one is open.
    #[must_use]
    pub fn inspector(&self) -> Option<&InspectorView> {
        self.inspector.open()
    }

    /// The inspector's steer input and who it addresses, if it has one on screen right now.
    ///
    /// `None` in three cases that mean the same thing to everyone downstream: nothing is open,
    /// something else holds focus (INS-5, D-018), or the rectangle the user dragged to is too short
    /// to hold a conversation and an input at once (INS-7). The renderer draws from this, the caret
    /// follows it, and [`Self::text_target`] refuses without it, so a draft can never be typed into
    /// somewhere the user cannot see.
    #[must_use]
    pub(crate) fn steer_input(&self, surfaces: &SurfaceTree) -> Option<(SteerSplit, AgentId)> {
        if self.focus.resolve(surfaces)? != SurfaceId::Inspector {
            return None;
        }
        let bounds = surfaces.get(SurfaceId::Inspector)?.bounds;
        let agent = self.inspector.open()?.agent.clone();
        let wanted = self.draft(&agent).requested_rows(inner_width(bounds.width));
        layout::steer_split(bounds, wanted).map(|split| (split, agent))
    }

    /// What layout needs in order to place the inspector.
    #[must_use]
    pub fn inspector_request(&self) -> Option<InspectorRequest> {
        self.inspector.request()
    }

    /// Resolves exactly one layer, innermost first. Returns whether anything was there to resolve.
    ///
    /// The order is transience: a selection is the most recent thing the user made and the cheapest
    /// to remake, so it goes before the surface it was made in. One layer per press is the whole of
    /// INV-6 — `Escape` is the key people press to back out of one mistake at a time.
    ///
    /// Focus returns to the conversation, but only when the inspector was holding it. Moving focus
    /// unconditionally would take the cursor out of the composer for a user who pressed `Escape`
    /// while typing, which is not what closing an overlay somewhere else asked for.
    pub fn dismiss(&mut self, surfaces: &SurfaceTree) -> bool {
        if self.clear_selection() {
            return true;
        }
        let held_focus = self.focus.resolve(surfaces) == Some(SurfaceId::Inspector);
        let dismissed = self.inspector.dismiss();
        if dismissed {
            if held_focus {
                self.focus.prefer(SurfaceId::Transcript);
            }
            // Unreachable through this ladder, whose first rung already took any selection. Kept so
            // that SEL-3 is guaranteed by every path that changes what a surface shows, rather than
            // by the order of the rungs above it.
            let _pruned = self.prune_selection();
            self.touch();
        }
        dismissed
    }

    /// Applies one inspector command.
    ///
    /// A command with nothing open is a no-op rather than a refusal: the router translates what the
    /// user pressed, and whether there is anything to act on is this side's question.
    pub fn inspect(&mut self, surfaces: &SurfaceTree, intent: InspectorIntent) {
        let changed = match intent {
            // Opening focuses it, so its input is usable without a second step (D-026). The
            // surface arrives with the next frame; a focus preference is resolved then, not now.
            InspectorIntent::Open => match self.agents.selected().map(|agent| agent.id.clone()) {
                Some(agent_id) => {
                    // Re-pointing an open inspector at another agent is the second way the surface
                    // under a selection changes what it is showing, alongside the roster moving
                    // (SEL-3).
                    let opened = self.inspector.show(agent_id);
                    let pruned = opened && self.prune_selection();
                    self.focus.prefer(SurfaceId::Inspector) || opened || pruned
                }
                None => false,
            },
            InspectorIntent::TogglePin => self.inspector.toggle_pin(),
            InspectorIntent::ToggleMaximize => self.inspector.toggle_maximized(),
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
}
