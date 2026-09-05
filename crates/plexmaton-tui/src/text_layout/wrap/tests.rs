use super::*;
use proptest::prelude::*;
use unicode_width::UnicodeWidthStr as _;

/// MD-2/SEL-2/TR-1: measured breaks preserve the previous width, style and source-range semantics.
#[test]
fn shared_break_geometry_keeps_empty_unicode_whitespace_and_replacement_behavior() {
    for source in [
        "",
        "short",
        "alpha beta gamma",
        "  a  b ",
        "中🙂e\u{301}",
        "\t x",
        "abcdefgh",
        "e\u{301}\u{301} x",
    ] {
        for width in [0, 1, 2, 4, 8, 58, 118] {
            for literal in [false, true] {
                let line = Line::styled(
                    source.to_owned(),
                    Style::new().fg(ratatui::style::Color::Cyan),
                );
                let expected = reference_ranges(line.clone(), width, literal);
                assert_eq!(
                    ranges(line, width, literal),
                    expected,
                    "{source:?} at {width}, literal={literal}"
                );
                assert_eq!(count(source, width, literal), expected.len());
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    /// MD-2/SEL-2: neither short-line admission nor style boundaries split or lose a grapheme.
    #[test]
    fn shared_geometry_matches_the_independent_vector_reference(
        parts in prop::collection::vec(prop_oneof![Just("a"), Just(" "), Just("中"), Just("🙂"),
            Just("e\u{301}"), Just("\u{301}"), Just("\u{200d}"), Just("\t"), Just("\r")], 0..90),
        width in 0_usize..80,
        literal in any::<bool>(),
    ) {
        let source = parts.concat();
        let spans: Vec<_> = parts.into_iter().enumerate().map(|(index, part)| {
            Span::styled(part, if index % 2 == 0 { Style::new().fg(ratatui::style::Color::Magenta) }
                else { Style::new().add_modifier(ratatui::style::Modifier::BOLD) })
        }).collect();
        let line = Line::from(spans).style(Style::new().add_modifier(ratatui::style::Modifier::ITALIC));
        let expected = reference_ranges(line.clone(), width, literal);
        prop_assert_eq!(ranges(line, width, literal), expected.clone());
        prop_assert_eq!(count(&source, width, literal), expected.len());
    }
}

// Frozen pre-extraction vector algorithm: a test oracle, never an alternate production path.
fn reference_ranges(
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
