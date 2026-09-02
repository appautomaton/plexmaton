//! Where an open inspector goes inside the conversation region.
//!
//! One rule governs all of it: the conversation keeps ten readable rows (D-023). Everything else —
//! the share a shelf takes by default, the height a user drags to, the size below which two
//! surfaces stop fitting — is that rule meeting a terminal of some particular size.
//!
//! Separated from the workspace's own row budget because the two answer different questions. That
//! one decides which regions exist; this one decides how one region is shared — the conversation
//! region with the inspector, and then the inspector's own rectangle with its input.

use ratatui::layout::{Constraint, Layout, Margin, Rect};

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
        // one. The inspector does not carry what that column was showing — it is a conversation
        // (INS-6, D-046) — so the selected agent's tools, artifacts and mail are off screen until
        // this closes. Recorded as a Phase 00 limitation rather than worked around here.
        // The second conversation earns a column of its own beside the first (D-024), and the two
        // are equals: ultrawide is sized for two conversations of the same width, and the agent
        // column and its activity are untouched.
        Presentation::Column => {
            let [transcript, inspector] =
                Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
                    .areas(conversation);
            // The composer belongs to the primary's column alone, and the second column runs
            // the full height beside it: the composer's rows were carved from the conversation
            // before the split, so they are given back to the column that has no composer.
            let inspector = Rect {
                height: inspector.height.saturating_add(base.composer.height),
                ..inspector
            };
            BodyRegions {
                transcript: Some(transcript),
                inspector: Some(inspector),
                composer: Rect {
                    width: transcript.width,
                    ..base.composer
                },
                ..base
            }
        }
        Presentation::Maximized => BodyRegions {
            transcript: None,
            inspector: Some(conversation),
            ..base
        },
        // The shelf floats over the conversation rather than splitting it (`ui-ux.md` §shelf):
        // the conversation keeps its whole rectangle, its title and its reading position, and the
        // shelf covers the top of its interior — the rows already read — leaving the guarantee
        // visible beneath. "Region" in the height formula is that interior.
        Presentation::Shelf => {
            let interior = conversation.inner(Margin::new(1, 1));
            let rows = shelf_rows(interior.height, request.rows);
            BodyRegions {
                inspector: band(interior, interior.y, rows),
                inspector_floats: true,
                transcript: Some(conversation),
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

/// An inspector's rectangle, divided between its conversation and its steer input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SteerSplit {
    /// What is left for the inspected agent's conversation.
    pub conversation: Rect,
    /// The strip the input is drawn into, at the bottom.
    pub input: Rect,
}

/// Divides an inspector's rectangle between its conversation and an input `wanted` rows tall.
///
/// `None` when the two will not both fit, which is the all-or-nothing rule the row budget uses: an
/// input squeezed to nothing is a place the cursor claims to be and is not (INS-5). It is the one
/// answer to whether the inspector has an input at all — the renderer draws from it and focus
/// derives the cursor from it, so the affordance and the caret cannot disagree (INS-7).
///
/// Geometry, and therefore here rather than in the renderer: rendering is a projection of state,
/// and which of two states a surface is in must not be decided inside a draw call.
#[must_use]
pub fn steer_split(bounds: Rect, wanted: u16) -> Option<SteerSplit> {
    let rows = wanted.min(bounds.height.saturating_sub(MIN_PANEL_HEIGHT));
    if rows < MIN_PANEL_HEIGHT {
        return None;
    }
    let conversation = Rect {
        height: bounds.height.saturating_sub(rows),
        ..bounds
    };
    Some(SteerSplit {
        conversation,
        input: Rect {
            y: conversation.bottom(),
            height: rows,
            ..bounds
        },
    })
}

#[cfg(test)]
mod tests {
    use ratatui::layout::{Margin, Rect};

    use super::{CONVERSATION_GUARANTEE, InspectorRequest, MIN_PANEL_HEIGHT, steer_split};
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

    /// Rows of the conversation's interior left readable beneath a shelf, or the whole interior.
    fn readable_rows(tree: &SurfaceTree) -> Option<u16> {
        let conversation = tree.get(SurfaceId::Transcript)?.bounds;
        let interior = conversation.inner(Margin::new(1, 1));
        let covered = tree
            .get(SurfaceId::Inspector)
            .filter(|shelf| interior.union(shelf.bounds) == interior)
            .map_or(0, |shelf| shelf.bounds.bottom().saturating_sub(interior.y));
        Some(interior.height.saturating_sub(covered))
    }

    /// INS-5, INS-7: the input and the conversation both fit or the input does not appear.
    ///
    /// The tiling half is what stops the two rectangles from being drawn over each other, and the
    /// boundary is where the surface changes what focus means — so both are pinned to a number
    /// rather than left to be read off the implementation.
    #[test]
    fn an_inspector_splits_for_its_input_only_when_both_still_fit() {
        let bounds = Rect::new(4, 2, 30, 12);
        let wanted = MIN_PANEL_HEIGHT;

        for height in 0..MIN_PANEL_HEIGHT * 2 {
            assert_eq!(
                steer_split(Rect { height, ..bounds }, wanted),
                None,
                "{height} rows cannot hold a conversation and an input, so it holds neither"
            );
        }

        let split = steer_split(bounds, wanted)
            .unwrap_or_else(|| panic!("twelve rows hold both several times over"));
        assert_eq!(
            split.conversation.height + split.input.height,
            bounds.height,
            "the two halves tile the rectangle: a shared row would be painted twice"
        );
        assert_eq!(split.input.y, split.conversation.bottom());
        assert_eq!(
            split.input.height, wanted,
            "an input that fits gets the rows it asked for"
        );
        assert_eq!(
            steer_split(bounds, u16::MAX).map(|split| split.conversation.height),
            Some(MIN_PANEL_HEIGHT),
            "and one that asks for everything still leaves a conversation behind"
        );
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
                let Some(after) = readable_rows(&open) else {
                    continue;
                };
                let before = readable_rows(&closed).unwrap_or_default();
                assert!(
                    after >= before.min(CONVERSATION_GUARANTEE),
                    "{context}: the conversation went from {before} readable rows to {after}"
                );
                let shelf = open
                    .get(SurfaceId::Inspector)
                    .unwrap_or_else(|| panic!("{context}: asserted open above"));
                let conversation = open
                    .get(SurfaceId::Transcript)
                    .unwrap_or_else(|| {
                        panic!("{context}: this presentation keeps the conversation")
                    })
                    .bounds;
                if shelf.z_index > 0 {
                    assert_eq!(
                        conversation.union(shelf.bounds),
                        conversation,
                        "{context}: a shelf floats inside the conversation it covers"
                    );
                }
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
        assert_eq!(
            height_of(&shelf, SurfaceId::Transcript),
            height_of(
                &workspace(Rect::new(0, 0, 120, 40), input(false)),
                SurfaceId::Transcript
            ),
            "a shelf floats over the conversation, which keeps its whole rectangle"
        );
        assert!(
            shelf
                .get(SurfaceId::Inspector)
                .is_some_and(|surface| surface.z_index > 0),
            "and it is the one surface above the base layer"
        );
        assert!(
            shelf.get(SurfaceId::Activity).is_some(),
            "and leaves the activity column alone"
        );

        // Narrow is "one major surface at a time": inspection is a full-region transition.
        let narrow = open(60, 40, InspectorRequest::default());
        assert!(narrow.get(SurfaceId::Transcript).is_none());
        assert!(narrow.get(SurfaceId::Inspector).is_some());

        // Ultrawide gives the second agent a column of its own beside the conversation (D-024);
        // the agent column, list and activity stacked, is untouched (D-014).
        let ultrawide = open(140, 40, InspectorRequest::default());
        let conversation = ultrawide
            .get(SurfaceId::Transcript)
            .unwrap_or_else(|| panic!("the conversation stays"))
            .bounds;
        let second = ultrawide
            .get(SurfaceId::Inspector)
            .unwrap_or_else(|| panic!("the second window is open"));
        assert_eq!(
            second.bounds.x,
            conversation.right(),
            "beside the conversation"
        );
        assert_eq!(second.bounds.y, conversation.y);
        assert_eq!(second.z_index, 0, "a column tiles; only a shelf floats");
        assert!(
            ultrawide.get(SurfaceId::Activity).is_some(),
            "and the activity stays in the agent column"
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
        let closed = workspace(Rect::new(0, 0, 120, 40), input(false));
        let region = readable_rows(&closed).unwrap_or_default();

        let greedy = with_rows(u16::MAX);
        assert_eq!(
            readable_rows(&greedy),
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
