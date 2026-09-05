//! Text ranges are offsets into the visible-text projection, never into decorated terminal cells.
use super::*;
use crate::{Palette, content, text_layout::Layout};
use plexmaton_core::TranscriptItemId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TextPoint {
    pub index: usize,
    pub item: TranscriptItemId,
    // An exact prefix validates that streaming reinterpretation did not move this endpoint.
    // Pure append keeps it valid; each endpoint is bounded by one producer-bounded entry.
    before: String,
}

impl TextPoint {
    pub fn new(index: usize, item: TranscriptItemId, offset: usize, text: &str) -> Option<Self> {
        Some(Self {
            index,
            item,
            before: text.get(..offset)?.to_owned(),
        })
    }
    pub fn offset(&self) -> usize {
        self.before.len()
    }
    pub fn matches(&self, item: &TranscriptEntryView, layout: &Layout) -> bool {
        &self.item == item.id() && layout.text.starts_with(&self.before)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TextRange {
    pub anchor: TextPoint,
    pub focus: TextPoint,
    pub width: u16,
}

impl TextRange {
    fn ordered(&self) -> (&TextPoint, &TextPoint) {
        if (self.anchor.index, self.anchor.offset()) <= (self.focus.index, self.focus.offset()) {
            (&self.anchor, &self.focus)
        } else {
            (&self.focus, &self.anchor)
        }
    }

    fn in_entry(&self, index: usize, length: usize) -> Option<std::ops::Range<usize>> {
        let (first, last) = self.ordered();
        if index < first.index || index > last.index {
            return None;
        }
        let start = if index == first.index {
            first.offset()
        } else {
            0
        };
        let end = if index == last.index {
            last.offset()
        } else {
            length
        };
        (start < end && end <= length).then_some(start..end)
    }
}

impl ViewState {
    pub(crate) fn begin_text_selection(
        &mut self,
        surface: SurfaceId,
        agent: AgentId,
        anchor: TextPoint,
        width: u16,
    ) {
        self.selection = Some(Selection {
            surface,
            agent,
            range: Range::Text(TextRange {
                focus: anchor.clone(),
                anchor,
                width,
            }),
        });
        self.touch();
    }

    pub(crate) fn extend_text_selection(
        &mut self,
        surface: SurfaceId,
        agent: &AgentId,
        focus: TextPoint,
    ) -> bool {
        let Some(selection) = &mut self.selection else {
            return false;
        };
        if selection.surface != surface || &selection.agent != agent {
            return false;
        }
        let Range::Text(range) = &mut selection.range else {
            return false;
        };
        if range.focus == focus {
            return false;
        }
        range.focus = focus;
        self.touch();
        true
    }

    pub(crate) fn text_selection_points(
        &self,
    ) -> Option<(SurfaceId, &AgentId, [&TextPoint; 2], u16)> {
        let selection = self.selection.as_ref()?;
        let Range::Text(range) = &selection.range else {
            return None;
        };
        Some((
            selection.surface,
            &selection.agent,
            [&range.anchor, &range.focus],
            range.width,
        ))
    }

    pub(crate) fn selected_text_range(
        &self,
        surface: SurfaceId,
        agent: &AgentId,
        index: usize,
        length: usize,
    ) -> Option<std::ops::Range<usize>> {
        let selection = self.selection.as_ref()?;
        if selection.surface != surface || &selection.agent != agent {
            return None;
        }
        let Range::Text(range) = &selection.range else {
            return None;
        };
        range.in_entry(index, length)
    }

    pub(super) fn copy_text_range(
        &self,
        agent: &AgentView,
        range: &TextRange,
    ) -> Option<CopyRequest> {
        let (first, last) = range.ordered();
        let mut parts = Vec::new();
        for (index, entry) in agent
            .entries()
            .enumerate()
            .skip(first.index)
            .take(last.index - first.index + 1)
        {
            let appearance = super::super::EntryAppearance {
                open: self.disclosure.is_open(entry.id()),
                ..Default::default()
            };
            let layout =
                content::transcript_layout(entry, &Palette::monochrome(), appearance, range.width);
            if index == first.index && !first.matches(entry, &layout)
                || index == last.index && !last.matches(entry, &layout)
            {
                return None;
            }
            if let Some(selected) = range.in_entry(index, layout.text.len()) {
                parts.push(layout.text.get(selected)?.to_owned());
            }
        }
        (!parts.is_empty()).then(|| CopyRequest {
            entries: parts.len(),
            text: parts.join("\n\n"),
        })
    }
}
