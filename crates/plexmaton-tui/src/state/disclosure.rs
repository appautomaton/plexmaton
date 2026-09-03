//! TUI-only disclosure and hover state for transcript entries.
//!
//! A tool's semantic detail lives on its transcript entry. This module remembers only how the
//! user asked to view that source: open identities and the one entry under the pointer. Neither is
//! a session event, provider replay fact, or second transcript (ENT-4).

use std::collections::BTreeSet;

use plexmaton_core::{AgentId, TranscriptItemId};

use super::{Selection, TranscriptEntryView, ViewState};
use crate::{
    surface::{SurfaceId, SurfaceTree},
    transcript::TranscriptMetrics,
};

/// One transcript entry resolved through the frame the user acted on.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EntryTarget {
    pub(crate) surface: SurfaceId,
    pub(crate) agent: AgentId,
    pub(crate) item: TranscriptItemId,
    /// Position in the frame that resolved the target, used only for selection appearance.
    pub(crate) index: usize,
}

/// How one entry should be painted this frame.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct EntryAppearance {
    pub(crate) selected: bool,
    pub(crate) open: bool,
    pub(crate) hovered: bool,
}

impl EntryAppearance {
    #[cfg(test)]
    pub(crate) const fn compact(selected: bool) -> Self {
        Self {
            selected,
            open: false,
            hovered: false,
        }
    }
}

/// Presentation choices that never enter semantic replay.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct DisclosureState {
    open: BTreeSet<TranscriptItemId>,
    hovered: Option<EntryTarget>,
}

impl DisclosureState {
    pub(crate) fn is_open(&self, item: &TranscriptItemId) -> bool {
        self.open.contains(item)
    }

    fn toggle(&mut self, item: TranscriptItemId) {
        if !self.open.remove(&item) {
            self.open.insert(item);
        }
    }

    fn hover(&mut self, target: Option<EntryTarget>) -> bool {
        if self.hovered == target {
            return false;
        }
        self.hovered = target;
        true
    }

    fn is_hovered(&self, surface: SurfaceId, agent: &AgentId, item: &TranscriptItemId) -> bool {
        self.hovered.as_ref().is_some_and(|hovered| {
            hovered.surface == surface && &hovered.agent == agent && &hovered.item == item
        })
    }
}

impl ViewState {
    pub(crate) const fn disclosure(&self) -> &DisclosureState {
        &self.disclosure
    }

    pub(crate) fn entry_appearance(
        &self,
        surface: SurfaceId,
        agent: &AgentId,
        item: &TranscriptItemId,
        selected: bool,
    ) -> EntryAppearance {
        EntryAppearance {
            selected,
            open: self.disclosure.is_open(item),
            hovered: self.disclosure.is_hovered(surface, agent, item),
        }
    }

    /// Resolves a foldable tool at one semantic entry index.
    pub(crate) fn entry_target(&self, surface: SurfaceId, index: usize) -> Option<EntryTarget> {
        let agent = self.agent_shown_by(surface)?;
        let entry = self.agents.get(&agent)?.entries().nth(index)?;
        let TranscriptEntryView::Tool(tool) = entry else {
            return None;
        };
        (tool.presentation.invocation.is_some() || tool.presentation.outcome.is_some()).then(|| {
            EntryTarget {
                surface,
                agent,
                item: tool.entry_id.clone(),
                index,
            }
        })
    }

    /// Changes only the visual hover target and never focus, selection, scroll, or semantic data.
    pub(crate) fn hover_entry(&mut self, target: Option<EntryTarget>) {
        let target = target.filter(|target| {
            !self
                .selected_in(target.surface, &target.agent)
                .contains(target.index)
        });
        if self.disclosure.hover(target) {
            self.touch();
        }
    }

    /// Toggles the tool at the moving end of the current selection (`Ctrl-O`).
    pub(crate) fn toggle_selected_entry(
        &mut self,
        surfaces: &SurfaceTree,
        metrics: &TranscriptMetrics,
    ) {
        let target = self
            .selection
            .as_ref()
            .and_then(|selection| self.entry_target(selection.surface, selection.focus_index()));
        if let Some(target) = target {
            self.toggle_entry(surfaces, metrics, target, false);
        }
    }

    /// Makes a pointer-targeted tool the one-entry selection and toggles its disclosure.
    pub(crate) fn toggle_pointer_entry(
        &mut self,
        surfaces: &SurfaceTree,
        metrics: &TranscriptMetrics,
        target: EntryTarget,
    ) {
        self.toggle_entry(surfaces, metrics, target, true);
    }

    fn toggle_entry(
        &mut self,
        surfaces: &SurfaceTree,
        metrics: &TranscriptMetrics,
        target: EntryTarget,
        select: bool,
    ) {
        if self.agent_shown_by(target.surface).as_ref() != Some(&target.agent) {
            return;
        }
        let Some(index) = self.agents.get(&target.agent).and_then(|agent| {
            agent
                .entries()
                .position(|entry| entry.id() == &target.item && matches!(entry, TranscriptEntryView::Tool(tool) if tool.presentation.invocation.is_some() || tool.presentation.outcome.is_some()))
        }) else {
            return;
        };

        // Expanding is a reading action, so keep the semantic item at the top where the last frame
        // put it instead of letting Tail pull the viewport past the content that just grew (TR-3).
        if let Some(viewport) = surfaces.viewport(target.surface)
            && let Some(anchor) =
                metrics.anchor_at(&target.agent, viewport.content_width, viewport.offset)
        {
            self.scroll.park_conversation(target.agent.clone(), anchor);
        }
        if select {
            self.selection = Some(Selection::at(target.surface, target.agent.clone(), index));
            let _changed = self.disclosure.hover(None);
        }
        self.disclosure.toggle(target.item);
        self.touch();
    }
}
