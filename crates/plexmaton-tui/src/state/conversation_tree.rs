//! Retained cursor, fold and pending-write state for one conversation-tree overlay.

use std::collections::BTreeSet;

use plexmaton_core::{
    AgentId, ConversationEntryId, HeadName, TreeNavigation, TreeNavigationTarget,
    TreeRewindEligibility, TreeRow, TreeSnapshot,
};

use crate::{state::TextInput, surface::SurfaceId};

#[path = "conversation_tree/api.rs"]
mod api;
#[path = "conversation_tree/operations.rs"]
mod operations;
#[path = "conversation_tree/presentation.rs"]
mod presentation;
#[cfg(test)]
#[path = "conversation_tree/tests.rs"]
mod tests;

/// Which stable identity is currently selected in the tree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TreeMode {
    Entries,
    Heads,
}

/// The kind of an admitted, still-unacknowledged tree write (TRE-4/TRE-8).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TreePending {
    Navigation,
    Edit,
}

/// A tree-local prompt that never borrows or mutates the conversation draft.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TreeEditor {
    RenameHead {
        head: HeadName,
        input: TextInput,
    },
    SetLabel {
        entry_id: ConversationEntryId,
        input: TextInput,
    },
    ConfirmAbandon(HeadName),
}

impl TreeEditor {
    pub(crate) fn input(&self) -> Option<&TextInput> {
        match self {
            Self::RenameHead { input, .. } | Self::SetLabel { input, .. } => Some(input),
            Self::ConfirmAbandon(_) => None,
        }
    }

    fn input_mut(&mut self) -> Option<&mut TextInput> {
        match self {
            Self::RenameHead { input, .. } | Self::SetLabel { input, .. } => Some(input),
            Self::ConfirmAbandon(_) => None,
        }
    }
}

/// One user's retained tree browsing session.
///
/// The projection is owned by the workspace view, while navigation and journal mutations stay at
/// the composition/runtime boundary. A pending write remains represented even if the user closes
/// the overlay, so dismissal never claims to undo an admitted mutation (TRE-4).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConversationTree {
    agent: AgentId,
    snapshot: Option<TreeSnapshot>,
    presentation: presentation::Presentation,
    unavailable: Option<String>,
    return_focus: SurfaceId,
    open: bool,
    pending: Option<TreePending>,
    editor: Option<TreeEditor>,
    mode: TreeMode,
    selected_entry: Option<ConversationEntryId>,
    selected_head: Option<HeadName>,
    folded: BTreeSet<ConversationEntryId>,
    notice: Option<String>,
    entry_offset: usize,
    head_offset: usize,
}

impl ConversationTree {
    pub(crate) fn new(
        agent: AgentId,
        snapshot: Result<TreeSnapshot, String>,
        return_focus: SurfaceId,
    ) -> Self {
        let mut tree = Self {
            agent,
            snapshot: None,
            presentation: presentation::Presentation::default(),
            unavailable: None,
            return_focus,
            open: true,
            pending: None,
            editor: None,
            mode: TreeMode::Entries,
            selected_entry: None,
            selected_head: None,
            folded: BTreeSet::new(),
            notice: None,
            entry_offset: 0,
            head_offset: 0,
        };
        tree.refresh(snapshot);
        tree
    }

    pub(crate) fn refresh(&mut self, snapshot: Result<TreeSnapshot, String>) {
        match snapshot {
            Ok(snapshot) if snapshot.origin.agent_id == self.agent => {
                let presentation = match presentation::Presentation::new(&snapshot.rows) {
                    Ok(presentation) => presentation,
                    Err(message) => {
                        self.snapshot = None;
                        self.presentation = presentation::Presentation::default();
                        self.unavailable = Some(message.to_owned());
                        self.entry_offset = 0;
                        self.head_offset = 0;
                        return;
                    }
                };
                self.presentation = presentation;
                self.snapshot = Some(snapshot);
                self.unavailable = None;
                self.notice = None;
                self.folded.retain(|id| self.presentation.has_children(id));
                self.reconcile_entries();
                self.reconcile_heads();
            }
            Ok(_) => {
                self.snapshot = None;
                self.unavailable =
                    Some("This snapshot belongs to a different conversation.".to_owned());
            }
            Err(message) => {
                self.snapshot = None;
                self.unavailable = Some(api::sanitize_message(&message));
            }
        }
        self.entry_offset = 0;
        self.head_offset = 0;
    }

