//! Quiescent durable transfer from Main to User control.

use super::*;

impl OwnedCollaboration {
    /// Closes normal admission, joins queued/active work, then appends one explicit Handoff.
    pub async fn handoff(
        &mut self,
        attempt: CollaborationAttempt,
    ) -> Result<OwnedHandoffReport, OwnedHandoffFailure> {
        if let Some(pending) = &self.pending_handoff {
            if pending.attempt != attempt {
                return Err(handoff_failure(
                    OwnedSchedulingError::HandoffPending,
                    attempt,
                    None,
                    None,
                ));
            }
        } else {
            self.begin_handoff(attempt).await?;
        }
        if !self
            .pending_handoff
            .as_ref()
            .expect("retained Handoff")
            .preflighted
        {
            self.finish_handoff_preflight().await?;
        }
        self.settle_pending_handoff().await?;
        self.finish_pending_handoff().await
    }

    async fn begin_handoff(
        &mut self,
        attempt: CollaborationAttempt,
    ) -> Result<(), OwnedHandoffFailure> {
        if self.shutting_down {
            return Err(handoff_failure(
                OwnedSchedulingError::ShuttingDown,
                attempt,
                None,
                None,
            ));
        }
        if self.writer.has_pending_admission() {
            return Err(handoff_failure(
                OwnedSchedulingError::AdmissionInProgress,
                attempt,
                None,
                None,
            ));
        }
        match &attempt.event {
            plexmaton_agent::collaboration::CollaborationEvent::HandoffCompleted { .. } => {}
            _ => {
                return Err(handoff_failure(
                    OwnedSchedulingError::HandoffRequired,
                    attempt,
                    None,
                    None,
                ));
            }
        }
        self.pending_handoff = Some(PendingHandoff {
            attempt,
            preflighted: false,
            already_durable: false,
            scheduled: None,
            stopped: None,
            schedule_settled: false,
            stop_settled: false,
        });
        self.finish_handoff_preflight().await
    }

    async fn finish_handoff_preflight(&mut self) -> Result<(), OwnedHandoffFailure> {
        let attempt = self
            .pending_handoff
            .as_ref()
            .expect("Handoff preflight retains its attempt")
            .attempt
            .clone();
        if let Err(error) = self.writer.preflight_handoff(attempt.clone()).await {
            if !matches!(&error, CollaborationWriterError::Busy) {
                self.pending_handoff.take();
            }
            return Err(handoff_failure(
                OwnedSchedulingError::Writer(error),
                attempt,
                None,
                None,
            ));
        }
        let delegation = match &attempt.event {
            plexmaton_agent::collaboration::CollaborationEvent::HandoffCompleted {
                delegation,
                ..
            } => delegation.clone(),
            _ => unreachable!("validated Handoff variant"),
        };
        let already_durable = match self.writer.delegation_view(delegation.clone()).await {
            Ok(view) => {
                view.controller == plexmaton_agent::collaboration::DelegationController::User
            }
            Err(error) => {
                self.pending_handoff.take();
                return Err(handoff_failure(
                    OwnedSchedulingError::Writer(error),
                    attempt,
                    None,
                    None,
                ));
            }
        };
        self.handoff_closed.insert(delegation.clone());
        if let Some(conversation) = self.runners.values().find_map(|slot| {
            (slot.delegation == delegation)
                .then(|| slot.runner.identity().endpoint().conversation.clone())
        }) {
            self.remove_wake(&conversation);
        }
        let pending = self
            .pending_handoff
            .as_mut()
            .expect("successful preflight retains Handoff");
        pending.preflighted = true;
        pending.already_durable = already_durable;
        Ok(())
    }

