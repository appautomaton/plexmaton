//! Grapheme-aware wrapping carries visible-text byte ranges beside styled rows.
#[cfg(test)]
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;

mod breaks;
use breaks::{Breaks, Row};
#[cfg(test)]
mod tests;

/// Count the same source breaks painting consumes, without allocating rows, styles or copy maps.
pub(crate) fn count(text: &str, width: usize, literal: bool) -> usize {
    Breaks::new(text, width, literal).count()
}

#[cfg(test)]
fn ranges(
    line: Line<'static>,
    width: usize,
    literal: bool,
) -> Vec<(Line<'static>, std::ops::Range<usize>)> {
    let mut text = String::new();
    let mut styles = Vec::new();
    for span in line.spans {
        text.push_str(&span.content);
        styles.push((text.len(), line.style.patch(span.style)));
    }
    styled_ranges(&text, &styles, width, literal)
        .into_iter()
        .map(|(run, range)| {
            let line = match run {
                Runs::Text(spans) => Line::from(
                    spans
                        .into_iter()
                        .map(|(text, style)| Span::styled(text, style))
                        .collect::<Vec<_>>(),
                ),
                Runs::Replacement(style) => Line::styled("�", style),
            };
            (line, range)
        })
        .collect()
}

/// Semantic and terminal styles consume exactly the same breaks and source intervals.
pub(super) enum Runs<S> {
    Text(Vec<(String, S)>),
    Replacement(S),
}

pub(super) fn styled_ranges<S: Clone + Default + PartialEq>(
    text: &str,
    styles: &[(usize, S)],
    width: usize,
    literal: bool,
) -> Vec<(Runs<S>, std::ops::Range<usize>)> {
    let mut style_index = 0;
    let default_style = S::default();
    let mut style_at = |offset| {
        while styles
            .get(style_index)
            .is_some_and(|(end, _)| offset >= *end)
        {
            style_index += 1;
        }
        styles
            .get(style_index)
            .map_or(&default_style, |(_, style)| style)
    };
    Breaks::new(text, width, literal)
        .map(|row| {
            let range = row.source();
            if matches!(row, Row::Replacement(_)) {
                return (Runs::Replacement(style_at(range.start).clone()), range);
            }
            let mut spans: Vec<(String, S)> = Vec::new();
            for (offset, grapheme) in text[range.clone()].grapheme_indices(true) {
                let style = style_at(range.start + offset);
                if let Some((text, _)) = spans.last_mut().filter(|(_, previous)| previous == style)
                {
                    text.push_str(grapheme);
                } else {
                    spans.push((grapheme.to_owned(), style.clone()));
                }
            }
            (Runs::Text(spans), range)
        })
        .collect()
}
