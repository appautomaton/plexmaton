//! The conversation's activity line: its mark, the logo's core in one cell moving on the motion
//! clock (MOT-1), then what the agent is doing and the readings that say for how long.

use plexmaton_core::ReasoningEffort;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::chrome::{attention_pill, selected_suffix};
use crate::{
    ViewState,
    state::CurrentWork,
    surface::SurfaceId,
    theme::{Palette, Role},
};

/// Outline, then filled, each through circle, rounded square, square and back. One icon family
/// from the Nerd Font the product already requires, so every frame centres alike. Rejected:
/// Unicode geometric shapes of mixed sizes, which the user watched shake in their terminal.
pub(crate) const MARK: [&str; 8] = [
    "\u{F0766}", // circle-outline
    "\u{F14FC}", // square-rounded-outline
    "\u{F0764}", // square-outline
    "\u{F14FC}",
    "\u{F0765}", // circle
    "\u{F14FB}", // square-rounded
    "\u{F0763}", // square
    "\u{F14FB}",
];

/// Three of the clock's fifteen phases a second per frame: 200 ms, so a cycle takes 1.6 s, the
/// effort rail's own period.
const PHASES_PER_FRAME: u16 = 3;

/// The frame for a clock phase.
pub(crate) fn mark(phase: u16) -> &'static str {
    MARK[usize::from(phase / PHASES_PER_FRAME) % MARK.len()]
}

/// The conversation's last row: what the agent is doing on the left; what the reader has
/// selected and what is still waiting on them on the right (ui-ux §input, COM-5, SEL-5, ATT-1).
///
/// Derived from the same facts the composer's divider used to carry, and drawn where the
/// conversation ends rather than where the user types, so the two never read as one thing.
pub(crate) fn activity_line(
    state: &ViewState,
    palette: &Palette,
    width: u16,
    approval_visible: bool,
) -> Line<'static> {
    // The label keeps the role it always had; the mark carries whose work it is: the user's own
    // blue, the provider's colour while a hosted search runs, and action required standing still.
    let work = match state.current_work() {
        None => None,
        Some(CurrentWork::Thinking) => {
            Some(("Thinking…".to_owned(), Role::Ambient, Role::SurfacePrimary))
        }
        Some(CurrentWork::Responding) => Some((
            "Responding…".to_owned(),
            Role::Ambient,
            Role::SurfacePrimary,
        )),
        Some(CurrentWork::RunningTool(tool)) => Some((
            format!("Running {tool}…"),
            Role::Ambient,
            Role::SurfacePrimary,
        )),
        Some(CurrentWork::RunningServerTool(tool)) => {
            Some((format!("Running {tool}…"), Role::Ambient, Role::ServerTool))
        }
        Some(CurrentWork::ApprovalRequired) if approval_visible => None,
        Some(CurrentWork::ApprovalRequired) => Some((
            "Approval required".to_owned(),
            Role::ActionRequired,
            Role::ActionRequired,
        )),
        Some(CurrentWork::Compacting) => Some((
            "Compacting…".to_owned(),
            Role::Ambient,
            Role::SurfacePrimary,
        )),
    };
    let mut right = Vec::new();
    let selected = selected_suffix(state, SurfaceId::Transcript);
    if let Some(note) = selected.strip_prefix(" · ") {
        right.push(Span::styled(note.to_owned(), palette.style(Role::Muted)));
    }
    if let Some(pill) = attention_pill(state, palette) {
        right.extend(pill.spans);
    }
    let right_width = Line::from(right.clone()).width();
    let mut left = Vec::new();
    if let Some((text, role, mark_role)) = work {
        let moving = state.activity_moves();
        let mark = if moving {
            mark(state.motion_phase())
        } else {
            MARK[0]
        };
        left.push(Span::styled(mark, palette.style(mark_role)));
        left.push(Span::raw(" "));
        left.push(Span::styled(text, palette.style(role)));
        if moving {
            let readings = activity_readings(state);
            let label = Line::from(left.clone()).width();
            // Dropped in this order when the row is short: effort, then quiet, then elapsed.
            let fits = |kept: &[&(usize, String)]| {
                label
                    + kept.iter().map(|(_, text)| 3 + text.width()).sum::<usize>()
                    + 1
                    + right_width
                    <= usize::from(width)
            };
            let mut kept: Vec<&(usize, String)> = readings.iter().collect();
            for drop in [1, 2, 0] {
                if fits(&kept) {
                    break;
                }
                kept.retain(|(slot, _)| *slot != drop);
            }
            for (_, reading) in kept {
                left.push(Span::styled(
                    format!(" · {reading}"),
                    palette.style(Role::Muted),
                ));
            }
        }
    }
    let used = Line::from(left.clone()).width() + right_width;
    let gap = usize::from(width).saturating_sub(used);
    let mut spans = left;
    if !right.is_empty() {
        spans.push(Span::raw(" ".repeat(gap)));
        spans.extend(right);
    }
    Line::from(spans)
}

/// The muted readings after the label, in display order and tagged with the slot that orders
/// their dropping: elapsed (0), effort (1), quiet (2). Each is left out until it is known.
fn activity_readings(state: &ViewState) -> Vec<(usize, String)> {
    let mut readings = Vec::new();
    if let Some(elapsed) = state.activity_elapsed() {
        readings.push((0, crate::state::elapsed_label(elapsed)));
    }
    if let Some(effort) = state
        .reasoning_effort()
        .filter(|effort| !matches!(effort, ReasoningEffort::Default | ReasoningEffort::None))
    {
        readings.push((1, format!("{} effort", effort.as_str())));
    }
    if let Some(quiet) = state.activity_quiet() {
        readings.push((
            2,
            format!("quiet for {}", crate::state::elapsed_label(quiet)),
        ));
    }
    readings
}

#[cfg(test)]
mod tests {
    use unicode_width::UnicodeWidthStr;

    use super::{MARK, PHASES_PER_FRAME, mark};

    /// MOT-2: every frame is one cell, so the mark can never widen the row.
    #[test]
    fn mot_2_every_activity_mark_frame_is_one_cell() {
        for frame in MARK {
            assert_eq!(frame.width(), 1, "{frame:?}");
        }
    }

    /// MOT-1: the cycle divides the clock's, so the mark never jumps where the clock wraps.
    #[test]
    fn the_mark_cycle_divides_the_clock_cycle() {
        let frames = u16::try_from(MARK.len()).expect("small");
        assert_eq!(480 % (frames * PHASES_PER_FRAME), 0);
        assert_eq!(mark(0), mark(480 - 480 % (frames * PHASES_PER_FRAME)));
        assert_ne!(mark(0), mark(PHASES_PER_FRAME));
    }
}
