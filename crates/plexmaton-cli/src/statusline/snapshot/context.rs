//! Prospective request encoding is fallible; observing a session is not provider admission.
use plexmaton_agent::BudgetError;
use plexmaton_provider::{ContextBudgetError, EncodeError};
use plexmaton_runtime::{ContextBudgetSnapshot, ContextBudgetUnavailable};
use serde::Serialize;

#[derive(Serialize)]
#[serde(tag = "availability", rename_all = "snake_case")]
pub(super) enum Context {
    Available {
        input_tokens: u64,
        output_reserve_tokens: u64,
        measured_prefix_tokens: Option<u64>,
        estimated_tokens: u64,
        opaque_replay_bytes: u64,
        estimator: &'static str,
    },
    Unavailable {
        reason: Reason,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Reason {
    ModelNotConfigured,
    PendingCommit,
    PersistenceFailed,
    IncompleteToolBatch,
    HistoryIncompatible,
    EncodingFailed,
    ProjectionFailed,
    ArithmeticOverflow,
    InvalidBudget,
}

impl Context {
    pub(super) fn capture(budget: Result<ContextBudgetSnapshot, ContextBudgetError>) -> Self {
        let reason = match budget {
            Ok(ContextBudgetSnapshot::Available(ledger)) => {
                return Self::Available {
                    input_tokens: ledger.input_tokens,
                    output_reserve_tokens: ledger.limits.output_reserve_tokens(),
                    measured_prefix_tokens: ledger
                        .anchor
                        .as_ref()
                        .map(|anchor| anchor.input_tokens()),
                    estimated_tokens: ledger.estimated_remainder.tokens,
                    opaque_replay_bytes: ledger.estimated_remainder.opaque_replay_bytes,
                    estimator: ledger.estimator.as_str(),
                };
            }
            Ok(ContextBudgetSnapshot::Unavailable(reason)) => match reason {
                ContextBudgetUnavailable::ModelNotConfigured => Reason::ModelNotConfigured,
                ContextBudgetUnavailable::PendingCommit => Reason::PendingCommit,
                ContextBudgetUnavailable::PersistenceFailed => Reason::PersistenceFailed,
                ContextBudgetUnavailable::IncompleteToolBatch => Reason::IncompleteToolBatch,
            },
            Err(ContextBudgetError::IncompleteToolBatch) => Reason::IncompleteToolBatch,
            Err(ContextBudgetError::Projection(_)) => Reason::ProjectionFailed,
            Err(ContextBudgetError::Arithmetic(error)) => match error {
                BudgetError::Overflow => Reason::ArithmeticOverflow,
                BudgetError::InvalidLimits | BudgetError::InvalidAnchor => Reason::InvalidBudget,
            },
            Err(ContextBudgetError::Encoding(error)) => match error {
                EncodeError::PlainReasoningInResponses
                | EncodeError::MissingThinkingSignature
                | EncodeError::OpaqueReplayInChat
                | EncodeError::UnrepresentableChatOrder
                | EncodeError::IncompatibleReplay { .. } => Reason::HistoryIncompatible,
                EncodeError::UnsupportedCollaboration
                | EncodeError::InvalidToolArguments
                | EncodeError::InvalidToolName
                | EncodeError::InvalidReplayJson(_)
                | EncodeError::InvalidReplayItem
                | EncodeError::OrphanToolResult(_) => Reason::EncodingFailed,
            },
        };
        Self::Unavailable { reason }
    }
}
