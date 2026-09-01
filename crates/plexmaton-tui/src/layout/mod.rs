//! Terminal geometry to named, registered surfaces.
//!
//! Layout is the only place that computes a workspace rectangle, and every rectangle it computes
//! is registered here. The renderer then draws from the registry rather than recomputing, so
//! painting and hit testing cannot disagree about where a region is.

mod inspector;

use ratatui::layout::{Constraint, Layout, Rect};

pub use inspector::InspectorRequest;

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

/// Requests the band shows before it starts scrolling instead of growing.
///
/// The queue is unbounded and the conversation is not negotiable, so past this the band keeps its
/// height and the rest of the queue arrives by scrolling it.
const ATTENTION_LISTED: usize = 3;

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

/// What the workspace needs from the projection in order to lay itself out.
///
/// A value rather than a borrow of `ViewState`, so layout stays testable without constructing a
/// projection, and a struct rather than a growing parameter list because the shelf adds to it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceInput {
    /// Whether the notice strip has anything to report.
    pub has_notices: bool,
    /// How many background requests are queued. Zero registers no band at all.
    pub attention: usize,
    /// Rows the composer asks for, borders included. Grows as the draft gains lines.
    pub composer_rows: u16,
    /// The open inspector, if one is open.
    pub inspector: Option<InspectorRequest>,
}

impl Default for WorkspaceInput {
    fn default() -> Self {
        Self {
            has_notices: false,
            attention: 0,
            // Two borders and one line: an empty composer is still a place to type.
            composer_rows: MIN_PANEL_HEIGHT,
            inspector: None,
        }
    }
}

