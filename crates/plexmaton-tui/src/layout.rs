//! Terminal geometry to named, registered surfaces.
//!
//! Layout is the only place that computes a workspace rectangle, and every rectangle it computes
//! is registered here. The renderer then draws from the registry rather than recomputing, so
//! painting and hit testing cannot disagree about where a region is.

use ratatui::layout::{Constraint, Layout, Rect};

use crate::surface::{Surface, SurfaceId, SurfaceKind, SurfaceTree};

/// Rows a bordered region needs before it can say anything: two borders and one line.
///
/// A region below this is worse than absent. It is still a focus stop and still a pointer target,
/// so the user can reach a surface that shows nothing and has no way to tell why.
const MIN_PANEL_HEIGHT: u16 = 3;

/// Rows the conversation keeps before an optional region may take its preferred height.
///
/// Deliberately above the bare minimum: activity detail must not win rows off the transcript down
/// to a single readable line, which is what a plain "does it fit" test would allow.
const TRANSCRIPT_COMFORT: u16 = 8;

/// Preferred heights of the regions that yield on a short terminal.
const RAIL_HEIGHT: u16 = 5;
const ACTIVITY_HEIGHT: u16 = 8;
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

    // The hint strip is one row and never negotiates; everything else bids for what is left.
    let footer = Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1);
    let above_footer = area.height.saturating_sub(footer.height);
    let notice_height = if has_notices {
        notice_rows(above_footer)
    } else {
        0
    };
    let body = Rect::new(
        area.x,
        area.y,
        area.width,
        above_footer.saturating_sub(notice_height),
    );
    let notices = band(area, body.bottom(), notice_height);

    let regions = body_regions(area, body);

    register(
        &mut tree,
        SurfaceId::Agents,
        regions.agents,
        SurfaceKind::Panel,
    );
    register(
        &mut tree,
        SurfaceId::Transcript,
        Some(regions.transcript),
        SurfaceKind::Panel,
    );
    register(
        &mut tree,
        SurfaceId::Activity,
        regions.activity,
        SurfaceKind::Panel,
    );
    // A bounded tail with no viewport of its own yet, so there is nothing for a pointer or a focus
    // stop to do in it. It becomes a panel when it gains scroll state in step 4.
    register(&mut tree, SurfaceId::Notices, notices, SurfaceKind::Chrome);
    register(
        &mut tree,
        SurfaceId::Footer,
        Some(footer),
        SurfaceKind::Chrome,
    );

    tree
}

/// Rows for the notice strip, which yields to the workspace rather than the other way round.
///
/// A fixed four rows is a third of the shortest supported terminal, and spending them here costs
/// the agent rail a screen that still had room for it. Degradation stays visible (D-003), so the
/// strip shrinks rather than disappearing; at the 12-row minimum it settles on three.
fn notice_rows(above_footer: u16) -> u16 {
    NOTICE_HEIGHT.min(above_footer.saturating_sub(RAIL_HEIGHT.saturating_add(MIN_PANEL_HEIGHT)))
}

/// What one frame's body is divided into. The conversation is the only region that always exists.
struct BodyRegions {
    agents: Option<Rect>,
    transcript: Rect,
    activity: Option<Rect>,
}

fn body_regions(area: Rect, body: Rect) -> BodyRegions {
    match LayoutClass::for_size(area.width, area.height) {
        // The second conversation column arrives with the inspector surface. Until then ultrawide
        // spends its extra width on the activity column rather than pretending to hold an agent
        // that does not exist yet.
        LayoutClass::Ultrawide => columns(body, 28, 52, 34),
        LayoutClass::Wide => columns(body, 26, 30, 30),
        LayoutClass::Medium => {
            let [agents, main] =
                Layout::horizontal([Constraint::Length(26), Constraint::Min(24)]).areas(body);
            let mut rows = main.height;
            let activity_height = reserve(&mut rows, ACTIVITY_HEIGHT, TRANSCRIPT_COMFORT);
            let transcript = Rect::new(main.x, main.y, main.width, rows);
            BodyRegions {
                agents: Some(agents),
                transcript,
                activity: band(main, transcript.bottom(), activity_height),
            }
        }
        // `TooSmall` returned before layout began, so it cannot reach here.
        LayoutClass::Narrow | LayoutClass::TooSmall => {
            let mut rows = body.height;
            // Reserved in the order the journey needs them. Agent identity outranks activity
            // detail, so on a short terminal the tools and artifacts are what go.
            let rail_height = reserve(&mut rows, RAIL_HEIGHT, MIN_PANEL_HEIGHT);
            let activity_height = reserve(&mut rows, ACTIVITY_HEIGHT, TRANSCRIPT_COMFORT);
            let transcript =
                Rect::new(body.x, body.y.saturating_add(rail_height), body.width, rows);
            BodyRegions {
                agents: band(body, body.y, rail_height),
                transcript,
                activity: band(body, transcript.bottom(), activity_height),
            }
        }
    }
}

/// Three full-height columns. The layout-class thresholds guarantee the width for all three.
fn columns(body: Rect, rail: u16, conversation: u16, activity: u16) -> BodyRegions {
    let [agents, transcript, activity] = Layout::horizontal([
        Constraint::Length(rail),
        Constraint::Min(conversation),
        Constraint::Length(activity),
    ])
    .areas(body);
    BodyRegions {
        agents: Some(agents),
        transcript,
        activity: Some(activity),
    }
}

/// Takes `want` rows if what remains still clears `floor`, and none at all otherwise.
///
/// All or nothing, deliberately. Ratatui's solver returns a zero-height rectangle rather than
/// failing, and half an agent rail is a border with no rail inside it. A region either gets enough
/// rows to say something or it does not appear and gives them to the conversation.
fn reserve(remaining: &mut u16, want: u16, floor: u16) -> u16 {
    if *remaining >= want.saturating_add(floor) {
        *remaining = remaining.saturating_sub(want);
        want
    } else {
        0
    }
}

