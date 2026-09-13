//! Conversation-tree composition: acknowledged reads and nonblocking navigation admission.

use anyhow::Context as _;
use plexmaton_core::{AgentId, TreeEdit, TreeNavigation, TreeSnapshot, TreeSourceRequest};
use plexmaton_runtime::{LiveRuntime, TreeAdmission, TreeRequestRefusal};
use plexmaton_tui::Workspace;

/// Reads only acknowledged facts; opening never materializes an automatic session or starts work.
pub(super) fn open(runtime: &LiveRuntime, workspace: &mut Workspace, agent: &AgentId) {
    workspace.show_conversation_tree(agent.clone(), snapshot(runtime, agent));
}

fn snapshot(runtime: &LiveRuntime, agent: &AgentId) -> Result<TreeSnapshot, String> {
    if agent != runtime.agent_id() {
        Err("This conversation is not available in the current runtime.".to_owned())
    } else if let Some((journal, _)) = runtime.acknowledged_conversation() {
        journal
            .tree_snapshot(agent)
            .map_err(|error| error.to_string())
    } else {
        Err("History is awaiting persistence or requires reopening. Refresh after the write completes.".to_owned())
    }
}

pub(super) fn complete_edit(runtime: &LiveRuntime, workspace: &mut Workspace, agent: &AgentId) {
    workspace.complete_tree_edit(agent, snapshot(runtime, agent));
}

pub(super) fn edit(
    runtime: &mut LiveRuntime,
    workspace: &mut Workspace,
    edit: TreeEdit,
) -> anyhow::Result<()> {
    let agent = edit.origin.agent_id.clone();
    match runtime
        .request_tree_edit(edit)
        .context("admit conversation-tree edit")?
    {
        TreeAdmission::Started => workspace.mark_tree_edit_pending(),
        TreeAdmission::NoOp => complete_edit(runtime, workspace, &agent),
        TreeAdmission::Refused(refusal) => {
            workspace.report_tree_edit_refusal(refusal_message(&refusal))
        }
    }
    Ok(())
}

pub(super) fn copy(
    runtime: &LiveRuntime,
    workspace: &mut Workspace,
    request: &TreeSourceRequest,
) -> Option<plexmaton_tui::CopyRequest> {
    match runtime.read_tree_source(request) {
        Ok(text) => Some(plexmaton_tui::CopyRequest { text, entries: 1 }),
        Err(error) => {
            workspace.report_tree_navigation_refusal(error.to_string());
            None
        }
    }
}

/// Admission returns immediately; the interaction loop keeps polling input and the owned writer.
pub(super) fn navigate(
    runtime: &mut LiveRuntime,
    workspace: &mut Workspace,
    navigation: TreeNavigation,
) -> anyhow::Result<()> {
    let agent = navigation.origin.agent_id.clone();
    match runtime
        .request_tree_navigation(navigation)
        .context("admit conversation-tree navigation")?
    {
        TreeAdmission::Started => workspace.mark_tree_navigation_pending(),
        TreeAdmission::NoOp => workspace.complete_tree_navigation(&agent, None),
        TreeAdmission::Refused(refusal) => {
            workspace.report_tree_navigation_refusal(refusal_message(&refusal));
        }
    }
    Ok(())
}

fn refusal_message(refusal: &TreeRequestRefusal) -> String {
    use TreeRequestRefusal as Refusal;
    match refusal {
        Refusal::Busy => {
            "Wait for active work and queued input to finish before changing branches.".to_owned()
        }
        Refusal::ShuttingDown => "The session is closing; navigation did not start.".to_owned(),
        Refusal::PersistenceFailed => {
            "History could not be saved. Reopen this session before navigating.".to_owned()
        }
        Refusal::PersistenceUnavailable => "This runtime has no durable history writer.".to_owned(),
        Refusal::PendingReport => {
            "The previous history change is still being applied. Refresh and try again.".to_owned()
        }
        Refusal::StaleOrigin { .. } => {
            "History changed since this tree was opened. Refresh before navigating.".to_owned()
        }
        Refusal::Navigation(reason) => reason.to_string(),
        Refusal::Edit(reason) => reason.to_string(),
    }
}

#[cfg(test)]
mod tests;
