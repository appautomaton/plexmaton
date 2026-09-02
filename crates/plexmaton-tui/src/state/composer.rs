//! The workspace's single text input.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::{Submission, SubmissionKind, ViewState};
use crate::{
    intent::TextIntent,
    surface::{SurfaceId, SurfaceTree},
};

/// Rows of draft the composer will show before it starts showing only the tail.
///
/// A bounded tail rather than a scrollable region, the same shape the notice strip uses. The
/// composer becomes a viewport's caller in step 4; until then the newest row is the one that
/// matters, because that is where the cursor is.
pub const MAX_VISIBLE_LINES: u16 = 3;

/// The draft the user is typing, and nothing else.
///
/// There is no cursor offset here on purpose. The router's key grammar has no binding that moves a
/// cursor, so the insertion point is always the end of the text; storing an offset nothing can
/// change would be a field to maintain and a second thing that could disagree with the string.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Composer {
    draft: String,
}

impl Composer {
    /// An empty draft, for an agent nobody has typed to yet.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            draft: String::new(),
        }
    }

    /// Appends one character at the insertion point.
    pub fn insert(&mut self, character: char) {
        self.draft.push(character);
    }

    /// Breaks the line without submitting.
    pub fn newline(&mut self) {
        self.draft.push('\n');
    }

    /// Removes the grapheme cluster before the insertion point.
    ///
    /// A cluster, not a `char`: `e` followed by a combining acute is one thing on screen and one
    /// press of `Backspace` must remove all of it. Truncating by `char` would leave a bare `e` and
    /// look like the key did nothing the first time.
    ///
    /// Returns whether anything was removed, so an empty draft does not cost a repaint.
    pub fn delete_backward(&mut self) -> bool {
        let Some(last) = self.draft.graphemes(true).next_back() else {
            return false;
        };
        let keep = self.draft.len().saturating_sub(last.len());
        self.draft.truncate(keep);
        true
    }

    /// Discards the draft, reporting whether there was one to discard.
    pub fn clear(&mut self) -> bool {
        if self.draft.is_empty() {
            return false;
        }
        self.draft.clear();
        true
    }

    /// Takes the draft for submission, leaving the composer empty.
    ///
    /// A draft that is only whitespace submits nothing: sending it would put an empty message in a
    /// transcript that cannot be edited afterwards.
    pub fn take_draft(&mut self) -> Option<String> {
        if self.draft.trim().is_empty() {
            return None;
        }
        Some(std::mem::take(&mut self.draft))
    }

    /// Returns the draft exactly as typed.
    #[must_use]
    pub fn draft(&self) -> &str {
        &self.draft
    }

    /// Returns whether the draft would submit anything.
    #[must_use]
    pub fn is_blank(&self) -> bool {
        self.draft.trim().is_empty()
    }

    /// Rows of the draft at `width`, newest last, bounded to what the composer shows.
    ///
    /// Rows the panel will actually paint, not the newlines the user typed. The draft is wrapped
    /// here rather than left for the widget, so the composer has one answer to "how tall is this
    /// and where does it end" — the widget's own wrapping then has nothing left to do, because
    /// every row it is handed already fits. Splitting on `'\n'` alone is what let a wrapped draft
    /// ask for one row, paint three, and put the caret on a fourth.
    #[must_use]
    pub fn visible_rows(&self, width: u16) -> Vec<String> {
        let rows = self.wrapped(width);
        let skip = rows.len().saturating_sub(usize::from(MAX_VISIBLE_LINES));
        rows.into_iter().skip(skip).collect()
    }

    /// Rows the composer asks layout for at `width`, borders included.
    #[must_use]
    pub fn requested_rows(&self, width: u16) -> u16 {
        let rows = u16::try_from(self.wrapped(width).len()).unwrap_or(MAX_VISIBLE_LINES);
        // Two borders, plus at least one row so an empty composer is still a place to type.
        rows.clamp(1, MAX_VISIBLE_LINES).saturating_add(2)
    }

    fn wrapped(&self, width: u16) -> Vec<String> {
        self.draft
            .split('\n')
            .flat_map(|line| wrap_line(line, usize::from(width)))
            .collect()
    }
}

/// Breaks one logical line into the rows it occupies at `width`.
///
/// Wraps after a space where there is one and hard-breaks a word that is longer than the row, which
/// is what the panels around it do. The space stays on the row it ended, so the caret lands where
/// the user last typed rather than at the start of the next row.
///
/// Measured in display width over grapheme clusters: a wide glyph takes two cells and a combining
/// mark takes none, so counting either characters or bytes wraps in the wrong place.
fn wrap_line(line: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    let mut rows: Vec<String> = Vec::new();
    let mut row = String::new();
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
            rows.push(std::mem::take(&mut row));
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
    rows.push(row);
    rows
}