    pub(crate) const fn agent(&self) -> &AgentId {
        &self.agent
    }

    pub(crate) const fn return_focus(&self) -> SurfaceId {
        self.return_focus
    }

    pub(crate) const fn is_open(&self) -> bool {
        self.open
    }

    pub(crate) const fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub(crate) const fn pending(&self) -> Option<TreePending> {
        self.pending
    }

    pub(crate) const fn mode(&self) -> TreeMode {
        self.mode
    }

    pub(crate) fn unavailable(&self) -> Option<&str> {
        self.unavailable.as_deref()
    }

    pub(crate) fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub(crate) fn editor(&self) -> Option<&TreeEditor> {
        self.editor.as_ref()
    }

    pub(crate) fn editor_input(&self) -> Option<&TextInput> {
        self.editor.as_ref().and_then(TreeEditor::input)
    }

    pub(crate) const fn selected_entry(&self) -> Option<&ConversationEntryId> {
        self.selected_entry.as_ref()
    }

    pub(crate) const fn selected_head(&self) -> Option<&HeadName> {
        self.selected_head.as_ref()
    }

    pub(crate) fn active_head_name(&self) -> &str {
        self.snapshot
            .as_ref()
            .map_or("", |snapshot| snapshot.origin.selected_head.as_str())
    }

    pub(crate) fn visible_entries(&self) -> Vec<&TreeRow> {
        let Some(snapshot) = &self.snapshot else {
            return Vec::new();
        };
        let mut hidden = BTreeSet::new();
        let mut visible = Vec::with_capacity(snapshot.rows.len());
        for index in &self.presentation.order {
            let row = &snapshot.rows[*index];
            let row_is_hidden = self
                .presentation
                .parent(&row.entry_id)
                .is_some_and(|parent| hidden.contains(parent) || self.folded.contains(parent));
            if row_is_hidden {
                hidden.insert(row.entry_id.clone());
            } else {
                visible.push(row);
            }
        }
        visible
    }

    pub(crate) fn rows(&self) -> &[TreeRow] {
        self.snapshot
            .as_ref()
            .map_or(&[], |snapshot| &snapshot.rows)
    }

    pub(crate) fn heads(&self) -> &[plexmaton_core::TreeHead] {
        self.snapshot
            .as_ref()
            .map_or(&[], |snapshot| &snapshot.heads)
    }

    pub(crate) fn is_folded(&self, id: &ConversationEntryId) -> bool {
        self.folded.contains(id)
    }

    pub(crate) fn has_children(&self, id: &ConversationEntryId) -> bool {
        self.presentation.has_children(id)
    }

    pub(crate) fn entries_count(&self) -> usize {
        self.visible_entries().len()
    }

    pub(crate) fn heads_count(&self) -> usize {
        self.heads().len()
    }

    pub(crate) fn scroll_offset(&self) -> usize {
        match self.mode {
            TreeMode::Entries => self.entry_offset,
            TreeMode::Heads => self.head_offset,
        }
    }

    pub(crate) fn selected_index(&self) -> Option<usize> {
        match self.mode {
            TreeMode::Entries => self.selected_entry.as_ref().and_then(|id| {
                self.visible_entries()
                    .iter()
                    .position(|row| &row.entry_id == id)
            }),
            TreeMode::Heads => self
                .selected_head
                .as_ref()
                .and_then(|name| self.heads().iter().position(|head| &head.name == name)),
        }
    }

