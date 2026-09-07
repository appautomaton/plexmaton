//! Bounded semantic facts shown when a tool entry opens (ENT-4).

use plexmaton_core::ToolDetail;

use super::{ToolCancellationReason, ToolOutcome};
use crate::admission::AdmissionRefusal;

/// Maximum UTF-8 bytes retained in one plain-text tool presentation.
///
/// Tool-specific sources may have a tighter bound. This final shared ceiling keeps a native tool
/// from widening its already-bounded model result when it adds transcript presentation (ENT-4).
pub const MAX_TOOL_PRESENTATION_TEXT_BYTES: usize = 64 * 1024;

const OMISSION_MARKER_RESERVE_BYTES: usize = 64;

/// Retains a bounded head and tail while keeping omitted bytes explicit (ENT-4).
#[must_use]
pub fn bounded_tool_text(source: &str, already_omitted_bytes: u64) -> ToolDetail {
    if source.len() <= MAX_TOOL_PRESENTATION_TEXT_BYTES {
        return ToolDetail::Text {
            source: source.to_owned(),
            omitted_bytes: already_omitted_bytes,
        };
    }

    let retained = MAX_TOOL_PRESENTATION_TEXT_BYTES - OMISSION_MARKER_RESERVE_BYTES;
    let mut head_end = retained / 2;
    while !source.is_char_boundary(head_end) {
        head_end = head_end.saturating_sub(1);
    }
    let mut tail_start = source.len().saturating_sub(retained - head_end);
    while !source.is_char_boundary(tail_start) {
        tail_start = tail_start.saturating_add(1);
    }
    let newly_omitted = source
        .len()
        .saturating_sub(head_end)
        .saturating_sub(source.len().saturating_sub(tail_start));
    let omitted_bytes = already_omitted_bytes.saturating_add(newly_omitted as u64);
    let marker = format!("\n...[{newly_omitted} bytes omitted]...\n");
    let mut retained_source = String::with_capacity(MAX_TOOL_PRESENTATION_TEXT_BYTES);
    retained_source.push_str(&source[..head_end]);
    retained_source.push_str(&marker);
    retained_source.push_str(&source[tail_start..]);

    ToolDetail::Text {
        source: retained_source,
        omitted_bytes,
    }
}

pub(crate) fn detail_fits_text_bound(detail: &ToolDetail) -> bool {
    match detail {
        ToolDetail::Command(command) => {
            command
                .source
                .len()
                .saturating_add(command.workspace_root.len())
                <= MAX_TOOL_PRESENTATION_TEXT_BYTES
        }
        ToolDetail::Text { source, .. } => source.len() <= MAX_TOOL_PRESENTATION_TEXT_BYTES,
        ToolDetail::Diff { patch } => patch.len() <= MAX_TOOL_PRESENTATION_TEXT_BYTES,
    }
}

/// One executor answer: the unchanged model-facing outcome plus optional transcript detail.
///
/// Presentation is a fact produced at the trusted tool boundary. It never participates in model
/// control flow, and a consumer must preserve it independently of completion ordering (ENT-2/4).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolExecutionResult {
    outcome: ToolOutcome,
    presentation: Option<ToolDetail>,
}

impl ToolExecutionResult {
    /// Pairs one outcome with the bounded semantic detail produced by its executor.
    #[must_use]
    pub const fn new(outcome: ToolOutcome, presentation: Option<ToolDetail>) -> Self {
        Self {
            outcome,
            presentation,
        }
    }

    /// Model-facing typed outcome, unchanged by presentation concerns.
    #[must_use]
    pub const fn outcome(&self) -> &ToolOutcome {
        &self.outcome
    }

    /// Bounded transcript detail produced by the executor, when one exists.
    #[must_use]
    pub const fn presentation(&self) -> Option<&ToolDetail> {
        self.presentation.as_ref()
    }

    /// Splits the result so the batch can retain presentation and place the outcome in model order.
    #[must_use]
    pub fn into_parts(self) -> (ToolOutcome, Option<ToolDetail>) {
        (self.outcome, self.presentation)
    }
}

pub(crate) fn unexecuted_outcome(outcome: &ToolOutcome) -> Option<ToolDetail> {
    let source = match outcome {
        ToolOutcome::AdmissionRefused { reason } => {
            format!("admission refused: {}", admission_refusal_name(*reason))
        }
        ToolOutcome::PermissionRefused { reason } => format!("permission refused: {reason}"),
        ToolOutcome::Forbidden => "forbidden by policy".to_owned(),
        ToolOutcome::Denied => "denied by user".to_owned(),
        ToolOutcome::Cancelled { reason } => {
            format!("cancelled: {}", cancellation_reason_name(*reason))
        }
        ToolOutcome::Succeeded { .. } | ToolOutcome::Failed { .. } => return None,
    };
    Some(bounded_tool_text(&source, 0))
}

const fn admission_refusal_name(reason: AdmissionRefusal) -> &'static str {
    match reason {
        AdmissionRefusal::UnknownTool => "unknown_tool",
        AdmissionRefusal::InvalidArguments => "invalid_arguments",
        AdmissionRefusal::DefinitionUnavailable => "definition_unavailable",
        AdmissionRefusal::StalePrecondition => "stale_precondition",
        AdmissionRefusal::SourceMismatch => "source_mismatch",
        AdmissionRefusal::AmbiguousTarget => "ambiguous_target",
        AdmissionRefusal::ConflictingArguments => "conflicting_arguments",
        AdmissionRefusal::Cancelled => "cancelled",
    }
}

const fn cancellation_reason_name(reason: ToolCancellationReason) -> &'static str {
    match reason {
        ToolCancellationReason::Interrupted => "interrupted",
        ToolCancellationReason::StepFailed => "step_failed",
        ToolCancellationReason::Shutdown => "shutdown",
        ToolCancellationReason::ProcessDied => "process_died",
    }
}
