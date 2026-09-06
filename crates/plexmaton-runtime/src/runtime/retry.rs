use super::*;
use plexmaton_agent::{RetryCandidate, RetryTarget};

impl LiveRuntime {
    /// Only acknowledged, idle state can expose an actionable retry.
    pub fn retry_candidate(&self) -> Option<RetryCandidate> {
        if self.shutdown_state != ShutdownState::Open
            || self.journal_failed
            || self.has_active_work()
            || self.pending_commit.is_some()
            || !self.pending_inputs.is_empty()
        {
            return None;
        }
        self.agent.retry_candidate()
    }

    /// Execute one checked retry through the ordinary owned commit/effect boundary.
    pub async fn retry(
        &mut self,
        target: RetryTarget,
        edited: Option<String>,
    ) -> Result<DispatchReport, RuntimeError> {
        self.retry_selected(target, edited, None).await
    }

    /// Edit/retry with an explicitly selected composer skill, including numeric names.
    pub async fn retry_skill(
        &mut self,
        target: RetryTarget,
        edited: String,
        name: String,
    ) -> Result<DispatchReport, RuntimeError> {
        self.retry_selected(target, Some(edited), Some(name)).await
    }

    async fn retry_selected(
        &mut self,
        target: RetryTarget,
        edited: Option<String>,
        selected: Option<String>,
    ) -> Result<DispatchReport, RuntimeError> {
        if self
            .retry_candidate()
            .is_none_or(|candidate| candidate.target != target)
        {
            return Err(RuntimeError::RetryUnavailable);
        }
        if let Some(text) = &edited {
            match self.resolve_skill_request(text, selected.as_deref()) {
                Ok(Some(name)) => {
                    self.prepare_skill_retry(target, text.clone(), name);
                    return Ok(self.take_report());
                }
                Err(error) => {
                    self.report.undelivered.push(UndeliveredInput::with_skill(
                        text.clone(),
                        selected,
                        UndeliveredReason::SkillUnavailable,
                    ));
                    self.report.skill_errors.push(error.to_string());
                    return Ok(self.take_report());
                }
                Ok(None) => {}
            }
        }
        let rejected = edited
            .as_ref()
            .map(|text| UndeliveredInput {
                text: text.clone(),
                skill: None,
                reason: UndeliveredReason::PersistenceFailed,
            })
            .into_iter()
            .collect();
        let reaction = self
            .agent
            .retry_at(&target, edited, self.clock.now())
            .map_err(|_| RuntimeError::RetryUnavailable)?;
        let transition = async {
            self.begin_transition(reaction, rejected, AfterCommit::None)?;
            self.finish_transition().await
        }
        .await;
        if self.journal_failed {
            self.finish_failed_owners().await;
        }
        transition?;
        Ok(self.take_report())
    }
}
