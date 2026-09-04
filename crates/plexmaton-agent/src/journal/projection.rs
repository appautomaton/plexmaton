use std::collections::{BTreeMap, BTreeSet};

use plexmaton_core::{
    AgentId, AttentionId, EventSequence, HeadName, SessionEvent, SessionEventEnvelope, ToolCallId,
    ToolCallStatus, ToolDetail, ToolPresentation, TranscriptItemId, TranscriptRole,
};

use super::{JournalEntryPayload, SessionJournal};
use crate::{ModelRequest, RequestItem, ToolCall, ToolOutcome};

#[cfg(test)]
mod event_tests;
mod events;
#[cfg(test)]
mod live_tests;
#[cfg(test)]
mod tests;
mod types;
#[cfg(test)]
mod validation_tests;

use events::visible_event;
pub use types::{JournalProjection, JournalProjectionError, RecoveryProjection};

#[derive(Clone)]
struct ToolProjection {
    agent_id: AgentId,
    item_id: TranscriptItemId,
    call: ToolCall,
    status: ToolCallStatus,
    revision: u64,
    outcome: Option<ToolOutcome>,
    presentation: ToolPresentation,
}

struct Projector {
    items: Vec<RequestItem>,
    events: Vec<SessionEventEnvelope>,
    next_event: u64,
    agents: BTreeSet<AgentId>,
    entries: BTreeMap<TranscriptItemId, AgentId>,
    attention: BTreeMap<AttentionId, AgentId>,
    tools: BTreeMap<ToolCallId, ToolProjection>,
    pending: Vec<ToolCallId>,
    batch_transitioned: bool,
    recovery: Option<RecoveryProjection>,
}

impl Projector {
    fn new() -> Self {
        Self {
            items: Vec::new(),
            events: Vec::new(),
            next_event: 1,
            agents: BTreeSet::new(),
            entries: BTreeMap::new(),
            attention: BTreeMap::new(),
            tools: BTreeMap::new(),
            pending: Vec::new(),
            batch_transitioned: false,
            recovery: None,
        }
    }

    fn emit(&mut self, event: SessionEvent) -> Result<(), JournalProjectionError> {
        let next = self
            .next_event
            .checked_add(1)
            .ok_or(JournalProjectionError::EventSequenceExhausted)?;
        self.events.push(SessionEventEnvelope {
            sequence: EventSequence::new(self.next_event),
            event,
        });
        self.next_event = next;
        Ok(())
    }

    fn finish_batch(&mut self, recover_final: bool) -> Result<(), JournalProjectionError> {
        if self.pending.is_empty() {
            return Ok(());
        }
        if self.pending.iter().any(|call_id| {
            self.tools
                .get(call_id)
                .is_none_or(|tool| tool.outcome.is_none())
        }) {
            if recover_final {
                self.recovery = Some(RecoveryProjection {
                    omitted_batch_calls: self.pending.clone(),
                });
                return Ok(());
            }
            return Err(JournalProjectionError::IncompleteToolBatchBeforeLaterFact(
                self.pending.clone(),
            ));
        }
        for call_id in &self.pending {
            let tool = self
                .tools
                .get(call_id)
                .unwrap_or_else(|| unreachable!("pending calls are tracked"));
            self.items.push(RequestItem::ToolCall(tool.call.clone()));
        }
        for call_id in self.pending.drain(..) {
            let tool = self
                .tools
                .get(&call_id)
                .unwrap_or_else(|| unreachable!("pending calls are tracked"));
            self.items.push(RequestItem::ToolResult {
                call_id,
                outcome: tool
                    .outcome
                    .clone()
                    .unwrap_or_else(|| unreachable!("complete batch outcomes were checked")),
            });
        }
        self.batch_transitioned = false;
        Ok(())
    }

