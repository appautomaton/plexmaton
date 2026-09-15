//! Focused-conversation interruption and child Stop settlement.

use plexmaton_core::AgentId;
use plexmaton_runtime::{DispatchReport, LiveRuntime, OwnedSchedulingError, OwnedStopReport};
use plexmaton_tui::Workspace;

use crate::{dispatch_live, input::apply_report, route_interrupt};

/// Routes `Ctrl-C` to the owner of the focused conversation (INV-7).
///
/// The root runtime is the only runtime this composition owns directly. Every other target is a
/// delegated child and must go through the collaboration owner; owner refusals are expected for an
/// idle, missing or resumed runner and are intentionally not session errors. In particular, a
/// child target is never retried against the root after a typed refusal.
pub(super) async fn apply_interrupt(
    target: AgentId,
    runtime: &mut LiveRuntime,
    workspace: &mut Workspace,
    collaboration: Option<&mut crate::collaboration::Collaboration>,
) -> anyhow::Result<()> {
    if target == *runtime.agent_id() {
        dispatch_live(runtime, workspace, route_interrupt(target)).await
    } else {
        if let Some(collaboration) = collaboration {
            let _refusal = collaboration.begin_child_stop(&target);
        }
        Ok(())
    }
}

/// Applies only the existing non-event ownership report from a settled child Stop.
///
/// A Stop result carries no new transcript fact. Successful report metadata still belongs to the
/// child runtime and must follow the same return path as any other dispatch result; typed owner
/// refusals are complete control outcomes and do not leave a CLI error or root fallback.
pub(super) fn apply_stop_settlement(
    outcome: Result<OwnedStopReport, OwnedSchedulingError>,
    to: AgentId,
    runtime: &LiveRuntime,
    workspace: &mut Workspace,
) {
    let Ok(report) = outcome else {
        return;
    };
    if let Some(scheduled) = report.scheduled {
        apply_report(runtime, workspace, to.clone(), scheduled);
    }
    if let Some(user_input) = report.user_input {
        match *user_input {
            Ok(user_input) => apply_report(runtime, workspace, to.clone(), user_input),
            Err(failure) => {
                if let Some(input) =
                    failure.into_undelivered(plexmaton_agent::UndeliveredReason::Interrupted)
                {
                    let mut returned = DispatchReport::default();
                    returned.undelivered.push(input);
                    apply_report(runtime, workspace, to.clone(), returned);
                }
            }
        }
    }
    apply_report(runtime, workspace, to, report.stopped);
}
