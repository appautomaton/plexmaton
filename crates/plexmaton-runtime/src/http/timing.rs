use std::time::Instant;

use plexmaton_agent::{
    DispatchedRequestTiming, ElapsedMillis, ModelError, RequestAttemptId, RequestAttemptTerminal,
    RequestAttemptTerminalState, RequestCost, RequestDispatchedOutcome,
    RequestNotDispatchedOutcome, StopReason, UnixMillis,
};
use plexmaton_core::TokenUsage;

use crate::runtime::{ModelCompletion, ModelOutput};

/// One provider attempt before the caller attaches agent-step or compaction ownership.
pub(super) struct AttemptReport {
    pub(super) terminal: RequestAttemptTerminal,
    pub(super) completion: ModelCompletion,
}

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
        reason: StopReason,
        usage: TokenUsage,
        cost: RequestCost,
    ) -> AttemptReport {
        self.finish(
            attempt_id,
            RequestDispatchedOutcome::Completed {
                stop_reason: reason,
            },
            usage,
            cost,
            ModelCompletion::Stopped(reason),
        )
    }

    pub(super) fn failed(
        self,
        attempt_id: RequestAttemptId,
        error: ModelError,
        usage: TokenUsage,
    ) -> AttemptReport {
        let outcome = match &error {
            ModelError::Transport { .. } => RequestDispatchedOutcome::TransportFailed,
            ModelError::ProviderFailed { .. } => RequestDispatchedOutcome::ProviderFailed,
            ModelError::RateLimited { .. } => RequestDispatchedOutcome::RateLimited,
            ModelError::ContextTooLong => RequestDispatchedOutcome::ContextTooLong,
            ModelError::Malformed { .. } => RequestDispatchedOutcome::Malformed,
        };
        self.finish(
            attempt_id,
            outcome,
            usage,
            RequestCost::Unavailable,
            ModelCompletion::Failed(error),
        )
    }

    pub(super) fn cancelled(
        self,
        attempt_id: RequestAttemptId,
        usage: TokenUsage,
    ) -> AttemptReport {
        self.finish(
            attempt_id,
            RequestDispatchedOutcome::Cancelled,
            usage,
            RequestCost::Unavailable,
            ModelCompletion::Cancelled,
        )
    }

    fn finish(
        self,
        attempt_id: RequestAttemptId,
        outcome: RequestDispatchedOutcome,
        usage: TokenUsage,
        cost: RequestCost,
        completion: ModelCompletion,
    ) -> AttemptReport {
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
                cost,
            },
        )
        .unwrap_or_else(|error| unreachable!("provider codec validated request usage: {error}"));
        AttemptReport {
            terminal,
            completion,
        }
    }

    fn elapsed(&self) -> ElapsedMillis {
        ElapsedMillis::try_from(self.started.elapsed())
            .unwrap_or_else(|error| unreachable!("one request duration fits u64 millis: {error}"))
    }
}

pub(super) fn not_dispatched_report(
    attempt_id: RequestAttemptId,
    outcome: RequestNotDispatchedOutcome,
    completion: ModelCompletion,
) -> AttemptReport {
    let terminal = RequestAttemptTerminal::new(
        attempt_id,
        RequestAttemptTerminalState::NotDispatched { outcome },
    )
    .unwrap_or_else(|error| unreachable!("not-dispatched state has no measurements: {error}"));
    AttemptReport {
        terminal,
        completion,
    }
}
