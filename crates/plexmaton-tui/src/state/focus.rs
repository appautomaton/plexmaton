//! Which surface the keyboard is addressing.

use crate::{
    intent::Direction,
    surface::{KeyboardFocus, SurfaceId, SurfaceTree},
};

/// The user's focus preference, resolved against whatever the current frame registered.
///
/// A preference rather than an assertion: the surface tree is rebuilt every frame, so the stored
/// identity may name a surface that is not on screen right now. Resolving lazily instead of
/// repairing after layout means there is nothing to keep in sync, and it is what makes SURF-5 fall
/// out — a surface that comes back gets its focus back. Repairing would also mean mutating state
/// during layout, which the anti-pattern list forbids.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct Focus {
    preferred: Option<SurfaceId>,
}

impl Focus {
    /// Resolves the surface that holds focus for the frame `surfaces` describes.
    ///
    /// A stored preference that is no longer a stop falls back to the first one, and the preference
    /// is left alone so the surface reclaims focus when it returns.
    pub(super) fn resolve(self, surfaces: &SurfaceTree) -> Option<SurfaceId> {
        self.preferred
            .filter(|id| {
                surfaces
                    .get(*id)
                    .is_some_and(|surface| surface.kind.is_focusable())
            })
            .or_else(|| surfaces.focus_ring().next())
    }

    /// Resolves where typed text would go, from the focused surface's kind alone (SURF-3).
    pub(super) fn keyboard(self, surfaces: &SurfaceTree) -> KeyboardFocus {
        self.resolve(surfaces)
            .and_then(|id| surfaces.get(id))
            .map_or_else(KeyboardFocus::default, |surface| {
                surface.kind.keyboard_focus()
            })
    }

    /// Moves focus one stop around the ring. Returns whether anything visible changed.
    pub(super) fn cycle(&mut self, surfaces: &SurfaceTree, direction: Direction) -> bool {
        let next = surfaces.next_focus(self.resolve(surfaces), direction);
        self.set(next)
    }

    /// Focuses the surface a press landed on, if that surface is a stop.
    ///
    /// A press on chrome routes but does not move focus, so clicking the hint strip cannot strand
    /// the keyboard somewhere it has nothing to act on.
    pub(super) fn point_at(&mut self, surfaces: &SurfaceTree, surface_id: SurfaceId) -> bool {
        if surfaces
            .get(surface_id)
            .is_some_and(|surface| surface.kind.is_focusable())
        {
            return self.set(Some(surface_id));
        }
        false
    }

    /// Whether the user's stored preference names this surface.
    ///
    /// The resolved focus needs a tree, and laying the tree out needs to know whether the composer
    /// is collapsed — so the collapse reads the preference instead. The two agree whenever the
    /// preferred surface is registered, which is the only case where the answer matters.
    pub(super) fn prefers(self, surface_id: SurfaceId) -> bool {
        self.preferred == Some(surface_id)
    }

    /// Prefers a surface that the *next* frame will register.
    ///
    /// Unlike [`Self::point_at`], this does not check the current tree, because opening a surface
    /// and drawing it are different frames. A preference resolved per frame is exactly what makes
    /// that safe: if the surface never appears, resolution falls back to the first ring stop and the
    /// preference costs nothing.
    pub(super) fn prefer(&mut self, surface_id: SurfaceId) -> bool {
        self.set(Some(surface_id))
    }

    fn set(&mut self, next: Option<SurfaceId>) -> bool {
        if next.is_none() || next == self.preferred {
            return false;
        }
        self.preferred = next;
        true
    }
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::Focus;
    use crate::surface::{Surface, SurfaceId, SurfaceKind, SurfaceTree};

    /// A tree holding exactly the named stops, so a surface can be taken away and given back.
    fn tree_of(ids: &[SurfaceId]) -> SurfaceTree {
        let mut tree = SurfaceTree::default();
        for id in ids {
            tree.insert(Surface {
                id: *id,
                bounds: Rect::new(0, 0, 10, 10),
                z_index: 0,
                kind: SurfaceKind::Panel,
                viewport: None,
            })
            .unwrap_or_else(|error| panic!("fixture must insert: {error}"));
        }
        tree
    }

    /// SURF-5: focus belongs to the surface, not to the frame that happened to draw it.
    #[test]
    fn focus_returns_to_a_surface_that_comes_back() {
        let full = tree_of(&[SurfaceId::Agents, SurfaceId::Transcript]);
        let reduced = tree_of(&[SurfaceId::Agents]);
        let mut focus = Focus::default();
        focus.point_at(&full, SurfaceId::Transcript);

        assert_eq!(
            focus.resolve(&reduced),
            Some(SurfaceId::Agents),
            "focus must never be delivered to a surface that is not on screen"
        );
        assert_eq!(
            focus.resolve(&full),
            Some(SurfaceId::Transcript),
            "and the preference must survive, or reopening loses where the user was"
        );
    }

    #[test]
    fn focus_resolves_to_nothing_when_no_surface_is_registered() {
        assert_eq!(Focus::default().resolve(&SurfaceTree::default()), None);
    }

    #[test]
    fn pointing_at_an_unfocusable_surface_reports_no_change() {
        let mut tree = tree_of(&[SurfaceId::Agents]);
        tree.insert(Surface {
            id: SurfaceId::Footer,
            bounds: Rect::new(0, 10, 10, 1),
            z_index: 0,
            kind: SurfaceKind::Chrome,
            viewport: None,
        })
        .unwrap_or_else(|error| panic!("fixture must insert: {error}"));
        let mut focus = Focus::default();

        assert!(
            !focus.point_at(&tree, SurfaceId::Footer),
            "reporting a change would cost a full repaint for nothing"
        );
        assert_eq!(focus.resolve(&tree), Some(SurfaceId::Agents));
    }
}
