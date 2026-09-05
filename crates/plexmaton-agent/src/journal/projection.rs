use std::collections::{BTreeMap, BTreeSet};

use plexmaton_core::{
    AgentId, AttentionId, EventSequence, HeadName, SessionEntryId, SessionEvent,
    SessionEventEnvelope, ToolCallId, ToolCallStatus, ToolPresentation, TranscriptItemId,
    TranscriptRole, TurnId,
};

use super::{JournalEntryPayload, SessionEntry, SessionJournal};
use crate::timing::UsageAccumulator;
use crate::{
    AssistantBlock, AssistantOutput, ContextAtom, ModelRequest, ModelStepId, RequestAttempt,
    RequestAttemptAuthorized, RequestAttemptId,
};

mod assistant;
#[cfg(test)]
mod event_tests;
mod events;
#[cfg(test)]
mod live_tests;
#[cfg(test)]
mod tests;
mod tools;
mod types;
mod usage;
#[cfg(test)]
mod validation_tests;

use events::visible_event;
use tools::{PendingBatch, ToolChange, ToolProjection};
pub use types::{JournalProjection, JournalProjectionError, RecoveryProjection};
use usage::{cumulative_usage_event, unknown_usage_event};

struct Projector {
    atoms: Vec<ContextAtom>,
    events: Vec<SessionEventEnvelope>,
    next_event: u64,
    agents: BTreeSet<AgentId>,
    entries: BTreeMap<TranscriptItemId, AgentId>,
    attention: BTreeMap<AttentionId, AgentId>,
    tools: BTreeMap<ToolCallId, ToolProjection>,
    pending: Option<PendingBatch>,
    recovery: Option<RecoveryProjection>,
    turns: BTreeMap<TurnId, AgentId>,
    steps: BTreeSet<ModelStepId>,
    turn_usage: BTreeMap<TurnId, UsageAccumulator>,
    unresolved_attempts: BTreeMap<TurnId, BTreeSet<RequestAttemptId>>,
}

