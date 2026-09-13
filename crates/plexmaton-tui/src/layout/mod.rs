//! Terminal geometry to named, registered surfaces.
//!
//! Layout is the only place that computes a workspace rectangle, and every rectangle it computes
//! is registered here. The renderer then draws from the registry rather than recomputing, so
//! painting and hit testing cannot disagree about where a region is.

mod column;
mod input_block;
mod inspector;
mod registration;

use ratatui::layout::{Constraint, Layout, Rect};

use input_block::{InputBlock, split_input};
pub use inspector::{InspectorRequest, SteerSplit, steer_split};

use crate::surface::SurfaceTree;

/// Rows a bordered region needs before it can say anything: two borders and one line.
///
/// A region below this is worse than absent. It is still a focus stop and still a pointer target,
/// so the user can reach a surface that shows nothing and has no way to tell why.
const MIN_PANEL_HEIGHT: u16 = 3;

/// Rows the conversation keeps before an optional region may take its preferred height.
const TRANSCRIPT_COMFORT: u16 = 8;

/// Preferred heights of the regions that yield on a short terminal.
const RAIL_HEIGHT: u16 = 5;
/// One row of text and the bottom edge of the box it closes (INS-5).
const COLLAPSED_COMPOSER_HEIGHT: u16 = 2;
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
    /// Agent column plus conversation.
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecisionMode {
    Inline,
    Modal,
}

/// What the workspace needs from the projection in order to lay itself out.
///
/// A value rather than a borrow of `ViewState`, so layout stays testable without constructing a
/// projection, and a struct rather than a growing parameter list because the shelf adds to it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceInput {
    /// Desired footer rows; layout protects the composer and readable conversation first.
    pub status_rows: u16,
    /// Whether the notice strip has anything to report.
    pub has_notices: bool,
    /// How many background requests are queued. Zero registers no band at all.
    pub attention: usize,
    /// Rows the decision region asks for, divider included. Zero registers no region at all.
    pub decision_rows: u16,
    /// Rows the waiting-input band asks for, divider included. Zero registers no region at all.
    pub queue_rows: u16,
    /// Rows below which the band would rather not be registered than be registered too short.
    pub queue_floor: u16,
    /// Primary approvals are inline inputs; a user-opened background request can be modal.
    pub decision_mode: DecisionMode,
    /// A user-opened command inspection overlays the approval without deciding it.
    pub command_inspection: bool,
    /// Rows the Drawer asks for, borders included. Zero registers no region at all.
    pub drawer_rows: u16,
    /// Whether the Drawer's open view is typed into or navigated, which decides its kind.
    pub drawer_focus: crate::KeyboardFocus,
    /// Whether the conversation tree covers the workspace above Status.
    pub conversation_tree: bool,
    /// Rows for the composer-anchored skill completion popup, including borders and footer.
    pub composer_menu_rows: u16,
    /// Whether there is a roster to show. With no sub-agents the rail is not registered at all.
    pub rail: bool,
    /// Rows the composer asks for, borders included. Grows as the draft gains lines.
    pub composer_rows: u16,
    /// The open inspector, if one is open.
    pub inspector: Option<InspectorRequest>,
}

