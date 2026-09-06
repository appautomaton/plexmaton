//! Width-independent visible text and the ranges its styled rows paint. Never terminal scraping.
use std::ops::Range;

use ratatui::{style::Style, text};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
mod cache;
pub(crate) mod math;
pub(crate) mod paint;
use paint::{Colors, Line, Paint, Span};
#[cfg(test)]
mod paint_tests;
#[cfg(test)]
mod tests;
pub(crate) use cache::{Cache, PreparedEntry};
pub(crate) mod wrap;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Fragment {
    pub column: usize,
    pub text: Range<usize>,
    pub kind: FragmentKind,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) enum FragmentKind {
    Text,
    Atomic { columns: usize },
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct Layout {
    pub lines: Vec<Line>,
    pub text: String,
    pub rows: Vec<Vec<Fragment>>,
    pub formulas: Vec<math::PlacedFormula>,
}

impl Layout {
    /// Account owned capacities, including composed paint layers, before retaining preparation.
    pub(crate) fn allocation_bytes(&self) -> usize {
        self.text.capacity()
            + self.formulas.capacity() * size_of::<math::PlacedFormula>()
            + self
                .formulas
                .iter()
                .map(math::PlacedFormula::allocation_bytes)
                .sum::<usize>()
            + self.rows.capacity() * size_of::<Vec<Fragment>>()
            + self
                .rows
                .iter()
                .map(|row| row.capacity() * size_of::<Fragment>())
                .sum::<usize>()
            + self.lines.capacity() * size_of::<Line>()
            + self.lines.iter().map(Line::allocation_bytes).sum::<usize>()
    }

    pub fn decoration(&mut self, line: Line) {
        self.lines.push(line);
        self.rows.push(Vec::new());
    }

    /// One semantic line; visual wraps never append newlines to its copy text.
    pub fn logical(
        &mut self,
        line: Line,
        width: usize,
        literal: bool,
        prefix: &str,
        style: impl Into<Paint>,
    ) {
        let offset = self.text.len();
        self.text.push_str(&line.to_string());
        self.text.push('\n');
        let style = style.into();
        for (mut line, range) in paint::ranges(line, width, literal) {
            let mut fragments = Vec::new();
            if !range.is_empty() {
                fragments.push(Fragment {
                    column: prefix.width(),
                    text: offset + range.start..offset + range.end,
                    kind: FragmentKind::Text,
                });
            }
            if !prefix.is_empty() {
                line.spans
                    .insert(0, Span::styled(prefix.to_owned(), style.clone()));
            }
            self.lines.push(line);
            self.rows.push(fragments);
        }
    }

    /// Append another projection without losing the identity of its copy ranges.
    pub fn append(&mut self, mut other: Self, prefix: &str, style: impl Into<Paint>) {
        let offset = self.text.len();
        let row = self.rows.len();
        self.text.push_str(&other.text);
        let style = style.into();
        for fragments in &mut other.rows {
            for fragment in fragments {
                fragment.column += prefix.width();
                fragment.text = offset + fragment.text.start..offset + fragment.text.end;
            }
        }
        if !prefix.is_empty() {
            for line in &mut other.lines {
                line.spans
                    .insert(0, Span::styled(prefix.to_owned(), style.clone()));
            }
        }
        self.lines.extend(other.lines);
        self.rows.extend(other.rows);
        for formula in &mut other.formulas {
            formula.column += prefix.width();
            formula.row += row;
            formula.text = offset + formula.text.start..offset + formula.text.end;
        }
        self.formulas.extend(other.formulas);
    }

    pub fn blank(&mut self) {
        if self.lines.last().is_some_and(|line| !line.spans.is_empty()) {
            self.decoration(Line::default());
            self.text.push('\n');
        }
    }

    pub fn finish(&mut self) {
        while self.lines.last().is_some_and(|line| line.spans.is_empty()) {
            self.lines.pop();
            self.rows.pop();
        }
        let end = self.text.trim_end_matches('\n').len();
        self.text.truncate(end);
    }

    /// Cell coordinates resolve to grapheme boundaries, including either cell of a wide glyph.
    pub fn offset_at(&self, row: usize, column: usize) -> Option<usize> {
        let fragments = self.rows.get(row)?;
        let mut last = None;
        for fragment in fragments {
            if column < fragment.column {
                return Some(fragment.text.start);
            }
            let text = self.text.get(fragment.text.clone())?;
            if let FragmentKind::Atomic { columns } = fragment.kind {
                if column < fragment.column + columns {
                    return Some(fragment.text.start);
                }
                last = Some(fragment.text.end);
                continue;
            }
            let mut x = fragment.column;
            for (byte, grapheme) in text.grapheme_indices(true) {
                let end = x + grapheme.width();
                if column < end {
                    return Some(fragment.text.start + byte);
                }
                x = end;
            }
            last = Some(fragment.text.end);
        }
        last
    }

    /// Every cell of a formula box names the same complete source interval.
    pub fn atom_at(&self, row: usize, column: usize) -> Option<Range<usize>> {
        self.rows.get(row)?.iter().find_map(|fragment| {
            let FragmentKind::Atomic { columns } = fragment.kind else {
                return None;
            };
            (column >= fragment.column && column < fragment.column + columns)
                .then(|| fragment.text.clone())
        })
    }

    /// Apply presentation-only highlighting to exactly the characters represented by the range.
    pub fn highlighted_lines(
        &self,
        selected: Range<usize>,
        palette: &crate::Palette,
        style: Style,
    ) -> Vec<text::Line<'static>> {
        let mut lines = self.painted_lines(palette);
        for (line, fragments) in lines.iter_mut().zip(&self.rows) {
            let columns: Vec<_> = fragments
                .iter()
                .filter_map(|fragment| {
                    let start = selected.start.max(fragment.text.start);
                    let end = selected.end.min(fragment.text.end);
                    if start >= end {
                        return None;
                    }
                    if let FragmentKind::Atomic { columns } = fragment.kind {
                        return Some(fragment.column..fragment.column + columns);
                    }
                    let before = self.text.get(fragment.text.start..start)?;
                    let chosen = self.text.get(start..end)?;
                    let x = fragment.column + before.width();
                    Some(x..x + chosen.width())
                })
                .collect();
            if columns.is_empty() {
                continue;
            }
            let mut spans: Vec<text::Span<'static>> = Vec::new();
            let mut x = 0;
            for span in &line.spans {
                for grapheme in span.content.graphemes(true) {
                    let end = x + grapheme.width();
                    let highlighted = columns
                        .iter()
                        .any(|range| x < range.end && end > range.start);
                    let applied = if highlighted {
                        line.style.patch(span.style).patch(style)
                    } else {
                        span.style
                    };
                    if let Some(last) = spans.last_mut().filter(|last| last.style == applied) {
                        last.content.to_mut().push_str(grapheme);
                    } else {
                        spans.push(text::Span::styled(grapheme.to_owned(), applied));
                    }
                    x = end;
                }
            }
            line.spans = spans;
        }
        lines
    }

    pub fn painted_lines(&self, palette: &crate::Palette) -> Vec<text::Line<'static>> {
        let colors = Colors::new(palette);
        self.lines
            .iter()
            .map(|line| line.paint_ref(&colors))
            .collect()
    }

    pub fn painted_entry(
        &self,
        palette: &crate::Palette,
        appearance: crate::state::EntryAppearance,
    ) -> Vec<text::Line<'static>> {
        let colors = Colors::new(palette);
        self.lines
            .iter()
            .map(|line| line.paint_entry(&colors, appearance))
            .collect()
    }
}
