//! Turns computed workspace geometry into the registered surface tree.
//!
//! The base rectangles arrive from layout unchanged. This module owns their stable identities and
//! z-order, including the bounded approval overlay, so painting and hit testing consume the same
//! SURF-1/SURF-4 registration.

use ratatui::layout::Rect;

use super::{BodyRegions, DecisionMode};
use crate::surface::{Surface, SurfaceId, SurfaceKind, SurfaceTree};

const BASE_Z_INDEX: u32 = 0;
const FLOATING_Z_INDEX: u32 = 1;
const MODAL_Z_INDEX: u32 = 10;
const CONFIGURATION_Z_INDEX: u32 = 15;
const COMMAND_PALETTE_Z_INDEX: u32 = 20;

/// Registers the complete supported workspace from rectangles computed by layout.
pub(super) fn surface_tree(
    status: Rect,
    notices: Option<Rect>,
    attention: Option<Rect>,
    regions: BodyRegions,
    decision_mode: DecisionMode,
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
    // Its own section between the conversation and the composer, not over either: the decision is
    // an input addressed to this conversation, so it lives in that conversation's box
    // (`ui-ux.md` §input) — but answering a tool call and typing the next instruction are two
    // inputs, and the composer keeps its rows while one waits on the other.
    register_at(
        &mut tree,
        SurfaceId::Approval,
        regions.decision,
        match decision_mode {
            DecisionMode::Inline => SurfaceKind::Panel,
            DecisionMode::Modal => SurfaceKind::Modal,
        },
        MODAL_Z_INDEX,
    );
    register_at(
        &mut tree,
        SurfaceId::Configuration,
        regions.configuration,
        SurfaceKind::Modal,
        CONFIGURATION_Z_INDEX,
    );
    // Above the decision region, because layers stack: opening the list over a waiting approval
    // leaves the approval exactly where it was, and one `Escape` pops one layer.
    register_at(
        &mut tree,
        SurfaceId::CommandPalette,
        regions.command_palette,
        SurfaceKind::CommandPalette,
        COMMAND_PALETTE_Z_INDEX,
    );
    register(
        &mut tree,
        SurfaceId::Status,
        Some(status),
        SurfaceKind::Chrome,
    );

    tree
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
