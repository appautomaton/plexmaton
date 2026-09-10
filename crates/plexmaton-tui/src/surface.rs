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

/// Shared interior spacing for drawing, wrapping, carets and pointer hit testing (SURF-3).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ContentInsets {
    pub(crate) sides: u16,
    pub(crate) vertical: u16,
}

impl ContentInsets {
    pub(crate) const fn for_surface(surface: SurfaceId, height: u16) -> Self {
        match surface {
            SurfaceId::Approval | SurfaceId::Drawer => Self {
                sides: 2,
                vertical: if height >= 10 { 1 } else { 0 },
            },
            _ => Self {
                sides: 0,
                vertical: 0,
            },
        }
    }

    pub(crate) const fn width(self, outer: u16) -> u16 {
        outer.saturating_sub(2 + self.sides * 2)
    }

    /// Retains the border in the returned rectangle so existing input geometry has one origin.
    pub(crate) fn inset(self, bounds: Rect) -> Rect {
        Rect::new(
            bounds.x.saturating_add(self.sides),
            bounds.y.saturating_add(self.vertical),
            bounds.width.saturating_sub(self.sides * 2),
            bounds.height.saturating_sub(self.vertical * 2),
        )
    }
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
    /// Painted but never interactive: the status line.
    Chrome,
    /// A text input. While it holds focus it owns the workspace's one cursor.
    Composer,
    /// One agent's detail, opened explicitly and dismissed with `Escape`.
    ///
    /// Named for what it is rather than for how it looks. `surface-model.md` predicted `Shelf`, but
    /// a shelf is one of three presentations this surface takes depending on terminal size, and
    /// changing presentation must not change identity — so a kind named after one geometry would be
    /// the wrong name at the other two.
    Inspector,
    /// A user-opened blocking decision surface. It owns navigation until answered or dismissed.
    Modal,
    /// The workspace's own input, pulled from the top edge with `⌃P` and filtered by typing.
    ///
    /// A blocking layer that also holds the cursor, which is why it is a kind and not a `Modal`: an
    /// approval is answered with `↑↓` and `Enter` and must never grow a caret, so one kind cannot
    /// answer "is there a cursor" for both. A Drawer page that is navigated rather than typed into
    /// registers as `Modal` for that frame. It belongs to the workspace rather than to any
    /// conversation, so unlike an approval it is not a section of anyone's box.
    Drawer,
    /// Inline completion list anchored to the primary composer; pointer-active without taking focus.
    Popup,
}

impl SurfaceKind {
    /// Whether a pointer event may resolve to a surface of this kind.
    #[must_use]
    pub const fn accepts_pointer(self) -> bool {
        matches!(
            self,
            Self::Panel
                | Self::Composer
                | Self::Inspector
                | Self::Modal
                | Self::Drawer
                | Self::Popup
        )
    }

    /// Whether a surface of this kind is a stop on the focus ring.
    #[must_use]
    pub const fn is_focusable(self) -> bool {
        matches!(
            self,
            Self::Panel | Self::Composer | Self::Inspector | Self::Modal | Self::Drawer
        )
    }

    /// Whether `Escape` closes a surface of this kind.
    ///
    /// The inspector is the workspace's one dismissible layer, which is what gives INV-6's ladder
    /// something to resolve. Deriving this from the kind rather than storing it keeps it in the same
    /// place as every other behavioural answer (SURF-3).
    #[must_use]
    pub const fn is_dismissible(self) -> bool {
        matches!(
            self,
            Self::Inspector | Self::Modal | Self::Drawer | Self::Popup
        )
    }

    /// Whether this surface prevents delivery to every lower surface, including outside its own
    /// visible rectangle.
    #[must_use]
    pub const fn blocks_below(self) -> bool {
        matches!(self, Self::Modal | Self::Drawer)
    }

