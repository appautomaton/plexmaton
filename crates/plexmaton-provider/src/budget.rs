//! Estimates the actual codec inputs; no transcript text is reconstructed from presentation.

use std::io;

use plexmaton_agent::{
    AtomBudget, BudgetError, BudgetLedger, BudgetLimits, ContextAtom, ContextAtomValue,
    ConversationJournal, JournalProjectionError, ModelRequest, TokenEstimate, TokenEstimator,
};
use plexmaton_core::HeadName;
use serde::Serialize;
use thiserror::Error;

use crate::{
    EncodeError, FunctionTool, ModelApi, ResolvedModel, encode_request, request_environment,
};

/// One authoritative request projection paired with its codec-derived occupancy ledger.
///
/// Callers that need both the atoms and their estimates must obtain them together so a
/// compaction planner cannot accidentally reimplement BUD-3 arithmetic.
#[derive(Debug)]
pub struct BudgetedContext {
    pub request: ModelRequest,
    pub ledger: BudgetLedger,
}

/// Pure, on-demand occupancy snapshot of a journal head under this exact model and tool set.
/// Automatic compaction/dispatch policy consumes the result; this function performs no effect.
pub fn budget_ledger(
    journal: &ConversationJournal,
    head: &HeadName,
    model: &ResolvedModel,
    tools: &[FunctionTool],
) -> Result<BudgetLedger, ContextBudgetError> {
    Ok(budgeted_context(journal, head, model, tools)?.ledger)
}

/// Builds the journal projection and its one matching codec-derived ledger together.
pub fn budgeted_context(
    journal: &ConversationJournal,
    head: &HeadName,
    model: &ResolvedModel,
    tools: &[FunctionTool],
) -> Result<BudgetedContext, ContextBudgetError> {
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
    let environment_estimate = estimate_environment(model, journal.conversation_id(), tools)?;
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
    let ledger = BudgetLedger::from_estimates(
        environment,
        model.token_estimator(),
        limits,
        environment_estimate,
        atoms,
        basis.anchor,
    )?;
    Ok(BudgetedContext {
        request: basis.request,
        ledger,
    })
}

/// Estimates one complete encoded request using the same BUD-3 byte heuristic as the ledger.
///
/// This is intentionally separate from provider acceptance: it validates replay and reports the
/// deterministic local occupancy used to reject an unfittable replacement before dispatch.
pub fn estimate_request(
    model: &ResolvedModel,
    request: &ModelRequest,
    tools: &[FunctionTool],
) -> Result<TokenEstimate, ContextBudgetError> {
    // Validate the exact whole wire request first (notably replay compatibility), then apply the
    // same independently rounded environment-plus-atom units as `BudgetLedger`.
    let _ = encode_request(model, request, tools, Some(model.max_output_tokens()))?;
    let mut total = estimate_environment(model, &request.session_id, tools)?;
    for atom in &request.atoms {
        total = total.checked_add(estimate_atom(model, atom)?)?;
    }
    Ok(total)
}

fn estimate_environment(
    model: &ResolvedModel,
    session_id: &plexmaton_core::ConversationId,
    tools: &[FunctionTool],
) -> Result<TokenEstimate, ContextBudgetError> {
    let empty = encode_request(
        model,
        &ModelRequest {
            session_id: session_id.clone(),
            atoms: Vec::new(),
        },
        tools,
        Some(model.max_output_tokens()),
    )?;
    Ok(TokenEstimate {
        tokens: estimate(model.token_estimator(), &empty)?,
        opaque_replay_bytes: 0,
    })
}

fn estimate_atom(
    model: &ResolvedModel,
    atom: &ContextAtom,
) -> Result<TokenEstimate, ContextBudgetError> {
    let encoded = match model.api() {
        ModelApi::OpenaiResponses => crate::responses::encode_atom(model, atom)?,
        ModelApi::OpenaiChatCompletions => crate::chat::encode_atom(model, atom)?,
        ModelApi::AnthropicMessages => crate::messages::encode_atom(model, atom)?,
        ModelApi::GoogleGenerateContent => crate::gemini::encode_atom(model, atom)?,
    };
    let output = match atom.value() {
        ContextAtomValue::Collaboration(_)
        | ContextAtomValue::User { .. }
        | ContextAtomValue::Skill(_)
        | ContextAtomValue::CompactionSummary { .. } => None,
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
