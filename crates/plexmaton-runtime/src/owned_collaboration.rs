//! Single-owner composition of bounded child runners and collaboration authority.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::num::NonZeroUsize;

use plexmaton_agent::collaboration::{
    CollaborationMailProjection, CollaborationRecord, ItemReceipt, MailEndpoint,
};
use plexmaton_core::{ConversationId, DelegationId};
use plexmaton_session_store::collaboration::CollaborationAttempt;
use thiserror::Error;

use crate::collaboration_ingress::{CollaborationIngressOwner, PendingIngress};
use crate::owned_runner::{ReserveStartError, ReservedChildStart, WakeHint};
use crate::{
    ChildStartError, CollaborationIngressSettlement, CollaborationWriter, CollaborationWriterError,
    DelegatedRuntimeBinding, DispatchReport, LiveRuntime, OwnedChildRunner, OwnedRunnerError,
    OwnedRunnerUpdate, RunnerIdentity, RuntimeError, ScheduledTurnRequest,
};

/// Hard process-local ceiling independent of the collaboration log's retained delegation limit.
pub const MAX_OWNED_RUNNERS: usize = 64;

/// Immutable concurrent-runner limit for one scheduling owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SchedulerLimits {
    runners: NonZeroUsize,
}

impl SchedulerLimits {
    /// Accepts a nonzero concurrent-runner ceiling no larger than the hard process bound.
    #[must_use]
    pub fn new(runners: usize) -> Option<Self> {
        NonZeroUsize::new(runners)
            .filter(|runners| runners.get() <= MAX_OWNED_RUNNERS)
            .map(|runners| Self { runners })
    }

    /// Maximum simultaneously owned runner tasks.
    #[must_use]
    pub const fn runners(self) -> usize {
        self.runners.get()
    }
}

impl Default for SchedulerLimits {
    fn default() -> Self {
        Self {
            runners: NonZeroUsize::new(8).expect("default runner limit is nonzero"),
        }
    }
}

/// Why a resumed root could not be given back the admissions its journal already references.
#[derive(Debug, Error)]
pub enum OwnedRootContextError {
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    #[error(transparent)]
    Writer(#[from] CollaborationWriterError),
}

/// Scheduling failure after the owner retained an exact retryable request.
#[derive(Debug, Error)]
pub enum OwnedSchedulingError {
    #[error("scheduling owner is shutting down")]
    ShuttingDown,
    #[error("the addressed child has no owned runner")]
    UnknownRunner,
    #[error("the request does not match the runner's exact endpoint or delegation")]
    RunnerMismatch,
    #[error("Handoff has closed normal admission for this delegation")]
    HandoffPending,
    #[error("the child runner normal lane is busy")]
    RunnerBusy,
    #[error("the child runner is closed")]
    RunnerClosed,
    #[error("the child runner stopped unexpectedly")]
    RunnerFailed,
    #[error("the selected provider has no typed collaboration representation")]
    ProviderUnsupported,
    #[error("another accepted schedule request must settle first")]
    ScheduleInProgress,
    #[error("another accepted collaboration admission must settle first")]
    AdmissionInProgress,
    #[error("another accepted Stop must settle first")]
    StopInProgress,
    #[error("the child runtime refused accepted work: {0}")]
    Runtime(#[source] RuntimeError),
    #[error("collaboration writer refused the operation: {0}")]
    Writer(#[source] CollaborationWriterError),
    #[error("the mutation must be a HandoffCompleted event")]
    HandoffRequired,
    #[error("runner updates must reach Finished before join")]
    UpdatesPending,
    #[error("shutdown has not been admitted")]
    ShutdownNotStarted,
    #[error("scheduling owner shutdown has already reached its terminal result")]
    ShutdownFinished,
    #[error("runner control failed: {0}")]
    Control(#[source] OwnedRunnerError),
}

/// Exact schedule request retained across any owner-level refusal.
#[derive(Debug)]
pub struct OwnedScheduleFailure {
    source: Box<OwnedSchedulingError>,
    request: Box<ScheduledTurnRequest>,
}

impl OwnedScheduleFailure {
    #[must_use]
    pub fn source(&self) -> &OwnedSchedulingError {
        self.source.as_ref()
    }

    #[must_use]
    pub fn into_request(self) -> ScheduledTurnRequest {
        *self.request
    }
}

impl fmt::Display for OwnedScheduleFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.source.fmt(formatter)
    }
}

impl std::error::Error for OwnedScheduleFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

/// Exact Handoff mutation retained if stop, quiescence or append fails.
#[derive(Debug)]
pub struct OwnedHandoffFailure {
    source: Box<OwnedSchedulingError>,
    attempt: Box<CollaborationAttempt>,
    scheduled: Option<Box<DispatchReport>>,
    stopped: Option<Box<DispatchReport>>,
}

impl OwnedHandoffFailure {
    #[must_use]
    pub fn source(&self) -> &OwnedSchedulingError {
        self.source.as_ref()
    }

