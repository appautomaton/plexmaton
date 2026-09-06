use std::{pin::Pin, time::Duration};

use plexmaton_agent::{CompactionAttemptFinished, ModelCall, ModelStepId, RequestAttemptId};
use plexmaton_provider::{CompactionInput, PreparedCompaction};
use tokio::time::Sleep;
use tokio_util::sync::CancellationToken;

use crate::runtime::RetainedFuture;

const MAX_SUMMARY_ATTEMPTS_PER_TURN: u8 = 3;
pub(in crate::runtime) const DEFAULT_COMPACTION_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::runtime) enum CompactionTrigger {
    TurnStartSoft,
    HardLimit,
    ContextError,
}

pub(in crate::runtime) enum Continuation {
    ModelCall {
        original: ModelCall,
        trigger: CompactionTrigger,
    },
    Cancelled,
}

pub(in crate::runtime) struct CompactionOperation {
    pub(in crate::runtime) prepared: PreparedCompaction,
    pub(in crate::runtime) continuation: Continuation,
    pub(in crate::runtime) phase: CompactionPhase,
}

impl CompactionOperation {
    pub(in crate::runtime) fn cancel(&mut self) {
        self.continuation = Continuation::Cancelled;
        if let CompactionPhase::Running { cancellation, .. } = &mut self.phase {
            cancellation.cancel();
        }
    }
}

pub(in crate::runtime) enum CompactionPhase {
    Transitioning,
    Authorizing {
        attempt_id: RequestAttemptId,
        input: CompactionInput,
    },
    Running {
        cancellation: CancellationToken,
        future: RetainedFuture<CompactionAttemptFinished>,
        deadline: Pin<Box<Sleep>>,
        timed_out: bool,
    },
    Finishing {
        finished: CompactionAttemptFinished,
    },
    Failing,
    Publishing,
}

#[derive(Default)]
pub(in crate::runtime) struct TurnCompactionBudget {
    turn_id: Option<plexmaton_core::TurnId>,
    summary_attempts: u8,
    context_recoveries: u8,
}

impl TurnCompactionBudget {
    fn select(&mut self, step_id: &ModelStepId) {
        if self.turn_id.as_ref() != Some(step_id.turn_id()) {
            self.turn_id = Some(step_id.turn_id().clone());
            self.summary_attempts = 0;
            self.context_recoveries = 0;
        }
    }

    pub(in crate::runtime) fn has_summary_attempt(&mut self, step_id: &ModelStepId) -> bool {
        self.select(step_id);
        self.summary_attempts < MAX_SUMMARY_ATTEMPTS_PER_TURN
    }

    pub(in crate::runtime) fn take_summary_attempt(&mut self, step_id: &ModelStepId) -> bool {
        if !self.has_summary_attempt(step_id) {
            return false;
        }
        self.summary_attempts += 1;
        true
    }

    pub(in crate::runtime) fn take_context_recovery(&mut self, step_id: &ModelStepId) -> bool {
        self.select(step_id);
        if self.context_recoveries != 0 {
            return false;
        }
        self.context_recoveries = 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_agent::ModelStepId;

    use super::TurnCompactionBudget;

    fn step(turn: &str, index: u16) -> ModelStepId {
        serde_json::from_value(serde_json::json!({ "turn_id": turn, "index": index }))
            .unwrap_or_else(|error| panic!("valid model step fixture: {error}"))
    }

    /// CPL-7: separate compaction operations share one three-attempt turn bound, while the one
    /// context-error recovery is independent and both counters reset for a new turn.
    #[test]
    fn cpl_7_turn_compaction_limits_are_bounded_independent_and_reset() {
        let mut budget = TurnCompactionBudget::default();

        for index in 1..=3 {
            assert!(budget.take_summary_attempt(&step("turn-a", index)));
        }
        assert!(!budget.take_summary_attempt(&step("turn-a", 4)));
        assert!(budget.take_context_recovery(&step("turn-a", 4)));
        assert!(!budget.take_context_recovery(&step("turn-a", 5)));
        assert!(!budget.take_summary_attempt(&step("turn-a", 5)));

        assert!(budget.take_context_recovery(&step("turn-b", 1)));
        assert!(!budget.take_context_recovery(&step("turn-b", 2)));
        for index in 1..=3 {
            assert!(budget.take_summary_attempt(&step("turn-b", index)));
        }
        assert!(!budget.take_summary_attempt(&step("turn-b", 4)));
    }
}
