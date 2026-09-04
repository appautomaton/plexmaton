//! How tall a conversation is, which part of it a frame has to build, and where its reader is.
//!
//! Wrapping a whole history in order to paint twenty rows of it is the cost this module removes.
//! Heights are measured one entry at a time and kept until that entry's revision or the panel's width
//! changes (TR-1) — per width, because two surfaces can draw one conversation at two sizes in the
//! same frame — and a frame builds lines only for the entries its viewport reaches (TR-2). Because
//! those heights are also what turn an entry into a row number, this is where a reading position is
//! resolved (TR-3). The contract is
//! [`specs/transcript-layout.md`](../../../.agents/specs/transcript-layout.md).

use std::{collections::BTreeMap, ops::Range};

use plexmaton_core::{AgentId, TranscriptItemId};
use ratatui::{
    text::Line,
    widgets::{Paragraph, Wrap},
};

use crate::{
    AgentView, TranscriptEntryView, ViewState, content,
    state::{DisclosureState, EntryAppearance},
    surface::SurfaceId,
    theme::Palette,
};

/// Where a reader is parked in one conversation.
///
/// An entry and a row inside it, never a row into the whole history: how many rows precede an entry
/// depends on the panel's width, so a stored row names different text at every terminal size
/// (TR-3).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TranscriptPosition {
    /// Following the newest line, wherever the conversation ends up.
    Tail,
    /// The top visible row is `rows` rows into the entry named by `item`.
    At {
        /// The entry at the top of the viewport.
        item: TranscriptItemId,
        /// How far into that entry the viewport starts.
        rows: usize,
    },
}

/// One entry's wrapped height, and what it was measured against.
///
/// The identity is stored alongside the height so a slot can be checked rather than trusted:
/// entries only ever arrive at the end today, and a measured slot that has drifted from the entry
/// at its position is re-measured instead of silently describing a different fact.
///
/// The width is not here: it is the key of the set this entry belongs to, so heights measured at
/// two widths sit side by side rather than overwriting each other.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Measured {
    id: TranscriptItemId,
    revision: u64,
    open: bool,
    compact_rows: usize,
    rows: usize,
}

/// One conversation's heights at one panel width.
#[derive(Debug)]
struct AtWidth {
    width: u16,
    items: Vec<Measured>,
}

/// Measurement widths one conversation keeps, most recently measured first.
///
/// Two, because a conversation is drawn at one width until the terminal or the composition
/// changes, and the width it had before is the one it comes back to: a resize, or a second window
/// opening and closing, must not re-wrap a whole history each way. One set per agent made each
/// measurement invalidate the previous width's heights, which cost a full re-wrap per change and
/// left the scroll path resolving anchors against whichever width happened to be measured last.
/// A third width cannot arise in one frame, because `render` draws a conversation for
/// exactly two surface identities; a third has to move this number with it, and what would say so
/// is the work count in `two_widths_of_one_conversation_do_not_invalidate_each_other`.
const MEASURED_WIDTHS: usize = 2;

/// Wrapped entry heights, retained across frames and keyed by agent and width.
///
/// It cannot live in `ViewState`, because the renderer takes the projection by shared reference and
/// keeping it that way is what makes "rendering never mutates state" checkable. It cannot live in
/// the renderer either: a cache that dies with the frame is not one. So the composition root owns
/// it, lends it to the frame that measures, and lends it to the scroll path, which needs the same
/// heights to turn a row back into an entry.
#[derive(Debug, Default)]
pub struct TranscriptMetrics {
    by_agent: BTreeMap<AgentId, Vec<AtWidth>>,
    wrapped: usize,
    built: usize,
}

impl TranscriptMetrics {
    /// Measures the compact projection, used by callers with no disclosure state.
    #[cfg(test)]
    pub(crate) fn measure(&mut self, agent: &AgentView, palette: &Palette, width: u16) -> usize {
        self.measure_with(agent, palette, width, &DisclosureState::default())
    }