    #[must_use]
    pub fn into_attempt(self) -> CollaborationAttempt {
        *self.attempt
    }

    /// Report produced while settling an already accepted schedule before Handoff failed.
    #[must_use]
    pub fn scheduled_report(&self) -> Option<&DispatchReport> {
        self.scheduled.as_deref()
    }

    /// Stop report retained when the later durable Handoff append failed.
    #[must_use]
    pub fn stop_report(&self) -> Option<&DispatchReport> {
        self.stopped.as_deref()
    }
}

impl fmt::Display for OwnedHandoffFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.source.fmt(formatter)
    }
}

impl std::error::Error for OwnedHandoffFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

struct RunnerSlot {
    delegation: DelegationId,
    runner: OwnedChildRunner,
    finished: bool,
    terminal_update: Option<OwnedRunnerUpdate>,
    shutdown_requested: bool,
    joined: bool,
    shutdown_report: Option<DispatchReport>,
    shutdown_error: Option<OwnedRunnerError>,
    input_unavailable: bool,
}

struct PendingOwnedSchedule {
    request: ScheduledTurnRequest,
    conversation: ConversationId,
    reserved: Option<ReservedChildStart>,
}

struct PendingHandoff {
    attempt: CollaborationAttempt,
    preflighted: bool,
    already_durable: bool,
    scheduled: Option<DispatchReport>,
    stopped: Option<DispatchReport>,
    schedule_settled: bool,
    stop_settled: bool,
}

struct PendingStop {
    conversation: ConversationId,
    scheduled: Option<DispatchReport>,
    user_input: Option<Result<DispatchReport, UserInputFailure>>,
    user_input_identity: Option<RunnerIdentity>,
    stop_started: bool,
}

/// Reports and receipt produced by one quiescent durable Handoff.
#[derive(Debug)]
pub struct OwnedHandoffReport {
    pub receipt: ItemReceipt,
    pub scheduled: Option<DispatchReport>,
    pub stopped: Option<DispatchReport>,
}

/// Retained cold-Handoff result that has no live runner incarnation to name.
#[derive(Debug)]
pub struct OwnedHandoffSettlement {
    delegation: DelegationId,
    outcome: Box<Result<OwnedHandoffReport, OwnedHandoffFailure>>,
}

impl OwnedHandoffSettlement {
    /// Canonical delegation whose direct Handoff wait was abandoned.
    #[must_use]
    pub const fn delegation(&self) -> &DelegationId {
        &self.delegation
    }

    /// Exact terminal result retained by the owner.
    pub fn outcome(&self) -> &Result<OwnedHandoffReport, OwnedHandoffFailure> {
        self.outcome.as_ref()
    }
}

/// Reports produced by settling accepted scheduling and then stopping one child.
#[derive(Debug)]
pub struct OwnedStopReport {
    pub scheduled: Option<DispatchReport>,
    pub user_input: Option<Box<Result<DispatchReport, UserInputFailure>>>,
    pub stopped: DispatchReport,
}

/// Operation result retained when shutdown replaces a cancelled caller wait.
#[derive(Debug)]
pub enum OwnedShutdownSettlement {
    Ingress(CollaborationIngressSettlement),
    Admission(Result<ItemReceipt, CollaborationWriterError>),
    Schedule(Result<DispatchReport, OwnedScheduleFailure>),
    UserInput(Result<DispatchReport, UserInputFailure>),
    UserTargetInput(Result<DispatchReport, UserTargetInputFailure>),
    Stop(Result<OwnedStopReport, OwnedSchedulingError>),
    Handoff(Result<OwnedHandoffReport, OwnedHandoffFailure>),
}

/// Joined runner reports plus every operation result settled during shutdown.
#[derive(Debug)]
pub struct OwnedShutdownReport {
    runners: Vec<(RunnerIdentity, DispatchReport)>,
    settlements: Vec<OwnedShutdownSettlement>,
}

/// Joined shutdown failure returned together with every result settled before it.
#[derive(Debug)]
pub struct OwnedShutdownFailure {
    source: Box<OwnedSchedulingError>,
    report: OwnedShutdownReport,
}

