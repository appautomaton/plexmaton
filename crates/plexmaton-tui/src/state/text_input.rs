//! Editable text with one insertion point.
//!
//! This is the text, and nothing about what any particular input is *for*. It does not know which
//! agent a draft addresses, what submitting means, or which surface is painting it: those are the
//! caller's, which is what lets the primary composer, an entered worker's steering input and the
//! Drawer's filter be three uses of one editing model rather than three editors.
//!
//! Movement is measured in graphemes and logical lines, so it never depends on a width. Only
//! presentation does: [`TextInput::visible_rows`] and [`TextInput::caret`] take the width they are
//! painted at, and both read the same wrapping, so the row a caret lands on is the row the user
//! sees it on.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

mod selection;
use selection::Selection;

/// The fewest rows an input's window may be asked to hold: what a sub-agent's input keeps, and
/// the primary composer's floor when the column is short (ui-ux §input).
pub const MAX_VISIBLE_LINES: u16 = 3;

/// One wrapped row, and where it starts in the source text.
///
/// The offset is what makes the mapping two-way. Without it a caret can only be placed by measuring
/// painted cells, which is how the workspace ended up with an insertion point that could only ever
/// be the end of the text.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Row {
    start: usize,
    text: String,
}

/// Where the caret sits in the rows an input is currently showing.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Caret {
    /// Row within the visible window, not within the whole text.
    pub row: u16,
    /// Display columns from the row's left edge, so a wide glyph advances it by two.
    pub column: u16,
}

/// One cursor motion. Every variant is width-independent by construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Motion {
    /// One grapheme cluster back.
    Left,
    /// One grapheme cluster forward.
    Right,
    /// To the start of the run of non-whitespace before the caret.
    WordLeft,
    /// Past the run of non-whitespace after the caret.
    WordRight,
    /// To the first byte of the logical line the caret is on.
    LineStart,
    /// To the last byte of the logical line the caret is on.
    LineEnd,
}

/// Text the user is editing, and where their next keystroke lands in it.
///
/// The insertion point is a byte offset that is always on a grapheme boundary and never past the
/// end. Every mutator restores both properties, so no caller has to check them.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TextInput {
    text: String,
    cursor: usize,
    selection: Option<Selection>,
}

