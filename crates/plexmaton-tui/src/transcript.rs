//! How tall a conversation is, which part of it a frame has to build, and where its reader is.
//!
//! Wrapping a whole history in order to paint twenty rows of it is the cost this module removes.
//! Heights are measured one item at a time and kept until that item's revision or the panel's width
//! changes (TR-1), and a frame builds lines only for the items its viewport reaches (TR-2). Because
//! those heights are also what turn an item into a row number, this is where a reading position is
//! resolved (TR-3). The contract is
//! [`specs/transcript-layout.md`](../../../.agents/specs/transcript-layout.md).

use std::{collections::BTreeMap, ops::Range};

use plexmaton_core::{AgentId, TranscriptItemId};
use ratatui::{
    text::Line,
    widgets::{Paragraph, Wrap},
};

use crate::{AgentView, TranscriptItemView, content, state::Selected, theme::Palette};

/// Where a reader is parked in one conversation.
///
/// An item and a row inside it, never a row into the whole history: how many rows precede an item
/// depends on the panel's width, so a stored row names different text at every terminal size
/// (TR-3).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TranscriptPosition {
    /// Following the newest line, wherever the conversation ends up.
    Tail,
    /// The top visible row is `rows` rows into `item`.
    At {
        /// The item at the top of the viewport.
        item: TranscriptItemId,
        /// How far into that item the viewport starts.
        rows: u16,
    },
}

/// One item's wrapped height, and what it was measured against.
///
/// The identity is stored alongside the height so a slot can be checked rather than trusted: items
/// only ever arrive at the end today, and an entry that has drifted from the item at its position
/// is re-measured instead of silently describing a different message.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Measured {
    id: TranscriptItemId,
    revision: u64,
    width: u16,
    rows: u16,
}

/// Wrapped item heights, retained across frames.
///
/// It cannot live in `ViewState`, because the renderer takes the projection by shared reference and
/// keeping it that way is what makes "rendering never mutates state" checkable. It cannot live in
/// the renderer either: a cache that dies with the frame is not one. So the composition root owns
/// it, lends it to the frame that measures, and lends it to the scroll path, which needs the same
/// heights to turn a row back into an item.
#[derive(Debug, Default)]
pub struct TranscriptMetrics {
    by_agent: BTreeMap<AgentId, Vec<Measured>>,
    wrapped: usize,
    built: usize,
}

impl TranscriptMetrics {
    /// Measures one agent's items at `width`, reusing every height that is still valid.
    ///
    /// Returns how many items the conversation now has. A streaming delta bumps one item's revision
    /// and costs one wrap; a resize changes the width and costs one pass; an unchanged frame costs
    /// none.
    pub(crate) fn measure(&mut self, agent: &AgentView, palette: &Palette, width: u16) -> usize {
        let entries = self.by_agent.entry(agent.id.clone()).or_default();
        let mut count = 0_usize;
        for item in agent.transcript() {
            let reusable = entries.get(count).is_some_and(|entry| {
                entry.id == item.id && entry.revision == item.revision && entry.width == width
            });
            if !reusable {
                let measured = Measured {
                    id: item.id.clone(),
                    revision: item.revision,
                    width,
                    rows: wrap_rows(item, palette, width),
                };
                self.wrapped = self.wrapped.saturating_add(1);
                match entries.get_mut(count) {
                    Some(slot) => *slot = measured,
                    None => entries.push(measured),
                }
            }
            count = count.saturating_add(1);
        }
        // Nothing removes a transcript item in Phase 00, so this only ever runs when an entry was
        // left by a longer conversation under the same identity. It is here so the cache cannot
        // describe more items than the conversation has.
        entries.truncate(count);
        count
    }

    /// Total rows a conversation occupies at the width it was last measured at.
    ///
    /// Saturating rather than widening: a viewport offset is a `u16` because that is what the
    /// terminal can address, so a conversation taller than that is already past what scrolling can
    /// reach.
    pub(crate) fn total_rows(&self, agent_id: &AgentId) -> u16 {
        self.items(agent_id)
            .iter()
            .fold(0_u16, |total, item| total.saturating_add(item.rows))
    }

    /// Which items a viewport starting at `offset` and `visible_rows` deep actually reaches.
    ///
    /// An empty range when the offset is past the end, which is what an unmeasured conversation and
    /// an offset clamped wrongly both look like — neither is a reason to draw something arbitrary.
    pub(crate) fn window(&self, agent_id: &AgentId, offset: u16, visible_rows: u16) -> Window {
        let items = self.items(agent_id);
        let Some((first, skip_rows)) = self.locate(agent_id, offset) else {
            return Window::empty(items.len());
        };

        let mut last = first;
        let mut built = 0_u16;
        for item in items.get(first..).unwrap_or_default() {
            built = built.saturating_add(item.rows);
            last = last.saturating_add(1);
            if built.saturating_sub(skip_rows) >= visible_rows {
                break;
            }
        }
        Window {
            items: first..last,
            skip_rows,
        }
    }

