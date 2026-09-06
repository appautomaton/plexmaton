//! A bounded LRU of palette-independent prepared layouts; heights outlive row eviction.
use super::*;
use crate::{
    TranscriptEntryView,
    preparation::{Key, PreparedText, Refusal},
    state::EntryAppearance,
};
use plexmaton_core::AgentId;
use std::{collections::VecDeque, sync::Arc};

const MAX_ENTRIES: usize = 128;
const MAX_BYTES: usize = 4 * 1024 * 1024;

struct Entry {
    key: Key,
    layout: Result<Arc<Layout>, Refusal>,
    bytes: usize,
}

#[derive(Default)]
pub(crate) struct Cache {
    pub(crate) math: crate::math::MathPresentation,
    entries: VecDeque<Entry>,
    bytes: usize,
    layouts: usize,
    needed: Vec<Key>,
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
    pub(crate) fn with_math(math: crate::math::MathPresentation) -> Self {
        Self {
            math,
            ..Self::default()
        }
    }
    pub(crate) const fn layouts(&self) -> usize {
        self.layouts
    }

    pub(crate) fn retain_widths(&mut self, agent: &AgentId, widths: &[u16]) {
        self.entries
            .retain(|entry| &entry.key.agent != agent || widths.contains(&entry.key.width));
        self.bytes = self.entries.iter().map(|entry| entry.bytes).sum();
    }

    pub(crate) fn begin_frame(&mut self) {
        self.needed.clear();
    }

    pub(crate) fn needed(&self) -> &[Key] {
        &self.needed
    }

    pub(crate) fn get(&self, key: &Key) -> Option<Result<Arc<Layout>, Refusal>> {
        self.entries
            .iter()
            .find(|entry| &entry.key == key)
            .map(|entry| entry.layout.clone())
    }

    pub(crate) fn for_entry(
        &self,
        agent: &AgentId,
        item: &TranscriptEntryView,
        width: u16,
        open: bool,
    ) -> Option<Result<Arc<Layout>, Refusal>> {
        if !Key::fits(agent, item) {
            return Some(Err(Refusal::Capacity));
        }
        self.entries
            .iter()
            .find(|entry| {
                entry.key.math == self.math && entry.key.matches(agent, item, width, open)
            })
            .map(|entry| entry.layout.clone())
    }

    pub(crate) fn for_source(&self, key: &Key) -> Option<Result<Arc<Layout>, Refusal>> {
        self.entries
            .iter()
            .find(|entry| {
                entry.key.agent == key.agent
                    && entry.key.item == key.item
                    && entry.key.revision == key.revision
                    && entry.key.open == key.open
            })
            .map(|entry| entry.layout.clone())
    }

    /// A reached miss is a declarative need, never permission to run a parser in a frame.
    pub(crate) fn mapped(
        &mut self,
        agent: &AgentId,
        item: &TranscriptEntryView,
        width: u16,
        appearance: EntryAppearance,
    ) -> Option<Result<Arc<Layout>, Refusal>> {
        if !Key::fits(agent, item) {
            return Some(Err(Refusal::Capacity));
        }
        let key = Key::new(agent, item, width, appearance.open).with_math(self.math);
        if let Some(index) = self.entries.iter().position(|entry| entry.key == key) {
            let entry = self.entries.remove(index)?;
            let result = entry.layout.clone();
            self.entries.push_front(entry);
            return Some(result);
        }
        if self.needed.len() < MAX_ENTRIES && !self.needed.contains(&key) {
            self.needed.push(key);
        }
        None
    }

    pub(crate) fn insert(&mut self, prepared: PreparedText) {
        let (key, layout) = prepared.into_parts();
        let bytes = size_of::<Entry>()
            + key.allocation_bytes()
            + layout.as_ref().map_or(0, |layout| {
                layout.allocation_bytes() + size_of::<Layout>() + 2 * size_of::<usize>()
            });
        if bytes > MAX_BYTES {
            return;
        }
        // A source revision replaces every older revision, but independent live widths coexist.
        self.entries.retain(|entry| {
            entry.key != key
                && (entry.key.agent != key.agent
                    || entry.key.item != key.item
                    || entry.key.revision == key.revision)
        });
        self.bytes = self.entries.iter().map(|entry| entry.bytes).sum();
        while self.entries.len() >= MAX_ENTRIES || self.bytes + bytes > MAX_BYTES {
            let Some(oldest) = self.entries.pop_back() else {
                break;
            };
            self.bytes -= oldest.bytes;
        }
        self.entries.push_front(Entry {
            key,
            layout: layout.map(Arc::new),
            bytes,
        });
        self.bytes += bytes;
        self.layouts = self.layouts.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Palette, TranscriptItemView, TranscriptTextKind, preparation::Request};
    use plexmaton_core::{TranscriptItemId, TranscriptRole};

    /// PRE-1/MD-4: identity allocation is bounded before a frame can retain request keys.
    #[test]
    fn oversized_preparation_identity_never_enters_the_cache_or_request_queue() {
        let agent = AgentId::new("agent").expect("agent");
        let item = TranscriptEntryView::Text(TranscriptItemView {
            id: TranscriptItemId::new("x".repeat(crate::preparation::MAX_KEY_BYTES))
                .expect("semantic ID"),
            role: TranscriptRole::Assistant,
            kind: TranscriptTextKind::Message,
            source: "**bounded source**".into(),
            revision: 1,
            finalized: true,
        });
        let mut cache = Cache::default();
        assert!(matches!(
            cache.mapped(&agent, &item, 80, EntryAppearance::default()),
            Some(Err(Refusal::Capacity))
        ));
        assert!(cache.entries.is_empty() && cache.needed.is_empty());
        assert_eq!(cache.bytes, 0);
        assert_eq!(
            Request::new(agent, item, 80, false).prepare().refusal(),
            Some(Refusal::Capacity)
        );
    }

