//! Focused child submission and cancellation-settlement routing.

use plexmaton_agent::Input;
use plexmaton_core::AgentId;
use plexmaton_runtime::{DispatchReport, LiveRuntime};
use plexmaton_tui::{Submission, Workspace};

use crate::{dispatch_live, input::apply_report, route_submission};

pub(super) async fn dispatch(
    submission: Submission,
    collaboration: Option<&mut crate::collaboration::Collaboration>,
    runtime: &mut LiveRuntime,
    workspace: &mut Workspace,
) -> anyhow::Result<()> {
    let addressed = route_submission(submission);
    if addressed.to == *runtime.agent_id() {
        return dispatch_live(runtime, workspace, addressed).await;
    }
    let Some(collaboration) = collaboration else {
        if let Input::Submitted { text } | Input::Steered { text } = addressed.input {
            workspace.return_skill_input(addressed.to, text, addressed.skill);
        }
        return Ok(());
    };
    let to = addressed.to.clone();
    let report = collaboration.dispatch_child_input(addressed);
    apply_report(runtime, workspace, to, report);
    Ok(())
}

pub(super) fn apply_settlement(
    report: DispatchReport,
    to: AgentId,
    runtime: &LiveRuntime,
    workspace: &mut Workspace,
) {
    apply_report(runtime, workspace, to, report);
}
