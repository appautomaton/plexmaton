use plexmaton_core::{
    AgentId, SessionEntryId, SessionEvent, ToolCallId, ToolCallStatus, ToolDetail,
    ToolPresentation, TranscriptItemId,
};

use super::{JournalProjectionError, Projector, RecoveryProjection};
use crate::{AssistantOutput, ContextAtom, ToolBatch, ToolBatchResult, ToolCall, ToolOutcome};

#[derive(Clone)]
pub(super) struct ToolProjection {
    pub(super) agent_id: AgentId,
    pub(super) item_id: TranscriptItemId,
    pub(super) call: ToolCall,
    pub(super) requested: bool,
    pub(super) status: ToolCallStatus,
    pub(super) revision: u64,
    pub(super) outcome: Option<ToolOutcome>,
    pub(super) presentation: ToolPresentation,
}

pub(super) struct PendingBatch {
    pub(super) output: AssistantOutput,
    pub(super) calls: Vec<ToolCallId>,
    pub(super) source_entries: Vec<SessionEntryId>,
}

pub(super) struct ToolChange {
    pub(super) agent_id: AgentId,
    pub(super) call_id: ToolCallId,
    pub(super) item_revision: u64,
    pub(super) status: ToolCallStatus,
    pub(super) presentation: ToolPresentation,
    pub(super) outcome: Option<ToolOutcome>,
}

impl Projector {
    pub(super) fn finish_batch(
        &mut self,
        recover_final: bool,
    ) -> Result<(), JournalProjectionError> {
        let Some(pending) = self.pending.take() else {
            return Ok(());
        };
        if pending.calls.iter().any(|call_id| {
            self.tools
                .get(call_id)
                .is_none_or(|tool| tool.outcome.is_none())
        }) {
            if recover_final {
                self.recovery = Some(RecoveryProjection {
                    omitted_batch_calls: pending.calls,
                });
                return Ok(());
            }
            return Err(JournalProjectionError::IncompleteToolBatchBeforeLaterFact(
                pending.calls,
            ));
        }
        let mut results = Vec::with_capacity(pending.calls.len());
        for call_id in &pending.calls {
            let tool = self
                .tools
                .get(call_id)
                .ok_or_else(|| JournalProjectionError::MissingToolCall(call_id.clone()))?;
            let outcome = tool
                .outcome
                .clone()
                .ok_or_else(|| JournalProjectionError::MissingToolOutcome(call_id.clone()))?;
            results.push(ToolBatchResult::new(call_id.clone(), outcome));
        }
        let batch = ToolBatch::new(pending.output, results)
            .map_err(JournalProjectionError::InvalidContext)?;
        self.atoms.push(
            ContextAtom::tool_batch(pending.source_entries, batch)
                .map_err(JournalProjectionError::InvalidContext)?,
        );
        Ok(())
    }

    pub(super) fn request_tool(
        &mut self,
        source: SessionEntryId,
        agent_id: AgentId,
        call_id: ToolCallId,
        presentation: ToolPresentation,
    ) -> Result<(), JournalProjectionError> {
        self.require_agent(&agent_id)?;
        let tool = self
            .tools
            .get_mut(&call_id)
            .ok_or_else(|| JournalProjectionError::MissingToolCall(call_id.clone()))?;
        if tool.agent_id != agent_id {
            return Err(JournalProjectionError::WrongToolAgent(call_id));
        }
        if tool.requested {
            return Err(JournalProjectionError::DuplicateToolRequest(call_id));
        }
        tool.requested = true;
        tool.presentation = presentation.clone();
        let event = SessionEvent::ToolCallChanged {
            agent_id,
            item_id: tool.item_id.clone(),
            item_revision: 0,
            call_id: call_id.clone(),
            label: tool.call.name.clone(),
            status: ToolCallStatus::Queued,
            presentation,
        };
        self.pending_source(&call_id, source)?;
        self.emit(event)
    }

    pub(super) fn change_tool(
        &mut self,
        source: SessionEntryId,
        change: ToolChange,
    ) -> Result<(), JournalProjectionError> {
        let ToolChange {
            agent_id,
            call_id,
            item_revision,
            status,
            presentation,
            outcome,
        } = change;
        let tool = self
            .tools
            .get_mut(&call_id)
            .ok_or_else(|| JournalProjectionError::MissingToolCall(call_id.clone()))?;
        if tool.agent_id != agent_id {
            return Err(JournalProjectionError::WrongToolAgent(call_id));
        }
        if !tool.requested {
            return Err(JournalProjectionError::MissingToolRequest(call_id));
        }
        let expected =
            tool.revision
                .checked_add(1)
                .ok_or(JournalProjectionError::UnexpectedToolRevision {
                    call_id: call_id.clone(),
                    expected: tool.revision,
                    actual: item_revision,
                })?;
        if item_revision != expected {
            return Err(JournalProjectionError::UnexpectedToolRevision {
                call_id,
                expected,
                actual: item_revision,
            });
        }
        if !tool.status.can_transition_to(status) {
            return Err(JournalProjectionError::InvalidToolTransition {
                call_id,
                from: tool.status,
                to: status,
            });
        }
        match (&outcome, terminal(status)) {
            (Some(_), false) => return Err(JournalProjectionError::PrematureToolOutcome(call_id)),
            (None, true) => return Err(JournalProjectionError::MissingToolOutcome(call_id)),
            (Some(outcome), true) if outcome.status() != status => {
                return Err(JournalProjectionError::ToolOutcomeMismatch(call_id));
            }
            _ => {}
        }
        let presentation = merge_presentation(&tool.presentation, presentation)
            .ok_or_else(|| JournalProjectionError::ToolPresentationConflict(call_id.clone()))?;
        tool.status = status;
        tool.revision = item_revision;
        tool.outcome = outcome;
        tool.presentation = presentation.clone();
        let event = SessionEvent::ToolCallChanged {
            agent_id,
            item_id: tool.item_id.clone(),
            item_revision,
            call_id: call_id.clone(),
            label: tool.call.name.clone(),
            status,
            presentation,
        };
        self.pending_source(&call_id, source)?;
        self.emit(event)
    }

    fn pending_source(
        &mut self,
        call_id: &ToolCallId,
        source: SessionEntryId,
    ) -> Result<(), JournalProjectionError> {
        let pending = self
            .pending
            .as_mut()
            .filter(|pending| pending.calls.contains(call_id))
            .ok_or_else(|| JournalProjectionError::MissingToolCall(call_id.clone()))?;
        pending.source_entries.push(source);
        Ok(())
    }
}

const fn terminal(status: ToolCallStatus) -> bool {
    matches!(
        status,
        ToolCallStatus::Succeeded
            | ToolCallStatus::Failed
            | ToolCallStatus::Denied
            | ToolCallStatus::Cancelled
    )
}

fn merge_presentation(
    current: &ToolPresentation,
    next: ToolPresentation,
) -> Option<ToolPresentation> {
    Some(ToolPresentation {
        invocation: merge_detail(&current.invocation, next.invocation)?,
        outcome: merge_detail(&current.outcome, next.outcome)?,
    })
}

fn merge_detail(
    current: &Option<ToolDetail>,
    next: Option<ToolDetail>,
) -> Option<Option<ToolDetail>> {
    match (current, next) {
        (Some(current), Some(next)) if current != &next => None,
        (Some(current), _) => Some(Some(current.clone())),
        (None, next) => Some(next),
    }
}