    /// Measures one agent's entries at `width`, reusing every height that is still valid.
    ///
    /// Returns how many entries the conversation now has. A streaming delta bumps one entry's revision
    /// and costs one wrap; a resize changes the width and costs one pass; an unchanged frame costs
    /// none.
    pub(crate) fn measure_with(
        &mut self,
        agent: &AgentView,
        palette: &Palette,
        width: u16,
        disclosure: &DisclosureState,
    ) -> usize {
        let cached = self.by_agent.entry(agent.id.clone()).or_default();
        // Front is most recently measured, so the width a frame stopped drawing at is the one
        // evicted. Both live widths are measured every frame, so neither can evict the other.
        match cached.iter().position(|entry| entry.width == width) {
            Some(0) => {}
            Some(index) => {
                let entry = cached.remove(index);
                cached.insert(0, entry);
            }
            None => cached.insert(
                0,
                AtWidth {
                    width,
                    items: Vec::new(),
                },
            ),
        }
        cached.truncate(MEASURED_WIDTHS);
        // The match above always leaves this width at the front, so the fallback is unreachable
        // rather than a case: reporting no entries is what a caller can safely draw if it ever is.
        let Some(entries) = cached.first_mut().map(|entry| &mut entry.items) else {
            return 0;
        };
        let mut count = 0_usize;
        for item in agent.entries() {
            let open = disclosure.is_open(item.id());
            let reusable = entries.get(count).is_some_and(|entry| {
                &entry.id == item.id() && entry.revision == item.revision() && entry.open == open
            });
            if !reusable {
                let compact_rows = wrap_rows(item, palette, width, false);
                let measured = Measured {
                    id: item.id().clone(),
                    revision: item.revision(),
                    open,
                    compact_rows,
                    rows: if open {
                        wrap_rows(item, palette, width, true)
                    } else {
                        compact_rows
                    },
                };
                self.wrapped = self.wrapped.saturating_add(1);
                match entries.get_mut(count) {
                    Some(slot) => *slot = measured,
                    None => entries.push(measured),
                }
            }
            count = count.saturating_add(1);
        }
        // Nothing removes a transcript entry yet, so this only ever runs when an entry was
        // left by a longer conversation under the same identity. It is here so the cache cannot
        // describe more entries than the conversation has.
        entries.truncate(count);
        count
    }

    /// Total rows a conversation occupies at `width`.
    ///
    /// Terminal coordinates remain `u16`, but the semantic row space must reach every retained
    /// line and every entry after it.
    pub(crate) fn total_rows(&self, agent_id: &AgentId, width: u16) -> usize {
        self.items(agent_id, width)
            .iter()
            .fold(0_usize, |total, item| total.saturating_add(item.rows))
    }

    /// Which entries a viewport starting at `offset` and `visible_rows` deep actually reaches.
    ///
    /// An empty range when the offset is past the end, which is what an unmeasured conversation and
    /// an offset clamped wrongly both look like — neither is a reason to draw something arbitrary.
    pub(crate) fn window(
        &self,
        agent_id: &AgentId,
        width: u16,
        offset: usize,
        visible_rows: u16,
    ) -> Window {
        let items = self.items(agent_id, width);
        let Some((first, skip_rows)) = self.locate(agent_id, width, offset) else {
            return Window::empty(items.len(), width);
        };

        let mut last = first;
        let mut built = 0_usize;
        for item in items.get(first..).unwrap_or_default() {
            built = built.saturating_add(item.rows);
            last = last.saturating_add(1);
            if built.saturating_sub(skip_rows) >= usize::from(visible_rows) {
                break;
            }
        }
        Window {
            items: first..last,
            skip_rows,
            width,
        }
    }

