use plexmaton_core::{
    AgentId, ConversationEntryId, ConversationId, HeadName, TreeEditAction, TreeHead, TreeLabel,
    TreeOrigin, TreePreview, TreeRevision, TreeRewindEligibility, TreeRow, TreeRowKind,
    TreeSnapshot,
};
use ratatui::layout::Rect;

use super::{ConversationTree, TreeMode, TreePending};
use crate::{
    intent::TextIntent,
    state::ViewState,
    surface::{Surface, SurfaceId, SurfaceKind, SurfaceTree},
};

fn id(value: &str) -> ConversationEntryId {
    ConversationEntryId::new(value).expect("entry id")
}

fn head(value: &str) -> HeadName {
    HeadName::new(value).expect("head name")
}

fn snapshot() -> TreeSnapshot {
    let agent_id = AgentId::new("primary").expect("agent id");
    let main = head("main");
    let old = head("old");
    let root = id("user-1");
    let child = id("assistant-2");
    let grandchild = id("user-3");
    TreeSnapshot {
        origin: TreeOrigin {
            conversation_id: ConversationId::new("conversation").expect("conversation id"),
            agent_id,
            selected_head: main.clone(),
            revision: TreeRevision::new(9),
        },
        heads: vec![
            TreeHead {
                name: main.clone(),
                target: Some(grandchild.clone()),
            },
            TreeHead {
                name: old.clone(),
                // A durable tip may be an audit atom without a displayed semantic message.
                target: Some(id("turn-status-tip")),
            },
        ],
        rows: vec![
            TreeRow {
                entry_id: root.clone(),
                parent_id: None,
                chronological_ordinal: 0,
                kind: TreeRowKind::User,
                preview: TreePreview {
                    text: "question".to_owned(),
                    truncated: false,
                },
                label: None,
                head_markers: Vec::new(),
                active_ancestry: true,
                rewind: TreeRewindEligibility::Eligible,
            },
            TreeRow {
                entry_id: child.clone(),
                parent_id: Some(root.clone()),
                chronological_ordinal: 1,
                kind: TreeRowKind::Assistant,
                preview: TreePreview {
                    text: "answer".to_owned(),
                    truncated: false,
                },
                label: None,
                head_markers: vec![old],
                active_ancestry: true,
                rewind: TreeRewindEligibility::Eligible,
            },
            TreeRow {
                entry_id: grandchild.clone(),
                parent_id: Some(child),
                chronological_ordinal: 2,
                kind: TreeRowKind::User,
                preview: TreePreview {
                    text: "next question".to_owned(),
                    truncated: false,
                },
                label: None,
                head_markers: vec![main],
                active_ancestry: true,
                rewind: TreeRewindEligibility::Eligible,
            },
        ],
    }
}

/// TRE-6: folding a selected row's ancestor moves selection to that visible ancestor.
#[test]
fn tre_6_folding_an_ancestor_keeps_selection_on_a_visible_identity() {
    let mut tree = ConversationTree::new(
        AgentId::new("primary").expect("agent"),
        Ok(snapshot()),
        SurfaceId::Composer,
    );
    tree.select_edge(true, 2);
    let selected = id("user-3");
    assert_eq!(tree.selected_entry(), Some(&selected));
    assert!(tree.toggle_fold(&id("user-1")));
    assert_eq!(tree.selected_entry(), Some(&id("user-1")));
    assert_eq!(tree.entries_count(), 1);
}

/// TRE-6: refresh follows identities and keeps independent cursors, folds and view mode.
#[test]
fn tre_6_refresh_retains_valid_cursor_fold_and_view_mode() {
    let mut tree = ConversationTree::new(
        AgentId::new("primary").expect("agent"),
        Ok(snapshot()),
        SurfaceId::Composer,
    );
    tree.toggle_fold(&id("user-1"));
    tree.toggle_mode();
    tree.refresh(Ok(snapshot()));
    assert_eq!(tree.mode(), TreeMode::Heads);
    assert!(tree.is_folded(&id("user-1")));
    assert_eq!(tree.selected_head().map(HeadName::as_str), Some("main"));
}

/// TRE-8: a branch cursor does not redefine the current-head marker in its snapshot.
#[test]
fn tre_8_active_head_is_distinct_from_the_selected_branch_cursor() {
    let mut tree = ConversationTree::new(
        AgentId::new("primary").expect("agent"),
        Ok(snapshot()),
        SurfaceId::Composer,
    );
    tree.toggle_mode();
    assert!(tree.hover_head(&head("old"), 2));
    assert_eq!(tree.selected_head().map(HeadName::as_str), Some("old"));
    assert_eq!(tree.active_head_name(), "main");
}

/// TRE-8: copy from a head resolves its nearest semantic marker, not an audit-only raw tip.
#[test]
fn tre_8_copy_from_a_head_uses_its_marked_semantic_row() {
    let mut tree = ConversationTree::new(
        AgentId::new("primary").expect("agent"),
        Ok(snapshot()),
        SurfaceId::Composer,
    );
    tree.toggle_mode();
    tree.hover_head(&head("old"), 2);
    let request = tree.source_request().expect("branch has a semantic marker");
    assert_eq!(request.entry_id, id("assistant-2"));
}

