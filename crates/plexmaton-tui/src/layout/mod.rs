//! Terminal geometry to named, registered surfaces.
//!
//! Layout is the only place that computes a workspace rectangle, and every rectangle it computes
//! is registered here. The renderer then draws from the registry rather than recomputing, so
//! painting and hit testing cannot disagree about where a region is.

mod input_block;
mod inspector;
mod registration;
mod strip;

use ratatui::layout::Rect;

use input_block::{InputBlock, split_input};
pub use inspector::{InspectorRequest, SteerSplit, steer_split};
pub use strip::rows as strip_rows;

use crate::surface::SurfaceTree;

/// Rows a bordered region needs before it can say anything: two borders and one line.
///
/// A region below this is worse than absent. It is still a focus stop and still a pointer target,
/// so the user can reach a surface that shows nothing and has no way to tell why.
const MIN_PANEL_HEIGHT: u16 = 3;

/// Rows the conversation keeps before an optional region may take its preferred height.
const TRANSCRIPT_COMFORT: u16 = 8;

/// Preferred heights of the regions that yield on a short terminal.
/// One row of text and the bottom edge of the box it closes (INS-5).
const COLLAPSED_COMPOSER_HEIGHT: u16 = 2;
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
    /// One major region; the full-screen agent navigator or one conversation owns it.
    Narrow,
    /// One conversation, with the agents strip above it.
    Medium,
    /// One conversation, with the agents strip above it; a second agent arrives as a shelf.
    Wide,
    /// Two conversations side by side; a second agent earns a column of its own.
    Ultrawide,
}