impl TextInput {
    /// Empty text, with the caret at its start.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            selection: None,
        }
    }

    /// The text exactly as typed.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Byte offset of the insertion point.
    #[must_use]
    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    /// Whether submitting would send anything.
    #[must_use]
    pub fn is_blank(&self) -> bool {
        self.text.trim().is_empty()
    }

    /// Inserts one character at the caret and steps over it.
    pub fn insert(&mut self, character: char) {
        self.delete_selection();
        self.text.insert(self.cursor, character);
        self.cursor = self.cursor.saturating_add(character.len_utf8());
        self.settle_cursor();
    }

    /// Breaks the line at the caret without submitting.
    pub fn newline(&mut self) {
        self.insert('\n');
    }

    /// Insert a paste atomically at the caret, replacing a selection without submitting.
    pub(crate) fn paste(&mut self, text: &str) -> bool {
        if text.is_empty() {
            return false;
        }
        self.delete_selection();
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        self.text.insert_str(self.cursor, &text);
        self.cursor += text.len();
        self.settle_cursor();
        true
    }

    /// Removes the grapheme cluster before the caret, reporting whether there was one.
    ///
    /// A cluster, not a `char`: `e` followed by a combining acute is one thing on screen and one
    /// press must remove all of it.
    pub fn delete_backward(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        let Some(start) = self.previous_boundary() else {
            return false;
        };
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
        self.settle_cursor();
        true
    }

    /// Removes the grapheme cluster at the caret, which does not move.
    pub fn delete_forward(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        let Some(end) = self.next_boundary() else {
            return false;
        };
        self.text.replace_range(self.cursor..end, "");
        self.settle_cursor();
        true
    }

    /// Removes the whitespace and then the word before the caret.
    pub fn delete_word_backward(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        let start = self.word_start();
        if start == self.cursor {
            return false;
        }
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
        self.settle_cursor();
        true
    }

    /// Removes everything between the start of the logical line and the caret.
    pub fn kill_to_line_start(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        let start = self.line_start();
        if start == self.cursor {
            return false;
        }
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
        self.settle_cursor();
        true
    }

    /// Removes everything between the caret and the end of the logical line.
    pub fn kill_to_line_end(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        let end = self.line_end();
        if end == self.cursor {
            return false;
        }
        self.text.replace_range(self.cursor..end, "");
        self.settle_cursor();
        true
    }

    /// Moves the caret, reporting whether it went anywhere.
    pub fn move_caret(&mut self, motion: Motion) -> bool {
        let target = match motion {
            Motion::Left => self.previous_boundary().unwrap_or(self.cursor),
            Motion::Right => self.next_boundary().unwrap_or(self.cursor),
            Motion::WordLeft => self.word_start(),
            Motion::WordRight => self.word_end(),
            Motion::LineStart => self.line_start(),
            Motion::LineEnd => self.line_end(),
        };
        self.place_caret(target)
    }

    /// Puts the caret at the grapheme boundary a pointer landed on.
    ///
    /// `row` is an index into the rows currently visible, which is what a click resolves to: the
    /// caller knows where it drew the input, not where the text scrolled to.
    pub fn click(&mut self, width: u16, window: u16, row: u16, column: u16) -> bool {
        let target = self.offset_at(width, window, row, column);
        self.place_caret(target)
    }

    /// Places an input's caret from a presentation-derived source offset (COM-2).
    pub(crate) fn place_caret(&mut self, target: usize) -> bool {
        let cleared = self.clear_selection();
        let before = self.cursor;
        self.cursor = target.min(self.text.len());
        self.settle_cursor();
        cleared || self.cursor != before
    }

    /// An edit can join clusters on either side of the insertion point. Restore the boundary
    /// against the complete text, rather than segmenting its prefix as if it ended there.
    fn settle_cursor(&mut self) {
        if self.cursor == self.text.len() {
            return;
        }
        self.cursor = self
            .text
            .grapheme_indices(true)
            .map(|(offset, _)| offset)
            .find(|offset| *offset >= self.cursor)
            .unwrap_or(self.text.len());
    }

    /// Discards the text, reporting whether there was any.
    pub fn clear(&mut self) -> bool {
        if self.text.is_empty() {
            return false;
        }
        self.text.clear();
        self.selection = None;
        self.cursor = 0;
        true
    }

    /// Takes the text for submission, leaving the input empty.
    ///
    /// Whitespace-only text submits nothing: sending it would put an empty message in a transcript
    /// that cannot be edited afterwards.
    pub fn take(&mut self) -> Option<String> {
        if self.is_blank() {
            return None;
        }
        self.selection = None;
        self.cursor = 0;
        Some(std::mem::take(&mut self.text))
    }

    /// Appends text the runtime could not deliver, leaving the caret after it.
    pub fn append_returned(&mut self, text: &str) {
        self.selection = None;
        if !self.text.is_empty() {
            self.text.push('\n');
        }
        self.text.push_str(text);
        self.cursor = self.text.len();
    }

    /// Completes the first skill token while preserving any request text after it.
    pub(crate) fn complete_initial_token(&mut self, name: &str) {
        let end = self
            .text
            .find(char::is_whitespace)
            .unwrap_or(self.text.len());
        let had_request = end < self.text.len();
        let replacement = if had_request {
            format!("${name}")
        } else {
            format!("${name} ")
        };
        self.text.replace_range(..end, &replacement);
        self.selection = None;
        self.cursor = replacement.len();
        self.settle_cursor();
    }

    /// Rows the input paints at `width` in a `window` that many rows tall: the ones around the
    /// caret.
    #[must_use]
    pub fn visible_rows(&self, width: u16, window: u16) -> Vec<String> {
        let rows = self.rows(width);
        let start = Self::window_start(&rows, self.caret_row(&rows), window);
        rows.into_iter()
            .skip(start)
            .take(usize::from(window.max(1)))
            .map(|row| row.text)
            .collect()
    }

    /// Where to paint the caret among [`TextInput::visible_rows`] at the same width and window.
    #[must_use]
    pub fn caret(&self, width: u16, window: u16) -> Caret {
        let rows = self.rows(width);
        let caret_row = self.caret_row(&rows);
        let start = Self::window_start(&rows, caret_row, window);
        let column = rows.get(caret_row).map_or(0, |row| {
            UnicodeWidthStr::width(&self.text[row.start..self.cursor.max(row.start)])
        });
        Caret {
            row: u16::try_from(caret_row.saturating_sub(start)).unwrap_or(0),
            column: u16::try_from(column).unwrap_or(u16::MAX),
        }
    }

    /// Rows the input asks layout for at `width`, up to `cap` lines, borders included.
    #[must_use]
    pub fn requested_rows(&self, width: u16, cap: u16) -> u16 {
        let rows = u16::try_from(self.rows(width).len()).unwrap_or(u16::MAX);
        // At least one row, so an empty input is still a place to type; two borders around it.
        rows.clamp(1, cap.max(1)).saturating_add(2)
    }

    /// Moves the caret one painted row up or down, keeping its display column, so a draft
    /// taller than its window is walked with the arrows and the window follows (ui-ux §input).
    pub fn move_row(&mut self, width: u16, direction: crate::Direction) -> bool {
        let rows = self.rows(width);
        let caret_row = self.caret_row(&rows);
        let target = match direction {
            crate::Direction::Backward => caret_row.checked_sub(1),
            crate::Direction::Forward => Some(caret_row.saturating_add(1)),
        };
        let Some(target) = target.filter(|target| *target < rows.len()) else {
            return false;
        };
        let column = rows.get(caret_row).map_or(0, |row| {
            UnicodeWidthStr::width(&self.text[row.start..self.cursor.max(row.start)])
        });
        let offset = Self::offset_in_row(&rows[target], u16::try_from(column).unwrap_or(u16::MAX));
        self.place_caret(offset)
    }

    fn offset_at(&self, width: u16, window: u16, row: u16, column: u16) -> usize {
        let rows = self.rows(width);
        if rows.is_empty() {
            return 0;
        }
        let start = Self::window_start(&rows, self.caret_row(&rows), window);
        let index = start
            .saturating_add(usize::from(row))
            .min(rows.len().saturating_sub(1));
        Self::offset_in_row(&rows[index], column)
    }

    /// The grapheme boundary at or before a display column of one painted row.
    fn offset_in_row(row: &Row, column: u16) -> usize {
        let mut offset = row.start;
        let mut used = 0_usize;
        for cluster in row.text.graphemes(true) {
            let next = used.saturating_add(UnicodeWidthStr::width(cluster));
            if next > usize::from(column) {
                break;
            }
            used = next;
            offset = offset.saturating_add(cluster.len());
        }
        offset
    }

    /// The window of rows the input shows: the tail, pulled up when the caret is above it.
    ///
    /// Computed rather than stored. A stored scroll offset would be a second answer to "which rows
    /// are on screen" that could disagree with the caret after an edit.
    fn window_start(rows: &[Row], caret_row: usize, window: u16) -> usize {
        let tail = rows.len().saturating_sub(usize::from(window.max(1)));
        caret_row.min(tail)
    }

    fn caret_row(&self, rows: &[Row]) -> usize {
        rows.iter()
            .rposition(|row| row.start <= self.cursor)
            .unwrap_or(0)
    }

    fn rows(&self, width: u16) -> Vec<Row> {
        let width = usize::from(width);
        let mut rows = Vec::new();
        let mut base = 0_usize;
        for line in self.text.split('\n') {
            rows.extend(wrap_indexed(line, width, base));
            // An insertion point after a full row occupies the next row, never the right border.
            if width > 0 && rows.last().is_some_and(|row| row.text.width() == width) {
                rows.push(Row {
                    start: base + line.len(),
                    text: String::new(),
                });
            }
            // The newline itself is a byte the rows do not carry.
            base = base.saturating_add(line.len()).saturating_add(1);
        }
        rows
    }

    fn previous_boundary(&self) -> Option<usize> {
        self.text[..self.cursor]
            .graphemes(true)
            .next_back()
            .map(|cluster| self.cursor.saturating_sub(cluster.len()))
    }

    fn next_boundary(&self) -> Option<usize> {
        self.text[self.cursor..]
            .graphemes(true)
            .next()
            .map(|cluster| self.cursor.saturating_add(cluster.len()))
    }

    fn line_start(&self) -> usize {
        self.text[..self.cursor]
            .rfind('\n')
            .map_or(0, |index| index.saturating_add(1))
    }

    fn line_end(&self) -> usize {
        self.text[self.cursor..]
            .find('\n')
            .map_or(self.text.len(), |index| self.cursor.saturating_add(index))
    }

    fn word_start(&self) -> usize {
        let mut offset = self.cursor;
        let mut seen_word = false;
        for cluster in self.text[..offset].graphemes(true).rev() {
            let whitespace = is_whitespace(cluster);
            if whitespace && seen_word {
                break;
            }
            if !whitespace {
                seen_word = true;
            }
            offset = offset.saturating_sub(cluster.len());
        }
        offset
    }

    fn word_end(&self) -> usize {
        let mut offset = self.cursor;
        let mut seen_word = false;
        for cluster in self.text[offset..].graphemes(true) {
            let whitespace = is_whitespace(cluster);
            if whitespace && seen_word {
                break;
            }
            if !whitespace {
                seen_word = true;
            }
            offset = offset.saturating_add(cluster.len());
        }
        offset
    }
}

