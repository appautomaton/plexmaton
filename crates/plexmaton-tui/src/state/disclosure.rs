//! TUI-only disclosure and hover state for transcript entries.
//!
//! A tool's semantic detail lives on its transcript entry. This module remembers only how the
//! user asked to view that source: open identities and the one entry under the pointer. Neither is
//! a session event, provider replay fact, or second transcript (ENT-4).

use std::collections::BTreeSet;

use plexmaton_core::{AgentId, TranscriptItemId};

use super::{TranscriptEntryView, ViewState};
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
    pub(crate) copy_hovered: bool,
}

impl EntryAppearance {
    #[cfg(test)]
    pub(crate) const fn compact(selected: bool) -> Self {
        Self {
            selected,
            open: false,
            hovered: false,
            copy_hovered: false,
        }
    }
}

/// Presentation choices that never enter semantic replay.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct DisclosureState {
    open: BTreeSet<TranscriptItemId>,
    hovered: Option<HoverTarget>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum HoverTarget {
    Approval {
        approval: super::ApprovalTarget,
        stage: crate::ApprovalStage,
        choice: crate::ApprovalChoice,
    },
    Entry {
        target: EntryTarget,
        copy_button: bool,
    },
    Retry {
        item: TranscriptItemId,
        command: super::RetryAction,
    },
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

    fn hover(
        &mut self,
        target: Option<EntryTarget>,
        copy: bool,
        retry: Option<(TranscriptItemId, super::RetryAction)>,
        approval: Option<(
            super::ApprovalTarget,
            crate::ApprovalStage,
            crate::ApprovalChoice,
        )>,
    ) -> bool {
        let target = if let Some((approval, stage, choice)) = approval {
            Some(HoverTarget::Approval {
                approval,
                stage,
                choice,
            })
        } else if let Some((item, command)) = retry {
            Some(HoverTarget::Retry { item, command })
        } else {
            target.map(|target| HoverTarget::Entry {
                target,
                copy_button: copy,
            })
        };
        if self.hovered == target {
            return false;
        }
        self.hovered = target;
        true
    }

    fn is_hovered(&self, surface: SurfaceId, agent: &AgentId, item: &TranscriptItemId) -> bool {
        matches!(&self.hovered, Some(HoverTarget::Entry { target, .. })
            if target.surface == surface && &target.agent == agent && &target.item == item)
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
            copy_hovered: matches!(
                self.disclosure.hovered,
                Some(HoverTarget::Entry {
                    copy_button: true,
                    ..
                })
            ) && self.disclosure.is_hovered(surface, agent, item),
        }
    }

    /// Resolves a message action, disclosure or drag anchor at one semantic entry index.
    pub(crate) fn entry_target(&self, surface: SurfaceId, index: usize) -> Option<EntryTarget> {
        let agent = self.agent_shown_by(surface)?;
        let entry = self.agents.get(&agent)?.entries().nth(index)?;
        Some(EntryTarget {
            surface,
            agent,
            item: entry.id().clone(),
            index,
        })
    }

    /// Changes only the visual hover target and never focus, selection, scroll, or semantic data.
    pub(crate) fn hover_entry(&mut self, target: Option<EntryTarget>) {
        self.hover_controls(target, false, None, None);
    }

    pub(crate) fn hover_controls(
        &mut self,
        target: Option<EntryTarget>,
        copy: bool,
        retry: Option<(TranscriptItemId, super::RetryAction)>,
        approval: Option<(
            super::ApprovalTarget,
            crate::ApprovalStage,
            crate::ApprovalChoice,
        )>,
    ) {
        let target = target.filter(|target| {
            !self
                .selected_in(target.surface, &target.agent)
                .contains(target.index)
        });
        if self.disclosure.hover(target, copy, retry, approval) {
            self.touch();
        }
    }

    pub(crate) fn approval_hovered(&self, choice: crate::ApprovalChoice) -> bool {
        self.approval().is_some_and(|view| {
            matches!(&self.disclosure.hovered,
            Some(HoverTarget::Approval { approval, stage, choice: hovered })
                if approval.matches(&view) && *stage == view.stage && *hovered == choice)
        })
    }

    pub(crate) fn retry_hovered(&self, item: &TranscriptItemId) -> Option<super::RetryAction> {
        match &self.disclosure.hovered {
            Some(HoverTarget::Retry { item: id, command }) if id == item => Some(*command),
            _ => None,
        }
    }

    pub(crate) fn hovered_message(&self, surface: SurfaceId) -> Option<&EntryTarget> {
        let Some(HoverTarget::Entry { target, .. }) = &self.disclosure.hovered else {
            return None;
        };
        (target.surface == surface && self.message_source(target).is_some()).then_some(target)
    }

    pub(crate) fn copy_message(&self, target: &EntryTarget) -> Option<super::CopyRequest> {
        self.message_source(target).map(|text| super::CopyRequest {
            text: text.to_owned(),
            entries: 1,
        })
    }

    pub(crate) fn message_source(&self, target: &EntryTarget) -> Option<&str> {
        if self.agent_shown_by(target.surface).as_ref() != Some(&target.agent) {
            return None;
        }
        let entry = self
            .agents
            .get(&target.agent)?
            .entries()
            .find(|entry| entry.id() == &target.item)?;
        let TranscriptEntryView::Text(item) = entry else {
            return None;
        };
        Some(&item.source)
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
            self.toggle_entry(surfaces, metrics, target);
        }
    }

    /// Disclosure is a reading action, not a copy selection. Both pointer and keyboard targets
    /// resolve by stable entry identity, while only explicit selection gestures select source.
    pub(crate) fn toggle_entry(
        &mut self,
        surfaces: &SurfaceTree,
        metrics: &TranscriptMetrics,
        target: EntryTarget,
    ) {
        if self.agent_shown_by(target.surface).as_ref() != Some(&target.agent) {
            return;
        }
        let retained_tool = self.agents.get(&target.agent).is_some_and(|agent| {
            agent
                .entries()
                .any(|entry| entry.id() == &target.item && matches!(entry, TranscriptEntryView::Tool(tool) if tool.presentation.invocation.is_some() || tool.presentation.outcome.is_some()))
        });
        if !retained_tool {
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
        // Changed geometry invalidates the old under-pointer target; it is not keyboard focus.
        let _changed = self.disclosure.hover(None, false, None, None);
        self.disclosure.toggle(target.item);
        self.touch();
    }
}
