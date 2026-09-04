//! Source-based selection owned by one editable input.

use super::TextInput;
use std::ops::Range;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Selection {
    Dragging { anchor: usize },
    Selected { anchor: usize },
}

impl TextInput {
    pub(crate) fn begin_selection(&mut self) {
        self.selection = Some(Selection::Dragging {
            anchor: self.cursor,
        });
    }

    pub(crate) fn is_dragging(&self) -> bool {
        matches!(self.selection, Some(Selection::Dragging { .. }))
    }

    pub(crate) fn drag_to(&mut self, width: u16, row: u16, column: u16) {
        self.drag_to_offset(self.offset_at(width, row, column));
    }

    pub(crate) fn drag_to_offset(&mut self, offset: usize) {
        if self.is_dragging() {
            self.cursor = offset.min(self.text.len());
            self.settle_cursor();
        }
    }

    pub(crate) fn finish_selection(&mut self) -> Option<String> {
        let Some(Selection::Dragging { anchor }) = self.selection else {
            return None;
        };
        let copied = self.selected_text().map(str::to_owned);
        self.selection = copied.as_ref().map(|_| Selection::Selected { anchor });
        copied
    }

    pub(crate) fn selected_range(&self) -> Option<Range<usize>> {
        let anchor = match self.selection? {
            Selection::Dragging { anchor } | Selection::Selected { anchor } => anchor,
        };
        (anchor != self.cursor).then_some(anchor.min(self.cursor)..anchor.max(self.cursor))
    }

    pub(crate) fn selected_text(&self) -> Option<&str> {
        self.selected_range().map(|range| &self.text[range])
    }

    pub(crate) fn clear_selection(&mut self) -> bool {
        self.selection.take().is_some()
    }

    pub(super) fn delete_selection(&mut self) -> bool {
        let selected = self.selected_range();
        self.selection = None;
        let Some(range) = selected else { return false };
        self.cursor = range.start;
        self.text.replace_range(range, "");
        self.settle_cursor();
        true
    }

    pub(crate) fn visible_ranges(&self, width: u16) -> Vec<Range<usize>> {
        let rows = self.rows(width);
        let start = Self::window_start(&rows, self.caret_row(&rows));
        rows.into_iter()
            .skip(start)
            .take(usize::from(super::MAX_VISIBLE_LINES))
            .map(|row| row.start..row.start + row.text.len())
            .collect()
    }
}
