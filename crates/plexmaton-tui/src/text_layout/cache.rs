//! A bounded LRU of styled layouts; the independent height cache still owns scroll geometry.
use super::*;
use crate::{
    Palette,
    markdown::{MAX_LINES, MAX_SOURCE_BYTES, may_format},
};
use crate::{TranscriptEntryView, TranscriptTextKind, content, state::EntryAppearance};
use plexmaton_core::{AgentId, TranscriptItemId, TranscriptRole};
use std::{borrow::Cow, collections::VecDeque};

const MAX_ENTRIES: usize = 128;
const MAX_BYTES: usize = 4 * 1024 * 1024;

struct Entry {
    agent: AgentId,
    item: TranscriptItemId,
    revision: u64,
    width: u16,
    palette: Palette,
    layout: Layout,
    open: bool,
    bytes: usize,
}

#[derive(Default)]
pub(crate) struct Cache {
    entries: VecDeque<Entry>,
    bytes: usize,
    layouts: usize,
}

impl std::fmt::Debug for Cache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextLayoutCache")
            .field("entries", &self.entries.len())
            .field("bytes", &self.bytes)
            .finish()
    }
}

impl Cache {
    pub(crate) const fn layouts(&self) -> usize {
        self.layouts
    }

    pub(crate) fn retain_widths(&mut self, agent: &AgentId, widths: &[u16], palette: &Palette) {
        self.entries.retain(|entry| {
            &entry.agent != agent || (widths.contains(&entry.width) && entry.palette == *palette)
        });
        self.bytes = self.entries.iter().map(|entry| entry.bytes).sum();
    }

    pub(crate) fn layout(
        &mut self,
        agent: &AgentId,
        item: &TranscriptEntryView,
        palette: &Palette,
        width: u16,
    ) -> Option<Cow<'_, Layout>> {
        let TranscriptEntryView::Text(text) = item else {
            return None;
        };
        if text.role != TranscriptRole::Assistant
            || text.kind != TranscriptTextKind::Message
            || text.source.len() > MAX_SOURCE_BYTES
            || !may_format(&text.source)
            || width == 0
        {
            return None;
        }
        Some(self.mapped(agent, item, palette, width, EntryAppearance::default()))
    }

    pub(crate) fn mapped(
        &mut self,
        agent: &AgentId,
        item: &TranscriptEntryView,
        palette: &Palette,
        width: u16,
        appearance: EntryAppearance,
    ) -> Cow<'_, Layout> {
        if let Some(index) = self.entries.iter().position(|entry| {
            &entry.agent == agent
                && &entry.item == item.id()
                && entry.revision == item.revision()
                && entry.width == width
                && entry.palette == *palette
                && entry.open == appearance.open
        }) && let Some(entry) = self.entries.remove(index)
        {
            self.entries.push_front(entry);
            return Cow::Borrowed(&self.entries[0].layout);
        }
        self.layouts += 1;
        let layout = content::transcript_layout(
            item,
            palette,
            EntryAppearance {
                open: appearance.open,
                ..EntryAppearance::default()
            },
            width,
        );
        let rows = &layout.lines;
        let bytes = size_of::<Entry>()
            + agent.as_str().len()
            + item.id().as_str().len()
            + layout.text.capacity()
            + layout.rows.capacity() * size_of::<Vec<Fragment>>()
            + layout
                .rows
                .iter()
                .map(|row| row.capacity() * size_of::<Fragment>())
                .sum::<usize>()
            + rows.capacity() * size_of::<Line<'static>>()
            + rows
                .iter()
                .map(|line| {
                    line.spans.capacity() * size_of::<Span<'static>>()
                        + line
                            .spans
                            .iter()
                            .map(|span| match &span.content {
                                Cow::Owned(text) => text.capacity(),
                                Cow::Borrowed(_) => 0,
                            })
                            .sum::<usize>()
                })
                .sum::<usize>();
        // Remove old revisions instead of letting a streaming message occupy the whole LRU.
        self.entries.retain(|entry| {
            &entry.agent != agent || &entry.item != item.id() || entry.revision == item.revision()
        });
        self.bytes = self.entries.iter().map(|entry| entry.bytes).sum();
        if bytes <= MAX_BYTES && rows.len() <= MAX_LINES + 1 {
            while self.entries.len() >= MAX_ENTRIES || self.bytes + bytes > MAX_BYTES {
                let Some(oldest) = self.entries.pop_back() else {
                    break;
                };
                self.bytes -= oldest.bytes;
            }
            self.entries.push_front(Entry {
                agent: agent.clone(),
                item: item.id().clone(),
                revision: item.revision(),
                width,
                palette: *palette,
                layout,
                open: appearance.open,
                bytes,
            });
            self.bytes += bytes;
            return Cow::Borrowed(&self.entries[0].layout);
        }
        Cow::Owned(layout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TranscriptItemView;

    /// MD-4: history and streaming revisions cannot grow the layout LRU past either bound.
    #[test]
    fn markdown_cache_bounds_entries_bytes_and_replaces_streamed_revisions() {
        let agent = AgentId::new("primary").expect("agent");
        let mut cache = Cache::default();
        let palette = Palette::pastel();
        for index in 0..200 {
            let item = TranscriptEntryView::Text(TranscriptItemView {
                id: TranscriptItemId::new(format!("message-{index}")).expect("item"),
                source: "**bounded** ".repeat(if index < 150 { 1 } else { 1000 }),
                role: TranscriptRole::Assistant,
                kind: TranscriptTextKind::Message,
                revision: 1,
                finalized: false,
            });
            cache.layout(&agent, &item, &palette, 60).expect("Markdown");
            assert!(cache.entries.len() <= MAX_ENTRIES && cache.bytes <= MAX_BYTES);
        }
        let mut item = TranscriptEntryView::Text(TranscriptItemView {
            id: TranscriptItemId::new("stream").expect("id"),
            source: "**live**".into(),
            role: TranscriptRole::Assistant,
            kind: TranscriptTextKind::Message,
            revision: 1,
            finalized: false,
        });
        cache.layout(&agent, &item, &palette, 60);
        let before = cache.layouts();
        cache.layout(&agent, &item, &palette, 60);
        assert_eq!(cache.layouts(), before);
        if let TranscriptEntryView::Text(text) = &mut item {
            text.revision += 1;
            text.source.push_str(" tail");
        }
        cache.layout(&agent, &item, &palette, 60);
        assert_eq!(
            cache
                .entries
                .iter()
                .filter(|entry| entry.item.as_str() == "stream")
                .count(),
            1
        );
        cache.retain_widths(&agent, &[80], &palette);
        assert!(cache.entries.is_empty());
        assert_eq!(cache.bytes, 0);
    }
}