impl Projector {
    fn new() -> Self {
        Self {
            atoms: Vec::new(),
            events: Vec::new(),
            next_event: 1,
            agents: BTreeSet::new(),
            entries: BTreeMap::new(),
            attention: BTreeMap::new(),
            tools: BTreeMap::new(),
            pending: None,
            recovery: None,
            turns: BTreeMap::new(),
            steps: BTreeSet::new(),
            turn_usage: BTreeMap::new(),
            unresolved_attempts: BTreeMap::new(),
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

    fn emit_message(
        &mut self,
        agent_id: AgentId,
        item_id: TranscriptItemId,
        role: TranscriptRole,
        text: String,
    ) -> Result<(), JournalProjectionError> {
        self.emit(SessionEvent::TranscriptItemStarted {
            agent_id: agent_id.clone(),
            item_id: item_id.clone(),
            role,
        })?;
        self.emit(SessionEvent::TranscriptDelta {
            agent_id: agent_id.clone(),
            item_id: item_id.clone(),
            item_revision: 1,
            text,
        })?;
        self.emit(SessionEvent::TranscriptItemFinalized {
            agent_id,
            item_id,
            item_revision: 2,
        })
    }

    fn user_message(
        &mut self,
        source: SessionEntryId,
        agent_id: AgentId,
        item_id: TranscriptItemId,
        text: String,
    ) -> Result<(), JournalProjectionError> {
        self.finish_batch(false)?;
        self.require_agent(&agent_id)?;
        self.claim_entry(&item_id, &agent_id)?;
        self.emit_message(agent_id, item_id, TranscriptRole::User, text.clone())?;
        self.atoms.push(ContextAtom::user(source, text));
        Ok(())
    }

    fn turn_started(
        &mut self,
        source: SessionEntryId,
        agent_id: AgentId,
        item_id: TranscriptItemId,
        turn_id: TurnId,
        text: String,
    ) -> Result<(), JournalProjectionError> {
        self.user_message(source, agent_id.clone(), item_id, text)?;
        if self
            .turns
            .insert(turn_id.clone(), agent_id.clone())
            .is_some()
        {
            return Err(JournalProjectionError::DuplicateTurn(turn_id));
        }
        self.emit(SessionEvent::AgentStatusChanged {
            agent_id,
            status: plexmaton_core::AgentStatus::Running,
        })
    }

    fn steering(
        &mut self,
        source: SessionEntryId,
        agent_id: AgentId,
        item_id: TranscriptItemId,
        turn_id: TurnId,
        text: String,
    ) -> Result<(), JournalProjectionError> {
        let Some(expected) = self.turns.get(&turn_id) else {
            return Err(JournalProjectionError::MissingTurn(turn_id));
        };
        if expected != &agent_id {
            return Err(JournalProjectionError::WrongTurnAgent(turn_id));
        }
        self.user_message(source, agent_id, item_id, text)
    }

    fn turn_finished(&mut self, fact: &crate::TurnFinished) -> Result<(), JournalProjectionError> {
        self.finish_batch(false)?;
        let Some(expected) = self.turns.get(&fact.turn_id) else {
            return Err(JournalProjectionError::MissingTurn(fact.turn_id.clone()));
        };
        if expected != &fact.agent_id {
            return Err(JournalProjectionError::WrongTurnAgent(fact.turn_id.clone()));
        }
        self.finish_unknown_usage(&fact.turn_id, &fact.agent_id)?;
        self.emit(SessionEvent::AgentStatusChanged {
            agent_id: fact.agent_id.clone(),
            status: plexmaton_core::AgentStatus::Idle,
        })
    }

    fn turn_status(
        &mut self,
        agent_id: AgentId,
        turn_id: TurnId,
        status: crate::ActiveTurnStatus,
    ) -> Result<(), JournalProjectionError> {
        let Some(expected) = self.turns.get(&turn_id) else {
            return Err(JournalProjectionError::MissingTurn(turn_id));
        };
        if expected != &agent_id {
            return Err(JournalProjectionError::WrongTurnAgent(turn_id));
        }
        self.emit(SessionEvent::AgentStatusChanged {
            agent_id,
            status: status.agent_status(),
        })
    }

    fn request_attempt_finished(
        &mut self,
        attempt: &RequestAttempt,
    ) -> Result<(), JournalProjectionError> {
        let Some(step_id) = attempt.authorization().owner().agent_step() else {
            return Ok(());
        };
        if let Some(pending) = self.unresolved_attempts.get_mut(step_id.turn_id()) {
            pending.remove(attempt.authorization().attempt_id());
        }
        let Some(agent_id) = self.turns.get(step_id.turn_id()).cloned() else {
            return Err(JournalProjectionError::MissingTurn(
                step_id.turn_id().clone(),
            ));
        };
        let Some(terminal) = attempt.terminal() else {
            return Ok(());
        };
        let Some(event) = cumulative_usage_event(
            attempt.authorization(),
            terminal,
            &agent_id,
            &mut self.turn_usage,
            self.unresolved_attempts
                .get(step_id.turn_id())
                .is_some_and(|pending| !pending.is_empty()),
        )?
        else {
            return Ok(());
        };
        self.emit(event)
    }

    fn request_attempt_authorized(&mut self, fact: &RequestAttemptAuthorized) {
        if let Some(step_id) = fact.owner().agent_step() {
            self.unresolved_attempts
                .entry(step_id.turn_id().clone())
                .or_default()
                .insert(fact.attempt_id().clone());
        }
    }

    fn finish_unknown_usage(
        &mut self,
        turn_id: &TurnId,
        agent_id: &AgentId,
    ) -> Result<(), JournalProjectionError> {
        if self
            .unresolved_attempts
            .get(turn_id)
            .is_some_and(|pending| !pending.is_empty())
        {
            self.emit(unknown_usage_event(
                turn_id,
                agent_id,
                self.turn_usage.get(turn_id),
            ))?;
        }
        Ok(())
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
            JournalEntryPayload::TurnStatusChanged { .. }
            | JournalEntryPayload::TurnStarted { .. }
            | JournalEntryPayload::TurnRetried { .. }
            | JournalEntryPayload::SteeringAccepted { .. }
            | JournalEntryPayload::AssistantOutput { .. }
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
        let path = self.path(head)?;
        let selected: BTreeSet<_> = path.iter().map(|entry| entry.id.clone()).collect();
        let mut ordered = Vec::with_capacity(path.len().saturating_mul(2));
        for entry in path {
            let sequence = self
                .entry_sequences
                .get(&entry.id)
                .copied()
                .unwrap_or_else(|| {
                    unreachable!("every accepted entry retains its journal sequence")
                });
            ordered.push((sequence, SelectedFact::Entry(entry)));
            if let JournalEntryPayload::TurnStarted { turn_id, .. }
            | JournalEntryPayload::TurnRetried { turn_id, .. } = &entry.payload
                && let Some(finished) = self.turn_finishes.get(turn_id)
                && selected.contains(&finished.fact.semantic_boundary)
            {
                ordered.push((
                    finished.sequence,
                    SelectedFact::TurnFinished(&finished.fact),
                ));
            }
        }
        for record in self.records() {
            if let super::JournalRecord::RequestAttemptAuthorized { fact, sequence, .. } = record
                && Self::boundary_is_selected(&selected, fact)
            {
                ordered.push((*sequence, SelectedFact::RequestAttemptAuthorized(fact)));
            }
            if let super::JournalRecord::RequestAttemptFinished { fact, sequence, .. } = record {
                let attempt = self
                    .request_attempt(fact.attempt_id())
                    .unwrap_or_else(|| unreachable!("accepted terminal retains its authorization"));
                if Self::boundary_is_selected(&selected, attempt.authorization()) {
                    ordered.push((*sequence, SelectedFact::RequestAttemptFinished(attempt)));
                }
            }
        }
        ordered.sort_by_key(|(sequence, _)| *sequence);
        let mut finished_turns = BTreeSet::new();
        for (_, fact) in ordered {
            match fact {
                SelectedFact::Entry(entry) => project_entry(&mut projector, entry)?,
                SelectedFact::TurnFinished(fact) => {
                    projector.turn_finished(fact)?;
                    finished_turns.insert(fact.turn_id.clone());
                }
                SelectedFact::RequestAttemptAuthorized(fact) => {
                    projector.request_attempt_authorized(fact);
                }
                SelectedFact::RequestAttemptFinished(attempt) => {
                    projector.request_attempt_finished(attempt)?;
                }
            }
        }
        // A crash prefix may have no TurnFinished yet; its surviving usage still has honest coverage.
        let unfinished: Vec<_> = projector
            .unresolved_attempts
            .keys()
            .filter(|turn_id| !finished_turns.contains(*turn_id))
            .cloned()
            .collect();
        for turn_id in unfinished {
            let agent_id = projector
                .turns
                .get(&turn_id)
                .ok_or_else(|| JournalProjectionError::MissingTurn(turn_id.clone()))?
                .clone();
            projector.finish_unknown_usage(&turn_id, &agent_id)?;
        }
        projector.finish_batch(true)?;
        let request_attempts = self
            .request_attempts()
            .filter(|attempt| Self::boundary_is_selected(&selected, attempt.authorization()))
            .cloned()
            .collect();
        Ok(JournalProjection {
            request: ModelRequest {
                session_id: self.session_id().clone(),
                atoms: projector.atoms,
            },
            events: projector.events,
            recovery: projector.recovery,
            request_attempts,
        })
    }
}

enum SelectedFact<'a> {
    Entry(&'a SessionEntry),
    TurnFinished(&'a crate::TurnFinished),
    RequestAttemptAuthorized(&'a RequestAttemptAuthorized),
    RequestAttemptFinished(&'a RequestAttempt),
}

fn project_entry(
    projector: &mut Projector,
    entry: &SessionEntry,
) -> Result<(), JournalProjectionError> {
    let source = entry.id.clone();
    match &entry.payload {
        JournalEntryPayload::TurnRetried {
            agent_id, turn_id, ..
        } => {
            projector.turns.insert(turn_id.clone(), agent_id.clone());
            projector.emit(SessionEvent::AgentStatusChanged {
                agent_id: agent_id.clone(),
                status: plexmaton_core::AgentStatus::Running,
            })
        }
        JournalEntryPayload::TurnStarted {
            agent_id,
            item_id,
            turn_id,
            text,
            ..
        } => projector.turn_started(
            source,
            agent_id.clone(),
            item_id.clone(),
            turn_id.clone(),
            text.clone(),
        ),
        JournalEntryPayload::SteeringAccepted {
            agent_id,
            item_id,
            turn_id,
            text,
            ..
        } => projector.steering(
            source,
            agent_id.clone(),
            item_id.clone(),
            turn_id.clone(),
            text.clone(),
        ),
        JournalEntryPayload::TurnStatusChanged {
            agent_id,
            turn_id,
            status,
        } => projector.turn_status(agent_id.clone(), turn_id.clone(), *status),
        JournalEntryPayload::AssistantOutput {
            agent_id,
            step_id,
            output,
        } => projector.assistant_output(source, agent_id.clone(), step_id.clone(), output.clone()),
        JournalEntryPayload::ToolCallRequested {
            agent_id,
            call_id,
            presentation,
        } => projector.request_tool(
            source,
            agent_id.clone(),
            call_id.clone(),
            presentation.clone(),
        ),
        JournalEntryPayload::ToolCallChanged {
            agent_id,
            call_id,
            item_revision,
            status,
            presentation,
            outcome,
        } => projector.change_tool(
            source,
            ToolChange {
                agent_id: agent_id.clone(),
                call_id: call_id.clone(),
                item_revision: *item_revision,
                status: *status,
                presentation: presentation.clone(),
                outcome: outcome.clone(),
            },
        ),
        other => projector.visible(other.clone()),
    }
}
