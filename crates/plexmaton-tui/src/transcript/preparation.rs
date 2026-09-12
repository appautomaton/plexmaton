//! Deferred geometry and the immutable maps belonging to one successfully painted frame.

use std::sync::Arc;

use super::*;
use crate::{
    preparation::{Key, PreparedText, Refusal},
    text_layout::{Layout, PreparedEntry},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HeightOrigin {
    Literal,
    Estimated,
    Unadmitted,
    Prepared { revision: u64 },
}

impl HeightOrigin {
    pub(super) fn for_preparation(
        prepared: Result<Option<&PreparedEntry>, Refusal>,
    ) -> Option<Self> {
        match prepared {
            Ok(Some(prepared)) => Some(Self::Prepared {
                revision: prepared.key.revision,
            }),
            Ok(None) => None,
            Err(_) => Some(Self::Unadmitted),
        }
    }
}

pub(super) struct EntryPosition {
    pub(super) index: usize,
    pub(super) start: usize,
    pub(super) visible_from: usize,
}

#[derive(Debug)]
pub(super) struct PaintedEntry {
    pub(super) key: Key,
    pub(super) surface: SurfaceId,
    pub(super) index: usize,
    pub(super) start: usize,
    pub(super) separator_rows: usize,
    pub(super) layout: Arc<Layout>,
}

/// Pins have a separate hard bound from the LRU. A failed frame cannot evict the maps input uses.
#[derive(Debug, Default)]
pub(super) struct PaintedText {
    pub(super) entries: Vec<PaintedEntry>,
    bytes: usize,
}

pub(super) fn measure_entry(
    item: &TranscriptEntryView,
    palette: &Palette,
    width: u16,
    open: bool,
    prepared: Result<Option<&PreparedEntry>, Refusal>,
    previous: Option<&Measured>,
) -> (usize, usize, HeightOrigin) {
    let placeholder = if matches!(item, TranscriptEntryView::Text(_)) {
        1
    } else {
        2
    };
    let rich = matches!(item, TranscriptEntryView::Text(text)
        if text.role == plexmaton_core::TranscriptRole::Assistant
            && text.kind == crate::TranscriptTextKind::Message
            && crate::markdown::may_format(&text.source));
    match prepared {
        Ok(Some(prepared)) => {
            let origin = HeightOrigin::Prepared {
                revision: prepared.key.revision,
            };
            return match &prepared.layout {
                Ok(layout) => {
                    let rows = layout.lines.len();
                    let compact = if !open {
                        rows
                    } else if rich {
                        placeholder
                    } else {
                        wrap_rows(item, palette, width, false)
                    };
                    (compact, rows, origin)
                }
                Err(_) => (placeholder, placeholder, origin),
            };
        }
        Err(_) => return (placeholder, placeholder, HeightOrigin::Unadmitted),
        Ok(None) => {}
    }
    if matches!(item, TranscriptEntryView::Text(text) if text.source.len() > 192 * 1024) {
        return (placeholder, placeholder, HeightOrigin::Estimated);
    }
    let compact = if rich {
        placeholder
    } else {
        wrap_rows(item, palette, width, false)
    };
    if open || rich {
        // A provisional height is never mistaken for prepared geometry. Retaining the old
        // height across an evicted stream revision avoids collapsing its visible placeholder.
        let rows = previous
            .filter(|old| &old.id == item.id())
            .map_or(placeholder, |old| old.body_rows);
        (
            if open { compact } else { rows },
            rows,
            HeightOrigin::Estimated,
        )
    } else if compact > crate::markdown::MAX_LINES + 1 {
        (placeholder, placeholder, HeightOrigin::Estimated)
    } else {
        (compact, compact, HeightOrigin::Literal)
    }
}

impl TranscriptMetrics {
    pub(crate) fn with_math(math: crate::math::MathPresentation) -> Self {
        Self {
            layouts: crate::text_layout::Cache::with_math(math),
            ..Self::default()
        }
    }

    pub(crate) const fn math(&self) -> crate::math::MathPresentation {
        self.layouts.math
    }

    pub(crate) fn begin_frame(&mut self) {
        self.layouts.begin_frame();
        self.drawing_text = PaintedText::default();
    }

    pub(crate) fn commit_frame(&mut self) {
        self.painted_text = std::mem::take(&mut self.drawing_text);
    }

    pub(crate) fn preparation_needed(&self) -> &[Key] {
        self.layouts.needed()
    }

    pub(crate) fn prepared(&self, key: &Key) -> Option<Result<Arc<Layout>, Refusal>> {
        self.layouts.get(key)
    }

    pub(crate) fn prepared_source(&self, key: &Key) -> Option<Result<Arc<Layout>, Refusal>> {
        self.layouts.for_source(key)
    }

    pub(crate) fn prefix_hint_with_budget(
        &self,
        agent: &AgentId,
        item: &TranscriptEntryView,
        width: u16,
        open: bool,
        budget: usize,
    ) -> Option<crate::markdown::PrefixHint> {
        self.layouts
            .prefix_hint_with_budget(agent, item, width, open, budget)
    }

    /// Copy iterates the bounded pinned set, not every selected history member. The returned
    /// source identities belong to the last successful frame, including retained text prefixes.
    pub(crate) fn painted_sources<'a>(
        &'a self,
        surface: SurfaceId,
        agent: &'a AgentId,
        items: std::ops::RangeInclusive<usize>,
    ) -> impl Iterator<Item = (usize, &'a Key, &'a Layout)> + Clone {
        self.painted_text
            .entries
            .iter()
            .filter(move |entry| {
                entry.surface == surface
                    && &entry.key.agent == agent
                    && items.contains(&entry.index)
            })
            .map(|entry| (entry.index, &entry.key, entry.layout.as_ref()))
    }

    pub(crate) fn accept_prepared(&mut self, prepared: PreparedText) {
        self.layouts.insert(prepared);
    }

    /// Input resolves against pinned painted data even after a newer completion reaches the LRU.
    pub(crate) fn painted_entry(
        &self,
        surface: SurfaceId,
        agent: &AgentId,
        width: u16,
        row: usize,
    ) -> Option<(usize, &Key, usize, &Layout)> {
        self.painted_text.entries.iter().find_map(|entry| {
            let inside = row.checked_sub(entry.start)?;
            (entry.surface == surface
                && &entry.key.agent == agent
                && entry.key.width == width
                && inside < entry.layout.rows.len() + entry.separator_rows)
                .then_some((entry.index, &entry.key, inside, entry.layout.as_ref()))
        })
    }

    pub(super) fn paint_entry(
        &mut self,
        item: &TranscriptEntryView,
        palette: &Palette,
        window: &Window,
        state: &ViewState,
        surface: SurfaceId,
        position: EntryPosition,
    ) -> Vec<Line<'static>> {
        let Some(agent) = state.agent_shown_by(surface) else {
            return Vec::new();
        };
        let appearance = state.entry_appearance(
            surface,
            &agent,
            item.id(),
            state.selected_in(surface, &agent).contains(position.index),
        );
        let measured = &self.items(&agent, window.width)[position.index];
        let rows = measured.body_rows;
        let separator_rows = measured.spacing.rows();
        let prepared = self.layouts.mapped(&agent, item, window.width, appearance);
        let failure = match prepared {
            Ok(Some(PreparedEntry {
                key,
                layout: Ok(layout),
                ..
            })) => {
                let bytes =
                    size_of::<PaintedEntry>() + key.allocation_bytes() + layout.allocation_bytes();
                if self.drawing_text.entries.len() < 128
                    && self.drawing_text.bytes + bytes <= 4 * 1024 * 1024
                {
                    let lines = if let Some(range) = state.selected_text_range(
                        surface,
                        &agent,
                        position.index,
                        layout.text.len(),
                    ) {
                        layout.highlighted_lines(
                            range,
                            palette,
                            palette.style(crate::Role::Selection),
                        )
                    } else {
                        layout.painted_entry(palette, appearance)
                    };
                    self.drawing_text.entries.push(PaintedEntry {
                        key: key.clone(),
                        surface,
                        index: position.index,
                        start: position.start,
                        separator_rows,
                        layout: layout.clone(),
                    });
                    self.drawing_text.bytes += bytes;
                    return lines;
                }
                Some(Refusal::Capacity)
            }
            Ok(Some(PreparedEntry {
                layout: Err(reason),
                ..
            })) => Some(*reason),
            Err(reason) => Some(reason),
            Ok(None) => None,
        };
        let label = match failure {
            None => "Preparing text…",
            Some(Refusal::Capacity) => "Text preparation limit · Copy source",
            Some(Refusal::InvalidRequest | Refusal::Unavailable) => {
                "Text unavailable · Copy source"
            }
        };
        let mut lines = vec![Line::default(); rows];
        // Keep a known-height evicted entry's status visible even when its first row is clipped.
        let visible = position.visible_from.min(rows.saturating_sub(1));
        if let Some(line) = lines.get_mut(visible) {
            *line = Line::styled(
                label
                    .chars()
                    .take(usize::from(window.width))
                    .collect::<String>(),
                palette.style(if failure.is_some() {
                    crate::Role::Failure
                } else {
                    crate::Role::Muted
                }),
            );
        }
        lines
    }
}