    /// What typing does while a surface of this kind holds focus.
    #[must_use]
    pub const fn keyboard_focus(self) -> KeyboardFocus {
        match self {
            Self::Panel | Self::Chrome | Self::Modal | Self::Popup => KeyboardFocus::Navigation,
            // The inspector carries the inspected agent's steer input, which renders only while it
            // holds focus (INS-5). There is still exactly one cursor: focus decides which surface
            // has it, and no surface has one without focus.
            Self::Composer | Self::Inspector | Self::Drawer => KeyboardFocus::TextInput,
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
    /// The list of sub-agents, carrying the attention count.
    Agents,
    /// The primary agent's conversation.
    Transcript,
    /// The second window: one sub-agent's conversation, open while one is selected (INS-1).
    ///
    /// Declared next to the conversation because that is where it lives in every presentation: over
    /// the conversation as a shelf, in place of it when maximized, and beside it as the second
    /// column. No fixed position matches reading order at all three — a surface that moves cannot —
    /// and SURF-3 prefers a ring that never reorders over one that reads correctly at one size.
    Inspector,
    /// The one text input, bound to the primary agent (COM-4), inside its conversation's box.
    ///
    /// Declared right after the second window so that `Tab` from the window's input lands here:
    /// the collapsed composer says `⇥ to return`, and the ring is what makes that true.
    Composer,
    /// Skill completions anchored immediately above the primary composer.
    ComposerMenu,
    /// Bounded tail of producer-defect notices. Registered only while one exists.
    Notices,
    /// Requests background agents have made of the user. Registered only while one is queued.
    ///
    /// The strips sit at the top of the screen but at the end of the ring, so focus starts on the
    /// list rather than on whatever arrived, and the ring runs list, conversation, input, strips.
    Attention,
    /// Submitted input no request carries yet, above the decision region (IQU-3).
    ///
    /// Chrome, not a panel: it reports what the user already said and offers nothing to do with it,
    /// so making it a focus stop and a pointer target would advertise actions it does not have.
    QueuedInput,
    /// A pending tool approval, inline for the main agent and modal for an explicitly opened background request.
    Approval,
    /// User-opened, read-only inspection of the exact pending shell command.
    CommandInspection,
    /// The Drawer, docked to the top edge over everything while it is open.
    ///
    /// Last in the ring because it is never a `Tab` destination: it is opened by its own chord and
    /// closed by `Escape`, and while it is open SURF-4 leaves it the only stop anyway.
    Drawer,
    /// The status line: the last row of the screen, under every pane.
    Status,
}

/// How much content a surface has, and how far through it the user is.
///
/// Filled in by the renderer, which is the only place that knows how tall content wraps to. Layout
/// registers the rectangle; measurement needs the text.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Viewport {
    /// Rows the content occupies once wrapped to this surface's width.
    pub content_rows: usize,
    /// The width those rows were wrapped at, borders excluded.
    ///
    /// It travels with the row count because it is what makes the row count mean anything: the same
    /// conversation is a different number of rows at every width, and two surfaces can be showing
    /// one conversation at two widths in the same frame. Anything turning a row back into a
    /// transcript item has to be told which of them it is holding (TR-1, TR-3).
    pub content_width: u16,
    /// Rows of the surface that show content, borders excluded.
    pub visible_rows: u16,
    /// Rows scrolled past the top. Always within `0..=max_offset`.
    pub offset: usize,
}

impl Viewport {
    /// The furthest the viewport can travel and still show content.
    #[must_use]
    pub const fn max_offset(self) -> usize {
        self.content_rows.saturating_sub(self.visible_rows as usize)
    }

