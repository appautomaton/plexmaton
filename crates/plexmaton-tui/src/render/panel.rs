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

use super::{BORDER_ROWS, chrome::block};
use crate::{
    ViewState,
    state::{ScrollPosition, inner_width},
    surface::{SurfaceId, Viewport},
    theme::{Palette, Role},
};

/// Rows a bordered region needs before it is worth drawing content into: two borders and one line.
const MIN_CONVERSATION_ROWS: u16 = 3;

/// One bordered, scrollable region, ready to draw.
pub(super) struct Panel {
    pub(super) body: Body,
    pub(super) title: String,
    pub(super) title_role: Role,
    /// Whether the region spends two rows on its own frame. A single-row region cannot.
    pub(super) bordered: bool,
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
    /// A conversation, measured item by item and built only where the viewport reaches (TR-2).
    Window {
        lines: Vec<Line<'static>>,
        /// Rows to skip inside the first built item.
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

/// Takes the inspector's input strip out of the inspector's own rectangle.
///
/// Returns what is left for the conversation, and where the input goes. The strip comes off the
/// bottom because that is where the main composer is, and a workspace whose two inputs sit in
/// different places is one the user has to look for.
///
/// A rectangle with no room for both keeps the conversation and shows no input. That is the same
/// all-or-nothing rule the row budget uses: an input squeezed to nothing is a place the cursor
/// claims to be and is not.
pub(super) fn steer_split(
    state: &ViewState,
    id: SurfaceId,
    bounds: Rect,
    has_focus: bool,
) -> (Rect, Option<(Rect, AgentId)>) {
    if id != SurfaceId::Inspector || !has_focus {
        return (bounds, None);
    }
    let Some(agent_id) = state.agent_shown_by(id) else {
        return (bounds, None);
    };
    let wanted = state
        .draft(&agent_id)
        .requested_rows(inner_width(bounds.width));
    let room = bounds.height.saturating_sub(MIN_CONVERSATION_ROWS);
    let rows = wanted.min(room);
    if rows < MIN_CONVERSATION_ROWS {
        return (bounds, None);
    }
    let above = Rect {
        height: bounds.height.saturating_sub(rows),
        ..bounds
    };
    let strip = Rect {
        y: above.bottom(),
        height: rows,
        ..bounds
    };
    (above, Some((strip, agent_id)))
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
            format!(" Steer {label} "),
            Role::Accent,
            true,
        ));
    frame.render_widget(paragraph, area);
    place_cursor(frame, area, &lines);
}

/// Draws a panel through its viewport and returns what it measured.
///
/// A whole body is measured by the same `Paragraph` that paints it, so the wrap deciding how tall
/// the content is and the wrap putting it on screen are one computation. A windowed body arrives
/// already measured, and scrolls by the rows into its first item rather than by rows into a history
/// it never built.
pub(super) fn draw_panel(
    frame: &mut Frame<'_>,
    palette: &Palette,
    area: Rect,
    focused: bool,
    panel: &Panel,
    parked: Option<ScrollPosition>,
) -> Viewport {
    // An unbordered region spends no rows on a frame, so none of the arithmetic below may take
    // them off. A collapsed composer is the only one, and it is one row tall (D-027).
    let frame_rows = if panel.bordered { BORDER_ROWS } else { 0 };
    let mut paragraph = Paragraph::new(panel.body.lines().to_vec()).wrap(Wrap { trim: false });
    if panel.bordered {
        paragraph = paragraph.block(block(
            palette,
            panel.title.clone(),
            panel.title_role,
            focused,
        ));
    }

    let (viewport, scroll) = match &panel.body {
        Body::Whole { follows_tail, .. } => {
            // `line_count` wraps at exactly the width it is given and then adds the block's border
            // rows, so it is asked for the inner width and those rows are taken back off.
            let inner_width = area.width.saturating_sub(frame_rows);
            let measured = u16::try_from(paragraph.line_count(inner_width)).unwrap_or(u16::MAX);
            let mut viewport = Viewport {
                content_rows: measured.saturating_sub(frame_rows),
                visible_rows: area.height.saturating_sub(frame_rows),
                offset: 0,
            };
            viewport.offset = resolve_offset(parked, *follows_tail, viewport.max_offset());
            (viewport, viewport.offset)
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
    max_offset: u16,
) -> u16 {
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
pub(super) fn place_cursor(frame: &mut Frame<'_>, area: Rect, lines: &[Line<'_>]) {
    let last = lines.last();
    // Display width, not character count: a wide glyph occupies two cells and the caret has to
    // land after both.
    let column = last.map_or(0, |line| {
        u16::try_from(UnicodeWidthStr::width(line.to_string().as_str())).unwrap_or(u16::MAX)
    });
    let rows = u16::try_from(lines.len()).unwrap_or(1).max(1);
    let inside_width = area.width.saturating_sub(BORDER_ROWS);
    let inside_height = area.height.saturating_sub(BORDER_ROWS);
    frame.set_cursor_position((
        area.x
            .saturating_add(1)
            .saturating_add(column.min(inside_width)),
        area.y.saturating_add(rows.min(inside_height)),
    ));
}