    fn message(
        &mut self,
        agent_id: AgentId,
        item_id: TranscriptItemId,
        role: TranscriptRole,
        text: String,
    ) -> Result<(), JournalProjectionError> {
        if role != TranscriptRole::System {
            self.finish_batch(false)?;
        }
        self.require_agent(&agent_id)?;
        self.claim_entry(&item_id, &agent_id)?;
        self.emit(SessionEvent::TranscriptItemStarted {
            agent_id: agent_id.clone(),
            item_id: item_id.clone(),
            role,
        })?;
        self.emit(SessionEvent::TranscriptDelta {
            agent_id: agent_id.clone(),
            item_id: item_id.clone(),
            item_revision: 1,
            text: text.clone(),
        })?;
        self.emit(SessionEvent::TranscriptItemFinalized {
            agent_id,
            item_id,
            item_revision: 2,
        })?;
        match role {
            TranscriptRole::User => self.items.push(RequestItem::User { text }),
            TranscriptRole::Assistant => self.items.push(RequestItem::Assistant { text }),
            TranscriptRole::Reasoning => self.items.push(RequestItem::Reasoning { text }),
            TranscriptRole::System => {}
        }
        Ok(())
    }

    fn request_tool(
        &mut self,
        agent_id: AgentId,
        item_id: TranscriptItemId,
        call: ToolCall,
        presentation: ToolPresentation,
    ) -> Result<(), JournalProjectionError> {
        if self.batch_transitioned {
            self.finish_batch(false)?;
        }
        if self.tools.contains_key(&call.call_id) {
            return Err(JournalProjectionError::DuplicateToolCall(call.call_id));
        }
        self.require_agent(&agent_id)?;
        self.claim_entry(&item_id, &agent_id)?;
        let call_id = call.call_id.clone();
        self.emit(SessionEvent::ToolCallChanged {
            agent_id: agent_id.clone(),
            item_id: item_id.clone(),
            item_revision: 0,
            call_id: call_id.clone(),
            label: call.name.clone(),
            status: ToolCallStatus::Queued,
            presentation: presentation.clone(),
        })?;
        self.tools.insert(
            call_id.clone(),
            ToolProjection {
                agent_id,
                item_id,
                call,
                status: ToolCallStatus::Queued,
                revision: 0,
                outcome: None,
                presentation,
            },
        );
        self.pending.push(call_id);
        Ok(())
    }

    fn change_tool(
        &mut self,
        agent_id: AgentId,
        call_id: ToolCallId,
        item_revision: u64,
        status: ToolCallStatus,
        presentation: ToolPresentation,
        outcome: Option<ToolOutcome>,
    ) -> Result<(), JournalProjectionError> {
        let tool = self
            .tools
            .get_mut(&call_id)
            .ok_or_else(|| JournalProjectionError::MissingToolCall(call_id.clone()))?;
        if tool.agent_id != agent_id {
            return Err(JournalProjectionError::WrongToolAgent(call_id));
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
            (Some(_), false) => {
                return Err(JournalProjectionError::PrematureToolOutcome(call_id));
            }
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
            call_id,
            label: tool.call.name.clone(),
            status,
            presentation,
        };
        self.batch_transitioned = true;
        self.emit(event)
    }

    fn require_agent(&self, agent_id: &AgentId) -> Result<(), JournalProjectionError> {
        if self.agents.contains(agent_id) {
            Ok(())
        } else {
            Err(JournalProjectionError::MissingAgent(agent_id.clone()))
        }
    }

    fn claim_entry(
        &mut self,
        item_id: &TranscriptItemId,
        agent_id: &AgentId,
    ) -> Result<(), JournalProjectionError> {
        if self.entries.contains_key(item_id) {
            return Err(JournalProjectionError::DuplicateTranscriptItem(
                item_id.clone(),
            ));
        }
        self.entries.insert(item_id.clone(), agent_id.clone());
        Ok(())
    }

