//! Cancellation-safe owner operation settlement.

use super::*;

impl OwnedCollaboration {
    pub(super) async fn settle_abandoned_operation(&mut self) -> Option<OwnedRunnerUpdate> {
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
            let identity = self
                .runners
                .get(&conversation)
                .map(|slot| slot.runner.identity().clone())?;
            let outcome = self.stop(&conversation).await;
            return Some(OwnedRunnerUpdate::StopSettled {
                identity,
                outcome: Box::new(outcome),
            });
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
}
