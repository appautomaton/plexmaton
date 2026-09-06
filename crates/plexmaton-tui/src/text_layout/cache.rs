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

#[derive(Debug)]
pub(crate) struct PreparedEntry {
    pub(crate) key: Key,
    pub(crate) layout: Result<Arc<Layout>, Refusal>,
    checkpoint: Option<crate::markdown::PrefixCheckpoint>,
    bytes: usize,
}

#[derive(Default)]
pub(crate) struct Cache {
    pub(crate) math: crate::math::MathPresentation,
    entries: VecDeque<PreparedEntry>,
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
    ) -> Result<Option<&PreparedEntry>, Refusal> {
        if !Key::fits(agent, item) {
            return Err(Refusal::Capacity);
        }
        Ok(self
            .presentation_index(agent, item, width, open)
            .map(|index| &self.entries[index]))
    }

    /// Only append-only text can reuse an older prefix. Other entry kinds contain current facts,
    /// and every cached refusal applies only to its exact requested revision (MD-4/ENT-2).
    fn presentation_index(
        &self,
        agent: &AgentId,
        item: &TranscriptEntryView,
        width: u16,
        open: bool,
    ) -> Option<usize> {
        let append_only = matches!(item, TranscriptEntryView::Text(_));
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                &entry.key.agent == agent
                    && &entry.key.item == item.id()
                    && entry.key.width == width
                    && entry.key.open == open
                    && entry.key.math == self.math
                    && (entry.key.revision == item.revision()
                        || (append_only
                            && entry.layout.is_ok()
                            && entry.key.revision < item.revision()))
            })
            .max_by_key(|(_, entry)| entry.key.revision)
            .map(|(index, _)| index)
    }

    pub(crate) fn for_source(&self, key: &Key) -> Option<Result<Arc<Layout>, Refusal>> {
        self.entries
            .iter()
            .find(|entry| {
                entry.key.agent == key.agent
                    && entry.key.item == key.item
                    && entry.key.revision == key.revision
                    && entry.key.open == key.open
                    && entry.key.math == key.math
            })
            .map(|entry| entry.layout.clone())
    }

    /// Build a bounded worker hint from the newest successful older Markdown revision. The full
    /// layout remains the cache's one retained presentation; only the row-aligned prefix clone is
    /// sent with the next request, so failed or evicted hints simply take the canonical path.
    /// Preflight a hint while its source/layout are still borrowed. A conservative full-layout
    /// bound avoids cloning retained presentation data when the workspace batch has no room.
    pub(crate) fn prefix_hint_with_budget(
        &self,
        agent: &AgentId,
        item: &TranscriptEntryView,
        width: u16,
        open: bool,
        budget: usize,
    ) -> Option<crate::markdown::PrefixHint> {
        let TranscriptEntryView::Text(text) = item else {
            return None;
        };
        if !matches!(
            (text.kind, text.role),
            (
                crate::TranscriptTextKind::Message,
                plexmaton_core::TranscriptRole::Assistant
            )
        ) || !crate::markdown::may_format(&text.source)
            || text.finalized
        {
            return None;
        }
        let entry = self
            .entries
            .iter()
            .filter(|entry| {
                &entry.key.agent == agent
                    && &entry.key.item == item.id()
                    && entry.key.width == width
                    && entry.key.open == open
                    && entry.key.math == self.math
                    && entry.key.revision < item.revision()
                    && entry.layout.is_ok()
                    && entry.checkpoint.is_some()
            })
            .max_by_key(|entry| entry.key.revision)?;
        let checkpoint = entry.checkpoint.as_ref()?;
        if !text.source.starts_with(checkpoint.source_prefix()) {
            return None;
        }
        let retained = entry.layout.as_ref().ok()?;
        let upper_bound = checkpoint
            .allocation_bytes()
            .saturating_add(retained.allocation_bytes())
            .saturating_add(size_of::<crate::markdown::PrefixHint>());
        if upper_bound > budget {
            return None;
        }
        let checkpoint = checkpoint.clone();
        let layout = retained.prefix(checkpoint.rows(), checkpoint.visible_text_bytes())?;
        crate::markdown::PrefixHint::new(checkpoint, layout)
            .filter(|hint| hint.allocation_bytes() <= budget)
    }

    /// A reached miss declares work. Lookup admission can refuse before allocating a key;
    /// retained results preserve identity for both successful rows and worker refusals.
    pub(crate) fn mapped(
        &mut self,
        agent: &AgentId,
        item: &TranscriptEntryView,
        width: u16,
        appearance: EntryAppearance,
    ) -> Result<Option<&PreparedEntry>, Refusal> {
        if !Key::fits(agent, item) {
            return Err(Refusal::Capacity);
        }
        let key = Key::new(agent, item, width, appearance.open).with_math(self.math);
        let index = self.presentation_index(agent, item, width, appearance.open);
        if index.is_none_or(|index| self.entries[index].key != key)
            && self.needed.len() < MAX_ENTRIES
            && !self.needed.contains(&key)
        {
            self.needed.push(key);
        }
        let Some(index) = index else {
            return Ok(None);
        };
        let entry = self
            .entries
            .remove(index)
            .expect("lookup returned a retained index");
        self.entries.push_front(entry);
        Ok(self.entries.front())
    }

    pub(crate) fn insert(&mut self, prepared: PreparedText) {
        // FR-4 counts admitted results delivered here, including refusals and defensive drops;
        // cache occupancy and whether this particular result survives retention are separate.
        self.layouts = self.layouts.saturating_add(1);
        let (key, layout, checkpoint) = prepared.into_parts();
        let bytes = size_of::<PreparedEntry>()
            + key.allocation_bytes()
            + checkpoint
                .as_ref()
                .map_or(0, crate::markdown::PrefixCheckpoint::allocation_bytes)
            + layout.as_ref().map_or(0, |layout| {
                layout.allocation_bytes() + size_of::<Layout>() + 2 * size_of::<usize>()
            });
        if bytes > MAX_BYTES {
            return;
        }
        // Retain the newest two versions of each geometry under the same global LRU bound.
        // A completion at another width must not erase the only usable rows at this width.
        let versions = || {
            self.entries
                .iter()
                .filter(|entry| entry.key.same_geometry(&key))
                .map(|entry| entry.key.revision)
                .chain(std::iter::once(key.revision))
        };
        let newest = versions().max().expect("the incoming version is present");
        let oldest = versions()
            .filter(|version| *version < newest)
            .max()
            .unwrap_or(newest);
        if key.revision < oldest {
            return;
        }
        self.entries.retain(|entry| {
            entry.key != key && (!entry.key.same_geometry(&key) || entry.key.revision >= oldest)
        });
        self.bytes = self.entries.iter().map(|entry| entry.bytes).sum();
        while self.entries.len() >= MAX_ENTRIES || self.bytes + bytes > MAX_BYTES {
            let Some(oldest) = self.entries.pop_back() else {
                break;
            };
            self.bytes -= oldest.bytes;
        }
        self.entries.push_front(PreparedEntry {
            key,
            layout: layout.map(Arc::new),
            checkpoint,
            bytes,
        });
        self.bytes += bytes;
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
            Err(Refusal::Capacity)
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
            .get(&Key::new(agent, item, width, false).with_math(cache.math))
            .is_none()
        {
            cache.insert(
                Request::new(agent.clone(), item.clone(), width, false)
                    .with_math(cache.math)
                    .prepare(),
            );
        }
        cache
            .mapped(agent, item, width, EntryAppearance::default())
            .expect("admitted fixture identity")
            .expect("prepared fixture")
            .layout
            .clone()
    }

    /// MD-4/PRE-1: an older Markdown presentation supplies real prefix work to the next worker
    /// request, while the merged result retains the canonical visible and atomic geometry.
    #[test]
    fn markdown_cache_supplies_a_bounded_frozen_prefix_to_preparation() {
        let agent = AgentId::new("primary").expect("agent");
        let item = |revision, source: &str| {
            TranscriptEntryView::Text(TranscriptItemView {
                id: TranscriptItemId::new("stream-prefix").expect("item"),
                source: source.into(),
                role: TranscriptRole::Assistant,
                kind: TranscriptTextKind::Message,
                revision,
                finalized: false,
            })
        };
        let first = item(1, "# Heading\n\n");
        let mut cache = Cache::default();
        cache.insert(Request::new(agent.clone(), first.clone(), 80, false).prepare());
        let next = item(2, "# Heading\n\nTail with $x$");
        assert!(
            cache
                .prefix_hint_with_budget(&agent, &next, 80, false, 0)
                .is_none(),
            "an optional hint is omitted when the batch has no remaining budget"
        );
        let hint = cache
            .prefix_hint_with_budget(&agent, &next, 80, false, usize::MAX)
            .expect("bounded prefix hint");
        let prepared = Request::new(agent.clone(), next.clone(), 80, false)
            .with_prefix(hint.clone())
            .prepare();
        assert!(prepared.reused_prefix());
        let canonical = Request::new(agent, next, 80, false).prepare();
        assert_eq!(prepared.result, canonical.result);

        let mut finalized = first.clone();
        if let TranscriptEntryView::Text(text) = &mut finalized {
            text.revision = 2;
            text.source.push_str("Tail with $x$");
            text.finalized = true;
        }
        let final_prepared = Request::new(
            AgentId::new("primary").expect("agent"),
            finalized,
            80,
            false,
        )
        .with_prefix(hint)
        .prepare();
        assert!(
            !final_prepared.reused_prefix(),
            "finalization uses canonical rendering"
        );
        assert!(
            final_prepared.checkpoint().is_none(),
            "finalized entries retain no streaming checkpoint"
        );
    }

    #[test]
    fn markdown_cache_advances_the_frozen_frontier_after_a_completed_tail_block() {
        let agent = AgentId::new("primary").expect("agent");
        let item = |revision, source: &str| {
            TranscriptEntryView::Text(TranscriptItemView {
                id: TranscriptItemId::new("advancing-prefix").expect("item"),
                source: source.into(),
                role: TranscriptRole::Assistant,
                kind: TranscriptTextKind::Message,
                revision,
                finalized: false,
            })
        };
        let mut cache = Cache::default();
        let first = item(1, "# Heading\n\nTail");
        cache.insert(Request::new(agent.clone(), first, 80, false).prepare());
        let second = item(2, "# Heading\n\nTail\n\nNext");
        let second_hint = cache
            .prefix_hint_with_budget(&agent, &second, 80, false, usize::MAX)
            .expect("heading prefix");
        let prepared = Request::new(agent.clone(), second.clone(), 80, false)
            .with_prefix(second_hint)
            .prepare();
        assert!(prepared.reused_prefix());
        assert_eq!(
            prepared
                .checkpoint()
                .expect("advanced checkpoint")
                .source_prefix(),
            "# Heading\n\nTail\n\n"
        );
        let canonical = Request::new(agent, second, 80, false).prepare();
        assert_eq!(prepared.result, canonical.result);
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
            2
        );
        cache.retain_widths(&agent, &[80]);
        assert!(cache.entries.is_empty());
        assert_eq!(cache.bytes, 0);
    }

    fn streamed(revision: u64) -> TranscriptEntryView {
        TranscriptEntryView::Text(TranscriptItemView {
            id: TranscriptItemId::new("stream").expect("item"),
            source: format!("**version {revision}**"),
            role: TranscriptRole::Assistant,
            kind: TranscriptTextKind::Message,
            revision,
            finalized: false,
        })
    }

    /// MD-4/FR-3: source freshness and LRU recency differ; another live width cannot evict the
    /// only compatible presentation, and painting an old version still requests the current one.
    #[test]
    fn pending_preparation_reuses_only_the_latest_compatible_revision() {
        let agent = AgentId::new("primary").expect("agent");
        let mut cache = Cache::default();
        for (revision, width) in [(1, 60), (1, 80), (2, 80), (3, 60)] {
            cache.insert(Request::new(agent.clone(), streamed(revision), width, false).prepare());
        }
        let item = streamed(2);
        let mapped = cache
            .mapped(&agent, &item, 60, EntryAppearance::default())
            .expect("compatible rows")
            .expect("successful old preparation");
        assert_eq!(
            mapped.key.revision, 1,
            "a future revision is not a fallback"
        );
        let mapped_key = mapped.key.clone();
        let mapped_layout = mapped.layout.as_ref().expect("prepared rows").clone();
        assert_eq!(mapped_layout.text, "version 1");
        assert_eq!(cache.needed(), &[Key::new(&agent, &item, 60, false)]);
        let measured = cache
            .for_entry(&agent, &item, 60, false)
            .expect("measured fallback")
            .expect("measured rows");
        assert_eq!(measured.key, mapped_key);
        assert!(Arc::ptr_eq(
            measured.layout.as_ref().expect("measured rows"),
            &mapped_layout
        ));

        cache.begin_frame();
        let mapped = cache
            .mapped(&agent, &streamed(4), 60, EntryAppearance::default())
            .expect("latest rows")
            .expect("successful preparation");
        assert_eq!(
            mapped.key.revision, 3,
            "touching revision 1 did not make it newest"
        );
        assert_eq!(cache.needed()[0].revision, 4);
        for revision in 4..20 {
            cache.insert(Request::new(agent.clone(), streamed(revision), 60, false).prepare());
            assert_eq!(
                cache
                    .entries
                    .iter()
                    .filter(|entry| entry.key.width == 60)
                    .count(),
                2
            );
            assert_eq!(
                cache
                    .entries
                    .iter()
                    .filter(|entry| entry.key.width == 80)
                    .count(),
                2
            );
            assert!(cache.bytes <= MAX_BYTES);
        }
    }

    /// MD-4/PRE-3: fallback never crosses an owner, geometry or capability, and an exact refusal
    /// is visible rather than silently replaced with old success or resubmitted on every paint.
    #[test]
    fn pending_preparation_preserves_geometry_boundaries_and_current_refusals() {
        let agent = AgentId::new("primary").expect("agent");
        for axis in 0..5 {
            let mut cache = Cache::default();
            let mut request = Request::new(agent.clone(), streamed(1), 60, false);
            match axis {
                0 => {
                    request = Request::new(
                        AgentId::new("other").expect("agent"),
                        streamed(1),
                        60,
                        false,
                    )
                }
                1 => {
                    let mut item = streamed(1);
                    if let TranscriptEntryView::Text(text) = &mut item {
                        text.id = TranscriptItemId::new("other").expect("item");
                    }
                    request = Request::new(agent.clone(), item, 60, false);
                }
                2 => request = Request::new(agent.clone(), streamed(1), 80, false),
                3 => request = Request::new(agent.clone(), streamed(1), 60, true),
                4 => request = request.with_math(crate::math::MathPresentation::Native),
                _ => unreachable!(),
            }
            cache.insert(request.prepare());
            assert!(
                matches!(
                    cache.mapped(&agent, &streamed(2), 60, EntryAppearance::default()),
                    Ok(None)
                ),
                "axis {axis}"
            );
        }
        let mut cache = Cache::default();
        cache.insert(Request::new(agent.clone(), streamed(1), 60, false).prepare());
        let key = Key::new(&agent, &streamed(2), 60, false);
        cache.insert(PreparedText::unavailable(key, Refusal::Unavailable));
        assert!(matches!(
            cache.mapped(&agent, &streamed(2), 60, EntryAppearance::default()),
            Ok(Some(PreparedEntry {
                layout: Err(Refusal::Unavailable),
                ..
            }))
        ));
        assert!(cache.needed().is_empty());
        assert_eq!(
            cache
                .mapped(&agent, &streamed(3), 60, EntryAppearance::default())
                .expect("old successful rows")
                .expect("success")
                .key
                .revision,
            1
        );
    }

    /// FR-4/MD-4: received preparation results are counted independently of whether retention
    /// keeps them; ignoring an older result cannot erase that completed work from the metric.
    #[test]
    fn preparation_result_count_includes_results_dropped_by_retention() {
        let agent = AgentId::new("primary").expect("agent");
        let mut cache = Cache::default();
        for revision in [3, 4, 1] {
            cache.insert(Request::new(agent.clone(), streamed(revision), 60, false).prepare());
        }
        assert_eq!(cache.layouts(), 3);
        assert_eq!(cache.entries.len(), 2);
        assert!(
            cache
                .get(&Key::new(&agent, &streamed(1), 60, false))
                .is_none()
        );
        let key = Key::new(&agent, &streamed(5), 60, false);
        cache.insert(PreparedText::unavailable(key.clone(), Refusal::Unavailable));
        let retained = cache
            .for_entry(&agent, &streamed(5), 60, false)
            .expect("admitted identity")
            .expect("retained refusal");
        assert_eq!(retained.key, key);
        assert!(matches!(retained.layout, Err(Refusal::Unavailable)));
        assert_eq!(cache.layouts(), 4);
    }
}