    fn prepared(
        cache: &mut Cache,
        agent: &AgentId,
        item: &TranscriptEntryView,
        width: u16,
    ) -> Result<Arc<Layout>, Refusal> {
        if cache
            .mapped(agent, item, width, EntryAppearance::default())
            .is_none()
        {
            cache.insert(Request::new(agent.clone(), item.clone(), width, false).prepare());
        }
        cache
            .mapped(agent, item, width, EntryAppearance::default())
            .expect("prepared fixture")
    }

    /// MD-4/MD-5: theme changes resolve new colors from the same retained geometry and style intent.
    #[test]
    fn markdown_theme_change_reuses_prepared_rows_and_resolves_current_colors() {
        let agent = AgentId::new("primary").expect("agent");
        let item = TranscriptEntryView::Text(TranscriptItemView {
            id: TranscriptItemId::new("styled-heading").expect("item"),
            source: "# Heading".into(),
            role: TranscriptRole::Assistant,
            kind: TranscriptTextKind::Message,
            revision: 1,
            finalized: true,
        });
        let mut cache = Cache::default();
        let base = Palette::ansi();
        let proposed = base.with_markdown_theme(crate::MarkdownTheme::Pastel);
        let plain = prepared(&mut cache, &agent, &item, 60)
            .expect("base")
            .painted_lines(&base)[0]
            .spans[0]
            .style;
        let colored = prepared(&mut cache, &agent, &item, 60)
            .expect("colored")
            .painted_lines(&proposed)[0]
            .spans[0]
            .style;
        assert_ne!(plain.fg, colored.fg);
        assert_eq!(cache.layouts(), 1);
        prepared(&mut cache, &agent, &item, 60).expect("hit");
        assert_eq!(cache.layouts(), 1);
    }

    /// MD-4: the byte limit evicts a real entry before the count limit can intervene.
    #[test]
    fn markdown_cache_byte_pressure_evicts_and_rebuilds_the_lru() {
        let agent = AgentId::new("primary").expect("agent");
        let item = |index| {
            TranscriptEntryView::Text(TranscriptItemView {
                id: TranscriptItemId::new(format!("large-{index}")).expect("item"),
                source: "**bounded** ".repeat(1000),
                role: TranscriptRole::Assistant,
                kind: TranscriptTextKind::Message,
                revision: 1,
                finalized: false,
            })
        };
        let first = item(0);
        let mut cache = Cache::default();
        prepared(&mut cache, &agent, &first, 60).expect("layout");
        assert_eq!(
            cache.entries.len(),
            1,
            "fixture must be individually cacheable"
        );
        let entry_bytes = cache.bytes;
        assert!(
            entry_bytes * MAX_ENTRIES > MAX_BYTES,
            "fixture must hit byte limit first"
        );
        let mut evicted = false;
        for index in 1..MAX_ENTRIES {
            prepared(&mut cache, &agent, &item(index), 60).expect("layout");
            assert!(cache.bytes <= MAX_BYTES);
            if !cache
                .entries
                .iter()
                .any(|entry| &entry.key.item == first.id())
            {
                evicted = true;
                assert!(cache.entries.len() < MAX_ENTRIES);
                break;
            }
        }
        assert!(evicted, "byte pressure never evicted the oldest entry");
        let before = cache.layouts();
        prepared(&mut cache, &agent, &first, 60).expect("rebuild");
        assert_eq!(cache.layouts(), before + 1);
        assert_eq!(&cache.entries[0].key.item, first.id());
        prepared(&mut cache, &agent, &first, 60).expect("hit");
        assert_eq!(cache.layouts(), before + 1);
    }

    /// MD-4: history and streaming revisions cannot grow the layout LRU past either bound.
    #[test]
    fn markdown_cache_bounds_entries_bytes_and_replaces_streamed_revisions() {
        let agent = AgentId::new("primary").expect("agent");
        let mut cache = Cache::default();
        for index in 0..200 {
            let item = TranscriptEntryView::Text(TranscriptItemView {
                id: TranscriptItemId::new(format!("message-{index}")).expect("item"),
                source: "**bounded** ".repeat(if index < 150 { 1 } else { 1000 }),
                role: TranscriptRole::Assistant,
                kind: TranscriptTextKind::Message,
                revision: 1,
                finalized: false,
            });
            prepared(&mut cache, &agent, &item, 60).expect("Markdown");
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
        prepared(&mut cache, &agent, &item, 60).expect("prepared");
        let before = cache.layouts();
        prepared(&mut cache, &agent, &item, 60).expect("prepared");
        assert_eq!(cache.layouts(), before);
        if let TranscriptEntryView::Text(text) = &mut item {
            text.revision += 1;
            text.source.push_str(" tail");
        }
        prepared(&mut cache, &agent, &item, 60).expect("prepared");
        assert_eq!(
            cache
                .entries
                .iter()
                .filter(|entry| entry.key.item.as_str() == "stream")
                .count(),
            1
        );
        cache.retain_widths(&agent, &[80]);
        assert!(cache.entries.is_empty());
        assert_eq!(cache.bytes, 0);
    }
}
