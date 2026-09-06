//! Turn coverage is derived from selected immutable attempts, including unresolved authorizations.

use std::collections::{BTreeMap, BTreeSet};

use plexmaton_core::{AgentId, ConversationEvent, HeadName, TokenUsage, TurnId};

use super::{ConversationJournal, JournalProjectionError};
use crate::timing::UsageAccumulator;
use crate::{RequestAttemptAuthorized, RequestAttemptTerminal, RequestAttemptTerminalState};

pub(super) fn cumulative_usage_event(
    authorization: &RequestAttemptAuthorized,
    terminal: &RequestAttemptTerminal,
    agent_id: &AgentId,
    totals: &mut BTreeMap<TurnId, UsageAccumulator>,
    other_unresolved: bool,
) -> Result<Option<ConversationEvent>, JournalProjectionError> {
    let Some(step_id) = authorization.owner().agent_step() else {
        return Ok(None);
    };
    let aggregate = match terminal.terminal() {
        RequestAttemptTerminalState::Dispatched { usage, .. } => totals
            .entry(step_id.turn_id().clone())
            .or_default()
            .add(usage.clone())
            .map_err(|()| JournalProjectionError::TurnUsageOverflow(step_id.turn_id().clone()))?,
        RequestAttemptTerminalState::NotDispatched { .. } => {
            let Some(total) = totals.get(step_id.turn_id()) else {
                return Ok(None);
            };
            // An interrupt may have published unknown coverage before this no-effect result.
            total.snapshot()
        }
    };
    let usage = match aggregate {
        TokenUsage::Complete(counts) if other_unresolved => TokenUsage::Partial(counts),
        aggregate => aggregate,
    };
    Ok(Some(ConversationEvent::TurnUsageUpdated {
        agent_id: agent_id.clone(),
        turn_id: step_id.turn_id().clone(),
        usage,
    }))
}

pub(super) fn unknown_usage_event(
    turn_id: &TurnId,
    agent_id: &AgentId,
    total: Option<&UsageAccumulator>,
) -> ConversationEvent {
    let usage = total
        .cloned()
        .unwrap_or_default()
        .add(TokenUsage::Unavailable)
        .unwrap_or_else(|()| unreachable!("marking coverage unknown adds no counts"));
    ConversationEvent::TurnUsageUpdated {
        agent_id: agent_id.clone(),
        turn_id: turn_id.clone(),
        usage,
    }
}

impl ConversationJournal {
    pub(crate) fn preview_cumulative_usage_event(
        &self,
        head: &HeadName,
        terminal: &RequestAttemptTerminal,
    ) -> Result<Option<ConversationEvent>, JournalProjectionError> {
        let selected: BTreeSet<_> = self
            .path(head)?
            .into_iter()
            .map(|entry| entry.id.clone())
            .collect();
        let mut totals = BTreeMap::new();
        for record in self.records() {
            let crate::JournalRecord::RequestAttemptFinished { fact, .. } = record else {
                continue;
            };
            let attempt = self
                .request_attempt(fact.attempt_id())
                .unwrap_or_else(|| unreachable!("accepted terminal retains its authorization"));
            if !Self::boundary_is_selected(&selected, attempt.authorization()) {
                continue;
            }
            let Some(step_id) = attempt.authorization().owner().agent_step() else {
                continue;
            };
            let start = self
                .turn_starts
                .get(step_id.turn_id())
                .ok_or_else(|| JournalProjectionError::MissingTurn(step_id.turn_id().clone()))?;
            let retained_terminal = attempt
                .terminal()
                .unwrap_or_else(|| unreachable!("finished record retains its terminal"));
            let _prior = cumulative_usage_event(
                attempt.authorization(),
                retained_terminal,
                &start.agent_id,
                &mut totals,
                false,
            )?;
        }
        let Some(attempt) = self.request_attempt(terminal.attempt_id()) else {
            return Ok(None);
        };
        if !Self::boundary_is_selected(&selected, attempt.authorization()) {
            return Ok(None);
        }
        let Some(step_id) = attempt.authorization().owner().agent_step() else {
            return Ok(None);
        };
        let start = self
            .turn_starts
            .get(step_id.turn_id())
            .ok_or_else(|| JournalProjectionError::MissingTurn(step_id.turn_id().clone()))?;
        let other_unresolved = self.request_attempts().any(|other| {
            other.terminal().is_none()
                && other.authorization().attempt_id() != terminal.attempt_id()
                && other
                    .authorization()
                    .owner()
                    .agent_step()
                    .is_some_and(|step| step.turn_id() == step_id.turn_id())
                && Self::boundary_is_selected(&selected, other.authorization())
        });
        cumulative_usage_event(
            attempt.authorization(),
            terminal,
            &start.agent_id,
            &mut totals,
            other_unresolved,
        )
    }

    pub(crate) fn unfinished_turn_usage_event(
        &self,
        head: &HeadName,
        turn_id: &TurnId,
    ) -> Result<Option<ConversationEvent>, JournalProjectionError> {
        if !self.request_attempts().any(|attempt| {
            attempt.terminal().is_none()
                && attempt
                    .authorization()
                    .owner()
                    .agent_step()
                    .is_some_and(|step| step.turn_id() == turn_id)
        }) {
            return Ok(None);
        }
        let selected: BTreeSet<_> = self
            .path(head)?
            .into_iter()
            .map(|entry| entry.id.clone())
            .collect();
        let mut total = UsageAccumulator::default();
        let mut unknown = false;
        for attempt in self.request_attempts().filter(|attempt| {
            attempt
                .authorization()
                .owner()
                .agent_step()
                .is_some_and(|step| step.turn_id() == turn_id)
                && Self::boundary_is_selected(&selected, attempt.authorization())
        }) {
            match attempt.terminal().map(RequestAttemptTerminal::terminal) {
                None => unknown = true,
                Some(RequestAttemptTerminalState::Dispatched { usage, .. }) => {
                    total
                        .add(usage.clone())
                        .map_err(|()| JournalProjectionError::TurnUsageOverflow(turn_id.clone()))?;
                }
                Some(RequestAttemptTerminalState::NotDispatched { .. }) => {}
            }
        }
        if !unknown {
            return Ok(None);
        }
        let start = self
            .turn_starts
            .get(turn_id)
            .ok_or_else(|| JournalProjectionError::MissingTurn(turn_id.clone()))?;
        Ok(Some(unknown_usage_event(
            turn_id,
            &start.agent_id,
            Some(&total),
        )))
    }
}
