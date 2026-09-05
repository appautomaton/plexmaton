//! Width-independent visible text and the ranges its styled rows paint. Never terminal scraping.
use std::ops::Range;

use ratatui::{
    style::Style,
    text::{Line, Span},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
mod cache;
#[cfg(test)]
mod tests;
pub(crate) use cache::Cache;
pub(crate) mod wrap;

#[derive(Clone, Debug)]
pub(crate) struct Fragment {
    pub column: usize,
    pub text: Range<usize>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Layout {
    pub lines: Vec<Line<'static>>,
    pub text: String,
    pub rows: Vec<Vec<Fragment>>,
}

impl Layout {
    pub fn decoration(&mut self, line: Line<'static>) {
        self.lines.push(line);
        self.rows.push(Vec::new());
    }

    /// One semantic line; visual wraps never append newlines to its copy text.
    pub fn logical(
        &mut self,
        line: Line<'static>,
        width: usize,
        literal: bool,
        prefix: &str,
        style: Style,
    ) {
        let offset = self.text.len();
        self.text.push_str(&line.to_string());
        self.text.push('\n');
        for (mut line, range) in wrap::ranges(line, width, literal) {
            let mut fragments = Vec::new();
            if !range.is_empty() {
                fragments.push(Fragment {
                    column: prefix.width(),
                    text: offset + range.start..offset + range.end,
                });
            }
            if !prefix.is_empty() {
                line.spans.insert(0, Span::styled(prefix.to_owned(), style));
            }
            self.lines.push(line);
            self.rows.push(fragments);
        }
    }

    /// Append another projection without losing the identity of its copy ranges.
    pub fn append(&mut self, mut other: Self, prefix: &str, style: Style) {
        let offset = self.text.len();
        self.text.push_str(&other.text);
        for fragments in &mut other.rows {
            for fragment in fragments {
                fragment.column += prefix.width();
                fragment.text = offset + fragment.text.start..offset + fragment.text.end;
            }
        }
        if !prefix.is_empty() {
            for line in &mut other.lines {
                line.spans.insert(0, Span::styled(prefix.to_owned(), style));
            }
        }
        self.lines.extend(other.lines);
        self.rows.extend(other.rows);
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

    /// Apply presentation-only highlighting to exactly the characters represented by the range.
    pub fn highlighted_lines(&self, selected: Range<usize>, style: Style) -> Vec<Line<'static>> {
        let mut lines = self.lines.clone();
        for (line, fragments) in lines.iter_mut().zip(&self.rows) {
            let columns: Vec<_> = fragments
                .iter()
                .filter_map(|fragment| {
                    let start = selected.start.max(fragment.text.start);
                    let end = selected.end.min(fragment.text.end);
                    if start >= end {
                        return None;
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
            let mut spans: Vec<Span<'static>> = Vec::new();
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
                        spans.push(Span::styled(grapheme.to_owned(), applied));
                    }
                    x = end;
                }
            }
            line.spans = spans;
        }
        lines
    }
}

pub(crate) fn into_lines(layout: std::borrow::Cow<'_, Layout>) -> Vec<Line<'static>> {
    match layout {
        std::borrow::Cow::Owned(layout) => layout.lines,
        std::borrow::Cow::Borrowed(layout) => layout.lines.clone(),
    }
}
