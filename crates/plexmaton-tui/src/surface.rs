use std::collections::BTreeMap;

use ratatui::layout::Rect;
use thiserror::Error;

use crate::intent::Direction;

/// Pointer coordinate in terminal cells.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Point {
    pub x: u16,
    pub y: u16,
}

/// What holds the workspace's single text cursor.
///
/// This is the whole reason a printable key is sometimes text and sometimes a command, so it is a
/// two-state fact rather than a set of booleans that could disagree. It lives beside the surface
/// model because SURF-3 derives it from the focused surface's kind; nothing asserts it separately.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum KeyboardFocus {
    /// A navigational surface holds focus and no cursor is on screen.
    #[default]
    Navigation,
    /// A text input holds the cursor.
    TextInput,
}

/// What a surface is, and therefore how events may reach it.
///
/// Whether a surface takes the pointer, is a focus stop, and holds the text cursor are three
/// questions with one answer each. Three booleans would admit eight combinations, most of which
/// mean nothing — chrome that owns the cursor, a focus stop the pointer cannot reach. Deriving
/// every answer from one kind makes those unrepresentable. Later kinds join in the delivery step
/// that earns them rather than in advance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SurfaceKind {
    /// A base workspace region: takes the pointer and is a focus stop.
    Panel,
    /// Painted but never interactive, such as a key-hint strip.
    Chrome,
}

impl SurfaceKind {
    /// Whether a pointer event may resolve to a surface of this kind.
    #[must_use]
    pub const fn accepts_pointer(self) -> bool {
        matches!(self, Self::Panel)
    }

    /// Whether a surface of this kind is a stop on the focus ring.
    #[must_use]
    pub const fn is_focusable(self) -> bool {
        matches!(self, Self::Panel)
    }

    /// What typing does while a surface of this kind holds focus.
    #[must_use]
    pub const fn keyboard_focus(self) -> KeyboardFocus {
        match self {
            Self::Panel | Self::Chrome => KeyboardFocus::Navigation,
        }
    }
}

/// Stable identity of an interactive surface.
///
/// Regions are named rather than numbered because layout, rendering, hit testing, and the tests
/// all have to agree on which region a click landed in. A number agreed by convention is exactly
/// how a click ends up delivered to the panel next door. Dynamic surfaces — inspectors, shelves —
/// arrive as variants carrying their own identity.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SurfaceId {
    /// The agent rail, carrying agents and the attention count.
    Agents,
    /// The selected agent's conversation.
    Transcript,
    /// Tools, artifacts, and mail belonging to the selected agent.
    Activity,
    /// Bounded tail of producer-defect notices. Registered only while one exists.
    Notices,
    /// The key-hint strip.
    Footer,
}

/// Geometry and interaction metadata for one surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Surface {
    pub id: SurfaceId,
    pub bounds: Rect,
    pub z_index: u32,
    pub kind: SurfaceKind,
}

/// Invalid mutation of the interaction tree.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum SurfaceTreeError {
    #[error("surface already exists: {0:?}")]
    DuplicateSurface(SurfaceId),
    #[error("unknown surface: {0:?}")]
    UnknownSurface(SurfaceId),
    #[error("surface z-order is exhausted")]
    ZOrderExhausted,
}

/// Central z-ordered registry used for hit testing and later pointer capture.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SurfaceTree {
    surfaces: BTreeMap<SurfaceId, Surface>,
}

impl SurfaceTree {
    /// Adds one surface with stable identity.
    pub fn insert(&mut self, surface: Surface) -> Result<(), SurfaceTreeError> {
        if self.surfaces.contains_key(&surface.id) {
            return Err(SurfaceTreeError::DuplicateSurface(surface.id));
        }
        self.surfaces.insert(surface.id, surface);
        Ok(())
    }

    /// Returns the topmost pointer-eligible surface containing the point.
    #[must_use]
    pub fn hit_test(&self, point: Point) -> Option<SurfaceId> {
        self.surfaces
            .values()
            .filter(|surface| surface.kind.accepts_pointer() && contains(surface.bounds, point))
            .max_by_key(|surface| (surface.z_index, surface.id))
            .map(|surface| surface.id)
    }

    /// Iterates the focus ring: the registered focus stops, in a deterministic order.
    ///
    /// The order is `SurfaceId` declaration order rather than geometric order. A ring that
    /// reorders itself when the terminal crosses a layout-class threshold costs the user exactly
    /// the muscle memory a ring exists to build; the enum is declared in reading order, so at
    /// every layout class the two agree anyway.
    pub fn focus_ring(&self) -> impl Iterator<Item = SurfaceId> + '_ {
        self.surfaces
            .values()
            .filter(|surface| surface.kind.is_focusable())
            .map(|surface| surface.id)
    }

    /// Returns the next focus stop after `current`, wrapping at both ends.
    ///
    /// A ring wraps where a list clamps. `ViewState::move_selection` stops at the last agent so a
    /// held key does not silently send the user back to the first, but a `Tab` that stops cycling
    /// is a dead key with no way to discover why.
    #[must_use]
    pub fn next_focus(
        &self,
        current: Option<SurfaceId>,
        direction: Direction,
    ) -> Option<SurfaceId> {
        let ring: Vec<_> = self.focus_ring().collect();
        let last = ring.len().checked_sub(1)?;
        let Some(index) = current.and_then(|id| ring.iter().position(|stop| *stop == id)) else {
            // Focus is not on the ring, so either direction enters it from its own end.
            return match direction {
                Direction::Forward => ring.first().copied(),
                Direction::Backward => ring.last().copied(),
            };
        };
        let next = match direction {
            Direction::Forward if index == last => 0,
            Direction::Forward => index.saturating_add(1),
            Direction::Backward if index == 0 => last,
            Direction::Backward => index.saturating_sub(1),
        };
        ring.get(next).copied()
    }

    /// Returns one registered surface.
    #[must_use]
    pub fn get(&self, surface_id: SurfaceId) -> Option<&Surface> {
        self.surfaces.get(&surface_id)
    }

    /// Iterates every registered surface in identity order.
    pub fn iter(&self) -> impl Iterator<Item = &Surface> {
        self.surfaces.values()
    }

    /// Returns how many surfaces are registered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.surfaces.len()
    }

    /// Returns whether nothing is registered, which is the case before the first frame is drawn.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.surfaces.is_empty()
    }

    /// Promotes one surface above every currently registered surface.
    pub fn promote(&mut self, surface_id: SurfaceId) -> Result<(), SurfaceTreeError> {
        let next_z = self
            .surfaces
            .values()
            .map(|surface| surface.z_index)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(SurfaceTreeError::ZOrderExhausted)?;
        let surface = self
            .surfaces
            .get_mut(&surface_id)
            .ok_or(SurfaceTreeError::UnknownSurface(surface_id))?;
        surface.z_index = next_z;
        Ok(())
    }
}

