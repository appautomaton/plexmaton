//! Refuse external work in the native fixture without consuming user-owned drafts.

use plexmaton_core::{PermissionChangeError, ReasoningEffort};
use plexmaton_tui::{
    ApprovalFeedback, ConfigurationSummary, ConversationPickerStatus, ConversationRequest, Flow,
    Outcome, Page, SwitchRefusal, TreeRequest, Workspace,
};
use ratatui::crossterm::event::Event;

const UNAVAILABLE: &str = "UI fixture | Action unavailable: no runtime or clipboard";

pub fn handle(workspace: &mut Workspace, event: &Event) -> Flow {
    let draft = workspace.state().primary_agent().map(|agent| {
        (
            agent.id.clone(),
            workspace.state().draft(&agent.id).text().to_owned(),
        )
    });
    let outcome = workspace.handle(event);
    if outcome.flow == Flow::Quit || outcome == Outcome::default() {
        return outcome.flow;
    }
    workspace.set_working_directory(UNAVAILABLE.into());
    let command_consumed = matches!(outcome.conversation, Some(ConversationRequest::New))
        || outcome.command.is_some()
        || matches!(outcome.tree, Some(TreeRequest::Refresh(_)));
    if command_consumed
        && let Some((agent, text)) = draft
        && workspace.state().draft(&agent).text().is_empty()
    {
        workspace.return_input(agent, text);
    }
    if let Some(submitted) = outcome.submitted {
        workspace.return_skill_input(submitted.to, submitted.text, submitted.skill);
    }
    if let Some(request) = outcome.conversation {
        match request {
            ConversationRequest::List => {
                workspace.set_conversation_picker_status(ConversationPickerStatus::ListFailed)
            }
            ConversationRequest::New | ConversationRequest::Saved(_) => {
                workspace.report_switch_refusal(SwitchRefusal::OpenFailed)
            }
        }
    }
    if let Some(page) = outcome.page {
        match page {
            Page::Configuration => workspace.show_configuration(ConfigurationSummary {
                configured_name: "UI fixture".into(),
                provider: "None (offline preview)".into(),
                model: "No model".into(),
                display_name: "No model".into(),
                reasoning_effort: ReasoningEffort::None,
            }),
            Page::Permissions => {
                workspace.open_permissions();
                refuse_permissions(workspace);
            }
        }
    }
    if outcome.permission.is_some() {
        refuse_permissions(workspace);
    }
    if outcome.effort.is_some() {
        workspace.report_effort(Err(UNAVAILABLE.into()));
    }
    if outcome.model.is_some() {
        workspace.report_model(Err(UNAVAILABLE.into()));
    }
    if let Some(approval) = outcome.approval {
        workspace.report_approval_refusal(
            &approval.to,
            &approval.approval_id,
            ApprovalFeedback::Unavailable,
            None,
        );
    }
    if let Some(TreeRequest::Refresh(agent)) = outcome.tree {
        workspace.show_conversation_tree(agent, Err(UNAVAILABLE.into()));
    }
    // Interrupts, clipboard and runtime-only requests have no pending owner in this fixture.
    // Their footer refusal never produces a successful receipt or synthetic history.
    Flow::Continue
}

fn refuse_permissions(workspace: &mut Workspace) {
    workspace.update_permissions(Err(PermissionChangeError::Unavailable), None);
}

#[cfg(test)]
#[path = "native_effects_tests.rs"]
mod tests;
