//! The agents strip: the roster, above the user's own conversation.
//!
//! Rows, not columns. The rail this replaced spent twenty-eight columns of every row on a list that
//! inked four of them, and cut every ask it carried to a third of a sentence; the strip spends a
//! bounded number of rows at the full width the conversation has, and an ask is readable in it.
//!
//! Carved before the second window is placed, so the strip is part of what `place_inspector`
//! divides: at ultrawide the primary keeps half the region and the strip halves with it, and a
//! maximized delegate leaves no primary for the strip to sit above (ui-ux §agents strip).

use ratatui::layout::Rect;

use super::{BodyRegions, MIN_PANEL_HEIGHT, TRANSCRIPT_COMFORT, granted};

/// Takes the strip's rows off the top of the conversation column.
///
/// The conversation is served first: the strip is an index of work happening elsewhere, and a
/// terminal too short for both keeps the thing the user is reading. Below its own floor the strip
/// is absent rather than a bordered box with no room for a line, and the roster is still one
/// `Ctrl-B` away.
pub(super) fn carve(regions: &mut BodyRegions, rows: u16) {
    let Some(conversation) = regions.transcript else {
        return;
    };
    let height = granted(
        rows,
        MIN_PANEL_HEIGHT,
        conversation.height.saturating_sub(TRANSCRIPT_COMFORT),
    );
    if height == 0 {
        return;
    }
    regions.agents = Some(Rect {
        height,
        ..conversation
    });
    regions.transcript = Some(Rect {
        y: conversation.y.saturating_add(height),
        height: conversation.height.saturating_sub(height),
        ..conversation
    });
}

/// Rows a strip holding `agents` asks for, borders included. Zero registers no strip at all.
///
/// The count is the projection's, and the cap is the roster's: this is the one place the two meet,
/// so the rows layout reserves and the rows the panel paints cannot drift apart.
#[must_use]
pub fn rows(agents: usize) -> u16 {
    if agents == 0 {
        return 0;
    }
    let lines = u16::try_from(agents.min(crate::content::STRIP_AGENTS)).unwrap_or(u16::MAX);
    lines.saturating_add(2)
}
