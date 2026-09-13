//! Joined collaboration-writer shutdown.

use super::*;

impl CollaborationWriter {
    /// Drains accepted commands, closes the file owner and joins its blocking worker.
    pub(crate) async fn shutdown(&mut self) -> Result<(), CollaborationWriterError> {
        if self.pending_admission.is_some() || self.pending_schedule.is_some() {
            return Err(CollaborationWriterError::Busy);
        }
        let mut quiescence = Ok(());
        if let Some(sender) = &self.sender {
            let (reply, result) = oneshot::channel();
            match sender.try_send(Command::RequireQuiescent { reply }) {
                Ok(()) => {
                    quiescence = result
                        .await
                        .map_err(|_| CollaborationWriterError::WorkerFailed)?;
                }
                Err(mpsc::error::TrySendError::Full(_)) => {
                    return Err(CollaborationWriterError::Busy);
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    self.worker_failed = true;
                }
            }
        }
        if matches!(
            &quiescence,
            Err(CollaborationWriterError::Store(
                CollaborationStoreError::ControlNotQuiescent
            ))
        ) {
            return quiescence;
        }
        self.finish_shutdown(quiescence).await
    }

    pub(super) async fn finish_shutdown(
        &mut self,
        quiescence: Result<(), CollaborationWriterError>,
    ) -> Result<(), CollaborationWriterError> {
        self.sender.take();
        let finished_failed = match self.finished.as_mut() {
            Some(finished) => (&mut *finished).await.unwrap_or(true),
            None => false,
        };
        self.finished.take();
        let joined = self
            .worker
            .take()
            .is_none_or(|worker| worker.join().is_ok());
        self.worker_failed |= finished_failed || !joined;
        if self.worker_failed {
            Err(CollaborationWriterError::WorkerFailed)
        } else {
            quiescence
        }
    }
}

impl Drop for CollaborationWriter {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            let _worker_failed = worker.join().is_err();
        }
    }
}
