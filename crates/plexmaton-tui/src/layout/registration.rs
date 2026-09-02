//! Turns computed workspace geometry into the registered surface tree.
//!
//! The base rectangles arrive from layout unchanged. This module owns their stable identities and
//! z-order, including the bounded approval overlay, so painting and hit testing consume the same
//! SURF-1/SURF-4 registration.

use ratatui::layout::Rect;

use super::BodyRegions;
use crate::surface::{Surface, SurfaceId, SurfaceKind, SurfaceTree};

const BASE_Z_INDEX: u32 = 0;
const FLOATING_Z_INDEX: u32 = 1;
const MODAL_Z_INDEX: u32 = 10;

/// Registers the complete supported workspace from rectangles computed by layout.
pub(super) fn surface_tree(
    area: Rect,
    approval: bool,
    status: Rect,
    notices: Option<Rect>,
    attention: Option<Rect>,
    regions: BodyRegions,
) -> SurfaceTree {
    let mut tree = SurfaceTree::default();

    register(
        &mut tree,
        SurfaceId::Agents,
        regions.agents,
        SurfaceKind::Panel,
    );
    register(
        &mut tree,
        SurfaceId::Transcript,
        regions.transcript,
        SurfaceKind::Panel,
    );
    register_at(
        &mut tree,
        SurfaceId::Inspector,
        regions.inspector,
        SurfaceKind::Inspector,
        if regions.inspector_floats {
            FLOATING_Z_INDEX
        } else {
            BASE_Z_INDEX
        },
    );
    register(
        &mut tree,
        SurfaceId::Activity,
        regions.activity,
        SurfaceKind::Panel,
    );
    register(
        &mut tree,
        SurfaceId::Composer,
        Some(regions.composer),
        SurfaceKind::Composer,
    );
    // A panel, not chrome: its tail can outgrow the strip, and a region the wheel can move must
    // also be reachable by keyboard -- every mouse interaction has a keyboard equivalent.
    register(&mut tree, SurfaceId::Notices, notices, SurfaceKind::Panel);
    register(
        &mut tree,
        SurfaceId::Attention,
        attention,
        SurfaceKind::Panel,
    );
    register_at(
        &mut tree,
        SurfaceId::Approval,
        approval.then(|| approval_rect(area)),
        SurfaceKind::Modal,
        MODAL_Z_INDEX,
    );
    register(
        &mut tree,
        SurfaceId::Status,
        Some(status),
        SurfaceKind::Chrome,
    );

    tree
}

/// Responsive approval card whose request detail scrolls instead of growing the terminal.
fn approval_rect(area: Rect) -> Rect {
    let width = area.width.saturating_sub(4).min(72);
    let height = area.height.saturating_sub(2).min(13);
    Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    )
}

/// Registers a base-layer region. Workspace regions tile the terminal as siblings.
fn register(tree: &mut SurfaceTree, id: SurfaceId, bounds: Option<Rect>, kind: SurfaceKind) {
    register_at(tree, id, bounds, kind, BASE_Z_INDEX);
}

/// Registers one present region at its drawing and hit-testing depth.
fn register_at(
    tree: &mut SurfaceTree,
    id: SurfaceId,
    bounds: Option<Rect>,
    kind: SurfaceKind,
    z_index: u32,
) {
    let Some(bounds) = bounds else {
        return;
    };
    tree.insert(Surface {
        id,
        bounds,
        z_index,
        kind,
        // Layout owns rectangles, not content. The renderer measures and fills this in.
        viewport: None,
    })
    .expect("each workspace region is registered exactly once, under a distinct identity");
}