impl OwnedShutdownFailure {
    #[must_use]
    pub fn source(&self) -> &OwnedSchedulingError {
        self.source.as_ref()
    }

    #[must_use]
    pub const fn report(&self) -> &OwnedShutdownReport {
        &self.report
    }
}

impl fmt::Display for OwnedShutdownFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.source.fmt(formatter)
    }
}

impl std::error::Error for OwnedShutdownFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

impl OwnedShutdownReport {
    #[must_use]
    pub fn runners(&self) -> &[(RunnerIdentity, DispatchReport)] {
        &self.runners
    }

    #[must_use]
    pub fn settlements(&self) -> &[OwnedShutdownSettlement] {
        &self.settlements
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.runners.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.runners.is_empty()
    }
}

/// One owner for collaboration admission, child generations and joined Handoff (SCH-2–SCH-4).
pub struct OwnedCollaboration {
    pub(crate) writer: CollaborationWriter,
    limits: SchedulerLimits,
    runners: BTreeMap<ConversationId, RunnerSlot>,
    pub(crate) handoff_closed: BTreeSet<DelegationId>,
    pending_schedule: Option<PendingOwnedSchedule>,
    pending_user_input: Option<PendingUserInput>,
    pub(crate) pending_user_target_input: Option<PendingUserTargetInput>,
    pub(crate) pending_user_target_settlement: Option<OwnedUserInputSettlement>,
    detached_user_input: Option<(RunnerIdentity, Result<DispatchReport, UserInputFailure>)>,
    pending_stop: Option<PendingStop>,
    pending_handoff: Option<PendingHandoff>,
    pub(crate) control_snapshots:
        BTreeMap<ConversationId, crate::collaboration_ingress::OwnedChildControlSnapshot>,
    pub(crate) ingress: Option<CollaborationIngressOwner>,
    pub(crate) pending_ingress: Option<PendingIngress>,
    pub(crate) child_factory: Option<crate::DelegatedChildFactory>,
    wakes: BTreeMap<ConversationId, PendingWake>,
    pub(crate) shutdown_settlements: Vec<OwnedShutdownSettlement>,
    shutdown_error: Option<Box<OwnedSchedulingError>>,
    shutting_down: bool,
    shutdown_finished: bool,
    #[cfg(test)]
    fail_next_registration: bool,
}

impl OwnedCollaboration {
    #[must_use]
    pub fn new(writer: CollaborationWriter, limits: SchedulerLimits) -> Self {
        Self {
            writer,
            limits,
            runners: BTreeMap::new(),
            handoff_closed: BTreeSet::new(),
            pending_schedule: None,
            pending_user_input: None,
            pending_user_target_input: None,
            pending_user_target_settlement: None,
            detached_user_input: None,
            pending_stop: None,
            pending_handoff: None,
            control_snapshots: BTreeMap::new(),
            ingress: None,
            pending_ingress: None,
            child_factory: None,
            wakes: BTreeMap::new(),
            shutdown_settlements: Vec::new(),
            shutdown_error: None,
            shutting_down: false,
            shutdown_finished: false,
            #[cfg(test)]
            fail_next_registration: false,
        }
    }

    pub(crate) fn has_update_source(&self) -> bool {
        self.pending_schedule.is_some()
            || self.pending_stop.is_some()
            || self.pending_user_input.is_some()
            || self.pending_user_target_input.is_some()
            || self.pending_user_target_settlement.is_some()
            || self.detached_user_input.is_some()
            || self.pending_handoff.is_some()
            || !self.wakes.is_empty()
            || self.runners.values().any(|slot| !slot.joined)
    }

    #[cfg(test)]
    pub(crate) fn hold_writer_for_test(
        &self,
    ) -> (std::sync::mpsc::Receiver<()>, std::sync::mpsc::Sender<()>) {
        self.writer.hold_for_test()
    }

    #[cfg(test)]
    pub(crate) fn panic_runner_for_test(&self, conversation: &ConversationId) {
        self.runners
            .get(conversation)
            .expect("test runner exists")
            .runner
            .panic_for_test();
    }

    /// Returns the unforgeable child binding owned by this collaboration writer.
    pub async fn delegated_binding(
        &self,
        delegation: DelegationId,
    ) -> Result<DelegatedRuntimeBinding, CollaborationWriterError> {
        self.writer
            .delegated_control(delegation)
            .await
            .map(DelegatedRuntimeBinding::new)
    }

