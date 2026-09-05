//! Grapheme-aware wrapping carries visible-text byte ranges beside styled rows.
use ratatui::{
    style::Style,
    text::{Line, Span},
};
use unicode_segmentation::UnicodeSegmentation;

mod breaks;
use breaks::{Breaks, Row};
#[cfg(test)]
mod tests;

/// Count the same source breaks painting consumes, without allocating rows, styles or copy maps.
pub(crate) fn count(text: &str, width: usize, literal: bool) -> usize {
    Breaks::new(text, width, literal).count()
}

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
    let mut style_at = |offset| {
        while styles
            .get(style_index)
            .is_some_and(|(end, _)| offset >= *end)
        {
            style_index += 1;
        }
        styles
            .get(style_index)
            .map_or(Style::default(), |(_, style)| *style)
    };
    Breaks::new(&text, width, literal)
        .map(|row| {
            let range = row.source();
            if matches!(row, Row::Replacement(_)) {
                return (Line::styled("�", style_at(range.start)), range);
            }
            let mut spans: Vec<Span<'static>> = Vec::new();
            for (offset, grapheme) in text[range.clone()].grapheme_indices(true) {
                let style = style_at(range.start + offset);
                if let Some(last) = spans.last_mut().filter(|last| last.style == style) {
                    last.content.to_mut().push_str(grapheme);
                } else {
                    spans.push(Span::styled(grapheme.to_owned(), style));
                }
            }
            (Line::from(spans), range)
        })
        .collect()
}
