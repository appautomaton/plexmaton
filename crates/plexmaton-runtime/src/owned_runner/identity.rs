//! Process-local runner and advisory wake identities.

use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU64, Ordering};

use plexmaton_agent::collaboration::MailEndpoint;
use plexmaton_core::{CollaborationItemId, TurnId};

/// Process-local incarnation of one child runner; zero is never issued.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RunnerGeneration(NonZeroU64);

static NEXT_RUNNER_GENERATION: AtomicU64 = AtomicU64::new(1);

impl RunnerGeneration {
    /// Constructs an explicit nonzero incarnation.
    #[must_use]
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Numeric incarnation used only to reject stale process-local updates.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

pub(crate) fn next_runner_generation() -> Option<RunnerGeneration> {
    NEXT_RUNNER_GENERATION
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(1)
        })
        .ok()
        .and_then(RunnerGeneration::new)
}

/// Exact child endpoint plus its current process-local runner incarnation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunnerIdentity {
    pub(super) endpoint: MailEndpoint,
    pub(super) generation: RunnerGeneration,
}

impl RunnerIdentity {
    /// Child endpoint owned by this runner.
    #[must_use]
    pub const fn endpoint(&self) -> &MailEndpoint {
        &self.endpoint
    }

    /// Incarnation that every emitted update carries.
    #[must_use]
    pub const fn generation(&self) -> RunnerGeneration {
        self.generation
    }

    #[cfg(test)]
    pub(crate) fn with_generation_for_test(&self, generation: RunnerGeneration) -> Self {
        Self {
            endpoint: self.endpoint.clone(),
            generation,
        }
    }
}

/// Advisory, process-local wake identity; it carries no task or mail content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WakeHint {
    runner: RunnerIdentity,
    admission: CollaborationItemId,
    turn: TurnId,
}

impl WakeHint {
    /// Creates a wake with fresh identities for its possible turn admission and session turn.
    #[must_use]
    pub fn new(runner: RunnerIdentity, admission: CollaborationItemId, turn: TurnId) -> Self {
        Self {
            runner,
            admission,
            turn,
        }
    }

    #[must_use]
    pub const fn runner(&self) -> &RunnerIdentity {
        &self.runner
    }

    pub(crate) const fn admission_id(&self) -> &CollaborationItemId {
        &self.admission
    }

    pub(crate) const fn turn(&self) -> &TurnId {
        &self.turn
    }
}
