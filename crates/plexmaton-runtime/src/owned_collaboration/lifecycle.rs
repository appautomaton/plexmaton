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
        let user_input_pending = self
            .pending_user_input
            .as_ref()
            .is_some_and(|pending| &pending.conversation == conversation);
        let target_only_pending =
            self.has_user_target_input_for(conversation) && !user_input_pending;
        let interrupted_target =
            target_only_pending && self.interrupt_user_target_input(conversation);
        let Some(slot) = self.runners.get_mut(conversation) else {
            return if interrupted_target {
                Ok(())
            } else {
                Err(OwnedSchedulingError::UnknownRunner)
            };
        };
        if slot.finished {
            return if interrupted_target {
                Ok(())
            } else {
                Err(OwnedSchedulingError::RunnerClosed)
            };
        }
        let stop_started = if schedule_pending || user_input_pending {
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
            user_input: None,
            user_input_identity: None,
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
        let user_input_pending = self.pending_stop.as_ref().is_some_and(|pending| {
            !pending.stop_started
                && self
                    .pending_user_input
                    .as_ref()
                    .is_some_and(|input| &input.conversation == conversation)
        });
        if user_input_pending {
            let identity = self
                .runners
                .get(conversation)
                .expect("pending user input retains its runner")
                .runner
                .identity()
                .clone();
            let user_input = self.finish_pending_user_input().await;
            self.clear_user_target_input_for(conversation);
            let pending = self
                .pending_stop
                .as_mut()
                .expect("pending Stop survives user-input settlement");
            pending.user_input = Some(user_input);
            pending.user_input_identity = Some(identity);
        }
        let stop_started = self
            .pending_stop
            .as_ref()
            .is_some_and(|pending| pending.stop_started);
        if !stop_started {
            let Some(slot) = self.runners.get_mut(conversation) else {
                let mut pending = self.pending_stop.take().expect("pending Stop");
                self.detach_stop_user_input(&mut pending);
                return Err(OwnedSchedulingError::UnknownRunner);
            };
            if slot.finished {
                let mut pending = self.pending_stop.take().expect("pending Stop");
                self.detach_stop_user_input(&mut pending);
                return Err(OwnedSchedulingError::RunnerClosed);
            }
            if let Err(error) = slot.runner.begin_stop() {
                let mut pending = self.pending_stop.take().expect("pending Stop");
                self.detach_stop_user_input(&mut pending);
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
        let mut pending = self
            .pending_stop
            .take()
            .expect("completed Stop retains its scheduling report");
        match stopped {
            Ok(stopped) => Ok(OwnedStopReport {
                scheduled: pending.scheduled,
                user_input: pending.user_input.map(Box::new),
                stopped,
            }),
            Err(error) => {
                self.detach_stop_user_input(&mut pending);
                Err(OwnedSchedulingError::Control(error))
            }
        }
    }

    fn detach_stop_user_input(&mut self, pending: &mut PendingStop) {
        if let Some(identity) = pending.user_input_identity.take()
            && let Some(outcome) = pending.user_input.take()
        {
            self.detached_user_input = Some((identity, outcome));
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
