//! Where an open inspector goes inside the conversation region.
//!
//! One rule governs all of it: the conversation keeps ten readable rows (D-023). Everything else —
//! the share a shelf takes by default, the height a user drags to, the size below which two
//! surfaces stop fitting — is that rule meeting a terminal of some particular size.
//!
//! Separated from the workspace's own row budget because the two answer different questions. That
//! one decides which regions exist; this one decides how one region is shared.

use super::{BodyRegions, LayoutClass, MIN_PANEL_HEIGHT, band};

/// Rows of the primary conversation an open inspector must leave readable (D-023).
const CONVERSATION_GUARANTEE: u16 = 10;

/// Share of the conversation region a shelf takes by default, in hundredths (D-016).
const SHELF_SHARE: u32 = 55;

/// Conversation region below which the shelf presentation is abandoned for maximized.
///
/// `ui-ux.md` gives the reason as the ten-row guarantee failing. It does not fail — below eighteen
/// rows the guarantee simply binds instead of the share, and the shelf shrinks toward nothing while
/// the conversation keeps its ten. What stops being true is that the *shelf* is worth being one: at
/// eighteen rows it is already down to two borders and six lines. Maximizing is the honest response
/// to a region that cannot hold two surfaces, and the number is kept as locked.
const MIN_SHELF_REGION: u16 = 18;

/// How an open inspector occupies the conversation region.
///
/// Chosen by terminal size, never stored: presentation is geometry, and `ui-ux.md` is explicit that
/// changing it must not change a surface's identity, scroll position, or focus.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Presentation {
    /// Docked to the top of the conversation, which keeps ten readable rows below it.
    Shelf,
    /// The secondary column, at a width that can hold a second agent beside the first.
    Column,
    /// The whole conversation region. Narrow's "one major surface at a time", and the fallback
    /// wherever two surfaces will not fit.
    Maximized,
}

/// What the workspace needs to know about an open inspector in order to place it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InspectorRequest {
    /// Whether the user asked for the whole conversation region.
    pub maximized: bool,
    /// Rows the user dragged the shelf to, if they ever did. Clamped, never trusted.
    pub rows: Option<u16>,
}

/// Gives an open inspector its rectangle, out of the conversation region or the secondary column.
///
/// The regions are split, never stacked. A docked shelf that took ten rows from the conversation
/// and also covered it would be counted twice by the tiling check and hit-tested by z-order for no
/// reason, so nothing in this workspace overlaps and nothing needs a z-index to resolve.
pub(super) fn place_inspector(
    base: BodyRegions,
    request: InspectorRequest,
    class: LayoutClass,
) -> BodyRegions {
    let Some(conversation) = base.transcript else {
        return base;
    };
    match presentation(class, request.maximized, conversation.height) {
        // The secondary column, in place of activity rather than beside it: D-024 allows exactly
        // one, and the inspector carries the same tools and artifacts that column was showing.
        Presentation::Column => BodyRegions {
            inspector: base.activity,
            activity: None,
            ..base
        },
        Presentation::Maximized => BodyRegions {
            transcript: None,
            inspector: Some(conversation),
            ..base
        },
        Presentation::Shelf => {
            let rows = shelf_rows(conversation.height, request.rows);
            BodyRegions {
                inspector: band(conversation, conversation.y, rows),
                transcript: band(
                    conversation,
                    conversation.y.saturating_add(rows),
                    conversation.height.saturating_sub(rows),
                ),
                ..base
            }
        }
    }
}

/// How an inspector occupies a conversation region this tall, at this terminal width.
const fn presentation(class: LayoutClass, maximized: bool, region_rows: u16) -> Presentation {
    if maximized {
        return Presentation::Maximized;
    }
    match class {
        LayoutClass::Ultrawide => Presentation::Column,
        // "One major surface at a time": at this width inspection is a full-region transition, not
        // a second thing sharing the screen. This is the Narrow contradiction recorded at step 2.
        LayoutClass::Narrow | LayoutClass::TooSmall => Presentation::Maximized,
        LayoutClass::Wide | LayoutClass::Medium => {
            if region_rows < MIN_SHELF_REGION {
                Presentation::Maximized
            } else {
                Presentation::Shelf
            }
        }
    }
}

