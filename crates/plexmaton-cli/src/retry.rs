use super::*;

pub(super) fn sync_actions(runtime: &LiveRuntime, workspace: &mut Workspace) {
    let actions = runtime
        .retry_candidate()
        .map(|candidate| plexmaton_tui::RetryActions {
            target: plexmaton_tui::RetryTarget {
                turn_id: candidate.target.turn_id,
                revision: candidate.target.head_revision.get(),
            },
            question_item: candidate.question_item,
            error_item: candidate.error_item,
            skill: candidate.skill,
        });
    workspace.set_retry_actions(actions);
}

pub(super) async fn execute(
    runtime: &mut LiveRuntime,
    workspace: &mut Workspace,
    submission: plexmaton_tui::RetrySubmission,
) -> anyhow::Result<()> {
    let target = plexmaton_agent::RetryTarget {
        turn_id: submission.target.turn_id,
        head_revision: plexmaton_agent::HeadRevision::new(submission.target.revision),
    };
    let edited_text = submission.edited_text.clone();
    let result = match (submission.edited_text, submission.skill) {
        (Some(text), Some(name)) => runtime.retry_skill(target, text, name).await,
        (edited, None) => runtime.retry(target, edited).await,
        (None, Some(_)) => Err(plexmaton_runtime::RuntimeError::InvalidSkillInput),
    };
    match result {
        Ok(mut report) => {
            retain_owned_edit(&mut report, edited_text.as_deref());
            input::apply_report(runtime, workspace, runtime.agent_id().clone(), report);
        }
        Err(plexmaton_runtime::RuntimeError::RetryUnavailable) => {}
        Err(error) => {
            let mut report = runtime.take_report();
            retain_owned_edit(&mut report, edited_text.as_deref());
            input::apply_report(runtime, workspace, runtime.agent_id().clone(), report);
            return Err(error).context("retry request");
        }
    }
    sync_actions(runtime, workspace);
    Ok(())
}

fn retain_owned_edit(report: &mut DispatchReport, edited_text: Option<&str>) {
    // The composer owns this edit until projection acknowledgement or explicit preparation handoff.
    // Remove only its returned copy; unrelated queued input keeps its ownership report.
    if report.accepted_retry_edit.is_none()
        && let Some(text) = edited_text
        && let Some(index) = report
            .undelivered
            .iter()
            .position(|input| input.text == text)
    {
        report.undelivered.remove(index);
    }
}

#[cfg(test)]
mod tests;
