//! TR-6: group separators belong to composition, never prepared source or hover paint.

use crate::TranscriptEntryView;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Spacing {
    pub(crate) before_feedback: usize,
    pub(crate) after_feedback: usize,
}

impl Spacing {
    pub(crate) fn between(
        item: &TranscriptEntryView,
        next: Option<&TranscriptEntryView>,
        has_after_feedback: bool,
        width: u16,
    ) -> Self {
        if width == 0 {
            return Self::default();
        }
        match (item, next) {
            (TranscriptEntryView::Text(_), _) => Self {
                before_feedback: 1,
                after_feedback: 0,
            },
            (TranscriptEntryView::Tool(_), Some(TranscriptEntryView::Text(_)))
                if !has_after_feedback =>
            {
                Self {
                    before_feedback: 0,
                    after_feedback: 1,
                }
            }
            _ => Self::default(),
        }
    }

    pub(crate) const fn rows(self) -> usize {
        self.before_feedback + self.after_feedback
    }
}
