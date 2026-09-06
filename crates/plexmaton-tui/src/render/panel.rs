//! What a bordered region is, and how one is drawn, measured, and given the cursor.
//!
//! Separated from the draw loop because the loop's question is *which surface is drawn as what*,
//! and this file's is *what drawing one costs*: how many rows the frame takes, where a stored
//! scroll position lands at this depth, and where the caret goes. The two change for different
//! reasons — a new surface identity touches the loop and nothing here.

use plexmaton_core::AgentId;
use ratatui::{
    Frame,
    layout::Rect,
    text::Line,
    widgets::{Padding, Paragraph, Wrap},
};

use super::chrome::{Badged as _, block, block_with, title};
use crate::{
    ViewState,
    state::{Caret, ScrollPosition, inner_width},
    surface::{ContentInsets, Viewport},
    theme::{Palette, Role},
};

const FULL_FRAME_ROWS: u16 = 2;

/// One bordered, scrollable region, ready to draw.
pub(super) struct Panel {
    pub(super) body: Body,
    pub(super) title: Line<'static>,
    /// A short status painted at the far end of the same border row, or nothing.
    pub(super) badge: Option<Line<'static>>,
    /// Which sides carry a frame, and so how many rows the content cannot have.
    pub(super) edges: Edges,
    /// How those sides are inked.
    pub(super) chrome: Chrome,
    pub(super) insets: ContentInsets,
    /// One row of the region's own, painted last inside it: the conversation's activity line.
    pub(super) footer: Option<Line<'static>>,
}

/// How a region's edges are painted. The geometry is the edges'; this is only ink.
///
/// The primary conversation and its composer are drawn without a box (ui-ux §input): the
/// conversation bare, the composer between two rules. The cells a box would have spent stay
/// reserved, so the caret, the pointer and every viewport keep the geometry they had.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Chrome {
    /// Glyph borders on every edge the region has.
    Box,
    /// A rule across each horizontal edge the region has; its side columns are blank.
    Rules,
    /// No ink: every edge the region has is a blank row or column.
    Bare,
}

impl Chrome {
    /// The cells the edges spend that no glyph occupies, reserved as padding.
    pub(super) fn hidden(self, edges: Edges) -> Padding {
        match self {
            Self::Box => Padding::ZERO,
            Self::Rules => Padding::new(1, 1, 0, 0),
            Self::Bare => Padding::new(
                1,
                1,
                u16::from(edges.has_top()),
                u16::from(edges.has_bottom()),
            ),
        }
    }
}

/// Which sides of a region carry a frame.
///
/// Two surfaces stacked in one box (ui-ux §layout classes) share one outline: the upper one has no
/// bottom edge and
/// the lower one's top edge is drawn as a divider joined to the sides, so together they read as one
/// box with two sections rather than as two boxes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Edges {
    /// A box of its own.
    All,
    /// Top and sides, open at the bottom: the upper section of a shared box.
    Upper,
    /// A divider on top, then sides and bottom: the lower section of a shared box.
    Lower,
    /// A divider on top and sides, open at the bottom: a section with one above and one below it.
    Middle,
    /// Sides and bottom only: the last line of a shared box, with no divider above it. The
    /// collapsed composer is this — one row reading where typing would go, not a box (INS-5).
    Closing,
}

impl Edges {
    pub(super) const fn has_top(self) -> bool {
        matches!(self, Self::All | Self::Upper | Self::Lower | Self::Middle)
    }

    pub(super) const fn has_bottom(self) -> bool {
        matches!(self, Self::All | Self::Lower | Self::Closing)
    }

    /// Rows the frame spends, which the content cannot have.
    pub(super) const fn rows(self) -> u16 {
        match self {
            Self::Upper | Self::Closing | Self::Middle => 1,
            Self::All | Self::Lower => FULL_FRAME_ROWS,
        }
    }

    /// Columns the frame spends, which the content cannot have.
    pub(super) const fn columns(self) -> u16 {
        match self {
            Self::All | Self::Upper | Self::Lower | Self::Closing | Self::Middle => 2,
        }
    }
}

/// What a panel has to draw, and how much of it the frame had to build.
///
/// `follows_tail` belongs to the whole-body arm alone: a windowed body arrives with its offset
/// already resolved through the reader's anchor, so a second answer here could only disagree.
pub(super) enum Body {
    /// Content short enough that building all of it costs nothing, measured as one paragraph.
    Whole {
        lines: Vec<Line<'static>>,
        /// Whether an untouched viewport opens at the end of its content rather than the start.
        follows_tail: bool,
    },
    /// A conversation, measured entry by entry and built only where the viewport reaches (TR-2).
    Window {
        lines: Vec<Line<'static>>,
        /// Rows to skip inside the first built entry.
        skip_rows: u16,
        viewport: Viewport,
    },
}

impl Body {
    pub(super) fn lines(&self) -> &[Line<'static>] {
        match self {
            Self::Whole { lines, .. } | Self::Window { lines, .. } => lines,
        }
    }
}

