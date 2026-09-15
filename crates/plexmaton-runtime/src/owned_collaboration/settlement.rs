//! Cancellation-safe owner operation settlement.

use super::*;

impl OwnedCollaboration {
    pub(crate) async fn settle_cold_handoff(&mut self) -> Option<OwnedHandoffSettlement> {
        let pending = self.pending_handoff.as_ref()?;
        let delegation = match &pending.attempt.event {
            plexmaton_agent::collaboration::CollaborationEvent::HandoffCompleted {
                delegation,
                ..
            } => delegation.clone(),
            _ => unreachable!("pending Handoff retains a Handoff event"),
        };
        if self
            .runners
            .values()
            .any(|slot| slot.delegation == delegation && !slot.joined)
        {
            return None;
        }
        let attempt = pending.attempt.clone();
        let outcome = loop {
            let outcome = self.handoff(attempt.clone()).await;
            if matches!(
                &outcome,
                Err(failure)
                    if matches!(
                        failure.source(),
                        OwnedSchedulingError::Writer(
                            CollaborationWriterError::Busy
                                | CollaborationWriterError::AdmissionBusy { .. }
                        )
                    )
            ) && self.writer.wait_writable().await.is_ok()
            {
                continue;
            }
            break outcome;
        };
        if outcome.is_err() {
            self.pending_handoff.take();
        }
        Some(OwnedHandoffSettlement {
            delegation,
            outcome: Box::new(outcome),
        })
    }

    pub(super) async fn settle_abandoned_operation(&mut self) -> Option<OwnedRunnerUpdate> {
        if let Some((identity, outcome)) = self.detached_user_input.take() {
            return Some(OwnedRunnerUpdate::UserInputSettled {
                identity,
                outcome: Box::new(outcome),
            });
        }
        let different_child_stop = self
            .pending_stop
            .as_ref()
            .zip(self.pending_user_input.as_ref())
            .filter(|(stop, input)| stop.conversation != input.conversation)
            .map(|(stop, _)| stop.conversation.clone());
        if let Some(conversation) = different_child_stop {
            return self.settle_stop_update(conversation).await;
        }
        if let Some(conversation) = self
            .pending_user_input
            .as_ref()
            .map(|pending| pending.conversation.clone())
        {
            let identity = self
                .runners
                .get(&conversation)
                .map(|slot| slot.runner.identity().clone())?;
            let outcome = self.finish_pending_user_input().await;
            return Some(OwnedRunnerUpdate::UserInputSettled {
                identity,
                outcome: Box::new(outcome),
            });
        }
        if let Some(pending) = &self.pending_handoff {
            let delegation = match &pending.attempt.event {
                plexmaton_agent::collaboration::CollaborationEvent::HandoffCompleted {
                    delegation,
                    ..
                } => delegation,
                _ => unreachable!("pending Handoff retains a Handoff event"),
            };
            let identity = self
                .runners
                .values()
                .find(|slot| &slot.delegation == delegation)
                .map(|slot| slot.runner.identity().clone());
            if let Some(identity) = identity {
                let attempt = pending.attempt.clone();
                let outcome = self.handoff(attempt).await;
                if outcome.is_err() {
                    self.pending_handoff.take();
                }
                return Some(OwnedRunnerUpdate::HandoffSettled {
                    identity,
                    outcome: Box::new(outcome),
                });
            }
        }
        if let Some(conversation) = self
            .pending_stop
            .as_ref()
            .map(|pending| pending.conversation.clone())
        {
            return self.settle_stop_update(conversation).await;
        }
        if let Some(pending) = &self.pending_schedule {
            let identity = self
                .runners
                .get(&pending.conversation)
                .map(|slot| slot.runner.identity().clone())?;
            let request = pending.request.clone();
            let wake = self
                .wakes
                .iter()
                .find(|(_, pending)| pending.ready.as_ref() == Some(&request))
                .map(|(conversation, _)| conversation.clone());
            let outcome =
                self.finish_pending_schedule()
                    .await
                    .map_err(|source| OwnedScheduleFailure {
                        source: Box::new(source),
                        request: Box::new(request),
                    });
            if let Some(conversation) = wake {
                self.wakes.remove(&conversation);
                return Some(match outcome {
                    Ok(report) => OwnedRunnerUpdate::WakeScheduled {
                        identity,
                        report: Box::new(report),
                    },
                    Err(failure) => OwnedRunnerUpdate::WakeRejected {
                        identity,
                        error: failure.source,
                    },
                });
            }
            return Some(OwnedRunnerUpdate::ScheduleSettled {
                identity,
                outcome: Box::new(outcome),
            });
        }
        None
    }

    async fn settle_stop_update(
        &mut self,
        conversation: ConversationId,
    ) -> Option<OwnedRunnerUpdate> {
        let identity = self
            .runners
            .get(&conversation)
            .map(|slot| slot.runner.identity().clone())?;
        let outcome = self.stop(&conversation).await;
        Some(OwnedRunnerUpdate::StopSettled {
            identity,
            outcome: Box::new(outcome),
        })
    }
}
