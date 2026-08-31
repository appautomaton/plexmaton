//! Terminal geometry to named, registered surfaces.
//!
//! Layout is the only place that computes a workspace rectangle, and every rectangle it computes
//! is registered here. The renderer then draws from the registry rather than recomputing, so
//! painting and hit testing cannot disagree about where a region is.

use ratatui::layout::{Constraint, Layout, Rect};

use crate::surface::{Surface, SurfaceId, SurfaceTree};

/// Rows reserved for the notice strip when it has something to report.
const NOTICE_HEIGHT: u16 = 4;

/// Smallest terminal that can still express the canonical journey.
///
/// Below this the honest response is one explicit notice, not a layout clipped until it lies.
pub(crate) const MIN_WIDTH: u16 = 48;
pub(crate) const MIN_HEIGHT: u16 = 12;

/// Responsive composition selected from terminal size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayoutClass {
    /// Below the supported minimum; render a notice rather than a broken workspace.
    TooSmall,
    /// One column; every region becomes a stacked band.
    Narrow,
    /// Agent column plus conversation; activity compresses to markers.
    Medium,
    /// Agent column and conversation; a second agent arrives as a shelf over the conversation.
    Wide,
    /// Two conversations side by side; a second agent earns a column of its own.
    Ultrawide,
}

impl LayoutClass {
    /// Chooses the composition for a terminal size in cells.
    #[must_use]
    pub const fn for_size(width: u16, height: u16) -> Self {
        if width < MIN_WIDTH || height < MIN_HEIGHT {
            Self::TooSmall
        } else if width >= 132 {
            Self::Ultrawide
        } else if width >= 96 {
            Self::Wide
        } else if width >= 72 {
            Self::Medium
        } else {
            Self::Narrow
        }
    }
}

/// Registers every workspace region for one frame.
///
/// A terminal below the minimum registers nothing: the notice that replaces the workspace has no
/// interactive region, so a pointer event there must resolve to nothing rather than to a guess.
#[must_use]
pub fn workspace(area: Rect, has_notices: bool) -> SurfaceTree {
    let mut tree = SurfaceTree::default();
    if LayoutClass::for_size(area.width, area.height) == LayoutClass::TooSmall {
        return tree;
    }

    let notice_height = if has_notices { NOTICE_HEIGHT } else { 0 };
    let [body, notices, footer] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(notice_height),
        Constraint::Length(1),
    ])
    .areas(area);

    let (agents, transcript, activity) = body_regions(area, body);

    register(&mut tree, SurfaceId::Agents, agents, true);
    register(&mut tree, SurfaceId::Transcript, transcript, true);
    register(&mut tree, SurfaceId::Activity, activity, true);
    if has_notices {
        // The strip is a bounded tail with no viewport of its own yet, so there is nothing for a
        // pointer to do in it. It becomes pointer-eligible when it gains scroll state in step 3.
        register(&mut tree, SurfaceId::Notices, notices, false);
    }
    register(&mut tree, SurfaceId::Footer, footer, false);

    tree
}

fn body_regions(area: Rect, body: Rect) -> (Rect, Rect, Rect) {
    match LayoutClass::for_size(area.width, area.height) {
        // The second conversation column arrives with the inspector surface. Until then ultrawide
        // spends its extra width on the activity column rather than pretending to hold an agent
        // that does not exist yet.
        LayoutClass::Ultrawide => {
            let [agents, transcript, activity] = Layout::horizontal([
                Constraint::Length(28),
                Constraint::Min(52),
                Constraint::Length(34),
            ])
            .areas(body);
            (agents, transcript, activity)
        }
        LayoutClass::Wide => {
            let [agents, transcript, activity] = Layout::horizontal([
                Constraint::Length(26),
                Constraint::Min(30),
                Constraint::Length(30),
            ])
            .areas(body);
            (agents, transcript, activity)
        }
        LayoutClass::Medium => {
            let [agents, main] =
                Layout::horizontal([Constraint::Length(26), Constraint::Min(24)]).areas(body);
            let [transcript, activity] =
                Layout::vertical([Constraint::Min(6), Constraint::Length(8)]).areas(main);
            (agents, transcript, activity)
        }
        // `TooSmall` returned before layout began, so it cannot reach here.
        LayoutClass::Narrow | LayoutClass::TooSmall => {
            let [agents, transcript, activity] = Layout::vertical([
                Constraint::Length(5),
                Constraint::Min(6),
                Constraint::Length(8),
            ])
            .areas(body);
            (agents, transcript, activity)
        }
    }
}

