//! Stop, Handoff, update multiplexing and joined owner shutdown.

use futures_util::stream::{FuturesUnordered, StreamExt as _};

use super::*;

impl OwnedCollaboration {
    #[cfg(test)]
    pub(crate) fn hold_runner_join_for_test(
        &mut self,
        conversation: &ConversationId,
    ) -> (
        std::sync::Arc<tokio::sync::Notify>,
        std::sync::Arc<tokio::sync::Notify>,
    ) {
        self.runners
            .get_mut(conversation)
            .expect("test runner exists")
            .runner
            .hold_join_for_test()
    }

    /// Admits one Stop without waiting for the child to release execution authority.
    ///
    /// The retained operation is settled by [`Self::next_update`] or [`Self::next_activity`], so
    /// a caller can keep pumping unrelated root activity while the child joins (SCH-2/SCH-4).
    pub fn begin_stop(
        &mut self,
        conversation: &ConversationId,
    ) -> Result<(), OwnedSchedulingError> {
        if let Some(pending) = &self.pending_stop {
            if &pending.conversation != conversation {
                return Err(OwnedSchedulingError::StopInProgress);
            }
            return Ok(());
        }
        if self.shutting_down {
            return Err(OwnedSchedulingError::ShuttingDown);
        }
        self.remove_wake(conversation);
        let schedule_pending = self
            .pending_schedule
            .as_ref()
            .is_some_and(|pending| &pending.conversation == conversation);
        let slot = self
            .runners
            .get_mut(conversation)
            .ok_or(OwnedSchedulingError::UnknownRunner)?;
        if slot.finished {
            return Err(OwnedSchedulingError::RunnerClosed);
        }
        let stop_started = if schedule_pending {
            false
        } else {
            slot.runner
                .begin_stop()
                .map_err(OwnedSchedulingError::Control)?;
            true
        };
        self.pending_stop = Some(PendingStop {
            conversation: conversation.clone(),
            scheduled: None,
            stop_started,
        });
        Ok(())
    }

    /// Stops one child and acknowledges only after its runtime has released execution authority.
    pub async fn stop(
        &mut self,
        conversation: &ConversationId,
    ) -> Result<OwnedStopReport, OwnedSchedulingError> {
        if let Some(pending) = &self.pending_stop
            && &pending.conversation != conversation
        {
            return Err(OwnedSchedulingError::StopInProgress);
        }
        if self.pending_stop.is_none() {
            self.begin_stop(conversation)?;
        } else {
            self.remove_wake(conversation);
        }
        self.finish_pending_stop(conversation).await
    }