fn contains(bounds: Rect, point: Point) -> bool {
    point.x >= bounds.x
        && point.x < bounds.x.saturating_add(bounds.width)
        && point.y >= bounds.y
        && point.y < bounds.y.saturating_add(bounds.height)
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::{KeyboardFocus, Point, Surface, SurfaceId, SurfaceKind, SurfaceTree};
    use crate::intent::Direction;

    fn insert(tree: &mut SurfaceTree, id: SurfaceId, kind: SurfaceKind) {
        tree.insert(Surface {
            id,
            bounds: Rect::new(0, 0, 20, 10),
            z_index: 0,
            kind,
        })
        .unwrap_or_else(|error| panic!("fixture must insert: {error}"));
    }

    #[test]
    fn hit_test_uses_pointer_location_and_z_order() {
        let mut tree = SurfaceTree::default();
        tree.insert(Surface {
            id: SurfaceId::Transcript,
            bounds: Rect::new(0, 0, 20, 10),
            z_index: 1,
            kind: SurfaceKind::Panel,
        })
        .unwrap_or_else(|error| panic!("fixture must insert: {error}"));
        tree.insert(Surface {
            id: SurfaceId::Notices,
            bounds: Rect::new(5, 2, 10, 6),
            z_index: 2,
            kind: SurfaceKind::Panel,
        })
        .unwrap_or_else(|error| panic!("fixture must insert: {error}"));

        assert_eq!(
            tree.hit_test(Point { x: 6, y: 3 }),
            Some(SurfaceId::Notices)
        );
        assert_eq!(
            tree.hit_test(Point { x: 1, y: 1 }),
            Some(SurfaceId::Transcript)
        );

        tree.promote(SurfaceId::Transcript)
            .unwrap_or_else(|error| panic!("fixture must promote: {error}"));
        assert_eq!(
            tree.hit_test(Point { x: 6, y: 3 }),
            Some(SurfaceId::Transcript)
        );
    }

    /// SURF-3: what a surface is decides how events reach it, so chrome is unreachable by both.
    #[test]
    fn chrome_is_neither_a_pointer_target_nor_a_focus_stop() {
        let mut tree = SurfaceTree::default();
        insert(&mut tree, SurfaceId::Footer, SurfaceKind::Chrome);

        assert_eq!(tree.hit_test(Point { x: 1, y: 1 }), None);
        assert_eq!(tree.focus_ring().count(), 0);
        assert_eq!(tree.next_focus(None, Direction::Forward), None);
    }

    #[test]
    fn the_focus_ring_wraps_in_both_directions() {
        let mut tree = SurfaceTree::default();
        insert(&mut tree, SurfaceId::Agents, SurfaceKind::Panel);
        insert(&mut tree, SurfaceId::Transcript, SurfaceKind::Panel);
        insert(&mut tree, SurfaceId::Footer, SurfaceKind::Chrome);

        let ring: Vec<_> = tree.focus_ring().collect();
        assert_eq!(
            ring,
            [SurfaceId::Agents, SurfaceId::Transcript],
            "chrome is not a stop, and the order is declaration order"
        );

        let forward = tree.next_focus(Some(SurfaceId::Transcript), Direction::Forward);
        assert_eq!(forward, Some(SurfaceId::Agents), "the last stop wraps");
        let backward = tree.next_focus(Some(SurfaceId::Agents), Direction::Backward);
        assert_eq!(backward, Some(SurfaceId::Transcript), "the first wraps too");
    }

    /// SURF-3: focus naming a surface that is not on the ring must resolve to a real stop.
    #[test]
    fn focus_outside_the_ring_enters_it_from_the_matching_end() {
        let mut tree = SurfaceTree::default();
        insert(&mut tree, SurfaceId::Agents, SurfaceKind::Panel);
        insert(&mut tree, SurfaceId::Transcript, SurfaceKind::Panel);

        for stale in [None, Some(SurfaceId::Notices)] {
            assert_eq!(
                tree.next_focus(stale, Direction::Forward),
                Some(SurfaceId::Agents)
            );
            assert_eq!(
                tree.next_focus(stale, Direction::Backward),
                Some(SurfaceId::Transcript)
            );
        }
    }

    #[test]
    fn no_kind_puts_a_cursor_on_screen_before_the_composer_exists() {
        for kind in [SurfaceKind::Panel, SurfaceKind::Chrome] {
            assert_eq!(kind.keyboard_focus(), KeyboardFocus::Navigation);
        }
    }
}
