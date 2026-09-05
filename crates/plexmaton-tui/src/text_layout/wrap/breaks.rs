//! One borrowed geometry walk for counting rows and constructing their styled/copy projection.
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation as _;
use unicode_width::UnicodeWidthStr as _;

#[derive(Debug, Eq, PartialEq)]
pub(super) enum Row {
    Text(Range<usize>),
    Replacement(Range<usize>),
}

impl Row {
    pub(super) fn source(&self) -> Range<usize> {
        match self {
            Self::Text(range) | Self::Replacement(range) => range.clone(),
        }
    }
}

pub(super) struct Breaks<'a> {
    text: &'a str,
    width: usize,
    literal: bool,
    next: Option<usize>,
}

impl<'a> Breaks<'a> {
    pub(super) fn new(text: &'a str, width: usize, literal: bool) -> Self {
        Self {
            text,
            width,
            literal,
            next: (width > 0).then_some(0),
        }
    }
}

impl Iterator for Breaks<'_> {
    type Item = Row;

    fn next(&mut self) -> Option<Self::Item> {
        let start = self.next?;
        let rest = &self.text[start..];
        // UTF-8 byte length is an upper bound on display columns, including zero-width controls.
        // Short lines therefore need neither a grapheme vector nor a second width calculation.
        if rest.len() <= self.width {
            self.next = None;
            return Some(Row::Text(start..self.text.len()));
        }
        let mut used = 0;
        let mut end = start;
        let mut space = None;
        for (offset, grapheme) in rest.grapheme_indices(true) {
            let columns = grapheme.width();
            if used + columns > self.width {
                if end == start {
                    let end = start + grapheme.len();
                    self.next = (end < self.text.len()).then_some(end);
                    return Some(Row::Replacement(start..end));
                }
                break;
            }
            if grapheme == " " {
                space = Some(start + offset);
            }
            used += columns;
            end = start + offset + grapheme.len();
        }
        let mut next = end;
        if !self.literal
            && end < self.text.len()
            && let Some(at) = space.filter(|at| *at > start)
        {
            end = at;
            next = at + 1;
        }
        self.next = (next < self.text.len()).then_some(next);
        Some(Row::Text(start..end))
    }
}
