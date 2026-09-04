//! Cancellation joins the exact retained provider future before ownership is released.

use super::LiveRuntime;
use crate::RuntimeError;

impl LiveRuntime {
    pub(super) async fn cancel_active(&mut self) -> Result<(), RuntimeError> {
        let Some(active) = self.active.as_mut() else {
            return Ok(());
        };
        active.cancellation.cancel();
        let step_id = active.step_id.clone();
        let joined = (&mut active.future)
            .await
            .map_err(|_| RuntimeError::ProviderFutureFailed(step_id.clone()));
        Box::pin(self.drain_ready_signals()).await?;
        self.active.take();
        joined?;
        Ok(())
    }

    pub(super) async fn discard_active_after_journal_failure(
        &mut self,
    ) -> Result<(), RuntimeError> {
        let Some(active) = self.active.as_mut() else {
            return Ok(());
        };
        active.cancellation.cancel();
        let step_id = active.step_id.clone();
        let joined = (&mut active.future)
            .await
            .map_err(|_| RuntimeError::ProviderFutureFailed(step_id));
        self.active.take();
        while self.signal_rx.try_recv().is_ok() {}
        joined.map(|_| ())
    }
}
