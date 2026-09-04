use std::time::Instant;

use plexmaton_agent::{
    DispatchedRequestTiming, ElapsedMillis, ModelError, ModelStepId, RequestAttemptId,
    RequestAttemptTerminal, RequestAttemptTerminalState, RequestDispatchedOutcome,
    RequestNotDispatchedOutcome, StopReason, UnixMillis,
};
use plexmaton_core::TokenUsage;

use crate::runtime::{ModelCompletion, ModelOutput, ModelTerminalReport};

pub(super) struct RequestTimer {
    dispatched_at: UnixMillis,
    started: Instant,
    headers_after_ms: Option<ElapsedMillis>,
    first_output_after_ms: Option<ElapsedMillis>,
}

impl RequestTimer {
    pub(super) fn start(dispatched_at: UnixMillis) -> Self {
        Self {
            dispatched_at,
            started: Instant::now(),
            headers_after_ms: None,
            first_output_after_ms: None,
        }
    }

    pub(super) fn headers_arrived(&mut self) {
        self.headers_after_ms = Some(self.elapsed());
    }

    pub(super) fn output_arrived(&mut self, output: &ModelOutput) {
        if self.first_output_after_ms.is_none() && output.is_first_output() {
            self.first_output_after_ms = Some(self.elapsed());
        }
    }

    pub(super) fn completed(
        self,
        attempt_id: RequestAttemptId,
        step_id: ModelStepId,
        reason: StopReason,
        usage: TokenUsage,
    ) -> ModelTerminalReport {
        self.finish(
            attempt_id,
            step_id,
            RequestDispatchedOutcome::Completed {
                stop_reason: reason,
            },
            usage,
            ModelCompletion::Stopped(reason),
        )
    }

    pub(super) fn failed(
        self,
        attempt_id: RequestAttemptId,
        step_id: ModelStepId,
        error: ModelError,
        usage: TokenUsage,
    ) -> ModelTerminalReport {
        let outcome = match &error {
            ModelError::Transport { .. } => RequestDispatchedOutcome::TransportFailed,
            ModelError::RateLimited { .. } => RequestDispatchedOutcome::RateLimited,
            ModelError::ContextTooLong => RequestDispatchedOutcome::ContextTooLong,
            ModelError::Malformed { .. } => RequestDispatchedOutcome::Malformed,
        };
        self.finish(
            attempt_id,
            step_id,
            outcome,
            usage,
            ModelCompletion::Failed(error),
        )
    }

    pub(super) fn cancelled(
        self,
        attempt_id: RequestAttemptId,
        step_id: ModelStepId,
        usage: TokenUsage,
    ) -> ModelTerminalReport {
        self.finish(
            attempt_id,
            step_id,
            RequestDispatchedOutcome::Cancelled,
            usage,
            ModelCompletion::Cancelled,
        )
    }

    fn finish(
        self,
        attempt_id: RequestAttemptId,
        step_id: ModelStepId,
        outcome: RequestDispatchedOutcome,
        usage: TokenUsage,
        completion: ModelCompletion,
    ) -> ModelTerminalReport {
        let timing = DispatchedRequestTiming::new(
            self.dispatched_at,
            self.headers_after_ms,
            self.first_output_after_ms,
            self.elapsed(),
        )
        .unwrap_or_else(|error| unreachable!("one request timer is monotonic: {error}"));
        let terminal = RequestAttemptTerminal::new(
            attempt_id,
            RequestAttemptTerminalState::Dispatched {
                timing,
                outcome,
                usage,
            },
        )
        .unwrap_or_else(|error| unreachable!("provider codec validated request usage: {error}"));
        ModelTerminalReport::new(step_id, terminal, completion)
    }

    fn elapsed(&self) -> ElapsedMillis {
        ElapsedMillis::try_from(self.started.elapsed())
            .unwrap_or_else(|error| unreachable!("one request duration fits u64 millis: {error}"))
    }
}

pub(super) fn not_dispatched_report(
    attempt_id: RequestAttemptId,
    step_id: ModelStepId,
    outcome: RequestNotDispatchedOutcome,
    completion: ModelCompletion,
) -> ModelTerminalReport {
    let terminal = RequestAttemptTerminal::new(
        attempt_id,
        RequestAttemptTerminalState::NotDispatched { outcome },
    )
    .unwrap_or_else(|error| unreachable!("not-dispatched state has no measurements: {error}"));
    ModelTerminalReport::new(step_id, terminal, completion)
}