    /// The position naming the item that row `offset` falls inside.
    ///
    /// `None` when nothing has been measured or the row is past the end: an anchor naming no item
    /// would be a position nothing can resolve.
    pub(crate) fn anchor_at(&self, agent_id: &AgentId, offset: u16) -> Option<TranscriptPosition> {
        let (index, rows) = self.locate(agent_id, offset)?;
        let item = self.items(agent_id).get(index)?.id.clone();
        Some(TranscriptPosition::At { item, rows })
    }

    /// The row a position resolves to, at the width the conversation was last measured.
    ///
    /// An anchor whose item is gone resolves to the tail. Nothing removes an item in Phase 00, so
    /// this is the answer prepared for a future that trims history: rejoining the live conversation
    /// is the least surprising place to land when the text someone was reading no longer exists.
    ///
    /// The row inside the item is clamped to that item's height at *this* width. The item is the
    /// durable half of an anchor; how many rows into it a reader was is width-dependent like every
    /// other row count, and an unclamped one would overshoot a message that wrapped shorter and put
    /// the next message on screen instead.
    pub(crate) fn offset_of(
        &self,
        agent_id: &AgentId,
        position: &TranscriptPosition,
        max_offset: u16,
    ) -> u16 {
        let TranscriptPosition::At { item, rows } = position else {
            return max_offset;
        };
        let mut start = 0_u16;
        for entry in self.items(agent_id) {
            if &entry.id == item {
                let inside = (*rows).min(entry.rows.saturating_sub(1));
                return start.saturating_add(inside).min(max_offset);
            }
            start = start.saturating_add(entry.rows);
        }
        max_offset
    }

    /// Builds the lines for the items a window names.
    ///
    /// Building lives here rather than in the renderer so that measuring and building a
    /// conversation are one module's job, and so the count below cannot be forgotten at a call
    /// site. `&mut self` buys nothing but the counter, which is the point of it.
    pub(crate) fn build(
        &mut self,
        agent: &AgentView,
        palette: &Palette,
        window: &Window,
        selected: Selected,
    ) -> Vec<Line<'static>> {
        let lines: Vec<_> = agent
            .transcript()
            .enumerate()
            .skip(window.items.start)
            .take(window.items.len())
            // The index is the item's position in the whole conversation, not in this window: a
            // selection names entries, and a window is only which of them this frame paints.
            .flat_map(|(index, item)| {
                content::transcript_item(item, palette, selected.contains(index))
            })
            .collect();
        self.built = self.built.saturating_add(lines.len());
        lines
    }

    /// How many items have been wrapped since this cache was created.
    ///
    /// Instrumentation, not bookkeeping: TR-1 is a claim about work done, and a claim about work
    /// can only be tested by something that counts it. [`Self::lines_built`] is the same idea for
    /// TR-2, and the two answer different questions — a cold frame must wrap every item to know how
    /// tall the conversation is, and must still build only the lines its viewport reaches.
    #[must_use]
    pub fn wrapped(&self) -> usize {
        self.wrapped
    }

    /// How many conversation lines have been built since this cache was created.
    #[must_use]
    pub fn lines_built(&self) -> usize {
        self.built
    }

    /// Cached item heights held across every conversation, whether or not one is on screen.
    ///
    /// This is what the renderer retains per hidden conversation. The transcript itself belongs to
    /// the projection and is there regardless, so counting entries here is the honest answer to
    /// what virtualization costs in memory rather than a figure for the whole workspace.
    #[must_use]
    pub fn retained(&self) -> usize {
        self.by_agent.values().map(Vec::len).sum()
    }

    /// The item index containing row `offset`, and how far into that item the row is.
    fn locate(&self, agent_id: &AgentId, offset: u16) -> Option<(usize, u16)> {
        let mut start = 0_u16;
        for (index, item) in self.items(agent_id).iter().enumerate() {
            let end = start.saturating_add(item.rows);
            if end > offset {
                return Some((index, offset.saturating_sub(start)));
            }
            start = end;
        }
        None
    }

    fn items(&self, agent_id: &AgentId) -> &[Measured] {
        self.by_agent.get(agent_id).map_or(&[], Vec::as_slice)
    }
}

/// The slice of a conversation one viewport reaches.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Window {
    /// Items to build, in arrival order.
    pub(crate) items: Range<usize>,
    /// Rows to skip inside the first of them, because the viewport starts partway through it.
    pub(crate) skip_rows: u16,
}