fn is_whitespace(cluster: &str) -> bool {
    cluster.chars().next().is_some_and(char::is_whitespace)
}

/// Breaks one logical line into the rows it occupies at `width`, carrying source offsets.
///
/// Wraps after a space where there is one and hard-breaks a word longer than the row, which is what
/// the panels around it do. The space stays on the row it ended, so the caret lands where the user
/// last typed rather than at the start of the next row.
///
/// Measured in display width over grapheme clusters: a wide glyph takes two cells and a combining
/// mark takes none, so counting either characters or bytes wraps in the wrong place.
fn wrap_indexed(line: &str, width: usize, base: usize) -> Vec<Row> {
    if width == 0 {
        return vec![Row {
            start: base,
            text: String::new(),
        }];
    }
    let mut rows: Vec<Row> = Vec::new();
    let mut row = String::new();
    let mut row_start = base;
    let mut row_width = 0_usize;
    // Byte offset in `row` just past the last space, which is where a break may go.
    let mut wrap_point: Option<usize> = None;

    for cluster in line.graphemes(true) {
        let cluster_width = UnicodeWidthStr::width(cluster);
        if row_width.saturating_add(cluster_width) > width && !row.is_empty() {
            let carry = match wrap_point {
                Some(byte) if byte < row.len() => row.split_off(byte),
                _ => String::new(),
            };
            let emitted = std::mem::take(&mut row);
            let carry_start = row_start.saturating_add(emitted.len());
            rows.push(Row {
                start: row_start,
                text: emitted,
            });
            row_start = carry_start;
            row_width = UnicodeWidthStr::width(carry.as_str());
            row = carry;
            wrap_point = None;
        }
        row.push_str(cluster);
        row_width = row_width.saturating_add(cluster_width);
        if cluster == " " {
            wrap_point = Some(row.len());
        }
    }
    rows.push(Row {
        start: row_start,
        text: row,
    });
    rows
}

