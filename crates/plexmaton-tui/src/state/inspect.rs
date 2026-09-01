//! What the user can ask of the inspector: open it, pin it, resize it, close it.
//!
//! Separated from the rest of the projection because it is a command surface rather than a fact
//! about the workspace. What the inspector *is* lives in [`super::inspector`]; this is the set of
//! things a user gesture can do to it, and every one of them answers the same way — change
//! something and say so, or change nothing and say that.

use crate::{
    intent::{InspectorIntent, PointerIntent},
    layout::InspectorRequest,
    surface::{SurfaceId, SurfaceTree},
};

use super::{InspectorView, ViewState};

impl ViewState {
    /// Returns the open inspector, if one is open.
    #[must_use]
    pub fn inspector(&self) -> Option<&InspectorView> {
        self.inspector.open()
    }

    /// What layout needs in order to place the inspector.
    #[must_use]
    pub fn inspector_request(&self) -> Option<InspectorRequest> {
        self.inspector.request()
    }

    /// Closes the topmost dismissible layer. Returns whether anything was open to close.
    ///
    /// Focus returns to the conversation, but only when the inspector was holding it. Moving focus
    /// unconditionally would take the cursor out of the composer for a user who pressed `Escape`
    /// while typing, which is not what closing an overlay somewhere else asked for.
    pub fn dismiss(&mut self, surfaces: &SurfaceTree) -> bool {
        let held_focus = self.focus.resolve(surfaces) == Some(SurfaceId::Inspector);
        let dismissed = self.inspector.dismiss();
        if dismissed {
            if held_focus {
                self.focus.prefer(SurfaceId::Transcript);
            }
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
                    let opened = self.inspector.show(agent_id);
                    self.focus.prefer(SurfaceId::Inspector) || opened
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
