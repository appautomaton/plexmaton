//! Joined owner shutdown and retained operation results.

use super::*;

impl OwnedCollaboration {
    /// Settles retained operations, then closes scheduling and admits every runner shutdown.
    pub async fn begin_shutdown(&mut self) -> Result<(), OwnedSchedulingError> {
        if self.shutting_down {
            return Ok(());
        }
        self.clear_wakes();
        self.settle_ingress_for_shutdown().await;
        if self.pending_handoff.is_none() && self.writer.has_pending_admission() {
            self.shutdown_settlements
                .push(OwnedShutdownSettlement::Admission(
                    self.writer.finish_pending_admission().await,
                ));
        }
        if self.pending_handoff.is_some() {
            let attempt = self
                .pending_handoff
                .as_ref()
                .expect("pending Handoff")
                .attempt
                .clone();
            let outcome = self.handoff(attempt).await;
            if outcome.is_err() {
                self.pending_handoff.take();
            }
            self.shutdown_settlements
                .push(OwnedShutdownSettlement::Handoff(outcome));
        }
        if let Some(conversation) = self
            .pending_stop
            .as_ref()
            .map(|pending| pending.conversation.clone())
        {
            let outcome = self.stop(&conversation).await;
            self.shutdown_settlements
                .push(OwnedShutdownSettlement::Stop(outcome));
        }
        if let Some(settlement) = self.pending_user_target_settlement.take() {
            self.shutdown_settlements
                .push(OwnedShutdownSettlement::UserTargetInput(
                    settlement.into_outcome(),
                ));
        }
        if let Some((_identity, outcome)) = self.detached_user_input.take() {
            self.shutdown_settlements
                .push(OwnedShutdownSettlement::UserInput(outcome));
        }
        if let Some(pending) = &self.pending_schedule {
            let request = pending.request.clone();
            let outcome =
                self.finish_pending_schedule()
                    .await
                    .map_err(|source| OwnedScheduleFailure {
                        source: Box::new(source),
                        request: Box::new(request),
                    });
            self.shutdown_settlements
                .push(OwnedShutdownSettlement::Schedule(outcome));
        }
        if self.pending_user_target_input.is_some() {
            let settlement = self.finish_pending_user_target_input().await;
            self.shutdown_settlements
                .push(OwnedShutdownSettlement::UserTargetInput(
                    settlement.into_outcome(),
                ));
        }
        if self.pending_user_input.is_some() {
            let outcome = self.finish_pending_user_input().await;
            self.shutdown_settlements
                .push(OwnedShutdownSettlement::UserInput(outcome));
        }
        self.shutting_down = true;
        for slot in self.runners.values_mut().filter(|slot| !slot.finished) {
            if !slot.shutdown_requested {
                match slot.runner.begin_shutdown() {
                    Ok(()) => slot.shutdown_requested = true,
                    Err(error) => slot.shutdown_error = Some(error),
                }
            }
        }
        Ok(())
    }

    /// Joins all owners and returns every settled result, including with a terminal failure.
    pub async fn finish_shutdown(&mut self) -> Result<OwnedShutdownReport, OwnedShutdownFailure> {
        if self.shutdown_finished {
            return Err(shutdown_failure(OwnedSchedulingError::ShutdownFinished));
        }
        if !self.shutting_down {
            return Err(shutdown_failure(OwnedSchedulingError::ShutdownNotStarted));
        }
        if self.runners.values().any(|slot| !slot.finished) {
            return Err(shutdown_failure(OwnedSchedulingError::UpdatesPending));
        }
        for slot in self.runners.values_mut() {
            if slot.joined && !slot.shutdown_requested {
                continue;
            }
            if slot.shutdown_requested {
                match slot.runner.finish_shutdown().await {
                    Ok(report) => slot.shutdown_report = Some(report),
                    Err(error) => slot.shutdown_error = Some(error),
                }
            } else if let Err(error) = slot.runner.join().await {
                slot.shutdown_error = Some(error);
            }
            slot.joined = true;
        }
        if let Some(error) = self
            .runners
            .values_mut()
            .find_map(|slot| slot.shutdown_error.take())
        {
            self.retain_shutdown_error(OwnedSchedulingError::Control(error));
        }
        if let Err(error) = self.writer.shutdown().await {
            self.retain_shutdown_error(OwnedSchedulingError::Writer(error));
        }
        let mut reports = Vec::with_capacity(self.runners.len());
        for slot in self.runners.values_mut() {
            if let Some(report) = slot.shutdown_report.take() {
                reports.push((slot.runner.identity().clone(), report));
            }
        }
        self.runners.clear();
        let settlements = std::mem::take(&mut self.shutdown_settlements);
        self.shutdown_finished = true;
        let report = OwnedShutdownReport {
            runners: reports,
            settlements,
        };
        match self.shutdown_error.take() {
            Some(source) => Err(OwnedShutdownFailure { source, report }),
            None => Ok(report),
        }
    }

    fn retain_shutdown_error(&mut self, error: OwnedSchedulingError) {
        if self.shutdown_error.is_none() {
            self.shutdown_error = Some(Box::new(error));
        }
    }
}

fn shutdown_failure(source: OwnedSchedulingError) -> OwnedShutdownFailure {
    OwnedShutdownFailure {
        source: Box::new(source),
        report: OwnedShutdownReport {
            runners: Vec::new(),
            settlements: Vec::new(),
        },
    }
}
