use plexmaton_core::{
    AgentId, ConversationEntryId, HeadName, TreeNavigation, TreeNavigationTarget, TreeRowKind,
};

use super::*;
use crate::{Input, JournalError, JournalRecord};

fn fixture() -> (Agent, ConversationEntryId) {
    let mut agent = Agent::new(AgentId::new("tree-editor").expect("agent"));
    let _ = agent.announce("Tree editor");
    let _ = agent.handle(Input::Submitted {
        text: "**exact** 中文\r\n  source".to_owned(),
    });
    let _ = agent.handle(Input::Interrupted);
    let entry = agent
        .journal()
        .tree_snapshot(&agent.tree_origin().agent_id)
        .expect("tree")
        .rows
        .into_iter()
        .find(|row| row.kind == TreeRowKind::User)
        .expect("user")
        .entry_id;
    (agent, entry)
}

fn edit(agent: &mut Agent, action: TreeEditAction) -> Result<Reaction, TreeEditRefusal> {
    agent.edit_tree(&TreeEdit {
        origin: agent.tree_origin(),
        action,
    })
}

fn assert_metadata_only(reaction: &Reaction) {
    assert!(reaction.events.is_empty());
    assert!(reaction.effects.is_empty());
    assert!(reaction.projection_reset.is_none());
    assert!(reaction.tree_navigation.is_none());
    assert!(reaction.undelivered.is_empty());
}

/// TRE-8: metadata cannot materialize a blank automatic session or poison its first input write.
#[test]
fn tre_8_empty_history_refuses_metadata_without_a_write() {
    let mut agent = Agent::new(AgentId::new("blank-tree").expect("agent"));
    let _ = agent.announce("Blank tree");
    let before = agent.journal().clone();
    let head = agent.selected_head().clone();
    for action in [
        TreeEditAction::RenameHead {
            head: head.clone(),
            renamed: HeadName::new("renamed").expect("head"),
        },
        TreeEditAction::AbandonHead { head },
        TreeEditAction::SetLabel {
            entry_id: ConversationEntryId::new("missing").expect("entry"),
            label: None,
        },
    ] {
        assert_eq!(edit(&mut agent, action), Err(TreeEditRefusal::EmptyTree));
        assert_eq!(agent.journal(), &before);
    }
}

/// TRE-8: set/clear annotations change only journal metadata and the tree revision, never immutable
/// entries, head revisions, request accounting, context epochs or replayed events.
#[test]
fn tre_8_label_set_clear_and_noop_leave_context_and_accounting_identical() {
    let (mut agent, entry_id) = fixture();
    let original = agent.tree_origin();
    let head_revision = agent
        .journal()
        .head_revision(agent.selected_head())
        .expect("head");
    let projection = agent
        .journal()
        .project(agent.selected_head())
        .expect("projection");
    let label = TreeLabel::new("Review 中文  ".to_owned()).expect("label");
    let action = TreeEditAction::SetLabel {
        entry_id: entry_id.clone(),
        label: Some(label.clone()),
    };
    let changed = edit(&mut agent, action.clone()).expect("set label");
    assert_metadata_only(&changed);
    assert!(matches!(
        changed.records.as_slice(),
        [JournalRecord::SetEntryLabel { .. }]
    ));
    assert_eq!(agent.journal().tree_label(&entry_id), Some(&label));
    assert_ne!(agent.tree_origin().revision, original.revision);
    assert_eq!(
        agent
            .journal()
            .head_revision(agent.selected_head())
            .expect("head"),
        head_revision
    );
    assert_eq!(
        agent
            .journal()
            .project(agent.selected_head())
            .expect("unchanged context"),
        projection
    );
    let no_op = edit(&mut agent, action.clone()).expect("same label");
    assert_metadata_only(&no_op);
    assert!(no_op.records.is_empty());
    assert_eq!(no_op.tree_edit.expect("receipt").mutation_sequence, None);
    let before_stale = agent.journal().clone();
    assert_eq!(
        agent.edit_tree(&TreeEdit {
            origin: original,
            action
        }),
        Err(TreeEditRefusal::StaleOrigin)
    );
    assert_eq!(agent.journal(), &before_stale);
    let cleared = edit(
        &mut agent,
        TreeEditAction::SetLabel {
            entry_id: entry_id.clone(),
            label: None,
        },
    )
    .expect("clear");
    assert_eq!(cleared.records.len(), 1);
    assert_eq!(agent.journal().tree_label(&entry_id), None);
    assert_eq!(
        agent
            .journal()
            .project(agent.selected_head())
            .expect("clear context"),
        projection
    );
}