/// Draws the inspector's input and puts the workspace's one cursor in it (INS-5, COM-1).
pub(super) fn render_steer(
    frame: &mut Frame<'_>,
    palette: &Palette,
    state: &ViewState,
    agent_id: &AgentId,
    area: Rect,
) {
    let label = state
        .agent(agent_id)
        .map_or_else(|| agent_id.to_string(), |agent| agent.label.clone());
    let lines = crate::content::input_lines(
        state.draft(agent_id),
        palette,
        inner_width(area.width),
        crate::state::input_window(area),
    );
    let paragraph = Paragraph::new(lines.clone())
        .wrap(Wrap { trim: false })
        .block(block(
            palette,
            title(
                palette,
                format!("Message {label}"),
                Role::SectionHeading,
                "",
            ),
            true,
            Edges::All,
        ));
    frame.render_widget(paragraph, area);
    place_cursor(
        frame,
        area,
        state
            .draft(agent_id)
            .caret(inner_width(area.width), crate::state::input_window(area)),
        Edges::All,
    );
}

/// Draws a panel through its viewport and returns what it measured.
///
/// A whole body is measured by the same `Paragraph` that paints it, so the wrap deciding how tall
/// the content is and the wrap putting it on screen are one computation. A windowed body arrives
/// already measured, and scrolls by the rows into its first entry rather than by rows into a history
/// it never built.
pub(super) fn draw_panel(
    frame: &mut Frame<'_>,
    palette: &Palette,
    area: Rect,
    focused: bool,
    panel: &Panel,
    parked: Option<ScrollPosition>,
) -> Viewport {
    let hidden = panel.chrome.hidden(panel.edges);
    let chrome = block_with(
        palette,
        panel.title.clone(),
        focused,
        panel.edges,
        panel.chrome,
    )
    .badge(panel.badge.clone())
    .padding(Padding::new(
        panel.insets.sides.saturating_add(hidden.left),
        panel.insets.sides.saturating_add(hidden.right),
        panel.insets.vertical.saturating_add(hidden.top),
        panel.insets.vertical.saturating_add(hidden.bottom),
    ));
    let mut inside = chrome.inner(area);
    // The footer is the region's own last row, not content: it neither scrolls nor counts.
    let footer = match &panel.footer {
        Some(line) if inside.height > 0 => {
            inside.height = inside.height.saturating_sub(1);
            Some((
                Rect::new(inside.x, inside.bottom(), inside.width, 1),
                line.clone(),
            ))
        }
        _ => None,
    };
    let mut lines = panel.body.lines().to_vec();
    if let Body::Window { viewport, .. } = &panel.body {
        let slack = usize::from(viewport.visible_rows).saturating_sub(viewport.content_rows);
        lines.splice(0..0, std::iter::repeat_n(Line::default(), slack));
    }
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(chrome, area);

    let (viewport, scroll) = match &panel.body {
        Body::Whole { follows_tail, .. } => {
            // `line_count` wraps at exactly the width it is given and then adds the block's border
            // rows, so it is asked for the inner width and those rows are taken back off.
            let inner_width = inside.width;
            let measured = paragraph.line_count(inner_width);
            let mut viewport = Viewport {
                content_rows: measured,
                content_width: inner_width,
                visible_rows: inside.height,
                offset: 0,
            };
            viewport.offset = resolve_offset(parked, *follows_tail, viewport.max_offset());
            (viewport, u16::try_from(viewport.offset).unwrap_or(u16::MAX))
        }
        Body::Window {
            skip_rows,
            viewport,
            ..
        } => (*viewport, *skip_rows),
    };

    frame.render_widget(paragraph.scroll((scroll, 0)), inside);
    if let Some((row, line)) = footer {
        frame.render_widget(Paragraph::new(line), row);
    }
    viewport
}

/// Turns a stored scroll position into a row offset for a viewport this deep.
///
/// An untouched surface has no stored position at all, and takes its anchor from its own kind of
/// content: a conversation opens at its newest line, a list at its first (TR-4).
const fn resolve_offset(
    parked: Option<ScrollPosition>,
    follows_tail: bool,
    max_offset: usize,
) -> usize {
    match parked {
        Some(position) => position.offset(max_offset),
        None if follows_tail => max_offset,
        None => 0,
    }
}

/// Places the workspace's one cursor where its input says the caret is.
///
/// The only `set_cursor_position` call site in the workspace. Ratatui hides the cursor unless a
/// frame asks for it, so "exactly one cursor" (COM-1) is a property of there being one caller.
///
/// The caret arrives already resolved to a row and a display column by the same wrap that produced
/// the rows being painted. Rejected: measuring the painted lines here, which could only ever put
/// the caret after the last one and is why the draft had no insertion point to move.
pub(super) fn place_cursor(frame: &mut Frame<'_>, area: Rect, caret: Caret, edges: Edges) {
    let inside_width = area.width.saturating_sub(edges.columns());
    let inside_height = area.height.saturating_sub(edges.rows());
    frame.set_cursor_position((
        area.x
            .saturating_add(1)
            .saturating_add(caret.column.min(inside_width)),
        area.y
            .saturating_add(1)
            .saturating_add(caret.row.min(inside_height.saturating_sub(1))),
    ));
}
