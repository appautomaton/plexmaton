use std::collections::BTreeMap;

use ratatui::layout::Rect;
use thiserror::Error;

/// Pointer coordinate in terminal cells.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Point {
    pub x: u16,
    pub y: u16,
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
    pub accepts_pointer: bool,
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
            .filter(|surface| surface.accepts_pointer && contains(surface.bounds, point))
            .max_by_key(|surface| (surface.z_index, surface.id))
            .map(|surface| surface.id)
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

    use super::{Point, Surface, SurfaceId, SurfaceTree};

    #[test]
    fn hit_test_uses_pointer_location_and_z_order() {
        let mut tree = SurfaceTree::default();
        tree.insert(Surface {
            id: SurfaceId::Transcript,
            bounds: Rect::new(0, 0, 20, 10),
            z_index: 1,
            accepts_pointer: true,
        })
        .unwrap_or_else(|error| panic!("fixture must insert: {error}"));
        tree.insert(Surface {
            id: SurfaceId::Notices,
            bounds: Rect::new(5, 2, 10, 6),
            z_index: 2,
            accepts_pointer: true,
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
}
