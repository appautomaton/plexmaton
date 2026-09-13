//! Reducer-facing operations for a retained conversation-tree session (TRE-4/TRE-6/TRE-8).

use plexmaton_core::{
    AgentId, ConversationEntryId, HeadName, TreeEdit, TreeNavigation, TreeSnapshot,
    TreeSourceRequest,
};

use crate::{intent::TextIntent, surface::SurfaceId};

use super::super::ViewState;
use super::{ConversationTree, TreePending};

pub(super) fn sanitize_message(message: &str) -> String {
    message
        .chars()
        .filter(|character| !character.is_control())
        .take(180)
        .collect()
}

impl ViewState {
    /// Whether the retained tree overlay is open, including when a too-small terminal has no tree
    /// surface registered yet.
    pub(crate) fn conversation_tree_open(&self) -> bool {
        self.conversation_tree
            .as_ref()
            .is_some_and(ConversationTree::is_open)
    }

    /// Whether an admitted navigation write still awaits its journal acknowledgment.
    pub(crate) fn tree_navigation_pending(&self) -> bool {
        self.conversation_tree
            .as_ref()
            .is_some_and(ConversationTree::is_pending)
    }

    pub(crate) fn show_conversation_tree(
        &mut self,
        agent: AgentId,
        snapshot: Result<TreeSnapshot, String>,
        return_focus: SurfaceId,
    ) {
        match self
            .conversation_tree
            .as_mut()
            .filter(|tree| tree.agent() == &agent)
        {
            Some(tree) => {
                tree.refresh(snapshot);
                tree.set_open(true);
            }
            None => {
                self.conversation_tree = Some(ConversationTree::new(agent, snapshot, return_focus));
            }
        }
        self.focus.prefer(SurfaceId::ConversationTree);
        self.touch();
    }

    pub(crate) fn tree(&self) -> Option<&ConversationTree> {
        self.conversation_tree.as_ref()
    }

    pub(crate) fn close_conversation_tree(&mut self) -> bool {
        let Some(tree) = self
            .conversation_tree
            .as_mut()
            .filter(|tree| tree.is_open())
        else {
            return false;
        };
        let return_focus = tree.return_focus();
        tree.clear_editor();
        tree.set_open(false);
        self.focus.prefer(return_focus);
        self.touch();
        true
    }

    pub(crate) fn tree_notice(&mut self, message: String) -> bool {
        let Some(tree) = self
            .conversation_tree
            .as_mut()
            .filter(|tree| tree.is_open())
        else {
            return false;
        };
        tree.set_notice(message);
        self.touch();
        true
    }

    pub(crate) fn tree_refuse_navigation(&mut self, message: String) -> bool {
        let Some(tree) = self.conversation_tree.as_mut() else {
            return false;
        };
        if !tree.is_open() && !tree.is_pending() {
            return false;
        }
        tree.set_pending(None);
        if tree.is_open() {
            tree.set_notice(message);
        }
        self.touch();
        true
    }

    pub(crate) fn tree_navigation(&mut self) -> Option<TreeNavigation> {
        let tree = self
            .conversation_tree
            .as_mut()
            .filter(|tree| tree.is_open() && !tree.is_pending() && tree.editor().is_none())?;
        let result = tree.navigation();
        self.touch();
        result
    }

    pub(crate) fn tree_refresh_request(&self) -> Option<AgentId> {
        self.conversation_tree
            .as_ref()
            .filter(|tree| tree.is_open() && !tree.is_pending() && tree.editor().is_none())
            .map(|tree| tree.agent().clone())
    }

    pub(crate) fn tree_move_cursor(&mut self, forward: bool, steps: usize, visible_rows: usize) {
        if self
            .conversation_tree
            .as_mut()
            .filter(|tree| tree.is_open() && !tree.is_pending() && tree.editor().is_none())
            .is_some_and(|tree| tree.move_cursor(forward, steps, visible_rows))
        {
            self.touch();
        }
    }

    pub(crate) fn tree_select_edge(&mut self, end: bool, visible_rows: usize) {
        if self
            .conversation_tree
            .as_mut()
            .filter(|tree| tree.is_open() && !tree.is_pending() && tree.editor().is_none())
            .is_some_and(|tree| tree.select_edge(end, visible_rows))
        {
            self.touch();
        }
    }

    pub(crate) fn tree_toggle_mode(&mut self) {
        if let Some(tree) = self
            .conversation_tree
            .as_mut()
            .filter(|tree| tree.is_open() && !tree.is_pending() && tree.editor().is_none())
        {
            tree.toggle_mode();
            self.touch();
        }
    }

    pub(crate) fn tree_toggle_fold(&mut self) {
        let changed = self
            .conversation_tree
            .as_mut()
            .filter(|tree| tree.is_open() && !tree.is_pending() && tree.editor().is_none())
            .is_some_and(|tree| {
                tree.selected_entry()
                    .cloned()
                    .is_some_and(|id| tree.toggle_fold(&id))
            });
        if changed {
            self.touch();
        }
    }

    pub(crate) fn tree_hover_entry(&mut self, id: &ConversationEntryId, visible_rows: usize) {
        if self
            .conversation_tree
            .as_mut()
            .filter(|tree| tree.is_open() && !tree.is_pending() && tree.editor().is_none())
            .is_some_and(|tree| tree.hover_entry(id, visible_rows))
        {
            self.touch();
        }
    }

