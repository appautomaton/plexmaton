//! Admission and dispatch of one Main-owned child turn through its registered runner.

use super::*;

impl OwnedCollaboration {
    /// Admits and starts one exact Main-owned child execution through the matching runner.
    pub async fn schedule(
        &mut self,
        request: ScheduledTurnRequest,
    ) -> Result<DispatchReport, OwnedScheduleFailure> {
        let recovery = request.clone();
        let result = self.schedule_inner(request).await;
        result.map_err(|source| OwnedScheduleFailure {
            source: Box::new(source),
            request: Box::new(recovery),
        })
    }

    async fn schedule_inner(
        &mut self,
        request: ScheduledTurnRequest,
    ) -> Result<DispatchReport, OwnedSchedulingError> {
        if self.writer.has_pending_admission() {
            return Err(OwnedSchedulingError::AdmissionInProgress);
        }
        if self.pending_stop.is_some() {
            return Err(OwnedSchedulingError::StopInProgress);
        }
        if let Some(pending) = &self.pending_schedule {
            if pending.request != request {
                return Err(OwnedSchedulingError::ScheduleInProgress);
            }
            return self.finish_pending_schedule().await;
        }
        if self.shutting_down {
            return Err(OwnedSchedulingError::ShuttingDown);
        }
        if self.handoff_closed.contains(request.delegation()) {
            return Err(OwnedSchedulingError::HandoffPending);
        }
        let recipient = request.boundary().recipient.clone();
        let slot = self
            .runners
            .get_mut(&recipient.conversation)
            .ok_or(OwnedSchedulingError::UnknownRunner)?;
        if slot.finished
            || slot.delegation != *request.delegation()
            || slot.runner.identity().endpoint() != &recipient
        {
            return Err(OwnedSchedulingError::RunnerMismatch);
        }
        if !slot.runner.supports_collaboration() {
            return Err(OwnedSchedulingError::ProviderUnsupported);
        }
        let reserved = match slot.runner.reserve_start() {
            Ok(reserved) => reserved,
            Err(ReserveStartError::Busy) => return Err(OwnedSchedulingError::RunnerBusy),
            Err(ReserveStartError::Closed) => return Err(OwnedSchedulingError::RunnerClosed),
        };
        self.writer
            .begin_schedule(request.clone())
            .map_err(OwnedSchedulingError::Writer)?;
        self.pending_schedule = Some(PendingOwnedSchedule {
            request,
            conversation: recipient.conversation,
            reserved: Some(reserved),
        });
        self.finish_pending_schedule().await
    }

    pub(super) async fn finish_pending_schedule(
        &mut self,
    ) -> Result<DispatchReport, OwnedSchedulingError> {
        let conversation = self
            .pending_schedule
            .as_ref()
            .ok_or(OwnedSchedulingError::RunnerFailed)?
            .conversation
            .clone();
        if self
            .pending_schedule
            .as_ref()
            .is_some_and(|pending| pending.reserved.is_some())
        {
            let prepared = match self.writer.finish_schedule().await {
                Ok(prepared) => prepared,
                Err(error) => {
                    self.pending_schedule.take();
                    return Err(OwnedSchedulingError::Writer(error));
                }
            };
            let reserved = self
                .pending_schedule
                .as_mut()
                .and_then(|pending| pending.reserved.take())
                .ok_or(OwnedSchedulingError::RunnerFailed)?;
            let slot = self
                .runners
                .get_mut(&conversation)
                .ok_or(OwnedSchedulingError::UnknownRunner)?;
            slot.runner.begin_reserved_start(reserved, prepared);
        }
        let result = self
            .runners
            .get_mut(&conversation)
            .ok_or(OwnedSchedulingError::UnknownRunner)?
            .runner
            .finish_start()
            .await;
        self.pending_schedule.take();
        match result {
            Ok(report) => Ok(report),
            #[cfg(test)]
            Err(ChildStartError::Busy(execution) | ChildStartError::Closed(execution)) => {
                drop(execution);
                Err(OwnedSchedulingError::RunnerClosed)
            }
            Err(ChildStartError::Runtime(error)) => Err(OwnedSchedulingError::Runtime(error)),
            Err(ChildStartError::WorkerFailed) => Err(OwnedSchedulingError::RunnerFailed),
        }
    }
}