impl Default for WorkspaceInput {
    fn default() -> Self {
        Self {
            status_rows: 1,
            has_notices: false,
            attention: 0,
            decision_rows: 0,
            queue_rows: 0,
            queue_floor: 0,
            decision_mode: DecisionMode::Inline,
            command_inspection: false,
            drawer_rows: 0,
            drawer_focus: crate::KeyboardFocus::TextInput,
            conversation_tree: false,
            composer_menu_rows: 0,
            rail: false,
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
/// Rows are handed out in the order the journey cannot do without them: the status line, then the
/// composer, then the notice strip, then the Attention band, and the workspace body takes what is
/// left. At the supported minimum that order is what decides which region disappears. The strips
/// sit at the top and the composer inside the conversation's box at the bottom, so what arrives
/// takes rows from what has been read rather than from what is being read or typed.
#[must_use]
pub fn workspace(area: Rect, input: WorkspaceInput) -> SurfaceTree {
    if LayoutClass::for_size(area.width, area.height) == LayoutClass::TooSmall {
        return SurfaceTree::default();
    }

    // One system row is guaranteed; additional script rows yield to the composer and readable
    // conversation. Global hints always replace the final row of this band (STL-4, INV-7).
    let status_height = input.status_rows.max(1).min(
        area.height
            .saturating_sub(input.composer_rows)
            .saturating_sub(TRANSCRIPT_COMFORT)
            .max(1),
    );
    let status = Rect::new(
        area.x,
        area.bottom().saturating_sub(status_height),
        area.width,
        status_height,
    );
    let budget = area.height.saturating_sub(status.height);

    // Typing is the one thing a workspace this small still has to allow, so the composer is served
    // before the notice strip and before the body, and only clamped to keep the conversation. It
    // lives inside the conversation's box (`ui-ux.md` §input), so its rows come out of that
    // column rather than off the bottom of the terminal; collapsed, it is still one framed line
    // (INS-5).
    // Collapsed is one row plus the box's bottom edge; anything else is lines plus a divider and
    // that edge.
    let wanted = if input.composer_rows <= 1 {
        COLLAPSED_COMPOSER_HEIGHT
    } else {
        input.composer_rows.max(MIN_PANEL_HEIGHT)
    };
    let composer_height = wanted
        .min(budget.saturating_sub(MIN_PANEL_HEIGHT))
        .max(COLLAPSED_COMPOSER_HEIGHT);
    // The decision region is a second section of the same box, above the composer and below the
    // conversation. It bids after the composer and never displaces it: answering a tool call and
    // typing the next instruction are two different inputs, and taking the second to show the
    // first is what made the box read as if the composer had been eaten.
    let decision_height = input
        .decision_rows
        .min(budget.saturating_sub(composer_height.saturating_add(MIN_PANEL_HEIGHT)));
    // The waiting-input band is the third section of the same box, above the decision. It bids
    // last of the three because it is the only one that is not an input: an approval is answered
    // and a draft is typed, while this reports what the user already said. It takes its rows from
    // the conversation rather than from the top of the screen because the user's own `Enter` is
    // what puts it there, and it belongs beside the composer they pressed it in.
    let queue_height = granted(
        input.queue_rows,
        input.queue_floor,
        budget.saturating_sub(
            composer_height
                .saturating_add(decision_height)
                .saturating_add(TRANSCRIPT_COMFORT),
        ),
    );
    let input_height = composer_height
        .saturating_add(decision_height)
        .saturating_add(queue_height);
    let mut rest = budget.saturating_sub(input_height);

    let notice_height = if input.has_notices {
        notice_rows(rest)
    } else {
        0
    };
    rest = rest.saturating_sub(notice_height);

    // Served after the notice strip for the same reason the strip outranks the agent rail: a
    // silently wrong projection has no other signal, while a blocked agent also shows as `Waiting`
    // in the rail and its request survives until the rows come back.
    let attention_height = attention_rows(input.attention, rest);
    rest = rest.saturating_sub(attention_height);

    // The strips take their rows from the top of the screen, never from the bottom: the newest
    // conversation rows and the composer stay where they are when a notice or a request arrives,
    // so nothing the user is reading or typing into moves (ATT-1).
    let notices = band(area, area.y, notice_height);
    let attention_top = area.y.saturating_add(notice_height);
    let attention = band(area, attention_top, attention_height);
    let body = Rect::new(
        area.x,
        attention_top.saturating_add(attention_height),
        area.width,
        rest.saturating_add(input_height),
    );

    let mut regions = body_regions(
        area,
        body,
        input.inspector,
        InputBlock {
            composer: composer_height,
            decision: decision_height,
            queue: queue_height,
            queue_floor: input.queue_floor,
        },
        input.rail,
    );
    // Over the body rather than carved from it: the Drawer belongs to the workspace, blocks
    // everything below it (SURF-4), and is gone again on `Escape`, so nothing beneath it should
    // have moved while it was open.
    let above_status = Rect::new(area.x, area.y, area.width, status.y.saturating_sub(area.y));
    regions.drawer = drawer_region(above_status, input.drawer_rows);
    regions.command_inspection = input.command_inspection.then(|| {
        let width = above_status.width.saturating_sub(4).min(110);
        let height = above_status
            .height
            .saturating_sub(4)
            .max(3)
            .min(above_status.height);
        Rect::new(
            above_status.x + (above_status.width - width) / 2,
            above_status.y + (above_status.height - height) / 2,
            width,
            height,
        )
    });

    registration::surface_tree(area, status, notices, attention, regions, &input)
}

/// The most lines the primary composer may take at this terminal height: a third of it, never
/// fewer than a sub-agent's input keeps. Past the cap the draft's window follows its caret
/// (ui-ux §input).
#[must_use]
pub fn composer_cap(height: u16) -> u16 {
    (height / 3).max(crate::state::MAX_VISIBLE_LINES)
}

/// Width the primary composer will occupy for this frame.
///
/// Height allocation cannot answer this for the caller: at ultrawide an open second window splits
/// the conversation column after the composer has reserved its rows. Reusing `body_regions` keeps
/// the width used to wrap the draft identical to the rectangle later registered for painting.
pub(super) fn composer_width(area: Rect, inspector: Option<InspectorRequest>) -> u16 {
    // `rail: true` is the narrower of the two answers and the one the composer has to survive: a
    // draft wrapped for the wider column would reflow the moment a sub-agent appeared.
    body_regions(
        area,
        area,
        inspector,
        InputBlock {
            queue: 0,
            queue_floor: 0,
            decision: 0,
            composer: MIN_PANEL_HEIGHT,
        },
        true,
    )
    .composer
    .width
}

/// Rows for the notice strip, which yields to the workspace rather than the other way round.
///
/// It outranks the agent rail and loses to the conversation. A projection that is silently wrong
/// is the failure the notice log exists to prevent and the user has no other way to detect it,
/// whereas a
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
    pub(super) agents: Option<Rect>,
    pub(super) transcript: Option<Rect>,
    pub(super) inspector: Option<Rect>,
    /// Whether the second window floats over the conversation rather than tiling beside it.
    pub(super) inspector_floats: bool,
    /// The composer, at the bottom of the conversation's column. Always present: the body was
    /// sized so that typing survives every other region.
    pub(super) composer: Rect,
    /// The decision region, directly above the composer, while a tool call is waiting on an answer.
    pub(super) decision: Option<Rect>,
    /// The waiting-input band, above the decision region, while any input has yet to be sent.
    pub(super) queue: Option<Rect>,
    /// The Drawer, docked to the top edge over the body while it is open.
    pub(super) drawer: Option<Rect>,
    pub(super) command_inspection: Option<Rect>,
}

/// The Drawer's one geometry (DRW-2): the top edge, the full width, and the rows its content asks
/// for, clamped to what lies above the status line. No layout-class switch, no margin, and a top
/// edge that stays put while the content height changes.
fn drawer_region(above_status: Rect, rows: u16) -> Option<Rect> {
    if rows == 0 {
        return None;
    }
    Some(Rect {
        height: rows.min(above_status.height),
        ..above_status
    })
}

fn body_regions(
    area: Rect,
    body: Rect,
    inspector: Option<InspectorRequest>,
    block: InputBlock,
    rail: bool,
) -> BodyRegions {
    // All three sections are carved from the conversation's column as one block, so the second
    // window's give-back and the shelf's guarantee are measured against the rows the conversation
    // actually keeps. The block is divided once every region has been placed.
    let input_height = block.total();
    let class = LayoutClass::for_size(area.width, area.height);
    // A roster of nobody is a bordered box saying so, in the column the conversation wanted. The
    // rail earns its rectangle by having something in it; until then the conversation is the
    // screen, which is what INS-1 says looking at the primary means.
    let mut base = if !rail {
        BodyRegions {
            agents: None,
            transcript: Some(body),
            inspector: None,
            inspector_floats: false,
            composer: Rect::default(),
            decision: None,
            queue: None,
            drawer: None,
            command_inspection: None,
        }
    } else {
        match class {
            LayoutClass::Ultrawide | LayoutClass::Wide => column::beside_conversation(body, 28),
            LayoutClass::Medium => {
                let [agents, transcript] =
                    Layout::horizontal([Constraint::Length(26), Constraint::Min(24)]).areas(body);
                BodyRegions {
                    agents: Some(agents),
                    transcript: Some(transcript),
                    inspector: None,
                    inspector_floats: false,
                    composer: Rect::default(),
                    decision: None,
                    queue: None,
                    drawer: None,
                    command_inspection: None,
                }
            }
            // `TooSmall` returned before layout began, so it cannot reach here.
            LayoutClass::Narrow | LayoutClass::TooSmall => {
                // The input rows are spoken for, so the rail bids against what is left.
                let mut rows = body.height.saturating_sub(input_height);
                let rail_height = reserve(&mut rows, RAIL_HEIGHT, MIN_PANEL_HEIGHT);
                let rows = rows.saturating_add(input_height);
                let transcript =
                    Rect::new(body.x, body.y.saturating_add(rail_height), body.width, rows);
                BodyRegions {
                    agents: band(body, body.y, rail_height),
                    transcript: Some(transcript),
                    inspector: None,
                    inspector_floats: false,
                    composer: Rect::default(),
                    decision: None,
                    queue: None,
                    drawer: None,
                    command_inspection: None,
                }
            }
        }
    };

    // The composer takes the bottom of the conversation's column before the second window is
    // placed, so a shelf's guarantee and a maximized window are both measured against what the
    // conversation actually has.
    let column = base
        .transcript
        .expect("every layout class starts with a conversation column");
    let input_height = input_height.min(column.height.saturating_sub(MIN_PANEL_HEIGHT));
    base.composer = Rect::new(
        column.x,
        column.bottom().saturating_sub(input_height),
        column.width,
        input_height,
    );
    base.transcript = Some(Rect {
        height: column.height.saturating_sub(input_height),
        ..column
    });

    let mut placed = match inspector {
        Some(request) => inspector::place_inspector(base, request, class),
        None => base,
    };
    split_input(&mut placed, block);
    placed
}

/// Rows a band that cannot scroll may have: what fits in `room`, or none if that is below `floor`.
///
/// Chrome has no scrollback, so a band handed fewer rows than it can say anything in does not
/// degrade — it goes on titling a queue whose messages and whose way back have both fallen off the
/// bottom. Below its floor the honest answer is the rows back, and the band absent.
pub(super) const fn granted(want: u16, floor: u16, room: u16) -> u16 {
    let height = if want < room { want } else { room };
    if height < floor { 0 } else { height }
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

/// Top-border controls share exact geometry with pointer hit testing.
pub(crate) fn command_inspection_controls(bounds: Rect) -> [Rect; 2] {
    let right = bounds.right().saturating_sub(1);
    [
        Rect::new(right.saturating_sub(6), bounds.y, 3, 1),
        Rect::new(right.saturating_sub(3), bounds.y, 3, 1),
    ]
}

/// TRE-6: the right-aligned `×` badge and its pointer hit region share one cell rectangle.
pub(crate) fn conversation_tree_close_control(bounds: Rect) -> Rect {
    if bounds.width < 5 || bounds.height == 0 {
        return Rect::default();
    }
    Rect::new(bounds.right().saturating_sub(4), bounds.y, 3, 1)
}

/// DRW-3: one centered bottom-border row owns both the handle and its padded hit region.
pub(crate) fn drawer_retract_control(bounds: Rect) -> Rect {
    const WIDTH: u16 = 8;
    if bounds.width < WIDTH + 2 || bounds.height == 0 {
        return Rect::default();
    }
    Rect::new(
        bounds.x + (bounds.width - WIDTH) / 2,
        bounds.bottom() - 1,
        WIDTH,
        1,
    )
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::{
        BodyRegions, DecisionMode, InspectorRequest, LayoutClass, MIN_PANEL_HEIGHT, WorkspaceInput,
        workspace,
    };
    use crate::surface::SurfaceId;

    /// The default composer, which is the shape every one of these sizes is checked against.
    /// A rail in every fixture: the geometry under test is the crowded one, and a workspace with
    /// no sub-agents simply has one region fewer to place.
    fn input(has_notices: bool) -> WorkspaceInput {
        WorkspaceInput {
            rail: true,
            has_notices,
            ..WorkspaceInput::default()
        }
    }

    /// SKP-4: a clipped menu never hides both the selected choice and its keys.
    #[test]
    fn skill_picker_is_absent_when_fewer_than_choice_footer_and_borders_fit() {
        let regions = BodyRegions {
            agents: None,
            transcript: Some(Rect::new(0, 0, 60, 2)),
            inspector: None,
            inspector_floats: false,
            composer: Rect::new(0, 4, 60, 3),
            decision: Some(Rect::new(0, 2, 60, 2)),
            queue: None,
            drawer: None,
            command_inspection: None,
        };
        let tree = super::registration::surface_tree(
            Rect::new(0, 0, 60, 10),
            Rect::new(0, 8, 60, 1),
            None,
            None,
            regions,
            &WorkspaceInput {
                decision_mode: DecisionMode::Inline,
                composer_menu_rows: 8,
                drawer_focus: crate::KeyboardFocus::TextInput,
                ..WorkspaceInput::default()
            },
        );
        assert!(tree.get(SurfaceId::ComposerMenu).is_none());
    }

    /// The same, with an inspector open in its default presentation.
    fn inspecting(has_notices: bool) -> WorkspaceInput {
        WorkspaceInput {
            rail: true,
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

    /// DRW-2: the Drawer spans the width at every class, keeps its top edge, and never reaches the
    /// status line.
    #[test]
    fn the_drawer_spans_the_width_and_keeps_its_top_edge() {
        for (width, height) in [
            (48, 12),
            (60, 40),
            (71, 40),
            (72, 40),
            (95, 40),
            (120, 40),
            (160, 40),
        ] {
            for rows in [5, 11, 16, 60] {
                let tree = workspace(
                    Rect::new(7, 11, width, height),
                    WorkspaceInput {
                        drawer_rows: rows,
                        attention: 1,
                        ..WorkspaceInput::default()
                    },
                );
                let status = tree.get(SurfaceId::Status).expect("status").bounds;
                let bounds = tree.get(SurfaceId::Drawer).expect("open drawer").bounds;
                assert_eq!((bounds.x, bounds.y, bounds.width), (7, 11, width));
                assert!(bounds.bottom() <= status.y);
                assert_eq!(bounds.height, rows.min(status.y - 11));
            }
        }
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
            // The base layer tiles; a surface above it lies inside exactly one base surface.
            let registered: Vec<_> = tree
                .iter()
                .filter(|surface| surface.z_index == 0)
                .map(|surface| surface.bounds)
                .collect();
            for above in tree.iter().filter(|surface| surface.z_index > 0) {
                let holders = registered
                    .iter()
                    .filter(|base| base.union(above.bounds) == **base)
                    .count();
                assert_eq!(
                    holders, 1,
                    "{width}x{height} {input:?}: {:?} floats over {holders} base surfaces",
                    above.id
                );
            }

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
                // The status line is one unbordered row by design; every bordered region needs
                // two borders and a line before it is worth drawing.
                let floor = if surface.id == SurfaceId::Status {
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
    fn only_the_status_line_is_beyond_the_pointer() {
        let tree = workspace(Rect::new(0, 0, 120, 24), input(true));
        let pointer_eligible = |id| {
            tree.get(id)
                .is_some_and(|surface| surface.kind.accepts_pointer())
        };

        for id in [
            SurfaceId::Agents,
            SurfaceId::Transcript,
            SurfaceId::Notices,
            SurfaceId::Composer,
        ] {
            assert!(pointer_eligible(id), "{id:?} must be reachable");
        }
        assert!(
            !pointer_eligible(SurfaceId::Status),
            "the status line says things; there is nothing in it to point at"
        );
    }

    /// SURF-3: the ring may lose stops on a short terminal, but it never reorders.
    ///
    /// Membership varies because a region with no room to draw is not registered at all. Order is
    /// the part the user builds muscle memory on, so that is the part held fixed.
    #[test]
    fn the_focus_ring_loses_stops_without_ever_reordering() {
        const CANONICAL: [SurfaceId; 6] = [
            SurfaceId::Agents,
            SurfaceId::Transcript,
            SurfaceId::Inspector,
            SurfaceId::Composer,
            SurfaceId::Notices,
            SurfaceId::Attention,
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

    /// Stage 3 retires the second agent-column surface; the rail keeps the full column.
    #[test]
    fn the_agent_column_is_one_surface_at_every_width() {
        for (width, height) in [(48, 12), (60, 30), (86, 40), (95, 40)] {
            let tree = workspace(Rect::new(0, 0, width, height), input(false));
            assert!(tree.get(SurfaceId::Transcript).is_some());
        }
        let wide = workspace(Rect::new(0, 0, 96, 30), input(false));
        let agents = wide
            .get(SurfaceId::Agents)
            .unwrap_or_else(|| panic!("wide keeps the agent rail"));
        let composer = wide
            .get(SurfaceId::Composer)
            .unwrap_or_else(|| panic!("wide keeps the composer"));
        assert_eq!(
            agents.bounds.bottom(),
            composer.bounds.bottom(),
            "the rail owns the entire body beside the conversation and composer"
        );
    }

    /// The rail earns its rectangle by having a roster, at every class.
    ///
    /// A workspace that has delegated nothing showed a bordered box reading `No sub-agents yet.` in
    /// the column the conversation wanted, on every screen and for the whole life of a session that
    /// may never delegate. Not registered rather than drawn empty: an empty panel is still a focus
    /// stop, a pointer target and a `Tab` the user has to press through.
    #[test]
    fn the_rail_is_registered_only_when_there_is_a_roster() {
        for (width, height) in SIZES {
            let empty = workspace(
                Rect::new(0, 0, width, height),
                WorkspaceInput {
                    rail: false,
                    ..WorkspaceInput::default()
                },
            );
            assert!(
                empty.get(SurfaceId::Agents).is_none(),
                "{width}x{height}: a roster of nobody registers no rail"
            );
            let conversation = empty
                .get(SurfaceId::Transcript)
                .unwrap_or_else(|| panic!("{width}x{height}: the conversation is registered"));
            let peopled = workspace(Rect::new(0, 0, width, height), input(false));
            let narrower = peopled
                .get(SurfaceId::Transcript)
                .unwrap_or_else(|| panic!("{width}x{height}: the conversation is registered"));
            assert!(
                conversation.bounds.width >= narrower.bounds.width
                    && conversation.bounds.height >= narrower.bounds.height,
                "{width}x{height}: the rows and columns the rail did not take go to the conversation"
            );
        }
    }

    /// What survives at the smallest supported terminal, in priority order.
    ///
    /// Twelve rows cannot hold a status line, a composer, a conversation, a notice strip and an
    /// agent rail at once, so this pins which of them goes. Typing and the conversation are the
    /// workspace. A producer defect the user cannot see is the failure the notice log exists to
    /// prevent, and
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

    /// IQU-2: the band bids last of the three sections and never takes a row from either input.
    ///
    /// Shrink `queue_rows`' clamp so it bids before the decision region and this fails at the
    /// crowded sizes: the approval loses rows to a report of input that is merely waiting.
    #[test]
    fn the_waiting_band_yields_to_both_inputs_and_to_a_readable_conversation() {
        for (width, height) in SIZES {
            let area = Rect::new(0, 0, width, height);
            let quiet = workspace(
                area,
                WorkspaceInput {
                    decision_rows: 6,
                    ..input(false)
                },
            );
            let crowded = workspace(
                area,
                WorkspaceInput {
                    decision_rows: 6,
                    queue_rows: 5,
                    // Paired the way the projection pairs them, so a band that fits nothing is
                    // absent here as it is on screen rather than present at whatever was spare.
                    queue_floor: 5,
                    ..input(false)
                },
            );
            let at = |tree: &crate::surface::SurfaceTree, id| tree.get(id).map(|s| s.bounds);
            assert_eq!(
                at(&crowded, SurfaceId::Composer),
                at(&quiet, SurfaceId::Composer),
                "{width}x{height}: the composer keeps its rows"
            );
            assert_eq!(
                at(&crowded, SurfaceId::Approval),
                at(&quiet, SurfaceId::Approval),
                "{width}x{height}: the decision region keeps its rows"
            );
            let Some(band) = at(&crowded, SurfaceId::QueuedInput) else {
                continue;
            };
            let conversation = at(&crowded, SurfaceId::Transcript)
                .unwrap_or_else(|| panic!("{width}x{height}: conversation"));
            assert_eq!(
                conversation.bottom(),
                band.y,
                "{width}x{height}: the band's rows come out of the conversation"
            );
            assert!(
                conversation.height >= MIN_PANEL_HEIGHT,
                "{width}x{height}: a band never leaves an unreadable conversation"
            );
        }
    }
}