impl LayoutClass {
    /// Chooses the composition for a terminal size in cells.
    ///
    /// The two-column width is what a second conversation needs and nothing else. It was 132 while
    /// a twenty-eight-column agent rail came out of the terminal first, leaving the two columns 52
    /// each; the roster is a strip of rows now, so the same two columns of 52 start 28 columns
    /// earlier. The threshold is the old one minus the rail it no longer has to pay for, which is
    /// why it is 104 and not a rounder number: the number the user sees is the column width, and
    /// that one did not change. Rejected: keeping 132, which went on charging the second
    /// conversation for a column nothing occupies.
    #[must_use]
    pub const fn for_size(width: u16, height: u16) -> Self {
        if width < MIN_WIDTH || height < MIN_HEIGHT {
            Self::TooSmall
        } else if width >= 104 {
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
    /// Rows the agents strip asks for, borders included. Zero registers no strip at all, which is
    /// what no sub-agents means: the conversation is the screen (INS-1).
    pub roster_rows: u16,
    /// Whether the user has the roster open. Closed, it returns every row it held.
    pub roster: bool,
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
            decision_rows: 0,
            queue_rows: 0,
            queue_floor: 0,
            decision_mode: DecisionMode::Inline,
            command_inspection: false,
            drawer_rows: 0,
            drawer_focus: crate::KeyboardFocus::TextInput,
            conversation_tree: false,
            composer_menu_rows: 0,
            roster_rows: 0,
            roster: true,
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

    // The strip takes its rows from the top of the screen, never from the bottom: the newest
    // conversation rows and the composer stay where they are when a notice arrives, so nothing the
    // user is reading or typing into moves.
    let notices = band(area, area.y, notice_height);
    let body = Rect::new(
        area.x,
        area.y.saturating_add(notice_height),
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
        if input.roster { input.roster_rows } else { 0 },
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

    registration::surface_tree(area, status, notices, regions, &input)
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
///
/// The roster no longer enters into it. While it was a column beside the conversation, a draft
/// wrapped without it reflowed the moment a sub-agent appeared, and this function had to assume
/// the narrower answer; a strip takes rows, so the composer's width is the same either way.
pub(super) fn composer_width(area: Rect, inspector: Option<InspectorRequest>) -> u16 {
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
        0,
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
    /// Whether the roster is the narrow full-region navigator above the retained conversations.
    pub(super) agents_floats: bool,
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
    roster_rows: u16,
) -> BodyRegions {
    // All three sections are carved from the conversation's column as one block, so the second
    // window's give-back and the shelf's guarantee are measured against the rows the conversation
    // actually keeps. The block is divided once every region has been placed.
    let input_height = block.total();
    let class = LayoutClass::for_size(area.width, area.height);
    // The body is the conversation's, at every width. What the roster needs it takes in rows from
    // the top of that conversation, never in columns from beside it: a list of four agents inked
    // four percent of the twenty-eight-column rail it used to hold, and rented the other ninety-six
    // percent from the one surface the user is reading. With no sub-agents it takes nothing, which
    // is what INS-1 means by the conversation being the screen.
    let mut base = BodyRegions {
        agents: None,
        transcript: Some(body),
        inspector: None,
        inspector_floats: false,
        agents_floats: false,
        composer: Rect::default(),
        decision: None,
        queue: None,
        drawer: None,
        command_inspection: None,
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
    // Before the second window, so the strip is one of the things that window divides: it belongs
    // to the user's own conversation and stops at that conversation's edge. Narrow has no strip —
    // one major surface at a time, and the roster arrives there as the full-region navigator.
    if !matches!(class, LayoutClass::Narrow) {
        strip::carve(&mut base, roster_rows);
    }

    let mut placed = match inspector {
        Some(request) => inspector::place_inspector(base, request, class),
        None => base,
    };
    split_input(&mut placed, block);
    if roster_rows > 0 && matches!(class, LayoutClass::Narrow) {
        placed.agents = Some(body);
        placed.agents_floats = true;
    }
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
    use crate::surface::{SurfaceId, SurfaceKind};

    /// The default composer, which is the shape every one of these sizes is checked against.
    /// A rail in every fixture: the geometry under test is the crowded one, and a workspace with
    /// no sub-agents simply has one region fewer to place.
    fn input(has_notices: bool) -> WorkspaceInput {
        WorkspaceInput {
            roster_rows: crate::layout::strip_rows(1),
            has_notices,
            ..WorkspaceInput::default()
        }
    }

    /// ui-ux §agents strip: the same panel is a strip above the conversation from Medium upward
    /// and the exclusive full-region navigator on Narrow. Closing reveals the untouched
    /// conversation, with the rows back rather than columns.
    #[test]
    fn the_roster_is_a_strip_above_the_conversation_or_the_narrow_full_region_navigator() {
        for (width, height) in [(140, 40), (120, 40), (88, 40), (71, 40), (60, 40), (48, 40)] {
            let area = Rect::new(0, 0, width, height);
            let open = workspace(area, input(false));
            let closed = workspace(
                area,
                WorkspaceInput {
                    roster: false,
                    ..input(false)
                },
            );
            let panel = open
                .get(SurfaceId::Agents)
                .unwrap_or_else(|| panic!("{width}x{height}: an open roster is registered"));
            assert!(
                closed.get(SurfaceId::Agents).is_none(),
                "{width}x{height}: a closed roster is not registered at all"
            );
            let before = open
                .get(SurfaceId::Transcript)
                .unwrap_or_else(|| panic!("{width}x{height}: conversation"))
                .bounds;
            let after = closed
                .get(SurfaceId::Transcript)
                .unwrap_or_else(|| panic!("{width}x{height}: conversation"))
                .bounds;
            if LayoutClass::for_size(width, height) == LayoutClass::Narrow {
                assert!(
                    panel.z_index > 0,
                    "{width}x{height}: the navigator replaces the major region"
                );
                assert_eq!(panel.kind, SurfaceKind::Modal);
                assert_eq!(
                    before, after,
                    "{width}x{height}: closing reveals unchanged conversation geometry"
                );
                let status = open
                    .get(SurfaceId::Status)
                    .unwrap_or_else(|| panic!("{width}x{height}: status"));
                assert_eq!(
                    panel.bounds,
                    Rect::new(0, 0, width, status.bounds.y),
                    "{width}x{height}: Agents owns the full body above status"
                );
            } else {
                assert_eq!(
                    panel.z_index, 0,
                    "{width}x{height}: a strip is part of the base layer"
                );
                assert_eq!(
                    panel.bounds.width, before.width,
                    "{width}x{height}: the strip is exactly as wide as the conversation under it"
                );
                assert_eq!(
                    panel.bounds.bottom(),
                    before.y,
                    "{width}x{height}: and sits directly on top of it"
                );
                assert_eq!(
                    after.width, before.width,
                    "{width}x{height}: closing costs the conversation no column, because the \
                     strip never took one"
                );
                assert!(
                    after.height > before.height,
                    "{width}x{height}: closing hands the rows back to the conversation"
                );
            }
        }
    }

    /// SURF-4: the narrow navigator covers conversation-owned layers while workspace overlays
    /// remain able to cover it.
    #[test]
    fn the_narrow_agents_navigator_has_the_workspace_layer_boundary() {
        let tree = workspace(
            Rect::new(0, 0, 60, 40),
            WorkspaceInput {
                roster_rows: crate::layout::strip_rows(1),
                roster: true,
                composer_menu_rows: 5,
                decision_rows: 8,
                command_inspection: true,
                conversation_tree: true,
                drawer_rows: 10,
                ..WorkspaceInput::default()
            },
        );
        let agents = tree.get(SurfaceId::Agents).expect("full-region navigator");
        for id in [
            SurfaceId::ComposerMenu,
            SurfaceId::Approval,
            SurfaceId::CommandInspection,
        ] {
            let surface = tree
                .get(id)
                .unwrap_or_else(|| panic!("{id:?} fixture layer"));
            assert!(
                surface.z_index < agents.z_index,
                "{id:?} must paint under the navigator"
            );
        }
        for id in [SurfaceId::ConversationTree, SurfaceId::Drawer] {
            let surface = tree
                .get(id)
                .unwrap_or_else(|| panic!("{id:?} fixture layer"));
            assert!(
                surface.z_index > agents.z_index,
                "{id:?} is a workspace overlay above the navigator"
            );
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
            agents_floats: false,
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
            roster_rows: crate::layout::strip_rows(1),
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
                    .map(move |base| (width, height, base))
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
        assert_eq!(LayoutClass::for_size(104, 40), LayoutClass::Ultrawide);
        assert_eq!(LayoutClass::for_size(103, 40), LayoutClass::Wide);
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
                if above.id == SurfaceId::Agents {
                    let covered = (above.bounds.y..above.bounds.bottom()).all(|y| {
                        (above.bounds.x..above.bounds.right()).all(|x| {
                            registered.iter().any(|base| {
                                x >= base.x && x < base.right() && y >= base.y && y < base.bottom()
                            })
                        })
                    });
                    assert!(
                        covered,
                        "{width}x{height} {input:?}: the full-region navigator must cover only retained base cells"
                    );
                    continue;
                }
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
        const CANONICAL: [SurfaceId; 5] = [
            SurfaceId::Agents,
            SurfaceId::Transcript,
            SurfaceId::Inspector,
            SurfaceId::Composer,
            SurfaceId::Notices,
        ];

        for (width, height, input) in shapes() {
            {
                let tree = workspace(Rect::new(0, 0, width, height), input);
                let ring: Vec<_> = tree.focus_ring().collect();
                let context = format!("{width}x{height} {input:?}");

                if tree
                    .get(SurfaceId::Agents)
                    .is_some_and(|surface| surface.kind.blocks_below())
                {
                    assert_eq!(
                        ring,
                        vec![SurfaceId::Agents],
                        "{context}: the full-region navigator owns focus"
                    );
                    continue;
                }

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

    /// The roster takes rows and never a column, so the conversation runs the full width at every
    /// size. This is the whole difference between the strip and the rail it replaced.
    #[test]
    fn the_strip_costs_the_conversation_rows_and_never_a_column() {
        for (width, height) in [(48, 12), (60, 30), (86, 40), (95, 40), (140, 40)] {
            let area = Rect::new(0, 0, width, height);
            let tree = workspace(area, input(false));
            let conversation = tree
                .get(SurfaceId::Transcript)
                .unwrap_or_else(|| panic!("{width}x{height}: conversation"));
            assert_eq!(
                conversation.bounds.width, width,
                "{width}x{height}: nothing stands beside the conversation"
            );
            let composer = tree
                .get(SurfaceId::Composer)
                .unwrap_or_else(|| panic!("{width}x{height}: composer"));
            assert_eq!(
                composer.bounds.width, width,
                "{width}x{height}: nor beside the composer"
            );
        }
        // The strip is short and stops where the conversation begins; the rail ran the whole body.
        let wide = workspace(Rect::new(0, 0, 96, 30), input(false));
        let agents = wide
            .get(SurfaceId::Agents)
            .unwrap_or_else(|| panic!("wide keeps the roster"));
        let composer = wide
            .get(SurfaceId::Composer)
            .unwrap_or_else(|| panic!("wide keeps the composer"));
        assert_eq!(
            agents.bounds.height,
            crate::layout::strip_rows(1),
            "one agent, one row, plus the box around it"
        );
        assert!(
            agents.bounds.bottom() < composer.bounds.y,
            "the strip is above the conversation, not beside it"
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
                    roster_rows: 0,
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
    /// Twelve rows cannot hold a status line, a composer, a readable conversation, a notice strip
    /// and the roster at once, so this pins which of them goes. Typing and the conversation are the
    /// workspace. A producer defect the user cannot see is the failure the notice log exists to
    /// prevent, and nothing else signals it. The roster is what yields: it is an index the user can
    /// bring back with one chord, and at this height it has nowhere to dock that does not take the
    /// conversation below its ten-row guarantee (INS-2).
    ///
    /// This replaced an assertion that the rail always survives a notice. That was true before the
    /// composer took three of these twelve rows, and the composer is not the thing to give up.
    #[test]
    fn the_smallest_terminal_keeps_typing_the_conversation_and_the_defect_notice() {
        let quiet = workspace(
            Rect::new(0, 0, 48, 12),
            WorkspaceInput {
                roster: false,
                ..input(false)
            },
        );
        assert!(quiet.get(SurfaceId::Composer).is_some());
        assert!(quiet.get(SurfaceId::Transcript).is_some());
        assert!(
            quiet.get(SurfaceId::Agents).is_none(),
            "twelve rows cannot hold both, and the conversation is not the thing to give up"
        );

        let degraded = workspace(
            Rect::new(0, 0, 48, 12),
            WorkspaceInput {
                roster: false,
                ..input(true)
            },
        );
        assert!(degraded.get(SurfaceId::Composer).is_some());
        assert!(degraded.get(SurfaceId::Transcript).is_some());
        assert!(
            degraded.get(SurfaceId::Notices).is_some(),
            "a silently wrong projection is the failure with no other signal"
        );
        assert!(
            degraded.get(SurfaceId::Agents).is_none(),
            "the roster yields here too, and it returns as soon as the rows do"
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