    /// Whether this viewport can move at all.
    ///
    /// A viewport that cannot is **not an eligible wheel target**, so the event reaches whatever is
    /// beneath it. "Cannot scroll" and "scrolled to the end" are deliberately different: the second
    /// consumes the event and stops, because a gesture whose target changes with scroll position is
    /// the spatial-memory failure the UI/UX contract exists to prevent (ui-ux §nested scrolling).
    #[must_use]
    pub const fn is_scrollable(self) -> bool {
        self.max_offset() > 0
    }
}

/// Geometry and interaction metadata for one surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Surface {
    pub id: SurfaceId,
    pub bounds: Rect,
    pub z_index: u32,
    pub kind: SurfaceKind,
    /// Present once the renderer has measured this surface's content. Chrome never has one.
    pub viewport: Option<Viewport>,
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
        if let Some(blocker) = self.top_blocker() {
            return Some(blocker.id);
        }
        self.surfaces
            .values()
            .filter(|surface| surface.kind.accepts_pointer() && contains(surface.bounds, point))
            .max_by_key(|surface| (surface.z_index, surface.id))
            .map(|surface| surface.id)
    }

    /// Returns the surface a wheel event over `point` should scroll.
    ///
    /// Not `hit_test`: eligibility is whether a viewport can actually move, so the wheel falls
    /// through a surface with nothing to scroll and reaches the one beneath it. A surface that is
    /// merely *at* its boundary is still eligible and still consumes the event (ui-ux §nested
    /// scrolling).
    #[must_use]
    pub fn wheel_target(&self, point: Point) -> Option<SurfaceId> {
        if let Some(blocker) = self.top_blocker() {
            return (contains(blocker.bounds, point)
                && blocker.viewport.is_some_and(Viewport::is_scrollable))
            .then_some(blocker.id);
        }
        self.surfaces
            .values()
            .filter(|surface| {
                surface.kind.accepts_pointer()
                    && contains(surface.bounds, point)
                    && surface.viewport.is_some_and(Viewport::is_scrollable)
            })
            .max_by_key(|surface| (surface.z_index, surface.id))
            .map(|surface| surface.id)
    }

    /// Records what the renderer measured for one surface.
    pub fn set_viewport(&mut self, surface_id: SurfaceId, viewport: Viewport) {
        if let Some(surface) = self.surfaces.get_mut(&surface_id) {
            surface.viewport = Some(viewport);
        }
    }

    /// Returns one surface's measured viewport, if it has been drawn.
    #[must_use]
    pub fn viewport(&self, surface_id: SurfaceId) -> Option<Viewport> {
        self.surfaces
            .get(&surface_id)
            .and_then(|surface| surface.viewport)
    }

    /// Whether any registered surface is one `Escape` would close.
    ///
    /// Read from the tree rather than stored beside it, so the ladder cannot believe a layer is open
    /// after the frame that drew it stopped registering it.
    #[must_use]
    pub fn has_dismissible(&self) -> bool {
        self.surfaces
            .values()
            .any(|surface| surface.kind.is_dismissible())
    }

    /// Iterates the focus ring: the registered focus stops, in a deterministic order.
    ///
    /// The order is `SurfaceId` declaration order rather than geometric order. A ring that
    /// reorders itself when the terminal crosses a layout-class threshold costs the user exactly
    /// the muscle memory a ring exists to build; the enum is declared in reading order, so at
    /// every layout class the two agree anyway.
    pub fn focus_ring(&self) -> impl Iterator<Item = SurfaceId> + '_ {
        let blocker = self.top_blocker().map(|surface| surface.id);
        self.surfaces
            .values()
            .filter(move |surface| {
                surface.kind.is_focusable() && blocker.is_none_or(|blocker| surface.id == blocker)
            })
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

    fn top_blocker(&self) -> Option<&Surface> {
        self.surfaces
            .values()
            .filter(|surface| surface.kind.blocks_below())
            .max_by_key(|surface| (surface.z_index, surface.id))
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
            viewport: None,
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
            viewport: None,
        })
        .unwrap_or_else(|error| panic!("fixture must insert: {error}"));
        tree.insert(Surface {
            id: SurfaceId::Notices,
            bounds: Rect::new(5, 2, 10, 6),
            z_index: 2,
            kind: SurfaceKind::Panel,
            viewport: None,
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
        insert(&mut tree, SurfaceId::Status, SurfaceKind::Chrome);

        assert_eq!(tree.hit_test(Point { x: 1, y: 1 }), None);
        assert_eq!(tree.focus_ring().count(), 0);
        assert_eq!(tree.next_focus(None, Direction::Forward), None);
    }

    #[test]
    fn the_focus_ring_wraps_in_both_directions() {
        let mut tree = SurfaceTree::default();
        insert(&mut tree, SurfaceId::Agents, SurfaceKind::Panel);
        insert(&mut tree, SurfaceId::Transcript, SurfaceKind::Panel);
        insert(&mut tree, SurfaceId::Status, SurfaceKind::Chrome);

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
        for kind in [SurfaceKind::Panel, SurfaceKind::Chrome, SurfaceKind::Modal] {
            assert_eq!(kind.keyboard_focus(), KeyboardFocus::Navigation);
        }
    }

    /// SURF-4: a modal is the only focus stop and catches pointer delivery even outside its card.
    #[test]
    fn a_blocking_surface_prevents_delivery_below_it() {
        let mut tree = SurfaceTree::default();
        insert(&mut tree, SurfaceId::Transcript, SurfaceKind::Panel);
        tree.insert(Surface {
            id: SurfaceId::Approval,
            bounds: Rect::new(5, 2, 10, 6),
            z_index: 10,
            kind: SurfaceKind::Modal,
            viewport: None,
        })
        .unwrap_or_else(|error| panic!("fixture must insert: {error}"));

        assert_eq!(tree.focus_ring().collect::<Vec<_>>(), [SurfaceId::Approval]);
        assert_eq!(
            tree.hit_test(Point { x: 1, y: 1 }),
            Some(SurfaceId::Approval),
            "a click outside the visible card must not fall through the modal"
        );
    }
}