    /// The position naming the entry that row `offset` falls inside, at `width`.
    ///
    /// `None` when nothing has been measured at that width or the row is past the end: an anchor
    /// naming no entry would be a position nothing can resolve.
    pub(crate) fn anchor_at(
        &self,
        agent_id: &AgentId,
        width: u16,
        offset: usize,
    ) -> Option<TranscriptPosition> {
        let (index, rows) = self.locate(agent_id, width, offset)?;
        let item = self.items(agent_id, width).get(index)?.id.clone();
        Some(TranscriptPosition::At { item, rows })
    }

    /// The semantic entry containing `row`, using the heights the last frame measured.
    pub(crate) fn compact_entry_at_row(
        &self,
        agent_id: &AgentId,
        width: u16,
        row: usize,
    ) -> Option<usize> {
        let (index, inside) = self.locate(agent_id, width, row)?;
        let measured = self.items(agent_id, width).get(index)?;
        (inside < measured.compact_rows).then_some(index)
    }

    /// The row a position resolves to at `width`.
    ///
    /// An anchor whose entry is gone resolves to the tail. Nothing removes an entry yet, so
    /// this is the answer prepared for a future that trims history: rejoining the live conversation
    /// is the least surprising place to land when the text someone was reading no longer exists.
    ///
    /// The row inside the entry is clamped to that entry's height at *this* width. The entry is the
    /// durable half of an anchor; how many rows into it a reader was is width-dependent like every
    /// other row count, and an unclamped one would overshoot an entry that wrapped shorter and put
    /// the next entry on screen instead.
    pub(crate) fn offset_of(
        &self,
        agent_id: &AgentId,
        width: u16,
        position: &TranscriptPosition,
        max_offset: usize,
    ) -> usize {
        let TranscriptPosition::At { item, rows } = position else {
            return max_offset;
        };
        let mut start = 0_usize;
        for entry in self.items(agent_id, width) {
            if &entry.id == item {
                let inside = (*rows).min(entry.rows.saturating_sub(1));
                return start.saturating_add(inside).min(max_offset);
            }
            start = start.saturating_add(entry.rows);
        }
        max_offset
    }

    /// Builds the lines for the entries a window names.
    ///
    /// Building lives here rather than in the renderer so that measuring and building a
    /// conversation are one module's job, and so the count below cannot be forgotten at a call
    /// site. `&mut self` buys nothing but the counter, which is the point of it.
    pub(crate) fn build(
        &mut self,
        agent: &AgentView,
        palette: &Palette,
        window: &Window,
        state: &ViewState,
        surface: SurfaceId,
    ) -> (Vec<Line<'static>>, u16) {
        let selected = state.selected_in(surface, &agent.id);
        // The window carries the width it was measured at, and building at any other one would
        // wrap the text differently from the heights the viewport was resolved against.
        let width = window.width;
        let mut lines: Vec<_> = agent
            .entries()
            .enumerate()
            .skip(window.items.start)
            .take(window.items.len())
            // The index is the entry's position in the whole conversation, not in this window: a
            // selection names entries, and a window is only which of them this frame paints.
            .flat_map(|(index, item)| {
                let appearance = state.entry_appearance(
                    surface,
                    &agent.id,
                    item.id(),
                    selected.contains(index),
                );
                content::transcript_entry(item, palette, appearance, width)
            })
            .collect();
        self.built = self.built.saturating_add(lines.len());
        let skip_rows = trim_scroll_prefix(&mut lines, window.skip_rows, window.width);
        (lines, skip_rows)
    }

    /// How many entries have been wrapped since this cache was created.
    ///
    /// Instrumentation, not bookkeeping: TR-1 is a claim about work done, and a claim about work
    /// can only be tested by something that counts it. [`Self::lines_built`] is the same idea for
    /// TR-2, and the two answer different questions — a cold frame must wrap every entry to know how
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

    /// Cached entry heights held across every conversation and width, on screen or not.
    ///
    /// This is what the renderer retains per hidden conversation. The transcript itself belongs to
    /// the projection and is there regardless, so counting entries here is the honest answer to
    /// what virtualization costs in memory rather than a figure for the whole workspace. Bounded by
    /// [`MEASURED_WIDTHS`] per agent, so a run of resizes cannot grow it.
    #[must_use]
    pub fn retained(&self) -> usize {
        self.by_agent
            .values()
            .flat_map(|cached| cached.iter().map(|entry| entry.items.len()))
            .sum()
    }

