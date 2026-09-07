//! Narrow collaboration-turn path; scheduling and ordinary input interception remain external.

use std::sync::Arc;

use plexmaton_agent::collaboration::{
    CollaborationError, CollaborationItemRef, CollaborationLedger, ResolvedContext,
    ResolvedTurnAdmission, TurnBoundary,
};
use plexmaton_agent::{ContextAtomValue, ModelCall};
use plexmaton_core::TurnId;

use super::{LiveRuntime, ShutdownState, transition::AfterCommit};
use crate::{DispatchReport, RuntimeError};

impl LiveRuntime {
    /// Captures the acknowledged idle session boundary for the collaboration log's ordering point.
    pub fn collaboration_boundary(
        &self,
        turn: TurnId,
    ) -> Result<(TurnBoundary, Option<CollaborationItemRef>), RuntimeError> {
        self.require_collaboration_idle()?;
        let boundary = self.agent.collaboration_boundary(turn)?;
        let previous = self
            .agent
            .journal()
            .previous_collaboration_inclusion(self.agent.selected_head())
            .map_err(|_| CollaborationError::InvalidTurnBoundary)?;
        Ok((boundary, previous))
    }

    /// Rebuilds only the current context's referenced admissions; reads no files and starts no work.
    pub fn restore_collaboration_context(
        &mut self,
        ledger: &CollaborationLedger,
    ) -> Result<(), RuntimeError> {
        self.require_collaboration_idle()?;
        let mut request = self
            .agent
            .journal()
            .project(self.agent.selected_head())
            .map_err(|_| CollaborationError::InvalidTurnBoundary)?
            .into_request();
        let mut context = ResolvedContext::default();
        for atom in &request.atoms {
            if let ContextAtomValue::Collaboration(value) = atom.value() {
                context.insert(ledger.resolve_turn(value.reference())?)?;
            }
        }
        context.resolve(&mut request, self.agent.journal())?;
        self.collaboration_context = context;
        Ok(())
    }

    /// Includes an acknowledged canonical admission and then uses JRN-7/TIM-2's dispatch barrier.
    /// Cancelling this future leaves the pending session transition owned by this runtime.
    pub async fn start_collaboration_turn(
        &mut self,
        resolved: Arc<ResolvedTurnAdmission>,
    ) -> Result<DispatchReport, RuntimeError> {
        self.require_collaboration_idle()?;
        if !self.driver.supports_collaboration() {
            return Err(CollaborationError::UnsupportedContext.into());
        }
        let (boundary, previous) =
            self.collaboration_boundary(resolved.admission().boundary.turn.clone())?;
        if boundary != resolved.admission().boundary || previous != resolved.admission().previous {
            return Err(CollaborationError::InvalidTurnBoundary.into());
        }
        // Resolve prior inclusions before writing a new boundary; a missing source must not
        // consume the new admission merely to discover that history cannot be materialized.
        let mut prior = self
            .agent
            .journal()
            .project(self.agent.selected_head())
            .map_err(|_| CollaborationError::InvalidTurnBoundary)?
            .into_request();
        self.collaboration_context
            .resolve(&mut prior, self.agent.journal())?;
        self.collaboration_context.insert(Arc::clone(&resolved))?;
        self.refresh_permission_snapshot()?;
        let reaction = self
            .agent
            .start_collaboration_turn(&resolved, self.clock.now())?;
        self.begin_transition(reaction, Vec::new(), AfterCommit::None)?;
        if let Err(error) = self.finish_transition().await {
            if self.journal_failed {
                self.finish_failed_owners().await;
            }
            return Err(error);
        }
        if self.journal_failed {
            self.finish_failed_owners().await;
            return Err(RuntimeError::JournalRequiresReopen);
        }
        Ok(self.take_report())
    }

    pub(super) fn resolve_collaboration_call(
        &self,
        call: &mut ModelCall,
    ) -> Result<(), CollaborationError> {
        if call
            .request
            .atoms
            .iter()
            .any(|atom| matches!(atom.value(), ContextAtomValue::Collaboration(_)))
            && !self.driver.supports_collaboration()
        {
            return Err(CollaborationError::UnsupportedContext);
        }
        self.collaboration_context
            .resolve(&mut call.request, self.agent.journal())
    }

    fn require_collaboration_idle(&self) -> Result<(), RuntimeError> {
        if self.journal.is_none() {
            return Err(CollaborationError::PersistenceRequired.into());
        }
        if self.journal_failed {
            return Err(RuntimeError::JournalRequiresReopen);
        }
        if self.shutdown_state != ShutdownState::Open {
            return Err(RuntimeError::ShuttingDown);
        }
        if self.pending_commit.is_some()
            || self.after_commit.is_some()
            || self.active.is_some()
            || self.pending_model_start.is_some()
            || !self.pending_inputs.is_empty()
            || self.preparing_input.is_some()
            || self.compaction.is_some()
            || !self.tools.is_empty()
            || self.agent.is_running()
        {
            return Err(CollaborationError::InvalidTurnBoundary.into());
        }
        Ok(())
    }
}
