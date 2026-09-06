//! Deferred geometry and the immutable maps belonging to one successfully painted frame.

use std::sync::Arc;

use super::*;
use crate::{
    preparation::{Key, PreparedText, Refusal},
    text_layout::Layout,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HeightOrigin {
    Literal,
    Estimated,
    Prepared,
}

#[derive(Debug)]
pub(super) struct PaintedEntry {
    pub(super) key: Key,
    pub(super) surface: SurfaceId,
    pub(super) index: usize,
    pub(super) start: usize,
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
    prepared: Option<&Result<Arc<Layout>, Refusal>>,
    previous: Option<&Measured>,
) -> (usize, usize, HeightOrigin) {
    if matches!(item, TranscriptEntryView::Text(text) if text.source.len() > 192 * 1024)
        && prepared.is_none()
    {
        return (2, 2, HeightOrigin::Estimated);
    }
    let rich = matches!(item, TranscriptEntryView::Text(text)
        if text.role == plexmaton_core::TranscriptRole::Assistant
            && text.kind == crate::TranscriptTextKind::Message
            && crate::markdown::may_format(&text.source));
    let compact = if rich {
        2
    } else {
        wrap_rows(item, palette, width, false)
    };
    match prepared {
        Some(Ok(layout)) => (
            if open { compact } else { layout.lines.len() },
            layout.lines.len(),
            HeightOrigin::Prepared,
        ),
        Some(Err(_)) => (2, 2, HeightOrigin::Prepared),
        None if open || rich => {
            // A provisional height is never mistaken for prepared geometry. Retaining the old
            // height across a stream delta avoids collapsing a still-visible entry to two rows.
            let rows = previous
                .filter(|old| &old.id == item.id())
                .map_or(2, |old| old.body_rows);
            (
                if open { compact } else { rows },
                rows,
                HeightOrigin::Estimated,
            )
        }
        None if compact > crate::markdown::MAX_LINES + 1 => (2, 2, HeightOrigin::Estimated),
        None => (compact, compact, HeightOrigin::Literal),
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
                && inside < entry.layout.rows.len())
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
        index: usize,
    ) -> Vec<Line<'static>> {
        let Some(agent) = state.agent_shown_by(surface) else {
            return Vec::new();
        };
        let appearance = state.entry_appearance(
            surface,
            &agent,
            item.id(),
            state.selected_in(surface, &agent).contains(index),
        );
        let prepared = self.layouts.mapped(&agent, item, window.width, appearance);
        let measured = &self.items(&agent, window.width)[index];
        let rows = measured.body_rows;
        let start = self.items(&agent, window.width)[..index]
            .iter()
            .map(|item| item.rows)
            .sum::<usize>()
            + measured.leading_rows;
        let failure = match prepared {
            Some(Ok(layout)) => {
                let key =
                    Key::new(&agent, item, window.width, appearance.open).with_math(self.math());
                let bytes =
                    size_of::<PaintedEntry>() + key.allocation_bytes() + layout.allocation_bytes();
                if self.drawing_text.entries.len() < 128
                    && self.drawing_text.bytes + bytes <= 4 * 1024 * 1024
                {
                    let lines = if let Some(range) =
                        state.selected_text_range(surface, &agent, index, layout.text.len())
                    {
                        layout.highlighted_lines(
                            range,
                            palette,
                            palette.style(crate::Role::Selection),
                        )
                    } else {
                        layout.painted_entry(palette, appearance)
                    };
                    self.drawing_text.entries.push(PaintedEntry {
                        key,
                        surface,
                        index,
                        start,
                        layout,
                    });
                    self.drawing_text.bytes += bytes;
                    return lines;
                }
                Some(Refusal::Capacity)
            }
            Some(Err(reason)) => Some(reason),
            None => None,
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
        let visible = window
            .skip_rows
            .saturating_sub(
                start.saturating_sub(
                    self.items(&agent, window.width)[..window.items.start]
                        .iter()
                        .map(|item| item.rows)
                        .sum::<usize>(),
                ),
            )
            .min(rows.saturating_sub(1));
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
