//! The conversation's input block: what it is made of, and how its rows are divided (IQU-2).
//!
//! One block for every purpose but painting — one column, one guarantee, one give-back to the
//! second window — so the sections are named here and divided here, after every region is placed.

use ratatui::layout::Rect;

use super::{BodyRegions, COLLAPSED_COMPOSER_HEIGHT};

/// The three sections carved from the bottom of the conversation's column, top to bottom.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct InputBlock {
    pub(super) queue: u16,
    pub(super) decision: u16,
    pub(super) composer: u16,
}

impl InputBlock {
    /// Rows the whole block takes from the conversation before it is divided.
    pub(super) const fn total(self) -> u16 {
        self.queue
            .saturating_add(self.decision)
            .saturating_add(self.composer)
    }
}

/// Divides the conversation's input block into its sections.
///
/// Last, after every region has been placed, because the sections share one rectangle for every
/// purpose but painting: one column, one guarantee, one give-back to the second window.
///
/// Bidding order and painting order are different questions. Typing survives first, then
/// answering, then the report of what is waiting — but the waiting band is painted above the
/// decision it outlives, because it describes the composer's own input.
pub(super) fn split_input(regions: &mut BodyRegions, block: InputBlock) {
    let whole = regions.composer;
    let mut spare = whole.height.saturating_sub(COLLAPSED_COMPOSER_HEIGHT);
    let mut claim = |want: u16| -> u16 {
        let height = want.min(spare);
        spare = spare.saturating_sub(height);
        height
    };
    let decision = claim(block.decision);
    let queue = claim(block.queue);
    let mut top = whole.y;
    let mut place = |height: u16| -> Option<Rect> {
        if height == 0 {
            return None;
        }
        let placed = Rect {
            y: top,
            height,
            ..whole
        };
        top = top.saturating_add(height);
        Some(placed)
    };
    regions.queue = place(queue);
    regions.decision = place(decision);
    regions.composer = Rect {
        y: top,
        height: whole.height.saturating_sub(top.saturating_sub(whole.y)),
        ..whole
    };
}