    pub(crate) fn selected_entry_row(&self) -> Option<&TreeRow> {
        let selected = self.selected_entry.as_ref()?;
        self.visible_entries()
            .into_iter()
            .find(|row| &row.entry_id == selected)
    }

    pub(crate) fn set_pending(&mut self, pending: Option<TreePending>) {
        self.pending = pending;
        if pending.is_some() {
            self.notice = None;
        }
    }

    pub(crate) fn set_notice(&mut self, message: String) {
        self.pending = None;
        self.notice = Some(api::sanitize_message(&message));
    }

    pub(crate) fn set_open(&mut self, open: bool) {
        self.open = open;
    }

    pub(crate) fn clear_editor(&mut self) -> bool {
        let changed = self.editor.take().is_some();
        if changed {
            self.notice = None;
        }
        changed
    }

    pub(crate) fn toggle_mode(&mut self) -> bool {
        self.mode = match self.mode {
            TreeMode::Entries => TreeMode::Heads,
            TreeMode::Heads => TreeMode::Entries,
        };
        true
    }

    pub(crate) fn move_cursor(&mut self, forward: bool, steps: usize, visible_rows: usize) -> bool {
        let count = match self.mode {
            TreeMode::Entries => self.entries_count(),
            TreeMode::Heads => self.heads_count(),
        };
        if count == 0 {
            return false;
        }
        let current = self.selected_index().unwrap_or(0);
        let next = if forward {
            current.saturating_add(steps).min(count.saturating_sub(1))
        } else {
            current.saturating_sub(steps)
        };
        if next == current {
            return false;
        }
        self.select_index(next, visible_rows);
        true
    }

    pub(crate) fn select_edge(&mut self, end: bool, visible_rows: usize) -> bool {
        let count = match self.mode {
            TreeMode::Entries => self.entries_count(),
            TreeMode::Heads => self.heads_count(),
        };
        let Some(index) = (count > 0).then_some(if end { count - 1 } else { 0 }) else {
            return false;
        };
        if self.selected_index() == Some(index) {
            return false;
        }
        self.select_index(index, visible_rows);
        true
    }

    pub(crate) fn hover_entry(&mut self, id: &ConversationEntryId, visible_rows: usize) -> bool {
        if self.mode != TreeMode::Entries
            || !self.visible_entries().iter().any(|row| &row.entry_id == id)
        {
            return false;
        }
        let changed = self.selected_entry.as_ref() != Some(id);
        self.selected_entry = Some(id.clone());
        if changed {
            self.ensure_visible(visible_rows);
        }
        changed
    }

    pub(crate) fn hover_head(&mut self, name: &HeadName, visible_rows: usize) -> bool {
        if self.mode != TreeMode::Heads || !self.heads().iter().any(|head| &head.name == name) {
            return false;
        }
        let changed = self.selected_head.as_ref() != Some(name);
        self.selected_head = Some(name.clone());
        if changed {
            self.ensure_visible(visible_rows);
        }
        changed
    }

    pub(crate) fn toggle_fold(&mut self, id: &ConversationEntryId) -> bool {
        if self.mode != TreeMode::Entries || !self.has_children(id) {
            return false;
        }
        if !self.folded.remove(id) {
            self.folded.insert(id.clone());
        }
        self.reconcile_entries();
        true
    }

