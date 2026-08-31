//! Where each surface is scrolled to.

use std::collections::BTreeMap;

use crate::{
    intent::ScrollDirection,
    surface::{SurfaceId, Viewport},
};

/// Rows one wheel notch moves a viewport.
///
/// Three rather than one, because a wheel notch on every platform already represents several lines
/// of intent, and a one-row response makes a long transcript feel stuck.
const WHEEL_ROWS: u16 = 3;

/// Scroll offsets, kept per surface and retained across frames.
///
/// Absence is meaningful: a surface the user has never scrolled has no entry, and takes its
/// anchor from its own kind of content — a conversation opens at its newest line, a list at its
/// first. Storing a default of zero instead would make "never touched" and "deliberately scrolled
/// to the top" the same state, and the transcript would open at the oldest message.
///
/// Retaining an entry for a surface that is not currently registered is the scroll half of SURF-5:
/// a surface that comes back is where the user left it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct ScrollState {
    offsets: BTreeMap<SurfaceId, u16>,
}

impl ScrollState {
    /// Returns where the user put this surface, if they ever did.
    pub(super) fn offset(&self, surface_id: SurfaceId) -> Option<u16> {
        self.offsets.get(&surface_id).copied()
    }

    /// Moves one viewport by a wheel notch, clamped to its content.
    ///
    /// Returns whether anything moved, so a wheel against a boundary does not cost a repaint. It
    /// still counts as consumed by the caller: an exhausted viewport stops the event rather than
    /// passing it on.
    pub(super) fn scroll(
        &mut self,
        surface_id: SurfaceId,
        viewport: Viewport,
        direction: ScrollDirection,
    ) -> bool {
        let current = self
            .offsets
            .get(&surface_id)
            .copied()
            .unwrap_or(viewport.offset);
        let next = match direction {
            ScrollDirection::Up => current.saturating_sub(WHEEL_ROWS),
            ScrollDirection::Down => current.saturating_add(WHEEL_ROWS),
        }
        .min(viewport.max_offset());

        if next == current {
            return false;
        }
        self.offsets.insert(surface_id, next);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{ScrollState, WHEEL_ROWS};
    use crate::{
        intent::ScrollDirection,
        surface::{SurfaceId, Viewport},
    };

    fn viewport(content_rows: u16, visible_rows: u16, offset: u16) -> Viewport {
        Viewport {
            content_rows,
            visible_rows,
            offset,
        }
    }

    #[test]
    fn an_untouched_surface_has_no_stored_offset() {
        // The distinction matters: without it a transcript would open at its oldest message.
        assert_eq!(ScrollState::default().offset(SurfaceId::Transcript), None);
    }

    #[test]
    fn scrolling_clamps_to_the_content_and_reports_a_boundary_as_no_movement() {
        let mut scroll = ScrollState::default();
        let view = viewport(20, 10, 0);

        assert!(scroll.scroll(SurfaceId::Transcript, view, ScrollDirection::Down));
        assert_eq!(scroll.offset(SurfaceId::Transcript), Some(WHEEL_ROWS));

        for _ in 0..10 {
            scroll.scroll(SurfaceId::Transcript, view, ScrollDirection::Down);
        }
        assert_eq!(
            scroll.offset(SurfaceId::Transcript),
            Some(view.max_offset()),
            "the last row of content stays on screen"
        );
        assert!(
            !scroll.scroll(SurfaceId::Transcript, view, ScrollDirection::Down),
            "a wheel against the end must not force a repaint"
        );
    }

    #[test]
    fn scrolling_up_from_an_untouched_surface_starts_where_it_was_drawn() {
        // A conversation opens at its newest line, so the first wheel-up must move from there and
        // not from the top of the history.
        let mut scroll = ScrollState::default();
        let view = viewport(40, 10, 30);

        assert!(scroll.scroll(SurfaceId::Transcript, view, ScrollDirection::Up));
        assert_eq!(
            scroll.offset(SurfaceId::Transcript),
            Some(30 - WHEEL_ROWS),
            "it moved from where the user was looking, not from row zero"
        );
    }

    #[test]
    fn a_viewport_with_nothing_to_scroll_never_moves() {
        let mut scroll = ScrollState::default();
        let view = viewport(4, 10, 0);

        assert!(!view.is_scrollable());
        assert!(!scroll.scroll(SurfaceId::Activity, view, ScrollDirection::Down));
        assert_eq!(scroll.offset(SurfaceId::Activity), None);
    }
}
