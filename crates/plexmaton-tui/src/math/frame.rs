//! Cell diff and native text share a single successfully committed frame.

use ratatui::{
    buffer::{Buffer, CellDiffOption},
    layout::Rect,
    style::Style,
};

use super::GlyphRun;

const MAX_RUNS: usize = 512;
const MAX_BYTES: usize = 256 * 1024;

/// An admitted native operation in absolute screen coordinates, with late-resolved terminal paint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeText {
    /// Printable text and native scale; `x`/`y` are absolute terminal cells for this frame.
    pub glyph: GlyphRun,
    /// Palette and selection treatment, before explicit mathematical foreground/font overrides.
    pub style: Style,
}

impl NativeText {
    fn rect(&self) -> Rect {
        Rect::new(
            self.glyph.x,
            self.glyph.y,
            self.glyph.columns,
            self.glyph.rows,
        )
    }
    fn position(&self) -> (u16, u16) {
        (self.glyph.y, self.glyph.x)
    }
}

/// The CLI serializes these phases on the same output that receives the ordinary cell diff.
pub enum NativeStage<'a> {
    /// Start one synchronized output transaction before ordinary terminal cells are flushed.
    Begin,
    /// Emit only changed native operations, then end the transaction and restore the cell cursor.
    End {
        /// Only these operations need to be written again.
        changed: &'a [&'a NativeText],
        /// Complete current scene for output validation and frame-review consumers.
        current: &'a [NativeText],
    },
}

#[derive(Debug, Default)]
pub(crate) struct NativeFrame {
    area: Rect,
    text: Vec<NativeText>,
    bytes: usize,
}

impl NativeFrame {
    pub(crate) fn text(&self) -> &[NativeText] {
        &self.text
    }
    pub(crate) fn new(area: Rect) -> Self {
        Self {
            area,
            ..Self::default()
        }
    }

    pub(crate) fn push(&mut self, text: NativeText) -> bool {
        // This conservative charge covers vector growth, owned text and bounded escape metadata.
        let bytes = 512 + text.glyph.text.capacity();
        if self.text.len() >= MAX_RUNS || self.bytes + bytes > MAX_BYTES {
            return false;
        }
        self.bytes += bytes;
        self.text.push(text);
        true
    }

    pub(crate) fn finish(&mut self) {
        self.text.sort_by_key(NativeText::position);
    }

    fn contains(&self, other: &Self, text: &NativeText) -> bool {
        self.area == other.area
            && self
                .text
                .binary_search_by_key(&text.position(), NativeText::position)
                .is_ok_and(|index| self.text[index] == *text)
    }

    pub(crate) fn mark_diff(&self, previous: &Self, buffer: &mut Buffer) {
        // Replacing an old multicell's top row erases the entire block. Force those current
        // cells into Ratatui's row-ordered diff before it can write anywhere in a lower row.
        for old in &previous.text {
            if !self.contains(previous, old) {
                mark(buffer, old.rect(), CellDiffOption::AlwaysUpdate);
            }
        }
        // An unchanged native reservation is already on screen; a blank cell diff must not erase it.
        for text in &self.text {
            if previous.contains(self, text) {
                mark(buffer, text.rect(), CellDiffOption::Skip);
            }
        }
    }

    pub(crate) fn changed<'a>(&'a self, previous: &Self) -> Vec<&'a NativeText> {
        self.text
            .iter()
            .filter(|text| !previous.contains(self, text))
            .collect()
    }
}

fn mark(buffer: &mut Buffer, rect: Rect, option: CellDiffOption) {
    let rect = rect.intersection(buffer.area);
    for y in rect.y..rect.bottom() {
        for x in rect.x..rect.right() {
            buffer[(x, y)].set_diff_option(option);
        }
    }
}
