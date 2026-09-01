//! The workspace's single text input.

use unicode_segmentation::UnicodeSegmentation;

/// Lines of draft the composer will show before it starts showing only the tail.
///
/// A bounded tail rather than a scrollable region, the same shape the notice strip uses. The
/// composer becomes a viewport's caller in step 4; until then the newest line is the one that
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

    /// Lines of the draft, newest last, bounded to what the composer shows.
    pub fn visible_lines(&self) -> impl Iterator<Item = &str> {
        let lines: Vec<&str> = self.draft.split('\n').collect();
        let skip = lines
            .len()
            .saturating_sub(usize::from(MAX_VISIBLE_LINES))
            .min(lines.len());
        lines.into_iter().skip(skip)
    }

    /// Rows the composer asks layout for, borders included.
    #[must_use]
    pub fn requested_rows(&self) -> u16 {
        let lines = u16::try_from(self.draft.split('\n').count()).unwrap_or(MAX_VISIBLE_LINES);
        // Two borders, plus at least one line so an empty composer is still a place to type.
        lines.clamp(1, MAX_VISIBLE_LINES).saturating_add(2)
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

        let visible: Vec<_> = composer.visible_lines().collect();
        assert_eq!(
            visible,
            ["two", "three", "four"],
            "the cursor is on the last line, so that is the line that must stay visible"
        );
        assert_eq!(composer.requested_rows(), MAX_VISIBLE_LINES + 2);
        assert_eq!(
            typed("").requested_rows(),
            3,
            "an empty composer is still a place to type"
        );
        assert_eq!(typed("one\ntwo").requested_rows(), 4);
    }
}
