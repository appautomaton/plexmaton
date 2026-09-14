//! Cancellation-safe regular and Handoff admission replies.

use super::*;

impl CollaborationWriter {
    /// Appends one exact fact; cancellation retains its accepted reply in this owner.
    pub(crate) async fn admit(
        &mut self,
        attempt: CollaborationAttempt,
    ) -> Result<ItemReceipt, CollaborationWriterError> {
        match &attempt.event {
            plexmaton_agent::collaboration::CollaborationEvent::TurnAdmitted { .. } => {
                return Err(CollaborationWriterError::TurnRequiresScheduling {
                    attempt: Box::new(attempt),
                });
            }
            plexmaton_agent::collaboration::CollaborationEvent::HandoffCompleted { .. } => {
                return Err(CollaborationWriterError::HandoffRequiresOwnership {
                    attempt: Box::new(attempt),
                });
            }
            _ => {}
        }
        self.admit_command(attempt).await
    }

    pub(crate) async fn admit_handoff(
        &mut self,
        attempt: CollaborationAttempt,
    ) -> Result<ItemReceipt, CollaborationWriterError> {
        debug_assert!(matches!(
            attempt.event,
            plexmaton_agent::collaboration::CollaborationEvent::HandoffCompleted { .. }
        ));
        self.admit_command(attempt).await
    }

    async fn admit_command(
        &mut self,
        attempt: CollaborationAttempt,
    ) -> Result<ItemReceipt, CollaborationWriterError> {
        if let Some(pending) = &self.pending_admission {
            if pending.attempt != attempt {
                return Err(CollaborationWriterError::AdmissionInProgress {
                    attempt: Box::new(attempt),
                });
            }
            return self.finish_pending_admission().await;
        }
        let (reply, result) = oneshot::channel();
        let Some(sender) = &self.sender else {
            return Err(CollaborationWriterError::AdmissionClosed {
                attempt: Box::new(attempt),
            });
        };
        let recovery = attempt.clone();
        let command = Command::Admit { attempt, reply };
        match sender.try_send(command) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(Command::Admit { attempt, .. })) => {
                return Err(CollaborationWriterError::AdmissionBusy {
                    attempt: Box::new(attempt),
                });
            }
            Err(mpsc::error::TrySendError::Closed(Command::Admit { attempt, .. })) => {
                return Err(CollaborationWriterError::AdmissionClosed {
                    attempt: Box::new(attempt),
                });
            }
            Err(_) => unreachable!("admission try_send returns its admission command"),
        }
        self.pending_admission = Some(PendingAdmission {
            attempt: recovery,
            result,
        });
        self.finish_pending_admission().await
    }

    pub(crate) fn has_pending_admission(&self) -> bool {
        self.pending_admission.is_some()
    }

    pub(crate) async fn finish_pending_admission(
        &mut self,
    ) -> Result<ItemReceipt, CollaborationWriterError> {
        let result = self
            .pending_admission
            .as_mut()
            .ok_or(CollaborationWriterError::WorkerFailed)?;
        let outcome = (&mut result.result)
            .await
            .map_err(|_| CollaborationWriterError::WorkerFailed)?;
        self.pending_admission.take();
        outcome
    }
}
