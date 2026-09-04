use plexmaton_agent::BudgetLedger;
use plexmaton_provider::{ContextBudgetError, budget_ledger};

use super::LiveRuntime;

/// On-demand projection for diagnostics and request planning, not a per-frame computation.
#[derive(Debug)]
pub enum ContextBudgetSnapshot {
    Available(Box<BudgetLedger>),
    Unavailable(ContextBudgetUnavailable),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextBudgetUnavailable {
    /// A synthetic driver has no configured model capacity.
    ModelNotConfigured,
    /// Staged in-memory facts have not yet been acknowledged by the journal writer.
    PendingCommit,
    /// The writer failed; reopen is required before any projection is authoritative.
    PersistenceFailed,
    /// The current tool batch is still waiting for one or more results.
    IncompleteToolBatch,
}

impl LiveRuntime {
    /// Estimates acknowledged context only. Does not poll a model or touch the filesystem.
    pub fn context_budget(&self) -> Result<ContextBudgetSnapshot, ContextBudgetError> {
        if self.journal_failed {
            return Ok(ContextBudgetSnapshot::Unavailable(
                ContextBudgetUnavailable::PersistenceFailed,
            ));
        }
        if self.pending_commit.is_some() {
            return Ok(ContextBudgetSnapshot::Unavailable(
                ContextBudgetUnavailable::PendingCommit,
            ));
        }
        let Some((model, tools)) = self.driver.budget_inputs() else {
            return Ok(ContextBudgetSnapshot::Unavailable(
                ContextBudgetUnavailable::ModelNotConfigured,
            ));
        };
        match budget_ledger(
            self.agent.journal(),
            self.agent.selected_head(),
            model,
            tools,
        ) {
            Ok(ledger) => Ok(ContextBudgetSnapshot::Available(Box::new(ledger))),
            Err(ContextBudgetError::IncompleteToolBatch) => Ok(ContextBudgetSnapshot::Unavailable(
                ContextBudgetUnavailable::IncompleteToolBatch,
            )),
            Err(error) => Err(error),
        }
    }
}
