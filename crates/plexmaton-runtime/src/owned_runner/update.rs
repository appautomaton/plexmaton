//! Tagged child and owner-operation updates.

use plexmaton_agent::collaboration::{CollaborationItemRef, TurnBoundary};

use super::{RunnerIdentity, WakeHint};
use crate::{
    DispatchReport, OwnedHandoffFailure, OwnedHandoffReport, OwnedScheduleFailure,
    OwnedSchedulingError, OwnedStopReport, RuntimeError, RuntimeUpdate,
};

/// One runtime output attributed to the runner incarnation that produced it.
#[derive(Debug)]
pub enum OwnedRunnerUpdate {
    /// Normal semantic event, ownership report or terminal runtime update.
    Runtime {
        identity: RunnerIdentity,
        update: Box<RuntimeUpdate>,
    },
    /// The runtime could no longer make normal progress and now accepts control only.
    Failed {
        identity: RunnerIdentity,
        error: RuntimeError,
    },
    /// The supervised child task panicked or stopped without a terminal update.
    WorkerFailed { identity: RunnerIdentity },
    /// A schedule whose caller was cancelled reached one retained terminal result.
    ScheduleSettled {
        identity: RunnerIdentity,
        outcome: Box<Result<DispatchReport, OwnedScheduleFailure>>,
    },
    /// A child input whose caller was cancelled reached one retained terminal result.
    UserInputSettled {
        identity: RunnerIdentity,
        outcome: Box<Result<DispatchReport, crate::UserInputFailure>>,
    },
    /// A Stop whose caller was cancelled reached one retained terminal result.
    StopSettled {
        identity: RunnerIdentity,
        outcome: Box<Result<OwnedStopReport, OwnedSchedulingError>>,
    },
    /// A Handoff whose caller was cancelled reached one retained terminal result.
    HandoffSettled {
        identity: RunnerIdentity,
        outcome: Box<Result<OwnedHandoffReport, OwnedHandoffFailure>>,
    },
    /// Fresh idle boundary for one coalesced wake; consumed by the scheduling owner.
    #[doc(hidden)]
    WakeReady {
        identity: RunnerIdentity,
        hint: WakeHint,
        boundary: Box<TurnBoundary>,
        previous: Option<CollaborationItemRef>,
    },
    /// Failure to take a fresh idle boundary for a retained wake.
    #[doc(hidden)]
    WakeFailed {
        identity: RunnerIdentity,
        hint: WakeHint,
        error: RuntimeError,
    },
    /// A retained wake could not yet cross a busy bounded owner.
    #[doc(hidden)]
    WakeDeferred { identity: RunnerIdentity },
    /// A canonical wake reached a deterministic typed refusal.
    #[doc(hidden)]
    WakeRejected {
        identity: RunnerIdentity,
        error: Box<OwnedSchedulingError>,
    },
    /// No canonical collaboration facts remained eligible when the wake was rechecked.
    #[doc(hidden)]
    WakeIdle { identity: RunnerIdentity },
    /// One coalesced wake created exactly one owned scheduling command.
    #[doc(hidden)]
    WakeScheduled {
        identity: RunnerIdentity,
        report: Box<DispatchReport>,
    },
}

impl OwnedRunnerUpdate {
    /// Exact endpoint and incarnation that produced this update.
    #[must_use]
    pub const fn identity(&self) -> &RunnerIdentity {
        match self {
            Self::Runtime { identity, .. }
            | Self::Failed { identity, .. }
            | Self::WorkerFailed { identity }
            | Self::ScheduleSettled { identity, .. }
            | Self::UserInputSettled { identity, .. }
            | Self::StopSettled { identity, .. }
            | Self::HandoffSettled { identity, .. }
            | Self::WakeReady { identity, .. }
            | Self::WakeFailed { identity, .. }
            | Self::WakeDeferred { identity }
            | Self::WakeRejected { identity, .. }
            | Self::WakeIdle { identity }
            | Self::WakeScheduled { identity, .. } => identity,
        }
    }

    /// Runtime payload for a normal child update.
    #[must_use]
    pub fn runtime_update(&self) -> Option<&RuntimeUpdate> {
        match self {
            Self::Runtime { update, .. } => Some(update.as_ref()),
            _ => None,
        }
    }

    /// Runtime failure emitted before the child task closed.
    #[must_use]
    pub fn runtime_failure(&self) -> Option<&RuntimeError> {
        match self {
            Self::Failed { error, .. } | Self::WakeFailed { error, .. } => Some(error),
            _ => None,
        }
    }

    /// Terminal result of a schedule whose original reply wait was cancelled.
    #[must_use]
    pub fn schedule_outcome(&self) -> Option<&Result<DispatchReport, OwnedScheduleFailure>> {
        match self {
            Self::ScheduleSettled { outcome, .. } => Some(outcome.as_ref()),
            _ => None,
        }
    }

    /// Terminal result of child input whose original reply wait was cancelled.
    #[must_use]
    pub fn user_input_outcome(&self) -> Option<&Result<DispatchReport, crate::UserInputFailure>> {
        match self {
            Self::UserInputSettled { outcome, .. } => Some(outcome.as_ref()),
            _ => None,
        }
    }

    /// Terminal result of a Stop whose original reply wait was cancelled.
    #[must_use]
    pub fn stop_outcome(&self) -> Option<&Result<OwnedStopReport, OwnedSchedulingError>> {
        match self {
            Self::StopSettled { outcome, .. } => Some(outcome.as_ref()),
            _ => None,
        }
    }

    /// Terminal result of a Handoff whose original reply wait was cancelled.
    #[must_use]
    pub fn handoff_outcome(&self) -> Option<&Result<OwnedHandoffReport, OwnedHandoffFailure>> {
        match self {
            Self::HandoffSettled { outcome, .. } => Some(outcome.as_ref()),
            _ => None,
        }
    }

    /// Report produced by a successfully scheduled canonical wake.
    #[must_use]
    pub fn wake_report(&self) -> Option<&DispatchReport> {
        match self {
            Self::WakeScheduled { report, .. } => Some(report.as_ref()),
            _ => None,
        }
    }

    /// Typed scheduling refusal produced by a canonical wake recheck.
    #[must_use]
    pub fn wake_rejection(&self) -> Option<&OwnedSchedulingError> {
        match self {
            Self::WakeRejected { error, .. } => Some(error.as_ref()),
            _ => None,
        }
    }

    /// Whether this is the terminal marker for its runner update stream.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        matches!(self, Self::Failed { .. } | Self::WorkerFailed { .. })
            || matches!(
                self,
                Self::Runtime { update, .. }
                    if matches!(update.as_ref(), RuntimeUpdate::Finished)
            )
    }
}