    /// The entry index containing row `offset`, and how far into that entry the row is.
    fn locate(&self, agent_id: &AgentId, width: u16, offset: usize) -> Option<(usize, usize)> {
        let mut start = 0_usize;
        for (index, item) in self.items(agent_id, width).iter().enumerate() {
            let end = start.saturating_add(item.rows);
            if end > offset {
                return Some((index, offset.saturating_sub(start)));
            }
            start = end;
        }
        None
    }

    /// Heights for one conversation at one width, empty when that pair was never measured.
    fn items(&self, agent_id: &AgentId, width: u16) -> &[Measured] {
        self.by_agent
            .get(agent_id)
            .and_then(|cached| cached.iter().find(|entry| entry.width == width))
            .map_or(&[], |entry| entry.items.as_slice())
    }
}

/// The slice of a conversation one viewport reaches.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Window {
    /// Entries to build, in arrival order.
    pub(crate) items: Range<usize>,
    /// Rows to skip inside the first of them, because the viewport starts partway through it.
    pub(crate) skip_rows: usize,
    /// Width at which `skip_rows` was measured.
    width: u16,
}

impl Window {
    /// A window that reaches nothing, positioned past the end of the conversation.
    const fn empty(len: usize, width: u16) -> Self {
        Self {
            items: len..len,
            skip_rows: 0,
            width,
        }
    }
}

/// Wraps one entry at the panel's inner width.
///
/// No block is attached: `Paragraph::line_count` adds a block's border rows when one is set, and an
/// entry's height is the entry alone. It is still the renderer's own wrapper, so measuring and
/// painting stay one computation (surface-model §viewports).
fn wrap_rows(item: &TranscriptEntryView, palette: &Palette, width: u16, open: bool) -> usize {
    if width == 0 {
        return 0;
    }
    // Measured unselected, deliberately: selection changes a style and never a character, so a
    // height that depended on it would invalidate the cache on every arrow press for no reason.
    let paragraph = Paragraph::new(content::transcript_entry(
        item,
        palette,
        EntryAppearance {
            open,
            ..EntryAppearance::default()
        },
        width,
    ))
    .wrap(Wrap { trim: false });
    paragraph.line_count(width)
}

