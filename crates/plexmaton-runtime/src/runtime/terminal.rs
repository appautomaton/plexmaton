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
        let result = (&mut active.future).await;
        Box::pin(self.drain_ready_signals()).await?;
        let panicked = result.is_err();
        self.retain_model_result(result);
        self.finish_attempt_audit_during_owner_action().await?;
        self.settle_model_completion().await?;
        if panicked {
            Err(RuntimeError::ProviderFutureFailed(step_id))
        } else {
            Ok(())
        }
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
