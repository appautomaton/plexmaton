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
    let result = runtime.retry(target, submission.edited_text).await;
    match result {
        Ok(mut report) => {
            retain_owned_edit(&mut report, edited_text.as_deref());
            restore_undelivered(workspace, runtime.agent_id().clone(), report);
        }
        Err(plexmaton_runtime::RuntimeError::RetryUnavailable) => {}
        Err(error) => {
            let mut report = runtime.take_report();
            retain_owned_edit(&mut report, edited_text.as_deref());
            restore_undelivered(workspace, runtime.agent_id().clone(), report);
            return Err(error).context("retry request");
        }
    }
    sync_actions(runtime, workspace);
    Ok(())
}

fn retain_owned_edit(report: &mut DispatchReport, edited_text: Option<&str>) {
    // The composer still owns this exact edit until a projection reset acknowledges it.
    // Remove only its returned copy; unrelated queued input keeps its ownership report.
    if report.persistence_failure.is_some()
        && let Some(text) = edited_text
        && let Some(index) = report.undelivered.iter().position(|input| {
            input.reason == plexmaton_agent::UndeliveredReason::PersistenceFailed
                && input.text == text
        })
    {
        report.undelivered.remove(index);
    }
}

#[cfg(test)]
mod tests;