    pub(crate) fn navigation(&mut self) -> Option<TreeNavigation> {
        if self.is_pending() {
            self.notice = Some("Navigation is already waiting for the journal.".to_owned());
            return None;
        }
        let Some(snapshot) = &self.snapshot else {
            self.notice = Some("History is unavailable. Press r to refresh.".to_owned());
            return None;
        };
        let target = match self.mode {
            TreeMode::Entries => {
                let Some(row) = self.selected_entry_row() else {
                    self.notice = Some("This conversation has no rewind target yet.".to_owned());
                    return None;
                };
                if row.rewind != TreeRewindEligibility::Eligible {
                    return None;
                }
                TreeNavigationTarget::Rewind(row.entry_id.clone())
            }
            TreeMode::Heads => {
                let Some(head) = self
                    .selected_head
                    .as_ref()
                    .filter(|name| snapshot.heads.iter().any(|head| &head.name == *name))
                else {
                    self.notice = Some("This conversation has no named branches.".to_owned());
                    return None;
                };
                TreeNavigationTarget::SelectHead(head.clone())
            }
        };
        self.notice = None;
        Some(TreeNavigation {
            origin: snapshot.origin.clone(),
            target,
        })
    }

    fn select_index(&mut self, index: usize, visible_rows: usize) {
        match self.mode {
            TreeMode::Entries => {
                if let Some(row) = self.visible_entries().get(index) {
                    self.selected_entry = Some(row.entry_id.clone());
                }
            }
            TreeMode::Heads => {
                if let Some(head) = self.heads().get(index) {
                    self.selected_head = Some(head.name.clone());
                }
            }
        }
        self.ensure_visible(visible_rows);
    }

    fn ensure_visible(&mut self, visible_rows: usize) {
        let visible_rows = visible_rows.max(1);
        let Some(index) = self.selected_index() else {
            return;
        };
        let count = match self.mode {
            TreeMode::Entries => self.entries_count(),
            TreeMode::Heads => self.heads_count(),
        };
        let offset = match self.mode {
            TreeMode::Entries => &mut self.entry_offset,
            TreeMode::Heads => &mut self.head_offset,
        };
        if index < *offset {
            *offset = index;
        } else if index >= offset.saturating_add(visible_rows) {
            *offset = index.saturating_add(1).saturating_sub(visible_rows);
        }
        *offset = (*offset).min(count.saturating_sub(visible_rows));
    }

    fn reconcile_entries(&mut self) {
        let visible = self.visible_entries();
        let ids: BTreeSet<_> = visible.iter().map(|row| row.entry_id.clone()).collect();
        if self
            .selected_entry
            .as_ref()
            .is_some_and(|id| ids.contains(id))
        {
            return;
        }
        let ancestor = self
            .selected_entry
            .as_ref()
            .and_then(|id| self.visible_ancestor(id, &ids));
        let active_target = self.snapshot.as_ref().and_then(|snapshot| {
            let target = snapshot
                .rows
                .iter()
                .find(|row| row.head_markers.contains(&snapshot.origin.selected_head))?;
            if ids.contains(&target.entry_id) {
                Some(target.entry_id.clone())
            } else {
                self.visible_ancestor(&target.entry_id, &ids)
            }
        });
        self.selected_entry = ancestor
            .or(active_target)
            .or_else(|| visible.first().map(|row| row.entry_id.clone()));
        self.entry_offset = 0;
    }

    fn visible_ancestor(
        &self,
        entry: &ConversationEntryId,
        visible: &BTreeSet<ConversationEntryId>,
    ) -> Option<ConversationEntryId> {
        let mut current = self.presentation.anchor(entry)?;
        for _ in 0..=self.presentation.order.len() {
            if visible.contains(current) {
                return Some(current.clone());
            }
            current = self.presentation.parent(current)?;
        }
        None
    }

    fn reconcile_heads(&mut self) {
        let heads = self.heads();
        if self
            .selected_head
            .as_ref()
            .is_some_and(|name| heads.iter().any(|head| &head.name == name))
        {
            return;
        }
        self.selected_head = self.snapshot.as_ref().and_then(|snapshot| {
            heads
                .iter()
                .find(|head| head.name == snapshot.origin.selected_head)
                .or_else(|| heads.first())
                .map(|head| head.name.clone())
        });
        self.head_offset = 0;
    }
}
