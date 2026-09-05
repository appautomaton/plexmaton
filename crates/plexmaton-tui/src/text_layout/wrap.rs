//! Grapheme-aware wrapping carries visible-text byte ranges beside styled rows.
use ratatui::{
    style::Style,
    text::{Line, Span},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub(crate) fn wrap(line: Line<'static>, width: usize, literal: bool) -> Vec<Line<'static>> {
    ranges(line, width, literal)
        .into_iter()
        .map(|(line, _)| line)
        .collect()
}

pub(crate) fn ranges(
    line: Line<'static>,
    width: usize,
    literal: bool,
) -> Vec<(Line<'static>, std::ops::Range<usize>)> {
    if width == 0 {
        return Vec::new();
    }
    let mut text = String::new();
    let mut styles = Vec::new();
    for span in line.spans {
        text.push_str(&span.content);
        styles.push((text.len(), line.style.patch(span.style)));
    }
    let mut style_index = 0;
    let cells: Vec<_> = text
        .grapheme_indices(true)
        .map(|(offset, grapheme)| {
            while styles
                .get(style_index)
                .is_some_and(|(end, _)| offset >= *end)
            {
                style_index += 1;
            }
            (
                grapheme,
                styles
                    .get(style_index)
                    .map_or(Style::default(), |(_, style)| *style),
                offset,
            )
        })
        .collect();
    let mut rows = Vec::new();
    let mut start = 0;
    while start < cells.len() {
        let mut end = start;
        let mut used = 0;
        let mut space = None;
        while end < cells.len() {
            let n = cells[end].0.width();
            if used + n > width {
                break;
            }
            if cells[end].0 == " " {
                space = Some(end);
            }
            used += n;
            end += 1;
        }
        if end == start {
            // A grapheme wider than the viewport gets a visible marker, not half a glyph.
            rows.push((
                Line::styled("�", cells[start].1),
                cells[start].2..cells[start].2 + cells[start].0.len(),
            ));
            start += 1;
            continue;
        }
        let mut next = end;
        if !literal
            && end < cells.len()
            && let Some(at) = space.filter(|at| *at > start)
        {
            end = at;
            next = at + 1;
        }
        let mut spans: Vec<Span<'static>> = Vec::new();
        for (text, style, _) in &cells[start..end] {
            if let Some(last) = spans.last_mut().filter(|last| last.style == *style) {
                last.content.to_mut().push_str(text);
            } else {
                spans.push(Span::styled((*text).to_owned(), *style));
            }
        }
        rows.push((
            Line::from(spans),
            cells[start].2..cells[end - 1].2 + cells[end - 1].0.len(),
        ));
        start = next;
    }
    if rows.is_empty() {
        rows.push((Line::default(), 0..0));
    }
    rows
}