/// Registers every workspace region for one frame.
///
/// A terminal below the minimum registers nothing: the notice that replaces the workspace has no
/// interactive region, so a pointer event there must resolve to nothing rather than to a guess.
///
/// Rows are handed out from the bottom up, in the order the journey cannot do without them: the
/// hint strip, then the composer, then the notice strip, and the workspace body takes what is
/// left. At the supported minimum that order is what decides which region disappears.
#[must_use]
pub fn workspace(area: Rect, input: WorkspaceInput) -> SurfaceTree {
    let mut tree = SurfaceTree::default();
    if LayoutClass::for_size(area.width, area.height) == LayoutClass::TooSmall {
        return tree;
    }

    // The hint strip is one row and never negotiates; everything else bids for what is left.
    let footer = Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1);
    let mut budget = area.height.saturating_sub(footer.height);

    // Typing is the one thing a workspace this small still has to allow, so the composer is served
    // before the notice strip and before the body, and only clamped to keep the conversation.
    // The floor is one row, not three: a collapsed composer is a single unbordered row (D-027),
    // and the projection is what decides it is collapsed.
    let composer_height = input
        .composer_rows
        .clamp(1, budget.saturating_sub(MIN_PANEL_HEIGHT).max(1));
    budget = budget.saturating_sub(composer_height);

    let notice_height = if input.has_notices {
        notice_rows(budget)
    } else {
        0
    };
    budget = budget.saturating_sub(notice_height);

    // Served after the notice strip for the same reason the strip outranks the agent rail: a
    // silently wrong projection has no other signal, while a blocked agent also shows as `Waiting`
    // in the rail and its request survives until the rows come back.
    let attention_height = attention_rows(input.attention, budget);
    budget = budget.saturating_sub(attention_height);

    let body = Rect::new(area.x, area.y, area.width, budget);
    let notices = band(area, body.bottom(), notice_height);
    let attention_top = body.bottom().saturating_add(notice_height);
    let attention = band(area, attention_top, attention_height);
    let composer = Rect::new(
        area.x,
        attention_top.saturating_add(attention_height),
        area.width,
        composer_height,
    );

    let regions = body_regions(area, body, input.inspector);

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
    register(
        &mut tree,
        SurfaceId::Inspector,
        regions.inspector,
        SurfaceKind::Inspector,
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
        Some(composer),
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
/// It outranks the agent rail and loses to the conversation. A projection that is silently wrong
/// is the failure D-003 exists to prevent and the user has no other way to detect it, whereas a
/// missing rail is visible in itself and recovered by resizing.
fn notice_rows(available: u16) -> u16 {
    NOTICE_HEIGHT.min(available.saturating_sub(MIN_PANEL_HEIGHT))
}

/// Rows for the Attention band: two borders plus up to three requests, and none at all below that.
///
/// All or nothing for the same reason `reserve` is: a band that cannot show one request is a focus
/// stop and a pointer target advertising a queue the user cannot read. The queue survives without
/// it — the rail carries the count, and the band returns when the rows do.
fn attention_rows(queued: usize, available: u16) -> u16 {
    if queued == 0 {
        return 0;
    }
    let listed = u16::try_from(queued.clamp(1, ATTENTION_LISTED)).unwrap_or(1);
    let want = listed.saturating_add(2);
    let room = available.saturating_sub(TRANSCRIPT_COMFORT);
    if room >= MIN_PANEL_HEIGHT {
        want.min(room)
    } else {
        0
    }
}

/// What one frame's body is divided into.
///
/// Every region is optional now, including the conversation: a maximized inspector is a full-region
/// transition, and registering a conversation with no rows to draw would leave a focus stop and a
/// pointer target showing nothing.
pub(super) struct BodyRegions {
    agents: Option<Rect>,
    pub(super) transcript: Option<Rect>,
    pub(super) inspector: Option<Rect>,
    pub(super) activity: Option<Rect>,
}

fn body_regions(area: Rect, body: Rect, inspector: Option<InspectorRequest>) -> BodyRegions {
    let class = LayoutClass::for_size(area.width, area.height);
    let base = match class {
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
                transcript: Some(transcript),
                inspector: None,
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
                transcript: Some(transcript),
                inspector: None,
                activity: band(body, transcript.bottom(), activity_height),
            }
        }
    };

    match inspector {
        Some(request) => inspector::place_inspector(base, request, class),
        None => base,
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
        transcript: Some(transcript),
        inspector: None,
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
        // Layout owns rectangles, not content. The renderer measures and fills this in.
        viewport: None,
    })
    .expect("each workspace region is registered exactly once, under a distinct identity");
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::{InspectorRequest, LayoutClass, MIN_PANEL_HEIGHT, WorkspaceInput, workspace};
    use crate::surface::SurfaceId;

    /// The default composer, which is the shape every one of these sizes is checked against.
    fn input(has_notices: bool) -> WorkspaceInput {
        WorkspaceInput {
            has_notices,
            ..WorkspaceInput::default()
        }
    }

    /// The same, with an inspector open in its default presentation.
    fn inspecting(has_notices: bool) -> WorkspaceInput {
        WorkspaceInput {
            inspector: Some(InspectorRequest::default()),
            ..input(has_notices)
        }
    }

    /// Every shape the workspace can be in, so a rule is checked against all of them or none.
    fn shapes() -> impl Iterator<Item = (u16, u16, WorkspaceInput)> {
        SIZES.into_iter().flat_map(|(width, height)| {
            [false, true].into_iter().flat_map(move |has_notices| {
                [input(has_notices), inspecting(has_notices)]
                    .into_iter()
                    .flat_map(move |base| {
                        // Zero, one, and more than the band lists: the three cases its height
                        // function distinguishes.
                        [0, 1, 9].into_iter().map(move |attention| {
                            (width, height, WorkspaceInput { attention, ..base })
                        })
                    })
            })
        })
    }

    /// The composer is never covered, at any size (`ui-ux.md` §shelf).
    #[test]
    fn the_composer_survives_every_presentation() {
        for (width, height, input) in shapes() {
            let tree = workspace(Rect::new(0, 0, width, height), input);
            assert!(
                tree.get(SurfaceId::Composer).is_some(),
                "{width}x{height}: a workspace you cannot type into is not one of the shapes"
            );
        }
    }

    /// Both sides of every layout-class threshold, the supported minimum, and short-but-wide
    /// shapes where only the height is under pressure.
    pub(super) const SIZES: [(u16, u16); 8] = [
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
        for (width, height, input) in shapes() {
            let area = Rect::new(0, 0, width, height);
            let tree = workspace(area, input);
            let registered: Vec<_> = tree.iter().map(|surface| surface.bounds).collect();

            let covered: u32 = registered.iter().map(|bounds| bounds.area()).sum();
            assert_eq!(
                covered,
                area.area(),
                "{width}x{height} {input:?}: registered regions must cover the terminal"
            );

            for (index, first) in registered.iter().enumerate() {
                for second in &registered[index.saturating_add(1)..] {
                    assert!(
                        first.intersection(*second).is_empty(),
                        "{width}x{height} {input:?}: {first:?} and {second:?} overlap"
                    );
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
        for (width, height, input) in shapes() {
            let tree = workspace(Rect::new(0, 0, width, height), input);

            for surface in tree.iter() {
                // The hint strip is one unbordered row by design; a bordered region needs two
                // borders and a line of content before the rectangle is worth registering.
                // The hint strip and a collapsed composer are single unbordered rows by design;
                // every bordered region needs two borders and a line before it is worth drawing.
                let floor = if surface.id == SurfaceId::Footer {
                    1
                } else {
                    MIN_PANEL_HEIGHT
                };
                assert!(
                    surface.bounds.height >= floor,
                    "{width}x{height} {input:?}: {:?} got {} rows",
                    surface.id,
                    surface.bounds.height
                );
            }
        }
    }

    #[test]
    fn the_notice_strip_is_registered_only_when_a_notice_exists() {
        let area = Rect::new(0, 0, 120, 24);

        assert!(
            workspace(area, input(false))
                .get(SurfaceId::Notices)
                .is_none()
        );
        assert!(
            workspace(area, input(true))
                .get(SurfaceId::Notices)
                .is_some()
        );
    }

    #[test]
    fn a_terminal_below_the_minimum_registers_nothing() {
        // The notice that replaces the workspace has no interactive region. Registering a region
        // anyway would let a click resolve to a panel the user cannot see.
        assert!(workspace(Rect::new(0, 0, 40, 10), input(true)).is_empty());
    }

    /// Anything the wheel can reach must also be reachable by keyboard, so pointer eligibility and
    /// the focus ring are one decision made by `SurfaceKind` rather than two that could disagree.
    #[test]
    fn only_the_hint_strip_is_beyond_the_pointer() {
        let tree = workspace(Rect::new(0, 0, 120, 24), input(true));
        let pointer_eligible = |id| {
            tree.get(id)
                .is_some_and(|surface| surface.kind.accepts_pointer())
        };

        for id in [
            SurfaceId::Agents,
            SurfaceId::Transcript,
            SurfaceId::Activity,
            SurfaceId::Notices,
            SurfaceId::Composer,
        ] {
            assert!(pointer_eligible(id), "{id:?} must be reachable");
        }
        assert!(
            !pointer_eligible(SurfaceId::Footer),
            "a hint strip shows keys; there is nothing in it to point at"
        );
    }

    /// SURF-3: the ring may lose stops on a short terminal, but it never reorders.
    ///
    /// Membership varies because a region with no room to draw is not registered at all. Order is
    /// the part the user builds muscle memory on, so that is the part held fixed.
    #[test]
    fn the_focus_ring_loses_stops_without_ever_reordering() {
        const CANONICAL: [SurfaceId; 7] = [
            SurfaceId::Agents,
            SurfaceId::Transcript,
            SurfaceId::Inspector,
            SurfaceId::Activity,
            SurfaceId::Notices,
            SurfaceId::Attention,
            SurfaceId::Composer,
        ];

        for (width, height, input) in shapes() {
            {
                let tree = workspace(Rect::new(0, 0, width, height), input);
                let ring: Vec<_> = tree.focus_ring().collect();
                let context = format!("{width}x{height} {input:?}");

                let mut canonical = CANONICAL.iter();
                for stop in &ring {
                    assert!(
                        canonical.any(|expected| expected == stop),
                        "{context}: ring {ring:?} is not in canonical order"
                    );
                }
                // Not "the conversation is always a stop": a maximized inspector takes the region
                // outright (INS-2), and then *it* is the conversation on screen. Widening this
                // test to every shape is what showed the older claim was stated too strongly.
                assert!(
                    ring.contains(&SurfaceId::Transcript) || ring.contains(&SurfaceId::Inspector),
                    "{context}: some conversation must be reachable"
                );
                assert!(
                    ring.contains(&SurfaceId::Composer),
                    "{context}: a workspace you cannot type into is not one of the shapes"
                );
            }
        }
    }

    /// The workspace sheds detail before identity, and never sheds the conversation.
    #[test]
    fn a_short_terminal_drops_activity_before_the_agent_rail() {
        let cramped = workspace(Rect::new(0, 0, 48, 12), input(false));
        assert!(cramped.get(SurfaceId::Transcript).is_some());
        assert!(cramped.get(SurfaceId::Agents).is_some(), "identity stays");
        assert!(
            cramped.get(SurfaceId::Activity).is_none(),
            "tools and artifacts are the detail that goes first"
        );

        let roomy = workspace(Rect::new(0, 0, 60, 30), input(false));
        assert!(
            roomy.get(SurfaceId::Activity).is_some(),
            "and comes back when the rows exist"
        );
    }

    /// What survives at the smallest supported terminal, in priority order.
    ///
    /// Twelve rows cannot hold a hint strip, a composer, a conversation, a notice strip and an
    /// agent rail at once, so this pins which of them goes. Typing and the conversation are the
    /// workspace. A producer defect the user cannot see is the failure D-003 exists to prevent, and
    /// nothing else signals it. A missing agent rail is visible in itself and returns on resize, so
    /// the rail is what yields.
    ///
    /// This replaced an assertion that the rail always survives a notice. That was true before the
    /// composer took three of these twelve rows, and the composer is not the thing to give up.
    #[test]
    fn the_smallest_terminal_keeps_typing_the_conversation_and_the_defect_notice() {
        let quiet = workspace(Rect::new(0, 0, 48, 12), input(false));
        assert!(quiet.get(SurfaceId::Composer).is_some());
        assert!(quiet.get(SurfaceId::Transcript).is_some());
        assert!(
            quiet.get(SurfaceId::Agents).is_some(),
            "with no defect to report there is room for the rail"
        );

        let degraded = workspace(Rect::new(0, 0, 48, 12), input(true));
        assert!(degraded.get(SurfaceId::Composer).is_some());
        assert!(degraded.get(SurfaceId::Transcript).is_some());
        assert!(
            degraded.get(SurfaceId::Notices).is_some(),
            "a silently wrong projection is the failure with no other signal"
        );
        assert!(
            degraded.get(SurfaceId::Agents).is_none(),
            "the rail is what yields, and it returns as soon as the rows do"
        );
    }
}
