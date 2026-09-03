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
    widgets::{Paragraph, Wrap},
};
use unicode_width::UnicodeWidthStr;

use super::chrome::{block, title};
use crate::{
    ViewState,
    state::{ScrollPosition, inner_width},
    surface::Viewport,
    theme::{Palette, Role},
};

const FULL_FRAME_ROWS: u16 = 2;

/// One bordered, scrollable region, ready to draw.
pub(super) struct Panel {
    pub(super) body: Body,
    pub(super) title: Line<'static>,
    /// Which sides carry a frame, and so how many rows the content cannot have.
    pub(super) edges: Edges,
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
    /// Sides and bottom only: the last line of a shared box, with no divider above it. The
    /// collapsed composer is this — one row reading where typing would go, not a box (INS-5).
    Closing,
}

impl Edges {
    /// Rows the frame spends, which the content cannot have.
    pub(super) const fn rows(self) -> u16 {
        match self {
            Self::Upper | Self::Closing => 1,
            Self::All | Self::Lower => FULL_FRAME_ROWS,
        }
    }

    /// Columns the frame spends, which the content cannot have.
    pub(super) const fn columns(self) -> u16 {
        match self {
            Self::All | Self::Upper | Self::Lower | Self::Closing => 2,
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
    let lines: Vec<Line<'static>> = state
        .draft(agent_id)
        .visible_rows(inner_width(area.width))
        .into_iter()
        .map(|row| Line::styled(row, palette.style(Role::Body)))
        .collect();
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
    place_cursor(frame, area, &lines, Edges::All);
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
    let frame_rows = panel.edges.rows();
    let frame_columns = panel.edges.columns();
    let mut lines = panel.body.lines().to_vec();
    // A conversation shorter than its panel sits at the bottom, the way one that overflows does:
    // the newest content is always at the bottom, so what a window floating over the top covers
    // is empty rows or rows already read, never what the user is reading (`ui-ux.md` §shelf).
    if let Body::Window { viewport, .. } = &panel.body {
        let slack = usize::from(viewport.visible_rows).saturating_sub(viewport.content_rows);
        lines.splice(0..0, std::iter::repeat_n(Line::default(), slack));
    }
    let mut paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    paragraph = paragraph.block(block(palette, panel.title.clone(), focused, panel.edges));

    let (viewport, scroll) = match &panel.body {
        Body::Whole { follows_tail, .. } => {
            // `line_count` wraps at exactly the width it is given and then adds the block's border
            // rows, so it is asked for the inner width and those rows are taken back off.
            let inner_width = area.width.saturating_sub(frame_columns);
            let measured = paragraph.line_count(inner_width);
            let mut viewport = Viewport {
                content_rows: measured.saturating_sub(usize::from(frame_rows)),
                content_width: inner_width,
                visible_rows: area.height.saturating_sub(frame_rows),
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

    frame.render_widget(paragraph.scroll((scroll, 0)), area);
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

/// Places the workspace's one cursor at the end of the composer's last visible line.
///
/// The only `set_cursor_position` call site in the workspace. Ratatui hides the cursor unless a
/// frame asks for it, so "exactly one cursor" (COM-1) is a property of there being one caller.
pub(super) fn place_cursor(frame: &mut Frame<'_>, area: Rect, lines: &[Line<'_>], edges: Edges) {
    let last = lines.last();
    // Display width, not character count: a wide glyph occupies two cells and the caret has to
    // land after both.
    let column = last.map_or(0, |line| {
        u16::try_from(UnicodeWidthStr::width(line.to_string().as_str())).unwrap_or(u16::MAX)
    });
    let rows = u16::try_from(lines.len()).unwrap_or(1).max(1);
    let inside_width = area.width.saturating_sub(edges.columns());
    let inside_height = area.height.saturating_sub(edges.rows());
    frame.set_cursor_position((
        area.x
            .saturating_add(1)
            .saturating_add(column.min(inside_width)),
        area.y.saturating_add(rows.min(inside_height)),
    ));
}