fn register(tree: &mut SurfaceTree, id: SurfaceId, bounds: Rect, accepts_pointer: bool) {
    tree.insert(Surface {
        id,
        bounds,
        // Every workspace region is a sibling. Shelves and modals introduce depth in later slices.
        z_index: 0,
        accepts_pointer,
    })
    .expect("each workspace region is registered exactly once, under a distinct identity");
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::{LayoutClass, workspace};
    use crate::surface::SurfaceId;

    #[test]
    fn layout_class_covers_every_threshold() {
        assert_eq!(LayoutClass::for_size(132, 40), LayoutClass::Ultrawide);
        assert_eq!(LayoutClass::for_size(131, 40), LayoutClass::Wide);
        assert_eq!(LayoutClass::for_size(96, 40), LayoutClass::Wide);
        assert_eq!(LayoutClass::for_size(95, 40), LayoutClass::Medium);
        assert_eq!(LayoutClass::for_size(72, 40), LayoutClass::Medium);
        assert_eq!(LayoutClass::for_size(71, 40), LayoutClass::Narrow);
        assert_eq!(LayoutClass::for_size(48, 12), LayoutClass::Narrow);
    }

    #[test]
    fn either_dimension_below_the_minimum_is_too_small() {
        assert_eq!(LayoutClass::for_size(47, 40), LayoutClass::TooSmall);
        assert_eq!(LayoutClass::for_size(200, 11), LayoutClass::TooSmall);
        assert_eq!(LayoutClass::for_size(47, 11), LayoutClass::TooSmall);
    }

    /// C-1: a rectangle that layout computed but did not register would leave a hole here.
    #[test]
    fn registered_surfaces_tile_the_terminal_without_gaps_or_overlap() {
        for (width, height, has_notices) in [
            (140, 30, true),
            (120, 24, true),
            (120, 24, false),
            (80, 20, false),
            (60, 30, true),
            (48, 12, false),
        ] {
            let area = Rect::new(0, 0, width, height);
            let tree = workspace(area, has_notices);
            let registered: Vec<_> = tree.iter().map(|surface| surface.bounds).collect();

            let covered: u32 = registered.iter().map(|bounds| bounds.area()).sum();
            assert_eq!(
                covered,
                area.area(),
                "{width}x{height} notices={has_notices}: registered regions must cover the terminal"
            );

            for (index, first) in registered.iter().enumerate() {
                for second in &registered[index.saturating_add(1)..] {
                    assert!(
                        first.intersection(*second).is_empty(),
                        "{width}x{height}: {first:?} and {second:?} overlap"
                    );
                }
            }
        }
    }

    #[test]
    fn the_notice_strip_is_registered_only_when_a_notice_exists() {
        let area = Rect::new(0, 0, 120, 24);

        assert!(workspace(area, false).get(SurfaceId::Notices).is_none());
        assert!(workspace(area, true).get(SurfaceId::Notices).is_some());
    }

    #[test]
    fn a_terminal_below_the_minimum_registers_nothing() {
        // The notice that replaces the workspace has no interactive region. Registering a region
        // anyway would let a click resolve to a panel the user cannot see.
        assert!(workspace(Rect::new(0, 0, 40, 10), true).is_empty());
    }

    #[test]
    fn the_wheel_cannot_reach_a_region_that_has_no_viewport() {
        let tree = workspace(Rect::new(0, 0, 120, 24), true);
        let pointer_eligible = |id| {
            tree.get(id)
                .map(|surface| surface.accepts_pointer)
                .unwrap_or_default()
        };

        assert!(pointer_eligible(SurfaceId::Transcript));
        assert!(pointer_eligible(SurfaceId::Agents));
        assert!(pointer_eligible(SurfaceId::Activity));
        assert!(
            !pointer_eligible(SurfaceId::Footer),
            "a hint strip is not a target"
        );
        assert!(
            !pointer_eligible(SurfaceId::Notices),
            "no viewport until step 3"
        );
    }
}