    /// Reads the current canonical delegation state through this owner's sole writer.
    pub async fn delegation_view(
        &self,
        delegation: DelegationId,
    ) -> Result<plexmaton_agent::collaboration::DelegationView, CollaborationWriterError> {
        self.writer.delegation_view(delegation).await
    }

    /// Admits a regular collaboration fact; turns and Handoff use their owned paths.
    pub async fn admit(
        &mut self,
        attempt: CollaborationAttempt,
    ) -> Result<ItemReceipt, CollaborationWriterError> {
        if self.pending_handoff.is_some() {
            return Err(CollaborationWriterError::AdmissionInProgress {
                attempt: Box::new(attempt),
            });
        }
        self.writer.admit(attempt).await
    }

    /// Finishes an accepted regular admission after its original caller wait was cancelled.
    pub async fn finish_admission(
        &mut self,
    ) -> Option<Result<ItemReceipt, CollaborationWriterError>> {
        if self.writer.has_pending_admission() {
            Some(self.writer.finish_pending_admission().await)
        } else {
            None
        }
    }

    /// Admits one turn for the user-owned root and freezes the sources it will read.
    ///
    /// The owned runner path cannot carry this: it reserves execution against the delegated control
    /// Main holds over a child, and nobody holds one over the root. The caller owns the root
    /// runtime and starts the turn itself.
    pub async fn admit_root_turn(
        &mut self,
        item: plexmaton_core::CollaborationItemId,
        boundary: plexmaton_agent::collaboration::TurnBoundary,
        previous: Option<plexmaton_agent::collaboration::CollaborationItemRef>,
    ) -> Result<
        std::sync::Arc<plexmaton_agent::collaboration::ResolvedTurnAdmission>,
        CollaborationWriterError,
    > {
        self.writer.admit_root_turn(item, boundary, previous).await
    }

    /// Every acknowledged record this root's log holds, in append order.
    pub async fn records(&self) -> Result<Vec<CollaborationRecord>, CollaborationWriterError> {
        self.writer.records().await
    }

    /// Resolves selected-session admission references through the canonical file owner.
    pub async fn resolve_session_references(
        &self,
        references: Vec<plexmaton_agent::collaboration::CollaborationItemRef>,
    ) -> Result<
        Vec<std::sync::Arc<plexmaton_agent::collaboration::ResolvedTurnAdmission>>,
        CollaborationWriterError,
    > {
        self.writer.resolve_context(references).await
    }

    /// Reads a complete bounded mail snapshot from the live canonical writer (CMP-1).
    pub async fn mail_snapshot(
        &self,
        endpoint: MailEndpoint,
    ) -> Result<CollaborationMailProjection, CollaborationWriterError> {
        self.writer.project_mail(endpoint).await
    }

    /// Rebuilds the root's own resolved collaboration context after a restart (CIN-3).
    ///
    /// A turn the root took over its inbox leaves a collaboration atom in its journal. On resume
    /// the projection rebuilds that atom as a reference, and resolving it needs an admission this
    /// process never admitted — so without this, every later turn in a conversation that once
    /// received mail is refused as unresolved. A child gets the same treatment when its runner
    /// registers; the root has no runner to hang it on.
    pub async fn restore_root_context(
        &self,
        runtime: &mut LiveRuntime,
    ) -> Result<(), OwnedRootContextError> {
        let references = runtime.collaboration_references()?;
        if references.is_empty() {
            return Ok(());
        }
        let resolved = self.writer.resolve_context(references).await?;
        runtime.restore_resolved_collaboration_context(resolved)?;
        Ok(())
    }
}

mod attention;
mod handoff;
mod inspection;
mod lifecycle;
mod registration;
mod scheduling;
mod settlement;
mod shutdown;
mod user_input;
mod user_target_input;
mod wake;

pub use registration::{RunnerRegistrationError, RunnerRegistrationReason};
use user_input::PendingUserInput;
pub use user_input::{UserInputFailure, UserInputRefusal, UserInputRequest};
use user_target_input::PendingUserTargetInput;
pub use user_target_input::{
    OwnedUserInputSettlement, UserTargetInputFailure, UserTargetInputRequest,
};
use wake::PendingWake;
pub use wake::{WakeAdmission, WakeFailure, WakeRefusal};

fn handoff_failure(
    source: OwnedSchedulingError,
    attempt: CollaborationAttempt,
    scheduled: Option<DispatchReport>,
    stopped: Option<DispatchReport>,
) -> OwnedHandoffFailure {
    OwnedHandoffFailure {
        source: Box::new(source),
        attempt: Box::new(attempt),
        scheduled: scheduled.map(Box::new),
        stopped: stopped.map(Box::new),
    }
}
