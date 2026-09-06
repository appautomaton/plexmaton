//! The user's request for one compaction: admission while idle, typed refusal, no continuation.

use plexmaton_core::AgentId;
use plexmaton_provider::{CompactionPreparationError, plan_compaction};

use super::{LiveRuntime, state::Continuation};
use crate::{CompactionRequest, CompactionRequestRefusal, RuntimeError};

impl LiveRuntime {
    /// Compacts an idle conversation because the user asked, with no model step to continue
    /// (CPL-9). A refusal is an answer, not an error; only ownership failures are errors.
    pub async fn request_compaction(
        &mut self,
        to: AgentId,
    ) -> Result<CompactionRequest, RuntimeError> {
        if to != self.agent_id {
            return Err(RuntimeError::WrongAgent {
                expected: self.agent_id.clone(),
                received: to,
            });
        }
        let request = async {
            self.finish_pending_inputs().await?;
            let request = self.begin_requested_compaction()?;
            if matches!(request, CompactionRequest::Started { .. }) {
                self.finish_transition().await?;
            }
            Ok(request)
        }
        .await;
        if self.journal_failed {
            self.finish_failed_owners().await;
        }
        request
    }

    /// Whether a compaction the user asked for owns the conversation right now.
    pub(in crate::runtime) fn requested_compaction_active(&self) -> bool {
        self.compaction
            .as_ref()
            .is_some_and(|operation| operation.requested)
    }

    fn begin_requested_compaction(&mut self) -> Result<CompactionRequest, RuntimeError> {
        use CompactionRequestRefusal as Refusal;
        if self.shutdown_state != crate::runtime::ShutdownState::Open {
            return Ok(CompactionRequest::Refused(Refusal::ShuttingDown));
        }
        if self.compaction.is_some() {
            return Ok(CompactionRequest::Refused(Refusal::CompactionActive));
        }
        if self.agent.pending_approvals().next().is_some() {
            return Ok(CompactionRequest::Refused(Refusal::ApprovalPending));
        }
        if self.agent.is_running() || self.has_active_work() {
            return Ok(CompactionRequest::Refused(Refusal::TurnActive));
        }
        if self.journal_failed {
            return Err(RuntimeError::JournalRequiresReopen);
        }
        let Some((model, tools)) = self.driver.budget_inputs() else {
            return Ok(CompactionRequest::Refused(Refusal::BudgetUnavailable));
        };
        let id = self.next_compaction_id();
        let prepared = match plan_compaction(
            self.agent.journal(),
            self.agent.selected_head(),
            model,
            tools,
            id.clone(),
        ) {
            Ok(prepared) => prepared,
            Err(error) => return Ok(CompactionRequest::Refused(planning_refusal(&error))),
        };
        let input = prepared.input().clone();
        self.start_compaction_attempt(prepared, input, Continuation::Requested)?;
        Ok(CompactionRequest::Started { id })
    }
}

/// Planning refusals grouped by what the user can do about them (CPL-9).
fn planning_refusal(error: &CompactionPreparationError) -> CompactionRequestRefusal {
    match error {
        CompactionPreparationError::Source(_) | CompactionPreparationError::Budget(_) => {
            CompactionRequestRefusal::SourceUnavailable
        }
        CompactionPreparationError::Plan(_)
        | CompactionPreparationError::NoUsefulReduction
        | CompactionPreparationError::ReplacementMakesNoProgress => {
            CompactionRequestRefusal::NothingToCompact
        }
        CompactionPreparationError::UnfittableEnvironment
        | CompactionPreparationError::OversizedRequiredUser
        | CompactionPreparationError::NoFittingInput
        | CompactionPreparationError::OutputTooLarge
        | CompactionPreparationError::UnfittableReplacement => {
            CompactionRequestRefusal::HistoryTooLarge
        }
    }
}
