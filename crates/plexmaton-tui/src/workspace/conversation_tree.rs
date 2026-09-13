//! Native tree intent composition. Durable facts and exact source cross the runtime boundary.

use plexmaton_core::{AgentId, TreeEdit, TreeNavigation, TreeSnapshot, TreeSourceRequest};

use super::{Outcome, Workspace};
use crate::{
    intent::{Direction, PointerIntent, ScrollDirection, TreeIntent},
    render::conversation_tree::{self, Hit},
    state::TreePending,
    surface::{Point, SurfaceId},
};

/// An addressed tree operation for the acknowledged journal owner (TRE-3/TRE-4/TRE-8).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TreeRequest {
    /// Read the current acknowledged tree for this conversation.
    Refresh(AgentId),
    /// Navigate to the exact head/message selected from a snapshot.
    Navigate(TreeNavigation),
    /// Change branch or message metadata without replacing conversation context.
    Edit(TreeEdit),
    /// Read exact semantic source, never rendered row cells.
    Copy(TreeSourceRequest),
}

impl Workspace {
    /// Opens or refreshes one modal without touching the caller's draft or skill binding.
    pub fn show_conversation_tree(
        &mut self,
        agent: AgentId,
        snapshot: Result<TreeSnapshot, String>,
    ) {
        let return_focus = self
            .state
            .focused(&self.surfaces)
            .unwrap_or(SurfaceId::Composer);
        self.cancel_tree_gesture();
        self.state
            .show_conversation_tree(agent, snapshot, return_focus);
    }

    /// A navigation was admitted; only its acknowledged receipt may replace the visible context.
    pub fn mark_tree_navigation_pending(&mut self) {
        self.state.tree_set_pending(Some(TreePending::Navigation));
    }

    /// The runtime already replaced projection; restore an empty draft and finish the owned view.
    pub fn complete_tree_navigation(
        &mut self,
        agent: &AgentId,
        returned: Option<(String, Option<String>)>,
    ) {
        if self.state.tree_complete_navigation(agent, returned) {
            self.cancel_tree_gesture();
        }
    }

    /// A typed refusal or failed write leaves draft/history untouched.
    pub fn report_tree_navigation_refusal(&mut self, message: String) {
        self.state.tree_refuse_navigation(message);
    }

    /// Metadata writes share ownership with navigation, but their receipt does not close browsing.
    pub fn mark_tree_edit_pending(&mut self) {
        self.state.tree_set_pending(Some(TreePending::Edit));
    }

    /// Refreshes the acknowledged metadata without reopening a view that the user dismissed.
    pub fn complete_tree_edit(&mut self, agent: &AgentId, snapshot: Result<TreeSnapshot, String>) {
        if self.state.tree_complete_edit(agent, snapshot) {
            self.pressed = None;
        }
    }

    /// Retains a failed metadata editor so its source can be corrected or dismissed.
    pub fn report_tree_edit_refusal(&mut self, message: String) {
        self.state.tree_report_edit_refusal(message);
    }

    /// Resolves any accepted tree operation once when its durable writer requires reopening.
    pub fn report_tree_persistence_failure(&mut self) {
        self.state.tree_refuse_navigation(
            "History could not be saved. Reopen this session before continuing.".to_owned(),
        );
    }