/// Removes complete logical lines before a large semantic scroll offset until Ratatui's `u16`
/// widget scroll can express the remainder. The semantic viewport keeps the full `usize` offset;
/// this is only an adapter at the terminal boundary.
fn trim_scroll_prefix(lines: &mut Vec<Line<'static>>, mut skip_rows: usize, width: u16) -> u16 {
    if width == 0 {
        return 0;
    }
    let mut remove = 0_usize;
    while skip_rows > usize::from(u16::MAX) {
        let Some(line) = lines.get(remove) else {
            break;
        };
        let rows = Paragraph::new(line.clone())
            .wrap(Wrap { trim: false })
            .line_count(width)
            .max(1);
        if rows > skip_rows {
            break;
        }
        skip_rows = skip_rows.saturating_sub(rows);
        remove = remove.saturating_add(1);
    }
    if remove > 0 {
        lines.drain(..remove);
    }
    u16::try_from(skip_rows).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{
        SessionEvent, ToolCallId, ToolCallStatus, ToolDetail, ToolPresentation, TranscriptItemId,
    };
    use ratatui::widgets::{Paragraph, Wrap};

    use super::{MEASURED_WIDTHS, TranscriptMetrics, TranscriptPosition};
    use crate::{
        AgentView, ViewState, content,
        state::{EntryAppearance, EntryTarget},
        surface::{SurfaceId, SurfaceTree},
        test_support::Conversation,
        theme::Palette,
    };

    /// The width the canonical conversation is measured at in these tests.
    const WIDTH: u16 = 46;

    fn agent(state: &ViewState) -> &AgentView {
        state
            .primary_agent()
            .unwrap_or_else(|| panic!("the canonical timeline creates a primary agent"))
    }

    fn anchored_item(position: &TranscriptPosition) -> &TranscriptItemId {
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

    /// ENT-2 and TR-1: a tool completion invalidates its stable entry and no sibling.
    #[test]
    fn a_tool_transition_remeasures_only_its_original_entry() {
        let palette = Palette::default();
        let mut conversation = Conversation::canonical();
        let agent_id = agent(&conversation.state).id.clone();
        let item_id = TranscriptItemId::new("primary-tool")
            .unwrap_or_else(|error| panic!("fixture: {error}"));
        let call_id = ToolCallId::new("call").unwrap_or_else(|error| panic!("fixture: {error}"));
        let event = |revision, status| SessionEvent::ToolCallChanged {
            agent_id: agent_id.clone(),
            item_id: item_id.clone(),
            item_revision: revision,
            call_id: call_id.clone(),
            label: "read_file".to_owned(),
            status,
            presentation: ToolPresentation::default(),
        };
        conversation.emit(event(0, ToolCallStatus::Queued));

        let mut metrics = TranscriptMetrics::default();
        let entries = metrics.measure(agent(&conversation.state), &palette, WIDTH);
        assert!(
            entries > 1,
            "a sibling entry is needed to expose a full rewrap"
        );

        let before = metrics.wrapped();
        conversation.emit(event(1, ToolCallStatus::Running));
        metrics.measure(agent(&conversation.state), &palette, WIDTH);
        assert_eq!(metrics.wrapped().saturating_sub(before), 1);

        let before = metrics.wrapped();
        conversation.emit(event(2, ToolCallStatus::Succeeded));
        metrics.measure(agent(&conversation.state), &palette, WIDTH);
        assert_eq!(
            metrics.wrapped().saturating_sub(before),
            1,
            "completion moved state on the original entry instead of rebuilding the conversation"
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
                .entries()
                .flat_map(|item| {
                    content::transcript_entry(
                        item,
                        &palette,
                        EntryAppearance::compact(false),
                        width,
                    )
                })
                .collect();
            let together = Paragraph::new(whole)
                .wrap(Wrap { trim: false })
                .line_count(width);

            assert_eq!(
                metrics.total_rows(&agent(state).id, width),
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
        let total = metrics.total_rows(id, WIDTH);

        let top = metrics.window(id, WIDTH, 0, 4);
        assert_eq!((top.items.start, top.skip_rows), (0, 0));

        let inside = metrics.window(id, WIDTH, 1, 4);
        assert_eq!(
            (inside.items.start, inside.skip_rows),
            (0, 1),
            "an offset inside the first item keeps that item and skips into it"
        );

        let bottom = metrics.window(id, WIDTH, total.saturating_sub(1), 4);
        assert_eq!(
            bottom.items.end, count,
            "the last row belongs to the last item"
        );

        // The panel paints `visible` rows from `offset`, so every one of them has to belong to an
        // item the window named. A window one item short leaves the bottom of the panel blank.
        let visible = 6_u16;
        for offset in 0..total {
            let window = metrics.window(id, WIDTH, offset, visible);
            let last_row = offset
                .saturating_add(usize::from(visible))
                .saturating_sub(1)
                .min(total.saturating_sub(1));
            let (needed, _) = metrics
                .locate(id, WIDTH, last_row)
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
        let total = metrics.total_rows(id, WIDTH);

        for row in 0..total {
            let anchor = metrics
                .anchor_at(id, WIDTH, row)
                .unwrap_or_else(|| panic!("row {row} of {total} is inside the conversation"));
            assert_eq!(
                metrics.offset_of(id, WIDTH, &anchor, total),
                row,
                "anchoring row {row} and resolving it back landed somewhere else"
            );
        }

        assert_eq!(
            metrics.anchor_at(id, WIDTH, total),
            None,
            "a row past the end names no item"
        );
        assert_eq!(
            metrics.offset_of(id, WIDTH, &TranscriptPosition::Tail, total),
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
        let narrow_total = metrics.total_rows(id, 40);
        let row = narrow_total / 2;
        let anchor = metrics
            .anchor_at(id, 40, row)
            .unwrap_or_else(|| panic!("the middle of the conversation is inside it"));

        metrics.measure(agent(state), &palette, 100);
        let wide_total = metrics.total_rows(id, 100);
        assert!(
            wide_total < narrow_total,
            "the widths have to wrap differently or this proves nothing: \
             {narrow_total} then {wide_total}"
        );

        let moved = metrics.offset_of(id, 100, &anchor, wide_total);
        let landed = metrics
            .anchor_at(id, 100, moved)
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

    /// TR-1: one conversation measured at two widths keeps both, and neither costs the other.
    ///
    /// A conversation changes width when the terminal does or when the composition does, and it
    /// comes back to the width it had. With one set of heights per agent, each measurement
    /// invalidated the other's: every change re-wrapped the whole history, and every reader was
    /// resolved against whichever width had been measured last.
    #[test]
    fn two_widths_of_one_conversation_do_not_invalidate_each_other() {
        let palette = Palette::default();
        let mut conversation = Conversation::canonical();
        conversation.extend(6);
        let id = agent(&conversation.state).id.clone();
        let mut metrics = TranscriptMetrics::default();

        let items = metrics.measure(agent(&conversation.state), &palette, WIDTH);
        metrics.measure(agent(&conversation.state), &palette, WIDTH / 2);
        let cold = metrics.wrapped();
        assert_eq!(cold, items * 2, "a cold cache measures each width once");

        metrics.measure(agent(&conversation.state), &palette, WIDTH);
        metrics.measure(agent(&conversation.state), &palette, WIDTH / 2);
        assert_eq!(
            metrics.wrapped(),
            cold,
            "a steady frame at two widths must re-wrap nothing at either of them"
        );

        conversation.append(" and more streamed text.");
        metrics.measure(agent(&conversation.state), &palette, WIDTH);
        metrics.measure(agent(&conversation.state), &palette, WIDTH / 2);
        assert_eq!(
            metrics.wrapped().saturating_sub(cold),
            2,
            "a delta costs one wrap per width on screen, not one history per width"
        );

        let narrow = metrics.total_rows(&id, WIDTH / 2);
        let wide = metrics.total_rows(&id, WIDTH);
        assert!(
            narrow > wide,
            "the widths have to wrap differently or this proves nothing: {narrow} then {wide}"
        );
    }

    /// Retention is bounded by the widths in use, not by the widths ever seen.
    ///
    /// A resize is a new width every row the user drags through, so a set per width with nothing
    /// evicting it would grow the cache for the length of the session.
    #[test]
    fn a_run_of_widths_retains_only_the_last_two() {
        let palette = Palette::default();
        let mut conversation = Conversation::canonical();
        let state = &conversation.extend(4).state;
        let mut metrics = TranscriptMetrics::default();

        let items = metrics.measure(agent(state), &palette, 30);
        for width in 31..60 {
            metrics.measure(agent(state), &palette, width);
        }

        assert_eq!(
            metrics.retained(),
            items * MEASURED_WIDTHS,
            "thirty widths were measured and two sets of heights are kept"
        );
        assert!(
            metrics.items(&agent(state).id, 30).is_empty(),
            "the width nothing has drawn at since is the one evicted"
        );
        assert!(
            !metrics.items(&agent(state).id, 59).is_empty(),
            "and the most recently measured width is still there"
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

        assert_eq!(metrics.window(&id, WIDTH, 0, 10).items, 0..0);
        assert_eq!(metrics.anchor_at(&id, WIDTH, 0), None);
    }

    /// ENT-4/TR-2: the maximum retained text can contain more logical lines than a terminal
    /// coordinate can name. Semantic offsets still reach its tail and every later entry; only the
    /// final widget scroll is narrowed after complete logical lines are removed.
    #[test]
    fn maximum_newline_detail_and_the_entry_after_it_remain_reachable() {
        const MAX_RETAINED_TEXT_BYTES: usize = 64 * 1024;

        let mut conversation = Conversation::canonical();
        let agent_id = agent(&conversation.state).id.clone();
        let item = TranscriptItemId::new("maximum-newline-tool")
            .unwrap_or_else(|error| panic!("fixture: {error}"));
        let call = ToolCallId::new("maximum-newline-tool")
            .unwrap_or_else(|error| panic!("fixture: {error}"));
        conversation.emit(SessionEvent::ToolCallChanged {
            agent_id: agent_id.clone(),
            item_id: item.clone(),
            item_revision: 0,
            call_id: call.clone(),
            label: "exec_command".to_owned(),
            status: ToolCallStatus::Queued,
            presentation: ToolPresentation::default(),
        });
        conversation.emit(SessionEvent::ToolCallChanged {
            agent_id: agent_id.clone(),
            item_id: item.clone(),
            item_revision: 1,
            call_id: call,
            label: "exec_command".to_owned(),
            status: ToolCallStatus::Running,
            presentation: ToolPresentation {
                invocation: Some(ToolDetail::Text {
                    source: "\n".repeat(MAX_RETAINED_TEXT_BYTES),
                    omitted_bytes: 0,
                }),
                outcome: None,
            },
        });
        conversation.extend(1);
        let mut state = conversation.state;
        let mut metrics = TranscriptMetrics::default();
        let target_index = agent(&state)
            .entries()
            .position(|entry| entry.id() == &item)
            .unwrap_or_else(|| panic!("the tool is in the semantic transcript"));
        state.toggle_pointer_entry(
            &SurfaceTree::default(),
            &metrics,
            EntryTarget {
                surface: SurfaceId::Transcript,
                agent: agent_id.clone(),
                item: item.clone(),
                index: target_index,
            },
        );
        let palette = Palette::default();
        let count = metrics.measure_with(agent(&state), &palette, WIDTH, state.disclosure());
        let total = metrics.total_rows(&agent_id, WIDTH);
        assert!(total > usize::from(u16::MAX));

        let measured = metrics.items(&agent_id, WIDTH);
        let tool_index = measured
            .iter()
            .position(|entry| entry.id == item)
            .unwrap_or_else(|| panic!("the tool was measured"));
        let tool_start = measured[..tool_index]
            .iter()
            .map(|entry| entry.rows)
            .sum::<usize>();
        let deep = tool_start.saturating_add(usize::from(u16::MAX) + 1);
        let deep_window = metrics.window(&agent_id, WIDTH, deep, 1);
        assert!(deep_window.skip_rows > usize::from(u16::MAX));
        let (deep_lines, widget_scroll) = metrics.build(
            agent(&state),
            &palette,
            &deep_window,
            &state,
            SurfaceId::Transcript,
        );
        assert!(!deep_lines.is_empty());
        assert_eq!(widget_scroll, u16::MAX);
        assert_eq!(
            deep_lines.len(),
            MAX_RETAINED_TEXT_BYTES + 2,
            "one complete logical line was removed before narrowing the widget scroll"
        );

        let tail = metrics.window(
            &agent_id,
            WIDTH,
            total.saturating_sub(usize::from(6_u16)),
            6,
        );
        assert_eq!(
            tail.items.end, count,
            "the entry after the large tool is reachable"
        );
        let (tail_lines, _) = metrics.build(
            agent(&state),
            &palette,
            &tail,
            &state,
            SurfaceId::Transcript,
        );
        assert!(
            tail_lines
                .iter()
                .any(|line| line.to_string().contains("Filler")),
            "the transcript tail includes the semantic entry after the maximum detail"
        );
    }
}