    async fn finish_pending_stop(
        &mut self,
        conversation: &ConversationId,
    ) -> Result<OwnedStopReport, OwnedSchedulingError> {
        let schedule_pending = self.pending_stop.as_ref().is_some_and(|pending| {
            !pending.stop_started
                && self
                    .pending_schedule
                    .as_ref()
                    .is_some_and(|schedule| &schedule.conversation == conversation)
        });
        if schedule_pending {
            let scheduled = match self.finish_pending_schedule().await {
                Ok(report) => report,
                Err(error) => {
                    self.pending_stop.take();
                    return Err(error);
                }
            };
            self.pending_stop
                .as_mut()
                .expect("pending Stop survives schedule settlement")
                .scheduled = Some(scheduled);
        }
        let stop_started = self
            .pending_stop
            .as_ref()
            .is_some_and(|pending| pending.stop_started);
        if !stop_started {
            let slot = self
                .runners
                .get_mut(conversation)
                .ok_or(OwnedSchedulingError::UnknownRunner)?;
            if slot.finished {
                self.pending_stop.take();
                return Err(OwnedSchedulingError::RunnerClosed);
            }
            if let Err(error) = slot.runner.begin_stop() {
                self.pending_stop.take();
                return Err(OwnedSchedulingError::Control(error));
            }
            self.pending_stop
                .as_mut()
                .expect("pending Stop survives control admission")
                .stop_started = true;
        }
        let stopped = self
            .runners
            .get_mut(conversation)
            .ok_or(OwnedSchedulingError::UnknownRunner)?
            .runner
            .finish_stop()
            .await;
        let pending = self
            .pending_stop
            .take()
            .expect("completed Stop retains its scheduling report");
        match stopped {
            Ok(stopped) => Ok(OwnedStopReport {
                scheduled: pending.scheduled,
                stopped,
            }),
            Err(error) => Err(OwnedSchedulingError::Control(error)),
        }
    }

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
        self.handoff_closed.insert(delegation.clone());
        if let Some(conversation) = self.runners.values().find_map(|slot| {
            (slot.delegation == delegation)
                .then(|| slot.runner.identity().endpoint().conversation.clone())
        }) {
            self.remove_wake(&conversation);
        }
        self.pending_handoff
            .as_mut()
            .expect("successful preflight retains Handoff")
            .preflighted = true;
        Ok(())
    }

    async fn settle_pending_handoff(&mut self) -> Result<(), OwnedHandoffFailure> {
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

    /// Whether an update belongs to the currently registered incarnation of its endpoint.
    #[must_use]
    pub fn accepts_update(&self, update: &OwnedRunnerUpdate) -> bool {
        self.runners
            .get(&update.identity().endpoint().conversation)
            .is_some_and(|slot| slot.runner.identity() == update.identity())
    }

    /// Multiplexes one cancellation-safe update and drops output from retired generations.
    pub async fn next_update(&mut self) -> Option<OwnedRunnerUpdate> {
        loop {
            if let Some(settled) = self.join_finished_runner().await {
                if let Some(update) = settled {
                    return Some(update);
                }
                continue;
            }
            if let Some(update) = self.settle_abandoned_operation().await {
                return Some(update);
            }
            if let Some(update) = self.drive_ready_wake().await {
                return Some(update);
            }
            if let Some(update) = self.queue_pending_wakes() {
                return Some(update);
            }
            let wait_for_writer = self.has_deferred_wakes();
            let mut pending = FuturesUnordered::new();
            for (conversation, slot) in &mut self.runners {
                if !slot.finished {
                    let conversation = conversation.clone();
                    pending.push(async move { (conversation, slot.runner.next_update().await) });
                }
            }
            let next = if wait_for_writer {
                tokio::select! {
                    _writable = self.writer.wait_writable() => {
                        drop(pending);
                        self.resume_deferred_wakes();
                        continue;
                    }
                    next = pending.next() => next,
                }
            } else {
                pending.next().await
            };
            let (conversation, update) = next?;
            drop(pending);
            let Some(update) = update else {
                self.remove_wake(&conversation);
                if let Some(slot) = self.runners.get_mut(&conversation) {
                    slot.finished = true;
                }
                continue;
            };
            if !self.accepts_update(&update) {
                continue;
            }
            self.resume_deferred_wakes();
            match update {
                OwnedRunnerUpdate::WakeReady {
                    identity,
                    hint,
                    boundary,
                    previous,
                } => {
                    let _accepted =
                        self.accept_wake_snapshot(&identity, &hint, *boundary, previous);
                    continue;
                }
                OwnedRunnerUpdate::WakeFailed {
                    identity,
                    hint,
                    error,
                } => {
                    if self.discard_wake(&identity, &hint) {
                        return Some(OwnedRunnerUpdate::WakeFailed {
                            identity,
                            hint,
                            error,
                        });
                    }
                    continue;
                }
                OwnedRunnerUpdate::Failed { ref identity, .. } => {
                    self.remove_wake(&identity.endpoint().conversation);
                }
                OwnedRunnerUpdate::Runtime {
                    ref identity,
                    ref update,
                } if matches!(update.as_ref(), crate::RuntimeUpdate::Finished) => {
                    self.remove_wake(&identity.endpoint().conversation);
                }
                _ => {}
            }
            if update.is_finished() {
                self.remove_wake(&conversation);
                if let Some(slot) = self.runners.get_mut(&conversation) {
                    slot.finished = true;
                    slot.terminal_update = Some(update);
                }
                continue;
            }
            return Some(update);
        }
    }

    /// Completes one retained terminal join before publishing its terminal update.
    async fn join_finished_runner(&mut self) -> Option<Option<OwnedRunnerUpdate>> {
        let conversation = self.runners.iter().find_map(|(conversation, slot)| {
            (slot.finished && !slot.joined).then(|| conversation.clone())
        })?;
        let slot = self
            .runners
            .get_mut(&conversation)
            .expect("selected finished runner remains registered");
        let joined = slot.runner.join().await;
        slot.joined = true;
        if joined.is_err() {
            slot.terminal_update.take();
            slot.shutdown_error = Some(OwnedRunnerError::WorkerFailed);
            return Some(Some(OwnedRunnerUpdate::WorkerFailed {
                identity: slot.runner.identity().clone(),
            }));
        }
        Some(slot.terminal_update.take())
    }
}
