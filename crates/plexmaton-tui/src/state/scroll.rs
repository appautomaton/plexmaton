//! Where each surface is scrolled to.

use std::collections::BTreeMap;

use plexmaton_core::AgentId;

use crate::{
    intent::ScrollDirection,
    surface::{SurfaceId, Viewport},
    transcript::{TranscriptMetrics, TranscriptPosition},
};

/// Rows one wheel notch moves a viewport.
///
/// Three rather than one, because a wheel notch on every platform already represents several lines
/// of intent, and a one-row response makes a long transcript feel stuck.
const WHEEL_ROWS: u16 = 3;

/// Where a surface is parked.
///
/// The two arms are not interchangeable, and that is the whole point. A surface at its newest row
/// is *following*, and stays there as content arrives; a surface holding the row number that
/// happened to be last is left behind by the next delta. Deriving one from the other — treating
/// `offset == max_offset` as following — loses the distinction at the exact moment it matters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScrollPosition {
    /// Pinned to the newest content, wherever the content ends up.
    Tail,
    /// Parked this many rows from the start of the content.
    Row(u16),
}

impl ScrollPosition {
    /// The row offset this position resolves to against a viewport of the given depth.
    pub(crate) const fn offset(self, max_offset: u16) -> u16 {
        match self {
            Self::Tail => max_offset,
            Self::Row(row) => {
                if row > max_offset {
                    max_offset
                } else {
                    row
                }
            }
        }
    }
}

/// Scroll positions, retained across frames.
///
/// Two stores, because a conversation is positioned differently from a list of short lines: it
/// parks against the message being read (TR-3), and that position belongs to the conversation
/// rather than to the panel showing it (TR-5). Everything else keeps a row.
///
/// Absence is meaningful in both. A surface the user has never scrolled has no entry and takes its
/// anchor from its own kind of content — a bounded tail view opens at its newest line, a list at
/// its first. Storing a default of zero would make "never touched" and "deliberately scrolled to
/// the top" the same state.
///
/// Retaining an entry for a surface that is not currently registered is the scroll half of SURF-5:
/// a surface that comes back is where the user left it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct ScrollState {
    panels: BTreeMap<SurfaceId, ScrollPosition>,
    /// Where each conversation's reader is, keyed by agent rather than by the panel showing them.
    ///
    /// The reading position belongs to the conversation (TR-5). Keyed by surface, selecting another
    /// agent and coming back would drop the user wherever the other conversation had been left.
    conversations: BTreeMap<AgentId, TranscriptPosition>,
}

impl ScrollState {
    /// Returns where the user put this panel, if they ever did.
    pub(super) fn panel(&self, surface_id: SurfaceId) -> Option<ScrollPosition> {
        self.panels.get(&surface_id).copied()
    }

    /// Returns where the reader of this conversation is, if they have ever moved.
    pub(super) fn conversation(&self, agent_id: &AgentId) -> Option<&TranscriptPosition> {
        self.conversations.get(agent_id)
    }

    /// Moves one panel's viewport by a wheel notch, clamped to its content.
    ///
    /// Returns whether anything moved, so a wheel against a boundary does not cost a repaint. It
    /// still counts as consumed by the caller: an exhausted viewport stops the event rather than
    /// passing it on.
    pub(super) fn scroll_panel(
        &mut self,
        surface_id: SurfaceId,
        viewport: Viewport,
        direction: ScrollDirection,
    ) -> bool {
        let max_offset = viewport.max_offset();
        let current = self
            .panels
            .get(&surface_id)
            .map_or(viewport.offset, |position| position.offset(max_offset));
        let Some(next) = step(current, direction, max_offset) else {
            return false;
        };
        // Arriving at the last row is a decision to follow, not a coincidence of arithmetic
        // (TR-4). A reader who scrolls back to the end expects the stream to carry them on.
        let position = if next == max_offset {
            ScrollPosition::Tail
        } else {
            ScrollPosition::Row(next)
        };
        self.panels.insert(surface_id, position);
        true
    }

    /// Moves one conversation's reader by a wheel notch, storing where they landed as an anchor.
    ///
    /// The metrics are the only thing that can turn a row back into an item, which is why they
    /// reach this far in: storing a row here would leave a number that means something different
    /// after the next resize (TR-3).
    ///
    /// Both directions are resolved at `viewport.content_width` — the width the frame that produced
    /// this viewport measured at. Reading whichever width the cache saw last turns a row of one
    /// panel into an item of the other, which is a reader landing on a message they never scrolled
    /// to whenever two surfaces show one conversation.
    pub(super) fn scroll_conversation(
        &mut self,
        agent_id: &AgentId,
        viewport: Viewport,
        direction: ScrollDirection,
        metrics: &TranscriptMetrics,
    ) -> bool {
        let max_offset = viewport.max_offset();
        let width = viewport.content_width;
        let current = self
            .conversations
            .get(agent_id)
            .map_or(viewport.offset, |position| {
                metrics.offset_of(agent_id, width, position, max_offset)
            });
        let Some(next) = step(current, direction, max_offset) else {
            return false;
        };
        let position = if next == max_offset {
            TranscriptPosition::Tail
        } else {
            // A row inside the conversation always names an item. Nothing else can be anchored to,
            // so a conversation nothing has measured is left alone rather than parked at a guess.
            let Some(anchor) = metrics.anchor_at(agent_id, width, next) else {
                return false;
            };
            anchor
        };
        self.conversations.insert(agent_id.clone(), position);
        true
    }
}