    fn visible(&mut self, payload: JournalEntryPayload) -> Result<(), JournalProjectionError> {
        match &payload {
            JournalEntryPayload::AgentCreated { agent_id, .. } => {
                if !self.agents.insert(agent_id.clone()) {
                    return Err(JournalProjectionError::DuplicateAgent(agent_id.clone()));
                }
            }
            JournalEntryPayload::AgentStatusChanged { agent_id, .. }
            | JournalEntryPayload::TurnUsageUpdated { agent_id, .. } => {
                self.require_agent(agent_id)?;
            }
            JournalEntryPayload::AttentionRequested {
                agent_id,
                attention_id,
                ..
            } => {
                self.require_agent(agent_id)?;
                self.validate_attention_owner(attention_id, agent_id)?;
                self.attention
                    .insert(attention_id.clone(), agent_id.clone());
            }
            JournalEntryPayload::AttentionResolved {
                agent_id,
                attention_id,
            } => {
                self.require_agent(agent_id)?;
                self.validate_attention_owner(attention_id, agent_id)?;
                self.attention.remove(attention_id);
            }
            JournalEntryPayload::MailDelivered {
                item_id, from, to, ..
            } => {
                self.require_agent(from)?;
                self.require_agent(to)?;
                self.claim_entry(item_id, from)?;
            }
            JournalEntryPayload::ArtifactAnnounced {
                agent_id, item_id, ..
            }
            | JournalEntryPayload::RuntimeWarning {
                agent_id, item_id, ..
            }
            | JournalEntryPayload::RuntimeError {
                agent_id, item_id, ..
            }
            | JournalEntryPayload::TurnInterruptedByRecovery {
                agent_id, item_id, ..
            } => {
                self.require_agent(agent_id)?;
                self.claim_entry(item_id, agent_id)?;
            }
            JournalEntryPayload::Message { .. }
            | JournalEntryPayload::ProviderReplay(_)
            | JournalEntryPayload::ToolCallRequested { .. }
            | JournalEntryPayload::ToolCallChanged { .. } => {
                unreachable!("model-bearing payloads are projected separately")
            }
        }
        self.emit(visible_event(payload))
    }

    fn validate_attention_owner(
        &self,
        attention_id: &AttentionId,
        agent_id: &AgentId,
    ) -> Result<(), JournalProjectionError> {
        if let Some(expected) = self.attention.get(attention_id)
            && expected != agent_id
        {
            return Err(JournalProjectionError::AttentionOwnerMismatch {
                attention_id: attention_id.clone(),
                expected: expected.clone(),
                actual: agent_id.clone(),
            });
        }
        Ok(())
    }
}

impl SessionJournal {
    /// Rebuilds both consumers from one selected immutable path (JRN-5).
    pub fn project(&self, head: &HeadName) -> Result<JournalProjection, JournalProjectionError> {
        let mut projector = Projector::new();
        for entry in self.path(head)? {
            project_entry(&mut projector, entry.payload.clone())?;
        }
        projector.finish_batch(true)?;
        Ok(JournalProjection {
            request: ModelRequest {
                items: projector.items,
            },
            events: projector.events,
            recovery: projector.recovery,
        })
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

fn project_entry(
    projector: &mut Projector,
    payload: JournalEntryPayload,
) -> Result<(), JournalProjectionError> {
    match payload {
        JournalEntryPayload::Message {
            agent_id,
            item_id,
            role,
            text,
        } => projector.message(agent_id, item_id, role, text),
        JournalEntryPayload::ProviderReplay(replay) => {
            projector.finish_batch(false)?;
            projector.items.push(RequestItem::ProviderReplay(replay));
            Ok(())
        }
        JournalEntryPayload::ToolCallRequested {
            agent_id,
            item_id,
            call,
            presentation,
        } => projector.request_tool(agent_id, item_id, call, presentation),
        JournalEntryPayload::ToolCallChanged {
            agent_id,
            call_id,
            item_revision,
            status,
            presentation,
            outcome,
        } => {
            projector.change_tool(
                agent_id,
                call_id,
                item_revision,
                status,
                presentation,
                outcome,
            )?;
            Ok(())
        }
        other => {
            projector.visible(other)?;
            Ok(())
        }
    }
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
