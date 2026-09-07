//! Narrow provider boundary for owned requests and idle model settings replacement.
use super::{ModelSignal, ModelTerminalReport};
use futures_util::{FutureExt as _, future::BoxFuture};
use plexmaton_agent::{
    CompactionAttemptFinished, CompactionFailure, CompactionInputMode, CompactionOutcome,
    ModelCall, RequestAttemptId, RequestAttemptTerminal, RequestAttemptTerminalState,
    RequestEnvironment, RequestNotDispatchedOutcome,
};
use plexmaton_provider::CompactionInput;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub(crate) trait ModelDriver: Send + Sync + 'static {
    /// Drivers explicitly opt in to receiving resolved collaboration context.
    fn supports_collaboration(&self) -> bool {
        false
    }

    fn with_model(
        &self,
        _model: plexmaton_provider::ResolvedModel,
        _key: plexmaton_provider::ApiKey,
    ) -> Result<std::sync::Arc<dyn ModelDriver>, crate::ModelChangeRefusal> {
        Err(crate::ModelChangeRefusal::Unavailable)
    }

    fn with_reasoning_effort(
        &self,
        _effort: plexmaton_core::ReasoningEffort,
    ) -> Result<std::sync::Arc<dyn ModelDriver>, crate::ModelChangeRefusal> {
        Err(crate::ModelChangeRefusal::Unavailable)
    }

    fn request_environment(&self) -> &RequestEnvironment;

    /// Synthetic drivers have no configured model limits; production exposes its exact inputs.
    fn budget_inputs(
        &self,
    ) -> Option<(
        &plexmaton_provider::ResolvedModel,
        &[plexmaton_provider::FunctionTool],
    )> {
        None
    }

    fn drive(
        &self,
        attempt_id: RequestAttemptId,
        call: ModelCall,
        signals: mpsc::Sender<ModelSignal>,
        cancellation: CancellationToken,
    ) -> BoxFuture<'static, ModelTerminalReport>;

    /// Runs one summarizer request without fabricating an agent step or exposing tool effects.
    fn summarize(
        &self,
        attempt_id: RequestAttemptId,
        input: CompactionInput,
        _max_summary_bytes: usize,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'static, CompactionAttemptFinished> {
        let _request = input.into_request();
        async move {
            let terminal = RequestAttemptTerminal::new(
                attempt_id,
                RequestAttemptTerminalState::NotDispatched {
                    outcome: RequestNotDispatchedOutcome::PreparationFailed,
                },
            )
            .unwrap_or_else(|error| unreachable!("default compaction terminal is valid: {error}"));
            CompactionAttemptFinished::new(
                terminal,
                CompactionInputMode::Verbatim,
                CompactionOutcome::Failed {
                    kind: CompactionFailure::Unavailable,
                    output: None,
                },
            )
            .unwrap_or_else(|error| unreachable!("default compaction failure is valid: {error}"))
        }
        .boxed()
    }
}