impl ViewState {
    /// Applies one edit to whichever input holds the cursor.
    ///
    /// The returned submission is a *command* for the runtime, never something to write into the
    /// transcript here: the projection has one writer, and it is the event stream (COM-3).
    ///
    /// The target comes from focus rather than from the intent. The router only produces a text
    /// intent while a text input holds the cursor, and it reads that from this same state, so the
    /// two cannot disagree about which of the two inputs is being typed into (INV-2).
    pub fn edit(&mut self, surfaces: &SurfaceTree, intent: TextIntent) -> Option<Submission> {
        let kind = match self.focus.resolve(surfaces)? {
            SurfaceId::Composer => SubmissionKind::Message,
            SurfaceId::Inspector => SubmissionKind::Steering,
            _ => return None,
        };
        let to = self.text_target(surfaces)?;
        let composer = self.composers.entry(to.clone()).or_default();
        let changed = match intent {
            TextIntent::Insert(character) => {
                composer.insert(character);
                true
            }
            TextIntent::Newline => {
                composer.newline();
                true
            }
            TextIntent::DeleteBackward => composer.delete_backward(),
            TextIntent::Submit => {
                let submitted = composer.take_draft();
                if submitted.is_some() {
                    self.touch();
                }
                return submitted.map(|text| Submission { to, text, kind });
            }
        };
        if changed {
            self.touch();
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{Composer, MAX_VISIBLE_LINES};

    fn typed(text: &str) -> Composer {
        let mut composer = Composer::default();
        for character in text.chars() {
            if character == '\n' {
                composer.newline();
            } else {
                composer.insert(character);
            }
        }
        composer
    }

    /// COM-2: one press of `Backspace` removes one thing on screen.
    #[test]
    fn backspace_removes_a_whole_grapheme_cluster() {
        // `e` plus a combining acute is one cluster and two chars; a family emoji is one cluster
        // and several code points joined by zero-width joiners.
        let mut composer = typed("a e\u{301}");
        assert!(composer.delete_backward());
        assert_eq!(
            composer.draft(),
            "a ",
            "the accent and its base go together"
        );

        let mut composer = typed("x");
        composer.insert('\u{1F468}');
        composer.insert('\u{200D}');
        composer.insert('\u{1F469}');
        composer.insert('\u{200D}');
        composer.insert('\u{1F467}');
        assert!(composer.delete_backward());
        assert_eq!(composer.draft(), "x", "a joined sequence is one cluster");
    }

    #[test]
    fn deleting_an_empty_draft_changes_nothing() {
        let mut composer = Composer::default();

        assert!(
            !composer.delete_backward(),
            "an unchanged draft must not report a change, or it costs a repaint"
        );
        assert_eq!(composer.draft(), "");
    }

    /// COM-3: an empty submission is not a message.
    #[test]
    fn a_blank_draft_submits_nothing_and_is_left_alone() {
        let mut whitespace = typed("  \n\t ");

        assert!(whitespace.is_blank());
        assert_eq!(whitespace.take_draft(), None);
        assert_eq!(
            whitespace.draft(),
            "  \n\t ",
            "a refused submission must not silently discard what was typed"
        );
    }

    #[test]
    fn taking_the_draft_returns_it_exactly_and_clears_it() {
        let mut composer = typed(" hello \n world ");

        assert_eq!(composer.take_draft().as_deref(), Some(" hello \n world "));
        assert_eq!(composer.draft(), "");
        assert_eq!(composer.take_draft(), None, "and it is gone");
    }

    #[test]
    fn the_composer_shows_the_newest_lines_and_stops_growing() {
        let composer = typed("one\ntwo\nthree\nfour");

        assert_eq!(
            composer.visible_rows(40),
            ["two", "three", "four"],
            "the cursor is on the last row, so that is the row that must stay visible"
        );
        assert_eq!(composer.requested_rows(40), MAX_VISIBLE_LINES + 2);
        assert_eq!(
            typed("").requested_rows(40),
            3,
            "an empty composer is still a place to type"
        );
        assert_eq!(typed("one\ntwo").requested_rows(40), 4);
    }

    /// COM-1: the composer measures the rows it will paint, not the newlines the user typed.
    ///
    /// Every panel wraps, so a draft with no newline in it still occupies several rows. Measuring
    /// `'\n'` alone made the composer ask for one row, paint the wrapped tail of three, and place
    /// the caret against a line the user could not see.
    #[test]
    fn a_draft_with_no_newline_still_occupies_the_rows_it_wraps_into() {
        let composer = typed("aaaa bbbb cccc dddd");

        assert_eq!(
            composer.visible_rows(5),
            ["bbbb ", "cccc ", "dddd"],
            "four rows at this width, and the composer shows the newest three of them"
        );
        assert_eq!(composer.requested_rows(5), MAX_VISIBLE_LINES + 2);
        assert_eq!(
            composer.visible_rows(10),
            ["aaaa bbbb ", "cccc dddd"],
            "it fills a row before it breaks, rather than breaking at every space"
        );
        assert_eq!(
            composer.requested_rows(40),
            3,
            "and the same draft is one row where the terminal is wide enough to hold it"
        );
    }

    /// A word longer than the row has nowhere to break, so it breaks anyway rather than overflowing.
    #[test]
    fn a_word_longer_than_the_row_is_broken_by_display_width() {
        assert_eq!(typed("abcdefgh").visible_rows(3), ["abc", "def", "gh"]);
        // Two cells each, so three of them are what fits in a row six wide.
        assert_eq!(typed("汉字汉字").visible_rows(6), ["汉字汉", "字"]);
        assert_eq!(
            typed("e\u{301}e\u{301}e\u{301}").visible_rows(2),
            ["e\u{301}e\u{301}", "e\u{301}"],
            "a combining mark is part of the cluster before it and costs no cell of its own"
        );
    }
}
