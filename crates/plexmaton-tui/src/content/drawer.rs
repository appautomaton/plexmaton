//! The Drawer's rows: the page list, and how to work it.
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::{chosen_row, command_summary, input_spans};
use crate::{
    ViewState,
    theme::{Palette, Role},
};

/// The Drawer's body: the filter, the rows of what it shows, and how to work them.
///
/// The chosen row carries `Chosen` across its whole width, a bar with weight and a hue, against
/// `Muted` rows: it is the one thing on this surface that `Enter` will act on, and it has to be
/// found without reading. `Selection` is deliberately not used — that role means content selected
/// for copying.
///
/// The keys are a muted last row rather than a badge on the title: they are a sentence, and the
/// badge is where a short status goes.
pub(crate) fn drawer(
    state: &ViewState,
    palette: &Palette,
    width: u16,
    height: u16,
) -> Vec<Line<'static>> {
    let Some(drawer) = state.drawer() else {
        return Vec::new();
    };
    if let Some(panel) = drawer.permissions() {
        return crate::content_permissions::content(panel, palette, width, height).lines;
    }
    let mut filter = Vec::new();
    if drawer.filter().text().is_empty() {
        filter.push(Span::styled("Type to filter", palette.style(Role::Muted)));
    } else {
        filter.extend(input_spans(
            drawer.filter(),
            palette,
            drawer.filter_range(width),
        ));
    }
    let gap = drawer.choice_gap(height);
    let mut lines = vec![Line::from(filter)];
    lines.extend((0..gap).map(|_| Line::default()));
    let pages = drawer.pages();
    if pages.is_empty() {
        lines.push(Line::styled(
            "No page matches".to_owned(),
            palette.style(Role::Muted),
        ));
    }
    // Names in one column, so the summaries line up whatever page is listed.
    let name_width = crate::Page::ALL
        .iter()
        .map(|page| page.name().width())
        .max()
        .unwrap_or(0);
    let window = drawer.choice_window(height);
    for (index, page) in pages
        .into_iter()
        .enumerate()
        .skip(window.start)
        .take(window.len())
    {
        let chosen = index == drawer.chosen_index();
        let marker = if chosen { "> " } else { "  " };
        let name = format!("{:<name_width$}  ", page.name());
        let remaining = usize::from(width).saturating_sub(2 + name.width());
        let summary = command_summary(page.summary(), remaining);
        lines.push(if chosen {
            chosen_row(
                vec![
                    Span::raw(marker),
                    Span::raw(name),
                    Span::styled(summary, palette.style(Role::Muted)),
                ],
                palette,
                width,
            )
        } else {
            Line::from(vec![
                Span::styled(marker, palette.style(Role::Muted)),
                Span::styled(name, palette.style(Role::Muted)),
                Span::styled(summary, palette.style(Role::Muted)),
            ])
        });
    }
    lines.extend((0..gap).map(|_| Line::default()));
    lines.push(Line::styled(
        "↑↓ choose · Enter open · Esc close".to_owned(),
        palette.style(Role::Muted),
    ));
    lines
}