    pub(super) fn apply_tree(&mut self, intent: TreeIntent) -> Outcome {
        if self.state.tree_navigation_pending() {
            // Escape can dismiss an accepted write's view, not undo its durable operation.
            if matches!(intent, TreeIntent::Close | TreeIntent::CancelEdit) {
                self.close_tree();
            }
            return Outcome::default();
        }
        let visible_rows = self.tree_visible_rows();
        let request = match intent {
            TreeIntent::Move(direction) => {
                self.state
                    .tree_move_cursor(direction == Direction::Forward, 1, visible_rows);
                None
            }
            TreeIntent::Home | TreeIntent::End => {
                self.state
                    .tree_select_edge(matches!(intent, TreeIntent::End), visible_rows);
                None
            }
            TreeIntent::ToggleBranches => {
                self.state.tree_toggle_mode();
                None
            }
            TreeIntent::ToggleFold => {
                self.state.tree_toggle_fold();
                None
            }
            TreeIntent::Navigate => self.state.tree_navigation().map(TreeRequest::Navigate),
            TreeIntent::Refresh => self.state.tree_refresh_request().map(TreeRequest::Refresh),
            TreeIntent::CopySource => self.state.tree_source_request().map(TreeRequest::Copy),
            TreeIntent::RenameHead => {
                self.state.tree_begin_rename_head();
                None
            }
            TreeIntent::EditLabel => {
                self.state.tree_begin_set_label();
                None
            }
            TreeIntent::AbandonHead => {
                self.state.tree_begin_abandon_head();
                None
            }
            TreeIntent::EditInput(input) => {
                self.state.tree_edit_input(input);
                None
            }
            TreeIntent::SubmitEdit => match self.state.tree_edit_request() {
                Ok(Some(edit)) => Some(TreeRequest::Edit(edit)),
                Ok(None) => {
                    self.state.tree_cancel_editor();
                    None
                }
                Err(error) => {
                    self.state.tree_notice(error);
                    None
                }
            },
            TreeIntent::CancelEdit => {
                self.state.tree_cancel_editor();
                None
            }
            TreeIntent::Close => {
                self.close_tree();
                None
            }
        };
        Outcome {
            tree: request,
            ..Outcome::default()
        }
    }

    fn close_tree(&mut self) {
        if self.state.close_conversation_tree() {
            self.cancel_tree_gesture();
        }
    }

    fn cancel_tree_gesture(&mut self) {
        if let Some(surface) = self.router.capture() {
            let _ = self.pointer(PointerIntent::Cancel { surface }, std::time::Instant::now());
        }
        self.router = crate::Router::default();
        self.pressed = None;
        self.pressed_entry = None;
        self.drag_autoscroll = None;
        self.hover_point = None;
    }

    pub(super) fn tree_visible_rows(&self) -> usize {
        self.surfaces
            .get(SurfaceId::ConversationTree)
            .map_or(1, |surface| {
                conversation_tree::row_capacity(&self.state, surface.bounds)
            })
    }

    pub(super) fn tree_scroll(&mut self, direction: ScrollDirection) {
        self.state.tree_move_cursor(
            direction == ScrollDirection::Down,
            1,
            self.tree_visible_rows(),
        );
    }

    pub(super) fn tree_hit(&self, at: Point) -> Option<Hit> {
        if !self.state.conversation_tree_open() {
            return None;
        }
        let bounds = self.surfaces.get(SurfaceId::ConversationTree)?.bounds;
        let hit = conversation_tree::hit(&self.state, bounds, at)?;
        // Child editors and pending writes hide row activation while retaining a real close path.
        (matches!(hit, Hit::Close)
            || (!self.state.tree_editor_open() && !self.state.tree_navigation_pending()))
        .then_some(hit)
    }

    pub(super) fn hover_tree(&mut self, hit: &Hit) {
        let visible_rows = self.tree_visible_rows();
        match hit {
            Hit::Entry(id) | Hit::Fold(id) => self.state.tree_hover_entry(id, visible_rows),
            Hit::Head(name) => self.state.tree_hover_head(name, visible_rows),
            Hit::Close => {}
        }
    }

    pub(super) fn activate_tree(&mut self, hit: &Hit) -> Outcome {
        self.hover_tree(hit);
        match hit {
            Hit::Close => self.apply_tree(TreeIntent::Close),
            Hit::Fold(_) => self.apply_tree(TreeIntent::ToggleFold),
            // A click chooses the same row as hover/arrows; Enter is the explicit navigation.
            Hit::Entry(_) | Hit::Head(_) => Outcome::default(),
        }
    }
}