/// TRE-8: an empty branch or a head without a semantic row has no source to copy.
#[test]
fn tre_8_copy_from_a_head_without_a_semantic_marker_is_unavailable() {
    let mut tree = ConversationTree::new(
        AgentId::new("primary").expect("agent"),
        Ok(snapshot()),
        SurfaceId::Composer,
    );
    tree.toggle_mode();
    // The current branch marker is present; add and select a distinct head with no row marker.
    let mut projection = snapshot();
    projection.heads.push(TreeHead {
        name: head("empty"),
        target: None,
    });
    tree.refresh(Ok(projection));
    tree.hover_head(&head("empty"), 2);
    assert!(tree.source_request().is_none());
}

/// TRE-4: an acknowledged navigation cannot strand a still-open Drawer on the removed tree.
#[test]
fn tre_4_navigation_ack_rebases_drawer_return_focus_before_tree_disappears() {
    let agent = AgentId::new("primary").expect("agent id");
    let mut state = ViewState::default();
    let mut surfaces = SurfaceTree::default();
    for (id, kind, z_index) in [
        (SurfaceId::Composer, SurfaceKind::Composer, 0),
        (SurfaceId::ConversationTree, SurfaceKind::Modal, 1),
    ] {
        surfaces
            .insert(Surface {
                id,
                bounds: Rect::new(0, 0, 80, 24),
                z_index,
                kind,
                viewport: None,
            })
            .unwrap_or_else(|error| panic!("fixture must insert: {error}"));
    }

    state.show_conversation_tree(agent.clone(), Ok(snapshot()), SurfaceId::Composer);
    assert_eq!(state.focused(&surfaces), Some(SurfaceId::ConversationTree));
    assert!(state.open_drawer(&surfaces));
    assert_eq!(
        state.drawer().map(|drawer| drawer.return_focus()),
        Some(SurfaceId::ConversationTree)
    );
    state.tree_set_pending(Some(TreePending::Navigation));

    assert!(state.tree_complete_navigation(&agent, None));
    assert!(!state.conversation_tree_open());
    assert_eq!(
        state.drawer().map(|drawer| drawer.return_focus()),
        Some(SurfaceId::Composer)
    );
    assert!(state.close_drawer());
    // The next painted frame no longer registers the closed tree. Resolving focus against the
    // still-open modal would describe stale geometry rather than the acknowledged frame.
    let mut after_close = SurfaceTree::default();
    after_close
        .insert(Surface {
            id: SurfaceId::Composer,
            bounds: Rect::new(0, 0, 80, 24),
            z_index: 0,
            kind: SurfaceKind::Composer,
            viewport: None,
        })
        .unwrap_or_else(|error| panic!("fixture must insert: {error}"));
    assert_eq!(state.focused(&after_close), Some(SurfaceId::Composer));
}

/// TRE-8: labels are explicit optional metadata against a stable row and snapshot origin.
#[test]
fn tre_8_label_edits_name_the_selected_entry_and_allow_an_explicit_clear() {
    let origin = snapshot().origin;
    let active_tip = id("user-3");
    let mut tree = ConversationTree::new(
        AgentId::new("primary").expect("agent id"),
        Ok(snapshot()),
        SurfaceId::Composer,
    );
    assert!(tree.begin_set_label());
    assert!(tree.edit_input(TextIntent::Paste("a label".to_owned())));
    let request = tree
        .edit_request()
        .expect("valid label edit")
        .expect("changed");
    assert_eq!(request.origin, origin);
    assert_eq!(
        request.action,
        TreeEditAction::SetLabel {
            entry_id: active_tip.clone(),
            label: Some(TreeLabel::new("a label".to_owned()).expect("valid label")),
        }
    );

    tree.clear_editor();
    let mut projection = snapshot();
    projection
        .rows
        .iter_mut()
        .find(|row| row.entry_id == active_tip)
        .expect("active tip row")
        .label = Some(TreeLabel::new("old".to_owned()).expect("valid label"));
    tree.refresh(Ok(projection));
    assert!(tree.begin_set_label());
    assert!(tree.edit_input(TextIntent::KillToLineStart));
    let clear = tree.edit_request().expect("valid clear").expect("changed");
    assert_eq!(
        clear.action,
        TreeEditAction::SetLabel {
            entry_id: active_tip,
            label: None,
        }
    );
}

/// TRE-8: branch renames are bounded before a mutation leaves the TUI.
#[test]
fn tre_8_branch_rename_is_bound_to_256_utf8_bytes() {
    let mut tree = ConversationTree::new(
        AgentId::new("primary").expect("agent id"),
        Ok(snapshot()),
        SurfaceId::Composer,
    );
    tree.toggle_mode();
    assert!(tree.begin_rename_head());
    assert!(tree.edit_input(TextIntent::KillToLineStart));
    assert!(tree.edit_input(TextIntent::Paste("x".repeat(257))));
    assert_eq!(
        tree.edit_request(),
        Err("A branch name must fit in 256 UTF-8 bytes.".to_owned())
    );
}
