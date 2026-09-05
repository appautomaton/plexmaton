//! Text hit resolution uses the same entry heights and mapped presentation as painting.
use super::*;

impl TranscriptMetrics {
    pub(crate) fn text_entry_at_row(
        &self,
        agent: &AgentId,
        width: u16,
        row: usize,
    ) -> Option<(usize, usize)> {
        let (index, inside) = self.locate(agent, width, row)?;
        let measured = self.items(agent, width).get(index)?;
        let inside = inside.checked_sub(measured.leading_rows)?;
        (inside < measured.body_rows).then_some((index, inside))
    }

    pub(crate) fn mapped_entry(
        &mut self,
        agent: &AgentId,
        item: &TranscriptEntryView,
        palette: &Palette,
        width: u16,
        appearance: EntryAppearance,
    ) -> std::borrow::Cow<'_, crate::text_layout::Layout> {
        self.layouts.mapped(agent, item, palette, width, appearance)
    }
}
