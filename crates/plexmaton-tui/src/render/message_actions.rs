//! Message actions use viewport coordinates; source text never positions chrome.

use crate::{
    Palette, TranscriptMetrics, ViewState,
    surface::{SurfaceId, Viewport},
    theme::Role,
};
use ratatui::{Frame, layout::Rect};

pub(super) fn render(
    frame: &mut Frame<'_>,
    state: &ViewState,
    palette: &Palette,
    metrics: &TranscriptMetrics,
    surface: SurfaceId,
    bounds: Rect,
    viewport: Viewport,
) {
    let Some(target) = state.hovered_message(surface) else {
        return;
    };
    let Some(rows) = metrics.message_rows(&target.agent, viewport.content_width, target.index)
    else {
        return;
    };
    let slack = usize::from(viewport.visible_rows).saturating_sub(viewport.content_rows);
    let first_row = rows.start as i128 - viewport.offset as i128 + slack as i128;
    if first_row >= 0
        && first_row < i128::from(viewport.visible_rows)
        && viewport.content_width >= 4
    {
        let y = bounds.y + 1 + first_row as u16;
        let right = bounds.x + 1 + viewport.content_width;
        let appearance = state.entry_appearance(surface, &target.agent, &target.item, false);
        let style = palette.style(if appearance.copy_hovered {
            Role::Accent
        } else {
            Role::Muted
        });
        let buffer = frame.buffer_mut();
        for x in right - 4..right {
            buffer[(x, y)].set_symbol(" ").set_style(style);
        }
        buffer[(right - 3, y)].set_symbol("󰆏");
    }
    for row in [rows.start as i128 - 1, rows.end as i128 - 1] {
        let local = row - viewport.offset as i128 + slack as i128;
        if local < 0 || local >= i128::from(viewport.visible_rows) {
            continue;
        }
        let y = bounds.y + 1 + local as u16;
        let left = bounds.x + 1;
        let right = left + viewport.content_width;
        let buffer = frame.buffer_mut();
        // Adjacent compact tool rows and clipped message tops may have no separator available.
        if !(left..right).all(|x| buffer[(x, y)].symbol().chars().all(char::is_whitespace)) {
            continue;
        }
        for x in left..right {
            buffer[(x, y)]
                .set_symbol("─")
                .set_style(palette.style(Role::Border));
        }
    }
}