fn band(column: Rect, top: u16, height: u16) -> Option<Rect> {
    (height > 0).then(|| Rect::new(column.x, top, column.width, height))
}

fn register(tree: &mut SurfaceTree, id: SurfaceId, bounds: Option<Rect>, kind: SurfaceKind) {
    let Some(bounds) = bounds else {
        return;
    };
    tree.insert(Surface {
        id,
        bounds,
        // Every workspace region is a sibling. Shelves and modals introduce depth in later slices.
        z_index: 0,
        kind,
    })
    .expect("each workspace region is registered exactly once, under a distinct identity");
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::{LayoutClass, workspace};
    use crate::surface::SurfaceId;

    /// Both sides of every layout-class threshold, the supported minimum, and short-but-wide
    /// shapes where only the height is under pressure.
    const SIZES: [(u16, u16); 8] = [
        (140, 40),
        (140, 12),
        (120, 24),
        (96, 12),
        (80, 20),
        (72, 12),
        (60, 30),
        (48, 12),
    ];

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

    /// SURF-1: a rectangle that layout computed but did not register would leave a hole here.
    #[test]
    fn registered_surfaces_tile_the_terminal_without_gaps_or_overlap() {
        for (width, height) in SIZES {
            for has_notices in [false, true] {
                let area = Rect::new(0, 0, width, height);
                let tree = workspace(area, has_notices);
                let registered: Vec<_> = tree.iter().map(|surface| surface.bounds).collect();

                let covered: u32 = registered.iter().map(|bounds| bounds.area()).sum();
                assert_eq!(
                    covered,
                    area.area(),
                    "{width}x{height} notices={has_notices}: registered regions must cover the \
                     terminal"
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
    }

    /// A region with no room to draw is worse than an absent one: it is still a focus stop and
    /// still a pointer target, and it shows nothing that would explain either.
    ///
    /// Ratatui's solver returns a zero-height rectangle rather than failing, so this is the check
    /// that keeps the workspace honest about what it can actually display.
    #[test]
    fn no_registered_region_is_too_small_to_draw() {
        for (width, height) in SIZES {
            for has_notices in [false, true] {
                let tree = workspace(Rect::new(0, 0, width, height), has_notices);

                for surface in tree.iter() {
                    // The hint strip is one unbordered row by design; a bordered region needs two
                    // borders and a line of content before the rectangle is worth registering.
                    let floor = if surface.id == SurfaceId::Footer {
                        1
                    } else {
                        super::MIN_PANEL_HEIGHT
                    };
                    assert!(
                        surface.bounds.height >= floor,
                        "{width}x{height} notices={has_notices}: {:?} got {} rows",
                        surface.id,
                        surface.bounds.height
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
                .is_some_and(|surface| surface.kind.accepts_pointer())
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
            "no viewport until step 4"
        );
    }

    /// SURF-3: the ring may lose stops on a short terminal, but it never reorders.
    ///
    /// Membership varies because a region with no room to draw is not registered at all. Order is
    /// the part the user builds muscle memory on, so that is the part held fixed.
    #[test]
    fn the_focus_ring_loses_stops_without_ever_reordering() {
        const CANONICAL: [SurfaceId; 3] = [
            SurfaceId::Agents,
            SurfaceId::Transcript,
            SurfaceId::Activity,
        ];

        for (width, height) in SIZES {
            for has_notices in [false, true] {
                let tree = workspace(Rect::new(0, 0, width, height), has_notices);
                let ring: Vec<_> = tree.focus_ring().collect();
                let context = format!("{width}x{height} notices={has_notices}");

                let mut canonical = CANONICAL.iter();
                for stop in &ring {
                    assert!(
                        canonical.any(|expected| expected == stop),
                        "{context}: ring {ring:?} is not in canonical order"
                    );
                }
                assert!(
                    ring.contains(&SurfaceId::Transcript),
                    "{context}: the conversation must always be a stop"
                );
            }
        }
    }

    /// The workspace sheds detail before identity, and never sheds the conversation.
    #[test]
    fn a_short_terminal_drops_activity_before_the_agent_rail() {
        let cramped = workspace(Rect::new(0, 0, 48, 12), false);
        assert!(cramped.get(SurfaceId::Transcript).is_some());
        assert!(cramped.get(SurfaceId::Agents).is_some(), "identity stays");
        assert!(
            cramped.get(SurfaceId::Activity).is_none(),
            "tools and artifacts are the detail that goes first"
        );

        let roomy = workspace(Rect::new(0, 0, 60, 30), false);
        assert!(
            roomy.get(SurfaceId::Activity).is_some(),
            "and comes back when the rows exist"
        );
    }

    /// The notice strip competes with the agent rail for the same rows on a short terminal, and
    /// must lose. A fixed four rows costs identity on a screen that still had room for it, while
    /// hiding the strip entirely would make a producer defect invisible (D-003).
    #[test]
    fn the_notice_strip_yields_rows_rather_than_costing_agent_identity() {
        let tree = workspace(Rect::new(0, 0, 48, 12), true);

        let rail = tree.get(SurfaceId::Agents);
        let strip = tree.get(SurfaceId::Notices);
        assert!(rail.is_some(), "the agent rail survives a notice");
        assert!(
            strip.is_some(),
            "and the notice stays visible while it does"
        );
        assert!(
            strip.is_some_and(|surface| surface.bounds.height < super::NOTICE_HEIGHT),
            "the strip is the one that gave rows up"
        );
    }
}