    pub(crate) fn tree_hover_head(&mut self, name: &HeadName, visible_rows: usize) {
        if self
            .conversation_tree
            .as_mut()
            .filter(|tree| tree.is_open() && !tree.is_pending() && tree.editor().is_none())
            .is_some_and(|tree| tree.hover_head(name, visible_rows))
        {
            self.touch();
        }
    }

    pub(crate) fn tree_complete_navigation(
        &mut self,
        agent: &AgentId,
        returned: Option<(String, Option<String>)>,
    ) -> bool {
        let Some((return_focus, overlay_was_open)) = self
            .conversation_tree
            .as_ref()
            .filter(|tree| tree.agent() == agent)
            .map(|tree| (tree.return_focus(), tree.is_open()))
        else {
            return false;
        };
        // A Drawer opened while the tree owns focus remembers the tree as its return target. The
        // ACK removes that target, so rebase the Drawer before the tree surface disappears.
        if let Some(drawer) = self.drawer.as_mut() {
            drawer.replace_return_focus(SurfaceId::ConversationTree, return_focus);
        }
        if let Some(tree) = self
            .conversation_tree
            .as_mut()
            .filter(|tree| tree.agent() == agent)
        {
            tree.set_pending(None);
            tree.clear_editor();
            tree.set_open(false);
        }
        // A Drawer opened over a pending tree is the new visible focus owner. Returning the
        // composer here would steal focus from that workspace-level interaction.
        if overlay_was_open && self.drawer().is_none() {
            self.focus.prefer(return_focus);
        }
        if let Some((text, skill)) = returned
            && self.draft(agent).text().is_empty()
        {
            self.return_skill_input(agent.clone(), text, skill);
        }
        self.touch();
        true
    }

    pub(crate) fn tree_set_pending(&mut self, pending: Option<TreePending>) {
        let changed = self.conversation_tree.as_mut().is_some_and(|tree| {
            let changed = tree.pending() != pending;
            tree.set_pending(pending);
            changed
        });
        if changed {
            self.touch();
        }
    }

    pub(crate) fn tree_complete_edit(
        &mut self,
        agent: &AgentId,
        snapshot: Result<TreeSnapshot, String>,
    ) -> bool {
        let Some(tree) = self
            .conversation_tree
            .as_mut()
            .filter(|tree| tree.agent() == agent)
        else {
            return false;
        };
        tree.set_pending(None);
        tree.clear_editor();
        tree.refresh(snapshot);
        self.touch();
        true
    }

    pub(crate) fn tree_report_edit_refusal(&mut self, message: String) -> bool {
        let Some(tree) = self.conversation_tree.as_mut() else {
            return false;
        };
        if tree.pending() != Some(TreePending::Edit) && !tree.is_open() {
            return false;
        }
        tree.set_notice(message);
        self.touch();
        true
    }

    pub(crate) fn tree_begin_rename_head(&mut self) -> bool {
        let changed = self
            .conversation_tree
            .as_mut()
            .filter(|tree| tree.is_open())
            .is_some_and(ConversationTree::begin_rename_head);
        if changed {
            self.touch();
        }
        changed
    }

    pub(crate) fn tree_begin_set_label(&mut self) -> bool {
        let changed = self
            .conversation_tree
            .as_mut()
            .filter(|tree| tree.is_open())
            .is_some_and(ConversationTree::begin_set_label);
        if changed {
            self.touch();
        }
        changed
    }

    pub(crate) fn tree_begin_abandon_head(&mut self) -> bool {
        let changed = self
            .conversation_tree
            .as_mut()
            .filter(|tree| tree.is_open())
            .is_some_and(ConversationTree::begin_abandon_head);
        if changed {
            self.touch();
        }
        changed
    }

    pub(crate) fn tree_edit_input(&mut self, intent: TextIntent) {
        if self
            .conversation_tree
            .as_mut()
            .filter(|tree| tree.is_open() && !tree.is_pending())
            .is_some_and(|tree| tree.edit_input(intent))
        {
            self.touch();
        }
    }

    pub(crate) fn tree_edit_request(&self) -> Result<Option<TreeEdit>, String> {
        self.conversation_tree
            .as_ref()
            .filter(|tree| tree.is_open() && !tree.is_pending())
            .ok_or_else(|| "The conversation tree is closed or busy.".to_owned())?
            .edit_request()
    }

    pub(crate) fn tree_cancel_editor(&mut self) -> bool {
        let changed = self
            .conversation_tree
            .as_mut()
            .filter(|tree| tree.is_open() && !tree.is_pending())
            .is_some_and(ConversationTree::clear_editor);
        if changed {
            self.touch();
        }
        changed
    }

    pub(crate) fn tree_source_request(&mut self) -> Option<TreeSourceRequest> {
        let request = self
            .conversation_tree
            .as_ref()
            .filter(|tree| tree.is_open() && !tree.is_pending() && tree.editor().is_none())
            .and_then(ConversationTree::source_request);
        if request.is_none()
            && let Some(tree) = self
                .conversation_tree
                .as_mut()
                .filter(|tree| tree.is_open())
        {
            tree.set_notice("No message source is available for this selection.".to_owned());
            self.touch();
        }
        request
    }

    pub(crate) fn tree_text_editor_open(&self) -> bool {
        self.conversation_tree
            .as_ref()
            .filter(|tree| tree.is_open())
            .is_some_and(|tree| tree.editor_input().is_some())
    }

    pub(crate) fn tree_editor_open(&self) -> bool {
        self.conversation_tree
            .as_ref()
            .filter(|tree| tree.is_open())
            .is_some_and(|tree| tree.editor().is_some())
    }
}
