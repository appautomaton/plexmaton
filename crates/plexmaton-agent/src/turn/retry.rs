use super::*;
use crate::{RetryCandidate, RetryTarget};

impl Agent {
    /// Available only at an idle, unanswered rate-limited tail.
    pub fn retry_candidate(&self) -> Option<RetryCandidate> {
        if self.is_running() {
            return None;
        }
        self.journal().retry_candidate(self.selected_head())
    }

    /// Retry preserves model input; edited retry preserves the old branch before replacing input.
    pub fn retry_at(
        &mut self,
        target: &RetryTarget,
        edited: Option<String>,
        at: UnixMillis,
    ) -> Result<Reaction, crate::JournalError> {
        let candidate = self
            .retry_candidate()
            .filter(|candidate| &candidate.target == target)
            .ok_or(crate::JournalError::RetryUnavailable)?;
        let mut reaction = Reaction::at(at);
        if let Some(text) = edited {
            if text.trim().is_empty() {
                return Err(crate::JournalError::RetryUnavailable);
            }
            self.record.branch_before_retry(&candidate, &mut reaction)?;
            self.open_turn(text, at, &mut reaction);
            reaction.events.clear();
            reaction.projection_reset = Some(
                self.record
                    .journal()
                    .project(self.record.selected_head())
                    .expect("new branch projects")
                    .events()
                    .to_vec(),
            );
        } else {
            let turn_id = self.record.next_turn_id();
            self.record.commit(
                JournalEntryPayload::TurnRetried {
                    agent_id: self.record.agent_id().clone(),
                    source_turn_id: target.turn_id.clone(),
                    turn_id: turn_id.clone(),
                    opened_at: at,
                },
                &mut reaction,
            );
            self.record.emit(
                &mut reaction,
                SessionEvent::AgentStatusChanged {
                    agent_id: self.record.agent_id().clone(),
                    status: AgentStatus::Running,
                },
            );
            self.open_step(turn_id, 1, &mut reaction);
        }
        Ok(reaction.into_output())
    }
}