impl Window {
    /// A window that reaches nothing, positioned past the end of the conversation.
    const fn empty(len: usize) -> Self {
        Self {
            items: len..len,
            skip_rows: 0,
        }
    }
}

/// Wraps one item at the panel's inner width.
///
/// No block is attached: `Paragraph::line_count` adds a block's border rows when one is set, and an
/// item's height is the item alone. It is still the renderer's own wrapper, so measuring and
/// painting stay one computation (D-039).
fn wrap_rows(item: &TranscriptItemView, palette: &Palette, width: u16) -> u16 {
    if width == 0 {
        return 0;
    }
    // Measured unselected, deliberately: selection changes a style and never a character, so a
    // height that depended on it would invalidate the cache on every arrow press for no reason.
    let paragraph =
        Paragraph::new(content::transcript_item(item, palette, false)).wrap(Wrap { trim: false });
    u16::try_from(paragraph.line_count(width)).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use ratatui::widgets::{Paragraph, Wrap};

    use super::{TranscriptMetrics, TranscriptPosition};
    use crate::{AgentView, ViewState, content, test_support::Conversation, theme::Palette};

    /// The width the canonical conversation is measured at in these tests.
    const WIDTH: u16 = 46;

    fn agent(state: &ViewState) -> &AgentView {
        state
            .selected_agent()
            .unwrap_or_else(|| panic!("the canonical timeline selects an agent"))
    }

    fn anchored_item(position: &TranscriptPosition) -> &plexmaton_core::TranscriptItemId {
        match position {
            TranscriptPosition::At { item, .. } => item,
            TranscriptPosition::Tail => panic!("this position was expected to name an item"),
        }
    }

    /// TR-1: a frame re-measures what changed, and nothing else.
    ///
    /// The four counts are the whole invariant. Drop the revision from the cache key and a delta
    /// stops being noticed; drop the width and a resize stops being noticed; drop the cache and
    /// every count collapses into "every item, every frame".
    #[test]
    fn measurement_is_proportional_to_what_changed() {
        let palette = Palette::default();
        let mut conversation = Conversation::canonical();
        conversation.extend(3);
        let mut metrics = TranscriptMetrics::default();

        let items = metrics.measure(agent(&conversation.state), &palette, WIDTH);
        assert!(
            items > 1,
            "the fixture needs several items to prove anything"
        );
        assert_eq!(metrics.wrapped(), items, "a cold cache measures everything");

        metrics.measure(agent(&conversation.state), &palette, WIDTH);
        assert_eq!(
            metrics.wrapped(),
            items,
            "an unchanged conversation at an unchanged width wraps nothing"
        );

        let before = metrics.wrapped();
        conversation.extend(1);
        metrics.measure(agent(&conversation.state), &palette, WIDTH);
        assert_eq!(
            metrics.wrapped().saturating_sub(before),
            1,
            "a new item costs one wrap, not a whole history"
        );

        let before = metrics.wrapped();
        conversation.append(" and more streamed text.");
        metrics.measure(agent(&conversation.state), &palette, WIDTH);
        assert_eq!(
            metrics.wrapped().saturating_sub(before),
            1,
            "a delta re-measures the item it changed, not the history behind it"
        );

        let before = metrics.wrapped();
        let items = metrics.measure(agent(&conversation.state), &palette, WIDTH - 1);
        assert_eq!(
            metrics.wrapped().saturating_sub(before),
            items,
            "a resize invalidates every height, because every one of them was width-dependent"
        );
    }

    /// TR-1's arithmetic premise: per-item heights sum to the height of the whole.
    ///
    /// Wrapping is per logical line, so an item's rows do not depend on its neighbours. That is what
    /// lets a frame measure items separately and still know the true content height — if it were
    /// false, every offset in the workspace would be off by the error.
    #[test]
    fn item_heights_sum_to_the_height_of_the_whole_conversation() {
        let palette = Palette::default();
        let mut conversation = Conversation::canonical();
        let state = &conversation.extend(4).state;
        let mut metrics = TranscriptMetrics::default();

        for width in [24_u16, 46, 118] {
            metrics.measure(agent(state), &palette, width);
            let whole: Vec<_> = agent(state)
                .transcript()
                .flat_map(|item| content::transcript_item(item, &palette, false))
                .collect();
            let together = Paragraph::new(whole)
                .wrap(Wrap { trim: false })
                .line_count(width);

            assert_eq!(
                usize::from(metrics.total_rows(&agent(state).id)),
                together,
                "measuring item by item disagreed with measuring the conversation at width {width}"
            );
        }
    }

    /// TR-2: the window is what the viewport reaches, and it starts partway into an item when the
    /// offset does.
    #[test]
    fn a_window_covers_the_viewport_and_starts_inside_the_item_it_lands_in() {
        let palette = Palette::default();
        let mut conversation = Conversation::canonical();
        let state = &conversation.extend(3).state;
        let id = &agent(state).id;
        let mut metrics = TranscriptMetrics::default();
        let count = metrics.measure(agent(state), &palette, WIDTH);
        let total = metrics.total_rows(id);

        let top = metrics.window(id, 0, 4);
        assert_eq!((top.items.start, top.skip_rows), (0, 0));

        let inside = metrics.window(id, 1, 4);
        assert_eq!(
            (inside.items.start, inside.skip_rows),
            (0, 1),
            "an offset inside the first item keeps that item and skips into it"
        );

        let bottom = metrics.window(id, total.saturating_sub(1), 4);
        assert_eq!(
            bottom.items.end, count,
            "the last row belongs to the last item"
        );

        // The panel paints `visible` rows from `offset`, so every one of them has to belong to an
        // item the window named. A window one item short leaves the bottom of the panel blank.
        let visible = 6_u16;
        for offset in 0..total {
            let window = metrics.window(id, offset, visible);
            let last_row = offset
                .saturating_add(visible)
                .saturating_sub(1)
                .min(total.saturating_sub(1));
            let (needed, _) = metrics
                .locate(id, last_row)
                .unwrap_or_else(|| panic!("row {last_row} of {total} is inside the conversation"));
            assert!(
                window.items.contains(&needed),
                "the window at {offset} was {:?} but row {last_row} lives in item {needed}",
                window.items
            );
        }
    }

    /// TR-3: an anchor names an item, and resolving it back gives the row it came from.
    ///
    /// The round trip is the invariant. A conversion that dropped the rows-into-item part would
    /// still pass an "is it the right item" assertion while sending the reader to that item's first
    /// line on every resize.
    #[test]
    fn an_anchor_round_trips_through_the_row_it_names() {
        let palette = Palette::default();
        let mut conversation = Conversation::canonical();
        let state = &conversation.extend(5).state;
        let id = &agent(state).id;
        let mut metrics = TranscriptMetrics::default();
        metrics.measure(agent(state), &palette, WIDTH);
        let total = metrics.total_rows(id);

        for row in 0..total {
            let anchor = metrics
                .anchor_at(id, row)
                .unwrap_or_else(|| panic!("row {row} of {total} is inside the conversation"));
            assert_eq!(
                metrics.offset_of(id, &anchor, total),
                row,
                "anchoring row {row} and resolving it back landed somewhere else"
            );
        }

        assert_eq!(
            metrics.anchor_at(id, total),
            None,
            "a row past the end names no item"
        );
        assert_eq!(
            metrics.offset_of(id, &TranscriptPosition::Tail, total),
            total,
            "following resolves to the end of whatever the content became"
        );
    }

    /// TR-3: the same anchor names the same message at a different width, where a row would not.
    #[test]
    fn an_anchor_survives_a_width_change_and_a_row_number_does_not() {
        let palette = Palette::default();
        let mut conversation = Conversation::canonical();
        let state = &conversation.extend(5).state;
        let id = &agent(state).id;
        let mut metrics = TranscriptMetrics::default();

        metrics.measure(agent(state), &palette, 40);
        let narrow_total = metrics.total_rows(id);
        let row = narrow_total / 2;
        let anchor = metrics
            .anchor_at(id, row)
            .unwrap_or_else(|| panic!("the middle of the conversation is inside it"));

        metrics.measure(agent(state), &palette, 100);
        let wide_total = metrics.total_rows(id);
        assert!(
            wide_total < narrow_total,
            "the widths have to wrap differently or this proves nothing: \
             {narrow_total} then {wide_total}"
        );

        let moved = metrics.offset_of(id, &anchor, wide_total);
        let landed = metrics
            .anchor_at(id, moved)
            .unwrap_or_else(|| panic!("the resolved row is inside the conversation"));
        assert_eq!(
            anchored_item(&landed),
            anchored_item(&anchor),
            "the anchor named a different message after the resize"
        );
        assert_ne!(
            moved, row,
            "and the row it resolves to did move, so keeping the number would have been wrong"
        );
    }

    #[test]
    fn a_conversation_nothing_has_measured_has_no_window_and_no_anchor() {
        let metrics = TranscriptMetrics::default();
        let id = Conversation::canonical()
            .state
            .primary_agent()
            .map(|agent| agent.id.clone())
            .unwrap_or_else(|| panic!("the canonical timeline creates a primary agent"));

        assert_eq!(metrics.window(&id, 0, 10).items, 0..0);
        assert_eq!(metrics.anchor_at(&id, 0), None);
    }
}