/// TRE-3/TRE-8: rename follows the selected pointer without rebuilding context, collision refuses
/// atomically, and abandoning an inactive pointer retains its source entries but not its name.
#[test]
fn tre_3_8_head_edits_preserve_selection_and_history_and_refuse_selected_abandon() {
    let (mut agent, user) = fixture();
    let old = agent.selected_head().clone();
    let renamed = HeadName::new("original").expect("name");
    let projection = agent.journal().project(&old).expect("projection");
    let reaction = edit(
        &mut agent,
        TreeEditAction::RenameHead {
            head: old,
            renamed: renamed.clone(),
        },
    )
    .expect("rename");
    assert_metadata_only(&reaction);
    assert_eq!(agent.selected_head(), &renamed);
    assert_eq!(
        agent.journal().project(&renamed).expect("same context"),
        projection
    );
    let no_op = edit(
        &mut agent,
        TreeEditAction::RenameHead {
            head: renamed.clone(),
            renamed: renamed.clone(),
        },
    )
    .expect("same name");
    assert!(no_op.records.is_empty());
    let before = agent.journal().clone();
    assert!(matches!(
        edit(
            &mut agent,
            TreeEditAction::AbandonHead {
                head: renamed.clone()
            }
        ),
        Err(TreeEditRefusal::Journal(
            JournalError::CannotAbandonSelectedHead(_)
        ))
    ));
    assert_eq!(agent.journal(), &before);
    let _ = agent
        .navigate(&TreeNavigation {
            origin: agent.tree_origin(),
            target: TreeNavigationTarget::Rewind(user.clone()),
        })
        .expect("fork");
    let branch = agent.selected_head().clone();
    let before = agent.journal().clone();
    assert!(matches!(
        edit(
            &mut agent,
            TreeEditAction::RenameHead {
                head: branch.clone(),
                renamed: renamed.clone()
            }
        ),
        Err(TreeEditRefusal::Journal(JournalError::UnavailableHeadName(
            _
        )))
    ));
    assert_eq!(agent.journal(), &before);
    let abandoned = edit(
        &mut agent,
        TreeEditAction::AbandonHead {
            head: renamed.clone(),
        },
    )
    .expect("abandon inactive");
    assert_metadata_only(&abandoned);
    assert_eq!(agent.selected_head(), &branch);
    assert_eq!(
        agent.journal().head_revision(&renamed),
        Err(JournalError::MissingHead(renamed))
    );
    assert!(
        agent
            .journal()
            .records()
            .iter()
            .any(|record| matches!(record,
        JournalRecord::AppendEntry { entry, .. } if entry.id == user)),
        "abandon must retain source"
    );
}

/// TRE-8: annotations cannot target absent or invisible records, and unbounded/control branch
/// names cannot enter through the user edit boundary. Every refusal leaves the complete journal equal.
#[test]
fn tre_8_invalid_metadata_targets_and_names_refuse_without_mutation() {
    let (mut agent, _) = fixture();
    for action in [
        TreeEditAction::SetLabel {
            entry_id: ConversationEntryId::new("absent").expect("entry"),
            label: Some(TreeLabel::new("label".to_owned()).expect("label")),
        },
        TreeEditAction::RenameHead {
            head: agent.selected_head().clone(),
            renamed: HeadName::new("x".repeat(257)).expect("nonempty"),
        },
        TreeEditAction::RenameHead {
            head: agent.selected_head().clone(),
            renamed: HeadName::new("bad\nname").expect("nonempty"),
        },
    ] {
        let before = agent.journal().clone();
        assert!(edit(&mut agent, action).is_err());
        assert_eq!(agent.journal(), &before);
    }
}