/// Rows a shelf takes, leaving the conversation its guarantee (D-016, D-023).
///
/// The default is a share of the region; a height the user dragged to replaces it. Both are clamped
/// by the guarantee, so dragging is a choice within the contract rather than a way out of it.
fn shelf_rows(region: u16, requested: Option<u16>) -> u16 {
    let ceiling = region
        .saturating_sub(CONVERSATION_GUARANTEE)
        .max(MIN_PANEL_HEIGHT);
    let default = u16::try_from(u32::from(region) * SHELF_SHARE / 100).unwrap_or(u16::MAX);
    requested
        .unwrap_or(default)
        .clamp(MIN_PANEL_HEIGHT, ceiling)
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::{CONVERSATION_GUARANTEE, InspectorRequest, MIN_PANEL_HEIGHT};
    use crate::{
        layout::{WorkspaceInput, tests::SIZES, workspace},
        surface::{SurfaceId, SurfaceTree},
    };

    fn input(has_notices: bool) -> WorkspaceInput {
        WorkspaceInput {
            has_notices,
            ..WorkspaceInput::default()
        }
    }

    fn inspecting(has_notices: bool) -> WorkspaceInput {
        WorkspaceInput {
            inspector: Some(InspectorRequest::default()),
            ..input(has_notices)
        }
    }

    fn height_of(tree: &SurfaceTree, id: SurfaceId) -> Option<u16> {
        tree.get(id).map(|surface| surface.bounds.height)
    }

    /// D-023: an inspector never takes the conversation below ten readable rows.
    ///
    /// The rule is about what the inspector *takes*, not an absolute floor — a twelve-row terminal
    /// has fewer than ten rows of conversation before anything opens, and the inspector is not what
    /// made that true. So the comparison is against the same workspace with nothing open, and the
    /// inspector may close the gap down to ten and no further.
    ///
    /// Maximized is not an exception smuggled past it: a full-region transition is something the
    /// user asked for and `Escape` reverses, and it leaves no conversation to guarantee rows to.
    #[test]
    fn an_open_inspector_leaves_ten_readable_rows_or_takes_the_region_outright() {
        for (width, height) in SIZES {
            for has_notices in [false, true] {
                let area = Rect::new(0, 0, width, height);
                let closed = workspace(area, input(has_notices));
                let open = workspace(area, inspecting(has_notices));
                let context = format!("{width}x{height} notices={has_notices}");

                assert!(
                    open.get(SurfaceId::Inspector).is_some(),
                    "{context}: an open inspector must reach the screen at every supported size"
                );
                let Some(after) = height_of(&open, SurfaceId::Transcript) else {
                    continue;
                };
                let before = height_of(&closed, SurfaceId::Transcript).unwrap_or_default();
                assert!(
                    after >= before.min(CONVERSATION_GUARANTEE),
                    "{context}: the conversation went from {before} rows to {after}"
                );
            }
        }
    }

    /// Presentation is chosen by the terminal and the user, and never stored.
    #[test]
    fn presentation_follows_the_terminal_and_the_users_maximize() {
        let open = |width, height, request| {
            workspace(
                Rect::new(0, 0, width, height),
                WorkspaceInput {
                    inspector: Some(request),
                    ..WorkspaceInput::default()
                },
            )
        };
        let shelf = open(120, 40, InspectorRequest::default());
        assert!(
            height_of(&shelf, SurfaceId::Transcript).is_some_and(|rows| rows > 0),
            "a shelf shares the region with the conversation"
        );
        assert!(
            shelf.get(SurfaceId::Activity).is_some(),
            "and leaves the activity column alone"
        );

        // Narrow is "one major surface at a time": inspection is a full-region transition.
        let narrow = open(60, 40, InspectorRequest::default());
        assert!(narrow.get(SurfaceId::Transcript).is_none());
        assert!(narrow.get(SurfaceId::Inspector).is_some());

        // Ultrawide spends its width on a second agent rather than on the activity column (D-024).
        let ultrawide = open(140, 40, InspectorRequest::default());
        assert!(ultrawide.get(SurfaceId::Transcript).is_some());
        assert_eq!(
            ultrawide.get(SurfaceId::Activity),
            None,
            "there is exactly one secondary column, and the inspector is now it"
        );

        let maximized = open(
            120,
            40,
            InspectorRequest {
                maximized: true,
                rows: None,
            },
        );
        assert!(
            maximized.get(SurfaceId::Transcript).is_none(),
            "the user asked for the whole region"
        );
    }

    /// A height the user dragged to is a choice inside the guarantee, never a way out of it.
    #[test]
    fn a_dragged_height_is_clamped_rather_than_obeyed() {
        let with_rows = |rows| {
            workspace(
                Rect::new(0, 0, 120, 40),
                WorkspaceInput {
                    inspector: Some(InspectorRequest {
                        maximized: false,
                        rows: Some(rows),
                    }),
                    ..WorkspaceInput::default()
                },
            )
        };
        let default = workspace(Rect::new(0, 0, 120, 40), inspecting(false));
        let region = height_of(&default, SurfaceId::Inspector).unwrap_or_default()
            + height_of(&default, SurfaceId::Transcript).unwrap_or_default();

        let greedy = with_rows(u16::MAX);
        assert_eq!(
            height_of(&greedy, SurfaceId::Transcript),
            Some(CONVERSATION_GUARANTEE),
            "dragging past the guarantee stops at it rather than through it"
        );
        assert_eq!(
            height_of(&greedy, SurfaceId::Inspector),
            Some(region.saturating_sub(CONVERSATION_GUARANTEE))
        );

        let flattened = with_rows(0);
        assert_eq!(
            height_of(&flattened, SurfaceId::Inspector),
            Some(MIN_PANEL_HEIGHT),
            "and dragging it shut stops where a region can still say something"
        );

        let chosen = with_rows(14);
        assert_eq!(
            height_of(&chosen, SurfaceId::Inspector),
            Some(14),
            "a height inside the contract is simply obeyed"
        );
    }
}
