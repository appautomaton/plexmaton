//! Transcript-specific semantic vocabulary ([ENT](../../../.agents/specs/transcript-entry.md)).

use serde::{Deserialize, Serialize};

/// Semantic author of a transcript item.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptRole {
    /// Authored by the person using the workspace.
    User,
    /// Authored by the agent that owns this transcript.
    Assistant,
    /// Provider-returned reasoning kept distinct from the final answer.
    Reasoning,
    /// Runtime-authored notice that belongs in the transcript rather than in the notice log.
    System,
}

/// Lifecycle of one visible tool call.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    /// Accepted into the current step's batch and awaiting trusted admission.
    Queued,
    /// Admitted, but parked until the user answers its approval request.
    AwaitingApproval,
    /// Executing now.
    Running,
    /// Finished and produced a usable result.
    Succeeded,
    /// Finished without a usable result.
    Failed,
    /// Finished without running because the user declined it.
    Denied,
    /// Stopped before completion by an explicit decision.
    Cancelled,
}

impl ToolCallStatus {
    /// Whether `next` is a valid later state for the same call (ENT-2).
    ///
    /// A projection applies this independently of the producer so replay cannot turn a terminal
    /// call back into live work or skip the approval decision that constrained it.
    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Queued,
                Self::AwaitingApproval | Self::Running | Self::Failed | Self::Cancelled
            ) | (
                Self::AwaitingApproval,
                Self::Running | Self::Denied | Self::Cancelled
            ) | (
                Self::Running,
                Self::Succeeded | Self::Failed | Self::Cancelled
            )
        )
    }
}

/// Original admitted shell source and the context in which it will execute.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CommandInvocation {
    /// Original shell source, including newlines and quoting; inspection copies this text.
    pub source: String,
    /// Canonical working directory selected at admission.
    pub workspace_root: String,
    /// Admitted foreground execution timeout.
    pub timeout_ms: u64,
}

/// Semantic detail retained for an openable tool transcript entry.
///
/// Producers enforce the applicable byte limit before constructing this value. Renderers may
/// clip or decorate it, but copy always returns this source.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolDetail {
    /// Exact admitted shell invocation. Indirection keeps other tool and admission states compact.
    Command(Box<CommandInvocation>),
    /// Bounded plain text, with any deliberate omission made explicit.
    Text {
        /// Exact retained semantic source.
        source: String,
        /// Bytes omitted at the producer boundary, or zero when the source is complete.
        omitted_bytes: u64,
    },
    /// A complete bounded canonical patch.
    Diff {
        /// Exact patch copied and rendered by the transcript.
        patch: String,
    },
}

/// Invocation and outcome detail for one tool entry.
///
/// Either side may be absent when the tool boundary has no safe detail to expose. Absence is
/// semantic and distinct from an empty string.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ToolPresentation {
    /// What the admitted call will do, after canonicalization.
    pub invocation: Option<ToolDetail>,
    /// What the call produced or why it stopped.
    pub outcome: Option<ToolDetail>,
}

#[cfg(test)]
mod tests {
    use super::ToolCallStatus as Status;

    #[test]
    fn tool_lifecycle_allows_only_forward_declared_transitions() {
        let allowed = [
            (Status::Queued, Status::AwaitingApproval),
            (Status::Queued, Status::Running),
            (Status::Queued, Status::Failed),
            (Status::Queued, Status::Cancelled),
            (Status::AwaitingApproval, Status::Running),
            (Status::AwaitingApproval, Status::Denied),
            (Status::AwaitingApproval, Status::Cancelled),
            (Status::Running, Status::Succeeded),
            (Status::Running, Status::Failed),
            (Status::Running, Status::Cancelled),
        ];
        let statuses = [
            Status::Queued,
            Status::AwaitingApproval,
            Status::Running,
            Status::Succeeded,
            Status::Failed,
            Status::Denied,
            Status::Cancelled,
        ];

        for from in statuses {
            for to in statuses {
                assert_eq!(
                    from.can_transition_to(to),
                    allowed.contains(&(from, to)),
                    "unexpected decision for {from:?} -> {to:?}"
                );
            }
        }
    }
}
