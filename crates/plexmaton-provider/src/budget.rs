//! Estimates the actual codec inputs; no transcript text is reconstructed from presentation.

use std::io;

use plexmaton_agent::{
    AtomBudget, BudgetError, BudgetLedger, BudgetLimits, ContextAtom, ContextAtomValue,
    JournalProjectionError, ModelRequest, SessionJournal, TokenEstimate, TokenEstimator,
};
use plexmaton_core::HeadName;
use serde::Serialize;
use thiserror::Error;

use crate::{
    EncodeError, FunctionTool, ModelApi, ResolvedModel, encode_request, request_environment,
};

/// Pure, on-demand occupancy snapshot of a journal head under this exact model and tool set.
/// Automatic compaction/dispatch policy consumes the result; this function performs no effect.
pub fn budget_ledger(
    journal: &SessionJournal,
    head: &HeadName,
    model: &ResolvedModel,
    tools: &[FunctionTool],
) -> Result<BudgetLedger, ContextBudgetError> {
    let environment = request_environment(model, tools, Some(model.max_output_tokens()));
    let basis = journal
        .budget_basis(head, &environment)
        .map_err(ContextBudgetError::Projection)?;
    if basis.recovery.is_some() {
        return Err(ContextBudgetError::IncompleteToolBatch);
    }
    let capacity =
        u64::from(model.context_window_tokens()) - u64::from(model.output_reserve_tokens());
    let limits = BudgetLimits::new(
        u64::from(model.context_window_tokens()),
        u64::from(model.output_reserve_tokens()),
        (capacity * 4 / 5).max(1),
    )?;
    let empty = encode_request(
        model,
        &ModelRequest { atoms: Vec::new() },
        tools,
        Some(model.max_output_tokens()),
    )?;
    let environment_estimate = TokenEstimate {
        tokens: estimate(model.token_estimator(), &empty)?,
        opaque_replay_bytes: 0,
    };
    let atoms = basis
        .request
        .atoms
        .iter()
        .map(|atom| {
            Ok(AtomBudget {
                source_entries: atom.source_entries().into(),
                estimate: estimate_atom(model, atom)?,
            })
        })
        .collect::<Result<Vec<_>, ContextBudgetError>>()?;
    Ok(BudgetLedger::from_estimates(
        environment,
        model.token_estimator(),
        limits,
        environment_estimate,
        atoms,
        basis.anchor,
    )?)
}

fn estimate_atom(
    model: &ResolvedModel,
    atom: &ContextAtom,
) -> Result<TokenEstimate, ContextBudgetError> {
    let encoded = match model.api() {
        ModelApi::OpenaiResponses => crate::responses::encode_atom(model, atom)?,
        ModelApi::OpenaiChatCompletions => crate::chat::encode_atom(model, atom)?,
    };
    let output = match atom.value() {
        ContextAtomValue::User { .. } => None,
        ContextAtomValue::Assistant(output) => Some(output),
        ContextAtomValue::ToolBatch(batch) => Some(batch.assistant()),
    };
    let opaque_replay_bytes =
        output
            .and_then(|output| output.replay())
            .map_or(Ok(0_u64), |replay| {
                replay.attachments().iter().try_fold(0_u64, |total, item| {
                    total
                        .checked_add(
                            u64::try_from(item.payload().len())
                                .map_err(|_| BudgetError::Overflow)?,
                        )
                        .ok_or(BudgetError::Overflow)
                })
            })?;
    Ok(TokenEstimate {
        tokens: estimate(model.token_estimator(), &encoded)?,
        opaque_replay_bytes,
    })
}

fn estimate(estimator: TokenEstimator, value: &impl Serialize) -> Result<u64, ContextBudgetError> {
    let mut counter = ByteCounter(0);
    serde_json::to_writer(&mut counter, value)
        .map_err(|_| ContextBudgetError::Arithmetic(BudgetError::Overflow))?;
    match estimator {
        TokenEstimator::Utf8HeuristicV1 => Ok(counter.0.div_ceil(4)),
    }
}

struct ByteCounter(u64);
impl io::Write for ByteCounter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_add(u64::try_from(bytes.len()).map_err(io::Error::other)?)
            .ok_or_else(|| io::Error::other("estimate overflow"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum ContextBudgetError {
    #[error("context could not be projected: {0:?}")]
    Projection(JournalProjectionError),
    #[error("context contains an incomplete tool batch")]
    IncompleteToolBatch,
    #[error(transparent)]
    Encoding(#[from] EncodeError),
    #[error(transparent)]
    Arithmetic(#[from] BudgetError),
}

#[cfg(test)]
mod tests;
