//! The effort spectrum and its hit geometry share one bounded cell layout.

use crate::{EffortPalette, Palette, Point, Role, ViewState};
use plexmaton_core::ReasoningEffort;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
};

const MARKERS: [&str; 4] = ["▲", "■", "⬢", "●"];

pub(crate) fn marker(phase: u16) -> &'static str {
    MARKERS[usize::from((phase / 24) % 4)]
}

fn stop(width: u16, index: usize) -> u16 {
    let left = if width >= 70 { 5 } else { 2 };
    let right = width.saturating_sub(left + 2);
    left + right.saturating_sub(left) * index as u16 / 5
}

fn tick_row(height: u16) -> u16 {
    if height >= 9 { 2 } else { 0 }
}

fn put(buffer: &mut Buffer, x: u16, y: u16, text: &str, style: Style) {
    if y < buffer.area.height && x < buffer.area.width {
        buffer.set_stringn(x, y, text, usize::from(buffer.area.width - x), style);
    }
}

pub(crate) fn lines(
    state: &ViewState,
    palette: &Palette,
    width: u16,
    height: u16,
) -> Vec<Line<'static>> {
    let rows = height.saturating_sub(1);
    let mut buffer = Buffer::empty(Rect::new(0, 0, width, rows));
    let colors = EffortPalette::default();
    let selected = state.selected_effort();
    let track = tick_row(height);
    let muted = palette.style(Role::Muted);
    let body = palette.style(Role::Body);
    if width >= 18 && rows >= 3 {
        let left = stop(width, 0);
        let right = stop(width, 5);
        if track > 0 {
            put(&mut buffer, left, 0, "Faster", muted);
            put(
                &mut buffer,
                right.saturating_sub(11),
                0,
                "More thought",
                muted,
            );
        }
        for x in left..=right {
            put(&mut buffer, x, track, "─", palette.style(Role::Border));
        }
        for (index, level) in ReasoningEffort::EXPLICIT.into_iter().enumerate() {
            let x = stop(width, index);
            let available = state.effort_available(level);
            let chosen = selected == Some(level);
            let phase = if chosen { state.effort_phase() } else { 0 };
            let glyph = if chosen && level == ReasoningEffort::Max {
                marker(phase)
            } else if chosen {
                "▲"
            } else {
                "╷"
            };
            put(
                &mut buffer,
                x,
                track,
                glyph,
                Style::new().fg(colors.color(level, available, phase, 0)),
            );
            let name = if width < 44 {
                ["n", "l", "m", "h", "x", "max"][index]
            } else {
                level.as_str()
            };
            let start = x.saturating_sub(name.len() as u16 / 2);
            for (i, ch) in name.chars().enumerate() {
                let mut style = Style::new().fg(colors.color(level, available, phase, i as u16));
                if chosen {
                    style = style.add_modifier(Modifier::BOLD);
                }
                put(
                    &mut buffer,
                    start + i as u16,
                    track + 1,
                    &ch.to_string(),
                    style,
                );
            }
        }
    }
    let detail = if rows >= 7 { track + 3 } else { 2 };
    if detail < rows.saturating_sub(1) {
        let caption = state
            .composer_menu()
            .effort_feedback
            .clone()
            .unwrap_or_else(|| match selected {
                Some(level) => format!("{} · this conversation", level.as_str()),
                None => "No matching allowed effort. Check model configuration.".to_owned(),
            });
        put(&mut buffer, 1, detail, &caption, body);
        if detail + 1 < rows.saturating_sub(1) {
            let description = match selected {
                Some(ReasoningEffort::None) => "Answer without a reasoning phase.",
                Some(ReasoningEffort::Low) => "A light pass for straightforward tasks.",
                Some(ReasoningEffort::Medium) => "A balanced pass for everyday work.",
                Some(ReasoningEffort::High) => "More room for difficult problems.",
                Some(ReasoningEffort::Xhigh) => "A deeper pass for demanding tasks.",
                Some(ReasoningEffort::Max) => {
                    "For the hardest tasks; may take longer and use more tokens."
                }
                Some(ReasoningEffort::Default) => "Use the provider's default effort.",
                None => "Unavailable levels cannot be selected.",
            };
            put(&mut buffer, 1, detail + 1, description, muted);
        }
    }
    let keys = if width >= 40 {
        " ←/→ adjust · Enter confirm · Esc cancel"
    } else {
        " ←→ · Enter set · Esc cancel"
    };
    put(&mut buffer, 0, rows.saturating_sub(1), keys, muted);
    (0..rows)
        .map(|y| {
            Line::from(
                (0..width)
                    .map(|x| {
                        let cell = &buffer[(x, y)];
                        Span::styled(cell.symbol().to_owned(), cell.style())
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}

pub(crate) fn hit(state: &ViewState, bounds: Rect, at: Point) -> Option<ReasoningEffort> {
    let width = bounds.width.saturating_sub(2);
    let row = bounds.y + 1 + tick_row(bounds.height);
    if width < 18
        || bounds.height < 4
        || at.y < row
        || at.y > row + 1
        || at.x <= bounds.x
        || at.x >= bounds.right().saturating_sub(1)
    {
        return None;
    }
    let x = at.x - bounds.x - 1;
    ReasoningEffort::EXPLICIT
        .into_iter()
        .enumerate()
        .min_by_key(|(index, _)| stop(width, *index).abs_diff(x))
        .map(|(_, effort)| effort)
        .filter(|effort| state.effort_available(*effort))
}

/// Recolor only the existing rule cells, keeping titles, focus weight and geometry intact.
pub(super) fn composer_rules(
    frame: &mut ratatui::Frame<'_>,
    state: &ViewState,
    bounds: Rect,
    edges: super::panel::Edges,
) {
    let Some(effort) = state.reasoning_effort() else {
        return;
    };
    let palette = EffortPalette::default();
    for y in [
        edges.has_top().then_some(bounds.y),
        edges
            .has_bottom()
            .then(|| bounds.bottom().saturating_sub(1)),
    ]
    .into_iter()
    .flatten()
    {
        for x in bounds.x..bounds.right() {
            let cell = &mut frame.buffer_mut()[(x, y)];
            if cell.symbol() == "─" {
                cell.set_fg(palette.rule_color(effort, x - bounds.x, bounds.width));
            }
        }
    }
}

pub(super) fn effort_spans(effort: ReasoningEffort, phase: u16) -> Vec<Span<'static>> {
    let palette = EffortPalette::default();
    effort
        .as_str()
        .chars()
        .enumerate()
        .map(|(i, ch)| {
            Span::styled(
                ch.to_string(),
                Style::new().fg(palette.color(effort, true, phase, i as u16)),
            )
        })
        .collect()
}

/// A collapsed, covered, retry-editing or clipped composer has no visible max label to animate.
pub(crate) fn composer_max_visible(state: &ViewState, surfaces: &crate::SurfaceTree) -> bool {
    if state.reasoning_effort() != Some(ReasoningEffort::Max)
        || state.drawer().is_some()
        || state.editing_retry()
        || state.steer_input(surfaces).is_some()
    {
        return false;
    }
    surfaces
        .get(crate::SurfaceId::Composer)
        .is_some_and(|surface| {
            surface.bounds.height >= 3
                && super::chrome::composer_title(state, &Palette::default()).width() + 2
                    <= usize::from(surface.bounds.width)
        })
}