    async fn settle_pending_handoff(&mut self) -> Result<(), OwnedHandoffFailure> {
        if self
            .pending_handoff
            .as_ref()
            .is_some_and(|pending| pending.already_durable)
        {
            return Ok(());
        }
        let attempt = self
            .pending_handoff
            .as_ref()
            .expect("preflighted Handoff retains its attempt")
            .attempt
            .clone();
        if !self
            .pending_handoff
            .as_ref()
            .expect("pending Handoff")
            .schedule_settled
        {
            if let Some(pending) = &self.pending_stop {
                let delegation = match &attempt.event {
                    plexmaton_agent::collaboration::CollaborationEvent::HandoffCompleted {
                        delegation,
                        ..
                    } => delegation,
                    _ => unreachable!("preflight admitted only Handoff"),
                };
                let target = self.runners.values().find_map(|slot| {
                    (&slot.delegation == delegation)
                        .then(|| slot.runner.identity().endpoint().conversation.clone())
                });
                if target.as_ref() != Some(&pending.conversation) {
                    return Err(handoff_failure(
                        OwnedSchedulingError::StopInProgress,
                        attempt,
                        None,
                        None,
                    ));
                }
                let report = match self.stop(&pending.conversation.clone()).await {
                    Ok(report) => report,
                    Err(error) => {
                        return Err(handoff_failure(error, attempt, None, None));
                    }
                };
                let pending = self
                    .pending_handoff
                    .as_mut()
                    .expect("Handoff survives prior Stop settlement");
                pending.scheduled = report.scheduled;
                pending.stopped = Some(report.stopped);
                pending.schedule_settled = true;
                pending.stop_settled = true;
                return Ok(());
            }
            let scheduled = if self.pending_schedule.is_some() {
                match self.finish_pending_schedule().await {
                    Ok(report) => Some(report),
                    Err(error) => {
                        return Err(handoff_failure(error, attempt, None, None));
                    }
                }
            } else {
                None
            };
            let pending = self
                .pending_handoff
                .as_mut()
                .expect("Handoff survives schedule settlement");
            pending.scheduled = scheduled;
            pending.schedule_settled = true;
        }
        if !self
            .pending_handoff
            .as_ref()
            .expect("pending Handoff")
            .stop_settled
        {
            let delegation = match &attempt.event {
                plexmaton_agent::collaboration::CollaborationEvent::HandoffCompleted {
                    delegation,
                    ..
                } => delegation,
                _ => unreachable!("preflight admitted only Handoff"),
            };
            let stopped = if let Some(slot) = self
                .runners
                .values_mut()
                .find(|slot| &slot.delegation == delegation && !slot.finished)
            {
                match slot.runner.stop().await {
                    Ok(report) => Some(report),
                    Err(error) => {
                        let pending = self
                            .pending_handoff
                            .as_ref()
                            .expect("failed Stop retains Handoff");
                        return Err(handoff_failure(
                            OwnedSchedulingError::Control(error),
                            attempt,
                            pending.scheduled.clone(),
                            None,
                        ));
                    }
                }
            } else {
                None
            };
            let pending = self
                .pending_handoff
                .as_mut()
                .expect("Handoff survives Stop settlement");
            pending.stopped = stopped;
            pending.stop_settled = true;
        }
        Ok(())
    }

    async fn finish_pending_handoff(&mut self) -> Result<OwnedHandoffReport, OwnedHandoffFailure> {
        let attempt = self
            .pending_handoff
            .as_ref()
            .expect("pending Handoff exists before it is finished")
            .attempt
            .clone();
        match self.writer.admit_handoff(attempt.clone()).await {
            Ok(receipt) => {
                let pending = self
                    .pending_handoff
                    .take()
                    .expect("acknowledged Handoff retains its reports");
                Ok(OwnedHandoffReport {
                    receipt,
                    scheduled: pending.scheduled,
                    stopped: pending.stopped,
                })
            }
            Err(error) => {
                let pending = self
                    .pending_handoff
                    .as_ref()
                    .expect("failed Handoff retains its reports");
                Err(handoff_failure(
                    OwnedSchedulingError::Writer(error),
                    attempt,
                    pending.scheduled.clone(),
                    pending.stopped.clone(),
                ))
            }
        }
    }
}
