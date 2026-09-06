//! Translation between user intents and runtime ownership reports; no transcript is authored here.
use anyhow::{Context as _, bail};
use plexmaton_agent::Input;
use plexmaton_core::AgentId;
use plexmaton_runtime::{CleanupFailure, DispatchReport, LiveRuntime, PersistenceFailure};
use plexmaton_tui::{
    ApprovalSubmission, CleanupNotice, PersistenceNotice, Submission, SubmissionKind, Workspace,
};

/// One user input after the TUI has settled both its addressee and delivery boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AddressedInput {
    pub(super) to: AgentId,
    pub(super) input: Input,
    pub(super) skill: Option<String>,
}

/// Preserves the route named by the visible input as the loop's own vocabulary (COM-4, LOOP-6).
pub(super) fn route_submission(submission: Submission) -> AddressedInput {
    let input = match submission.kind {
        SubmissionKind::Message => Input::Submitted {
            text: submission.text,
        },
        SubmissionKind::Steering => Input::Steered {
            text: submission.text,
        },
    };
    AddressedInput {
        to: submission.to,
        input,
        skill: submission.skill,
    }
}

/// Turns the focused conversation identity into the loop's interrupt input (INV-7).
pub(super) fn route_interrupt(to: AgentId) -> AddressedInput {
    AddressedInput {
        to,
        input: Input::Interrupted,
        skill: None,
    }
}

/// Preserves the exact pending identity and typed answer chosen on the approval surface (APV-4).
pub(super) fn route_approval(approval: ApprovalSubmission) -> AddressedInput {
    AddressedInput {
        to: approval.to,
        input: Input::ApprovalDecided {
            approval_id: approval.approval_id,
            decision: approval.decision,
        },
        skill: None,
    }
}

/// Gives addressed visible input to the live runtime; semantic events return on its event stream.
///
/// The projection is never written directly here. A message reaches the screen as the runtime's
/// own events or not at all, which is what keeps the transcript to one writer (COM-3).
///
pub(super) async fn dispatch_live(
    runtime: &mut LiveRuntime,
    workspace: &mut Workspace,
    addressed: AddressedInput,
) -> anyhow::Result<()> {
    if matches!(
        addressed.input,
        Input::Streamed { .. }
            | Input::SkillSubmitted { .. }
            | Input::SkillSteered { .. }
            | Input::Failed { .. }
            | Input::ToolAdmissionResolved(_)
            | Input::PermissionsChanged
            | Input::PermissionPrepared(_)
            | Input::ToolFinished { .. }
            | Input::ShuttingDown
    ) {
        bail!("the TUI produced an input reserved for the producer");
    }
    let to = addressed.to.clone();
    let report = match addressed.skill {
        Some(name) => {
            runtime
                .submit_skill(addressed.to, addressed.input, name)
                .await
        }
        None => runtime.submit(addressed.to, addressed.input).await,
    }
    .context("dispatch user input")?;
    restore_undelivered(workspace, to, report);
    Ok(())
}

pub(super) fn restore_undelivered(workspace: &mut Workspace, to: AgentId, report: DispatchReport) {
    if report.accepted_retry_edit.is_some() {
        workspace.complete_retry_edit();
    }
    if let Some(projection) = report.projection_reset {
        workspace.complete_retry_edit();
        workspace.replace_projection(projection);
    }
    for refusal in report.unresolved_approvals {
        use plexmaton_agent::{
            ApprovalDecisionRefusal as Refusal, PermissionChangeError as Change,
        };
        use plexmaton_tui::ApprovalFeedback;
        let feedback = match refusal.reason {
            Refusal::NotPending => ApprovalFeedback::NotPending,
            Refusal::Preparing => ApprovalFeedback::Preparing,
            Refusal::PolicyChanged => ApprovalFeedback::PolicyChanged,
            Refusal::Permission(Change::Capacity) => ApprovalFeedback::Capacity,
            Refusal::Permission(Change::Unavailable) => ApprovalFeedback::Unavailable,
            Refusal::Permission(Change::Ineffective) => ApprovalFeedback::Ineffective,
            Refusal::Permission(Change::NotFound) => ApprovalFeedback::NotFound,
            Refusal::Permission(Change::StaleRevision) => ApprovalFeedback::PolicyChanged,
        };
        workspace.report_approval_refusal(
            &to,
            &refusal.approval_id,
            feedback,
            refusal.current_offer,
        );
    }
    for grant in report.saved_project_permissions {
        workspace.report_saved_project_permission(&to, grant);
    }
    for input in report.undelivered {
        workspace.return_skill_input(to.clone(), input.text, input.skill);
    }
    for message in report.skill_errors {
        workspace.report_skill_diagnostic(message);
    }
    if let Some(outcome) = report.requested_compaction {
        use plexmaton_runtime::RequestedCompactionOutcome;
        workspace.report_compaction(
            &to,
            match outcome {
                RequestedCompactionOutcome::Published { .. } => {
                    plexmaton_tui::CompactionNote::Published
                }
                RequestedCompactionOutcome::Failed { kind, .. } => {
                    plexmaton_tui::CompactionNote::Failed {
                        reason: kind.to_string(),
                    }
                }
            },
        );
    }
    for failure in report.cleanup_failures {
        let notice = match failure {
            CleanupFailure::Provider => CleanupNotice::Provider,
            CleanupFailure::Tools => CleanupNotice::Tools,
            CleanupFailure::JournalWriter => CleanupNotice::JournalWriter,
        };
        workspace.report_cleanup_failure(notice);
    }
    if let Some(failure) = report.persistence_failure {
        let notice = match failure {
            PersistenceFailure::NotWritten => PersistenceNotice::NotWritten,
            PersistenceFailure::OutcomeUnknown => PersistenceNotice::OutcomeUnknown,
        };
        workspace.report_persistence_failure(notice);
    }
}