/// One wheel notch from `current`, or `None` if it would not move.
fn step(current: u16, direction: ScrollDirection, max_offset: u16) -> Option<u16> {
    let next = match direction {
        ScrollDirection::Up => current.saturating_sub(WHEEL_ROWS),
        ScrollDirection::Down => current.saturating_add(WHEEL_ROWS),
    }
    .min(max_offset);
    (next != current).then_some(next)
}

#[cfg(test)]
mod tests {
    use super::{ScrollPosition, ScrollState, WHEEL_ROWS};
    use crate::{
        intent::ScrollDirection,
        surface::{SurfaceId, Viewport},
    };

    fn viewport(content_rows: u16, visible_rows: u16, offset: u16) -> Viewport {
        Viewport {
            content_rows,
            visible_rows,
            offset,
            // These fixtures scroll panels, which park on a row. The width a conversation's
            // anchor would be resolved at has no part in it.
            ..Viewport::default()
        }
    }

    #[test]
    fn an_untouched_panel_has_no_stored_position() {
        // The distinction matters: without it a transcript would open at its oldest message.
        assert_eq!(ScrollState::default().panel(SurfaceId::Notices), None);
    }

    #[test]
    fn scrolling_clamps_to_the_content_and_reports_a_boundary_as_no_movement() {
        let mut scroll = ScrollState::default();
        let view = viewport(20, 10, 0);

        assert!(scroll.scroll_panel(SurfaceId::Notices, view, ScrollDirection::Down));
        assert_eq!(
            scroll.panel(SurfaceId::Notices),
            Some(ScrollPosition::Row(WHEEL_ROWS))
        );

        for _ in 0..10 {
            scroll.scroll_panel(SurfaceId::Notices, view, ScrollDirection::Down);
        }
        assert_eq!(
            scroll.panel(SurfaceId::Notices),
            Some(ScrollPosition::Tail),
            "the last row of content stays on screen, and stays there as content arrives"
        );
        assert!(
            !scroll.scroll_panel(SurfaceId::Notices, view, ScrollDirection::Down),
            "a wheel against the end must not force a repaint"
        );
    }

    /// TR-4: following survives content arriving; a row number does not.
    ///
    /// The taller viewport is the same surface after new content. A stored row would still name
    /// row 10 of a conversation that now ends at row 40, leaving the reader silently behind.
    #[test]
    fn a_followed_viewport_moves_with_its_content_and_a_parked_one_does_not() {
        let mut scroll = ScrollState::default();
        let shallow = viewport(20, 10, 0);
        let grown = viewport(50, 10, 0);

        for _ in 0..10 {
            scroll.scroll_panel(SurfaceId::Notices, shallow, ScrollDirection::Down);
        }
        assert_eq!(
            scroll
                .panel(SurfaceId::Notices)
                .map(|position| position.offset(grown.max_offset())),
            Some(grown.max_offset()),
            "a following surface is at the end of whatever the content became"
        );

        scroll.scroll_panel(SurfaceId::Notices, grown, ScrollDirection::Up);
        let parked = scroll
            .panel(SurfaceId::Notices)
            .unwrap_or_else(|| panic!("scrolling up parks the surface"));
        assert_eq!(
            parked,
            ScrollPosition::Row(grown.max_offset().saturating_sub(WHEEL_ROWS)),
            "scrolling away from the end stops following"
        );
        assert_eq!(
            parked.offset(viewport(90, 10, 0).max_offset()),
            grown.max_offset().saturating_sub(WHEEL_ROWS),
            "and further content leaves those rows exactly where they were"
        );
    }

    #[test]
    fn scrolling_up_from_an_untouched_surface_starts_where_it_was_drawn() {
        // A conversation opens at its newest line, so the first wheel-up must move from there and
        // not from the top of the history.
        let mut scroll = ScrollState::default();
        let view = viewport(40, 10, 30);

        assert!(scroll.scroll_panel(SurfaceId::Notices, view, ScrollDirection::Up));
        assert_eq!(
            scroll.panel(SurfaceId::Notices),
            Some(ScrollPosition::Row(30 - WHEEL_ROWS)),
            "it moved from where the user was looking, not from row zero"
        );
    }

    #[test]
    fn a_viewport_with_nothing_to_scroll_never_moves() {
        let mut scroll = ScrollState::default();
        let view = viewport(4, 10, 0);

        assert!(!view.is_scrollable());
        assert!(!scroll.scroll_panel(SurfaceId::Activity, view, ScrollDirection::Down));
        assert_eq!(scroll.panel(SurfaceId::Activity), None);
    }
}