/// Breaks one logical line into the rows it occupies at `width`.
pub(crate) fn wrap_line(line: &str, width: usize) -> Vec<String> {
    wrap_indexed(line, width, 0)
        .into_iter()
        .map(|row| row.text)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Caret, Motion, TextInput};

    fn typed(text: &str) -> TextInput {
        let mut input = TextInput::new();
        for character in text.chars() {
            input.insert(character);
        }
        input
    }

    /// COM-2: one press of `Backspace` removes one thing on screen.
    #[test]
    fn backspace_removes_a_whole_grapheme_cluster() {
        let mut input = typed("e\u{301}");
        assert!(input.delete_backward());
        assert_eq!(input.text(), "");
    }

    /// COM-2: joining clusters by insertion or deletion cannot leave a caret inside the result.
    #[test]
    fn edits_that_join_clusters_restore_the_grapheme_boundary() {
        let mut emoji = typed("👩👩");
        emoji.click(10, 3, 0, 2);
        emoji.insert('\u{200d}');
        assert_eq!(emoji.cursor(), emoji.text().len());
        emoji.delete_backward();
        assert_eq!(emoji.text(), "");

        let mut backward = typed("a\n\u{301}");
        backward.move_caret(Motion::LineStart);
        backward.delete_backward();
        assert_eq!(backward.text(), "a\u{301}");
        assert_eq!(backward.cursor(), backward.text().len());

        let mut forward = typed("a\n\u{301}");
        forward.move_caret(Motion::LineStart);
        forward.move_caret(Motion::Left);
        forward.delete_forward();
        assert_eq!(forward.text(), "a\u{301}");
        assert_eq!(forward.cursor(), forward.text().len());
    }

    /// COM-2: an unchanged input costs no repaint.
    #[test]
    fn deleting_an_empty_draft_changes_nothing() {
        let mut input = TextInput::new();
        assert!(!input.delete_backward());
        assert!(!input.delete_forward());
        assert_eq!(input.cursor(), 0);
    }

    /// COM-3: whitespace is not a message.
    #[test]
    fn a_blank_draft_submits_nothing_and_is_left_alone() {
        let mut input = typed("   ");
        assert_eq!(input.take(), None);
        assert_eq!(input.text(), "   ");
    }

    /// COM-3: submitting hands over exactly what was typed.
    #[test]
    fn taking_the_draft_returns_it_exactly_and_clears_it() {
        let mut input = typed("ship it");
        assert_eq!(input.take().as_deref(), Some("ship it"));
        assert_eq!(input.text(), "");
        assert_eq!(input.cursor(), 0);
    }

    /// COM-2: the insertion point is wherever the caret is, not the end of the text.
    #[test]
    fn editing_happens_at_the_caret_rather_than_at_the_end() {
        let mut input = typed("ac");
        assert!(input.move_caret(Motion::Left));
        input.insert('b');
        assert_eq!(input.text(), "abc");
        assert_eq!(input.cursor(), 2);
        assert!(input.delete_forward());
        assert_eq!(input.text(), "ab");
    }

    /// COM-2: motion is measured in clusters, so it never lands mid-grapheme.
    #[test]
    fn motion_steps_over_whole_clusters_and_stops_at_the_ends() {
        let mut input = typed("a\u{301}b");
        assert!(input.move_caret(Motion::LineStart));
        assert_eq!(input.cursor(), 0);
        assert!(!input.move_caret(Motion::Left));
        assert!(input.move_caret(Motion::Right));
        assert_eq!(input.cursor(), "a\u{301}".len());
        assert!(input.move_caret(Motion::LineEnd));
        assert!(!input.move_caret(Motion::Right));
    }

    /// COM-2: word motion and word deletion agree on where a word starts.
    #[test]
    fn word_deletion_takes_the_trailing_space_and_the_word() {
        let mut input = typed("one two  ");
        assert!(input.delete_word_backward());
        assert_eq!(input.text(), "one ");
        let mut back = typed("one two");
        assert!(back.move_caret(Motion::WordLeft));
        assert_eq!(back.cursor(), "one ".len());
    }

    /// COM-2: the line kills bound to the logical line, not to a wrapped row.
    #[test]
    fn killing_binds_to_the_logical_line_the_caret_is_on() {
        let mut input = typed("first\nsecond");
        assert!(input.kill_to_line_start());
        assert_eq!(input.text(), "first\n");
        let mut ahead = typed("keep drop");
        ahead.move_caret(Motion::LineStart);
        for _ in 0..5 {
            ahead.move_caret(Motion::Right);
        }
        assert!(ahead.kill_to_line_end());
        assert_eq!(ahead.text(), "keep ");
    }

    /// COM-1: the caret's painted row and column come from the same wrap the rows do.
    #[test]
    fn the_caret_reports_the_row_and_column_it_is_painted_on() {
        let mut input = typed("aaa bbb");
        assert_eq!(
            input.visible_rows(4, 3),
            vec!["aaa ".to_owned(), "bbb".to_owned()]
        );
        assert_eq!(input.caret(4, 3), Caret { row: 1, column: 3 });
        input.move_caret(Motion::LineStart);
        assert_eq!(input.caret(4, 3), Caret { row: 0, column: 0 });
    }

    /// COM-1: a wide glyph advances the caret by the cells it occupies.
    #[test]
    fn a_wide_glyph_advances_the_caret_by_two_cells() {
        let input = typed("宽");
        assert_eq!(input.caret(10, 3), Caret { row: 0, column: 2 });
    }

    /// COM-2: the visible window follows the caret instead of always showing the tail.
    ///
    /// Both sides of the boundary, because the window is a `min`: a caret still inside the tail
    /// must not move it, and one above must.
    #[test]
    fn the_visible_window_follows_the_caret_above_the_tail() {
        let mut input = typed("l1\nl2\nl3\nl4");
        assert_eq!(input.visible_rows(10, 3), ["l2", "l3", "l4"]);
        assert_eq!(input.caret(10, 3), Caret { row: 2, column: 2 });

        // Eight clusters back is the start of `l2`, the first row of the tail. Still inside it.
        for _ in 0..8 {
            input.move_caret(Motion::Left);
        }
        assert_eq!(input.visible_rows(10, 3), ["l2", "l3", "l4"]);
        assert_eq!(input.caret(10, 3), Caret { row: 0, column: 0 });

        // Three more is the start of `l1`, which the tail does not reach.
        for _ in 0..3 {
            input.move_caret(Motion::Left);
        }
        assert_eq!(input.visible_rows(10, 3), ["l1", "l2", "l3"]);
        assert_eq!(input.caret(10, 3), Caret { row: 0, column: 0 });
    }

    /// COM-1: a click resolves to the boundary under it, and round-trips with the caret.
    #[test]
    fn a_click_lands_on_the_boundary_under_it() {
        let mut input = typed("aaa bbb");
        assert!(input.click(4, 3, 0, 2));
        assert_eq!(input.cursor(), 2);
        assert_eq!(input.caret(4, 3), Caret { row: 0, column: 2 });
        input.click(4, 3, 1, 99);
        assert_eq!(input.cursor(), "aaa bbb".len());
    }

    /// COM-3: returned text is appended without inventing an insertion point elsewhere.
    #[test]
    fn returned_text_lands_after_the_existing_draft() {
        let mut input = typed("kept");
        input.append_returned("undelivered");
        assert_eq!(input.text(), "kept\nundelivered");
        assert_eq!(input.cursor(), input.text().len());
    }
}
