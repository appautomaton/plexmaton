//! Bounded task ownership for one delegated live runtime.

use plexmaton_agent::Input;
use plexmaton_agent::collaboration::MailEndpoint;
use std::fmt;
#[cfg(test)]
use std::sync::Arc;
use thiserror::Error;
#[cfg(test)]
use tokio::sync::Notify;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::{DispatchReport, LiveRuntime, PreparedChildExecution, RuntimeError};

const NORMAL_CAPACITY: usize = 1;
const CONTROL_CAPACITY: usize = 1;
const SESSION_CAPACITY: usize = 1;
const UPDATE_CAPACITY: usize = 1;

/// Failure to place or execute one normal-lane child turn.
pub enum ChildStartError {
    /// The bounded normal lane was full; caller ownership is unchanged.
    #[cfg(test)]
    Busy(Box<PreparedChildExecution>),
    /// The runner was closed before accepting the command; caller ownership is unchanged.
    #[cfg(test)]
    Closed(Box<PreparedChildExecution>),
    /// An accepted command reached a runtime that could not start it.
    Runtime(RuntimeError),
    /// The owner stopped without acknowledging an accepted command.
    WorkerFailed,
}

impl ChildStartError {
    /// Recovers authority only when the normal command was never accepted.
    #[cfg(test)]
    #[must_use]
    pub fn into_unaccepted(self) -> Option<PreparedChildExecution> {
        match self {
            #[cfg(test)]
            Self::Busy(execution) | Self::Closed(execution) => Some(*execution),
            Self::Runtime(_) | Self::WorkerFailed => None,
        }
    }
}

impl fmt::Debug for ChildStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(test)]
            Self::Busy(_) => formatter.write_str("ChildStartError::Busy(..)"),
            #[cfg(test)]
            Self::Closed(_) => formatter.write_str("ChildStartError::Closed(..)"),
            Self::Runtime(error) => formatter.debug_tuple("Runtime").field(error).finish(),
            Self::WorkerFailed => formatter.write_str("ChildStartError::WorkerFailed"),
        }
    }
}

impl fmt::Display for ChildStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(test)]
            Self::Busy(_) => formatter.write_str("child runner normal lane is full"),
            #[cfg(test)]
            Self::Closed(_) => formatter.write_str("child runner is closed"),
            Self::Runtime(error) => write!(formatter, "child runtime refused the turn: {error}"),
            Self::WorkerFailed => formatter.write_str("child runner stopped unexpectedly"),
        }
    }
}

impl std::error::Error for ChildStartError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Runtime(error) => Some(error),
            _ => None,
        }
    }
}

/// Inspection, control or task-ownership failure.
#[derive(Debug, Error)]
pub enum OwnedRunnerError {
    /// The child does not have durable delegated control and remains caller-owned.
    #[error("runtime is not a bound delegated child")]
    NotDelegated,
    /// The child control owner cannot currently prove its state.
    #[error("delegated child control is unavailable: {0}")]
    ControlUnavailable(#[source] RuntimeError),
    /// The owner no longer accepts control commands.
    #[error("child runner is closed")]
    Closed,
    /// Another accepted control command currently occupies the reserved lane.
    #[error("child runner control lane is busy")]
    ControlBusy,
    /// The disposable inspection lane is currently occupied.
    #[error("child runner inspection lane is busy")]
    InspectionBusy,
    /// This runner cannot issue an authenticated collaboration session snapshot.
    #[error("child runner has no authenticated collaboration session source")]
    CollaborationUnavailable,
    /// The owner stopped without answering an accepted control command.
    #[error("child runner stopped unexpectedly")]
    WorkerFailed,
    /// The live runtime failed while executing a control command.
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
}

/// Spawn refusal that returns the live runtime without dropping its journal owner.
pub struct OwnedRunnerSpawnError {
    source: OwnedRunnerError,
    runtime: Box<LiveRuntime>,
}

impl OwnedRunnerSpawnError {
    /// Returns the runtime for explicit shutdown or another valid owner.
    #[must_use]
    pub fn into_runtime(self) -> LiveRuntime {
        *self.runtime
    }
}

impl fmt::Debug for OwnedRunnerSpawnError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OwnedRunnerSpawnError")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for OwnedRunnerSpawnError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.source.fmt(formatter)
    }
}

impl std::error::Error for OwnedRunnerSpawnError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

enum NormalCommand {
    Start {
        execution: PreparedChildExecution,
        reply: oneshot::Sender<Result<DispatchReport, RuntimeError>>,
    },
    WakeSnapshot {
        hint: WakeHint,
    },
}

/// One process-local slot accepted by the child owner before collaboration admission mutates.
pub(crate) struct ReservedChildStart(mpsc::OwnedPermit<NormalCommand>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReserveStartError {
    Busy,
    Closed,
}

enum ControlCommand {
    Stop {
        reply: oneshot::Sender<Result<DispatchReport, RuntimeError>>,
    },
    Shutdown {
        reply: oneshot::Sender<Result<DispatchReport, RuntimeError>>,
    },
    #[cfg(test)]
    Panic,
}

enum SessionCommand {
    SessionSource {
        reply: oneshot::Sender<Option<crate::CollaborationSessionSource>>,
    },
    LinkCollaborationItem {
        reference: plexmaton_agent::collaboration::CollaborationItemRef,
        reply: oneshot::Sender<Result<(), RuntimeError>>,
    },
}

/// Exclusive task owner with separately bounded normal, control and update lanes (SCH-1/SCH-2).
pub struct OwnedChildRunner {
    identity: RunnerIdentity,
    collaboration_supported: bool,
    normal: Option<mpsc::Sender<NormalCommand>>,
    control: Option<mpsc::Sender<ControlCommand>>,
    session: Option<mpsc::Sender<SessionCommand>>,
    updates: mpsc::Receiver<OwnedRunnerUpdate>,
    worker: Option<JoinHandle<()>>,
    start_reply: Option<oneshot::Receiver<Result<DispatchReport, RuntimeError>>>,
    stop_reply: Option<oneshot::Receiver<Result<DispatchReport, RuntimeError>>>,
    shutdown_reply: Option<oneshot::Receiver<Result<DispatchReport, RuntimeError>>>,
    shutdown_outcome: Option<Result<DispatchReport, RuntimeError>>,
    #[cfg(test)]
    join_gate: Option<(Arc<Notify>, Arc<Notify>)>,
}

impl OwnedChildRunner {
    /// Moves one bound delegated runtime into a joined child task.
    pub fn spawn(
        runtime: LiveRuntime,
        generation: RunnerGeneration,
    ) -> Result<Self, OwnedRunnerSpawnError> {
        match runtime.delegation_controller() {
            Ok(Some(_)) => {}
            Ok(None) => {
                return Err(OwnedRunnerSpawnError {
                    source: OwnedRunnerError::NotDelegated,
                    runtime: Box::new(runtime),
                });
            }
            Err(error) => {
                return Err(OwnedRunnerSpawnError {
                    source: OwnedRunnerError::ControlUnavailable(error),
                    runtime: Box::new(runtime),
                });
            }
        }
        let identity = RunnerIdentity {
            endpoint: MailEndpoint {
                agent: runtime.agent_id().clone(),
                conversation: runtime.conversation_id().clone(),
            },
            generation,
        };
        let collaboration_supported = runtime.supports_collaboration();
        let (normal, normal_rx) = mpsc::channel(NORMAL_CAPACITY);
        let (control, control_rx) = mpsc::channel(CONTROL_CAPACITY);
        let (session, session_rx) = mpsc::channel(SESSION_CAPACITY);
        let (updates_tx, updates) = mpsc::channel(UPDATE_CAPACITY);
        let worker_identity = identity.clone();
        let worker = tokio::spawn(actor::run_owned_child(
            runtime,
            worker_identity,
            normal_rx,
            control_rx,
            session_rx,
            updates_tx,
        ));
        Ok(Self {
            identity,
            collaboration_supported,
            normal: Some(normal),
            control: Some(control),
            session: Some(session),
            updates,
            worker: Some(worker),
            start_reply: None,
            stop_reply: None,
            shutdown_reply: None,
            shutdown_outcome: None,
            #[cfg(test)]
            join_gate: None,
        })
    }

    /// Endpoint and generation attached to every later update.
    #[must_use]
    pub const fn identity(&self) -> &RunnerIdentity {
        &self.identity
    }

    /// Provider capability captured before the runtime enters task ownership.
    pub(crate) const fn supports_collaboration(&self) -> bool {
        self.collaboration_supported
    }

    /// Queues one content-free wake snapshot in the bounded normal lane.
    pub(crate) fn queue_wake(&self, hint: WakeHint) -> Result<(), ReserveStartError> {
        let sender = self.normal.as_ref().ok_or(ReserveStartError::Closed)?;
        sender
            .try_send(NormalCommand::WakeSnapshot { hint })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => ReserveStartError::Busy,
                mpsc::error::TrySendError::Closed(_) => ReserveStartError::Closed,
            })
    }

    /// Immediately refuses normal-lane backpressure without consuming execution authority.
    #[cfg(test)]
    pub async fn start(
        &mut self,
        execution: PreparedChildExecution,
    ) -> Result<DispatchReport, ChildStartError> {
        self.begin_start(execution)?;
        self.finish_start().await
    }

    /// Admits one normal command and retains its result receiver across caller cancellation.
    #[cfg(test)]
    pub fn begin_start(
        &mut self,
        execution: PreparedChildExecution,
    ) -> Result<(), ChildStartError> {
        let reservation = match self.reserve_start() {
            Ok(reservation) => reservation,
            Err(ReserveStartError::Busy) => {
                return Err(ChildStartError::Busy(Box::new(execution)));
            }
            Err(ReserveStartError::Closed) => {
                return Err(ChildStartError::Closed(Box::new(execution)));
            }
        };
        self.begin_reserved_start(reservation, execution);
        Ok(())
    }

    /// Reserves bounded normal capacity without admitting a durable collaboration turn.
    pub(crate) fn reserve_start(&self) -> Result<ReservedChildStart, ReserveStartError> {
        if self.start_reply.is_some() {
            return Err(ReserveStartError::Busy);
        }
        let sender = self.normal.clone().ok_or(ReserveStartError::Closed)?;
        sender
            .try_reserve_owned()
            .map(ReservedChildStart)
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => ReserveStartError::Busy,
                mpsc::error::TrySendError::Closed(_) => ReserveStartError::Closed,
            })
    }

    /// Transfers a previously reserved normal slot and execution to this child owner.
    pub(crate) fn begin_reserved_start(
        &mut self,
        reservation: ReservedChildStart,
        execution: PreparedChildExecution,
    ) {
        let (reply, result) = oneshot::channel();
        let command = NormalCommand::Start { execution, reply };
        reservation.0.send(command);
        self.start_reply = Some(result);
    }

    /// Finishes the accepted normal command without losing its report to cancellation.
    pub async fn finish_start(&mut self) -> Result<DispatchReport, ChildStartError> {
        let result = self
            .start_reply
            .as_mut()
            .ok_or(ChildStartError::WorkerFailed)?;
        let outcome = match (&mut *result).await {
            Ok(Ok(report)) => Ok(report),
            Ok(Err(error)) => Err(ChildStartError::Runtime(error)),
            Err(_) => Err(ChildStartError::WorkerFailed),
        };
        self.start_reply.take();
        outcome
    }

    /// Uses reserved control capacity and acknowledges only after interruption has joined work.
    pub async fn stop(&mut self) -> Result<DispatchReport, OwnedRunnerError> {
        self.begin_stop()?;
        self.finish_stop().await
    }

    /// Admits one Stop into reserved control capacity and retains its reply.
    pub fn begin_stop(&mut self) -> Result<(), OwnedRunnerError> {
        if self.stop_reply.is_some() {
            return Ok(());
        }
        let (reply, result) = oneshot::channel();
        let sender = self.control.as_ref().ok_or(OwnedRunnerError::Closed)?;
        match sender.try_send(ControlCommand::Stop { reply }) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                return Err(OwnedRunnerError::ControlBusy);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return Err(OwnedRunnerError::Closed);
            }
        }
        self.stop_reply = Some(result);
        Ok(())
    }

    /// Finishes an accepted Stop; cancellation leaves its result available for retry.
    pub async fn finish_stop(&mut self) -> Result<DispatchReport, OwnedRunnerError> {
        self.begin_stop()?;
        let result = self
            .stop_reply
            .as_mut()
            .ok_or(OwnedRunnerError::WorkerFailed)?;
        let outcome = match (&mut *result).await {
            Ok(result) => result.map_err(OwnedRunnerError::from),
            Err(_) => Err(OwnedRunnerError::WorkerFailed),
        };
        self.stop_reply.take();
        outcome
    }

    /// Receives one update already tagged with its producing runner incarnation.
    pub async fn next_update(&mut self) -> Option<OwnedRunnerUpdate> {
        self.updates.recv().await
    }

    /// Reads one disposable session snapshot without occupying control capacity.
    pub(crate) async fn session_source(
        &self,
    ) -> Result<crate::CollaborationSessionSource, OwnedRunnerError> {
        let (reply, result) = oneshot::channel();
        let sender = self.session.as_ref().ok_or(OwnedRunnerError::Closed)?;
        match sender.try_send(SessionCommand::SessionSource { reply }) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                return Err(OwnedRunnerError::InspectionBusy);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return Err(OwnedRunnerError::Closed);
            }
        }
        result
            .await
            .map_err(|_| OwnedRunnerError::WorkerFailed)?
            .ok_or(OwnedRunnerError::CollaborationUnavailable)
    }

    /// Persists one canonical collaboration placement through the child runtime's journal owner.
    pub(crate) async fn link_collaboration_item(
        &self,
        reference: plexmaton_agent::collaboration::CollaborationItemRef,
    ) -> Result<(), OwnedRunnerError> {
        let (reply, result) = oneshot::channel();
        self.session
            .as_ref()
            .ok_or(OwnedRunnerError::Closed)?
            .send(SessionCommand::LinkCollaborationItem { reference, reply })
            .await
            .map_err(|_| OwnedRunnerError::Closed)?;
        result
            .await
            .map_err(|_| OwnedRunnerError::WorkerFailed)?
            .map_err(OwnedRunnerError::from)
    }

    #[cfg(test)]
    pub(crate) fn panic_for_test(&self) {
        self.control
            .as_ref()
            .expect("test runner control is open")
            .try_send(ControlCommand::Panic)
            .expect("test panic enters reserved control capacity");
    }

    #[cfg(test)]
    pub(crate) fn hold_join_for_test(&mut self) -> (Arc<Notify>, Arc<Notify>) {
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        self.join_gate = Some((Arc::clone(&entered), Arc::clone(&release)));
        (entered, release)
    }

    /// Atomically admits semantic shutdown and retains its acknowledgement across cancellation.
    pub fn begin_shutdown(&mut self) -> Result<(), OwnedRunnerError> {
        if self.shutdown_reply.is_some() || self.shutdown_outcome.is_some() {
            return Ok(());
        }
        let (reply, result) = oneshot::channel();
        let sender = self.control.as_ref().ok_or(OwnedRunnerError::Closed)?;
        match sender.try_send(ControlCommand::Shutdown { reply }) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                return Err(OwnedRunnerError::ControlBusy);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return Err(OwnedRunnerError::Closed);
            }
        }
        self.normal.take();
        self.control.take();
        self.session.take();
        self.shutdown_reply = Some(result);
        Ok(())
    }

    /// Joins an admitted shutdown after its tagged update stream has reached Finished.
    pub async fn finish_shutdown(&mut self) -> Result<DispatchReport, OwnedRunnerError> {
        self.begin_shutdown()?;
        if self.shutdown_outcome.is_none() {
            let reply = self
                .shutdown_reply
                .as_mut()
                .ok_or(OwnedRunnerError::WorkerFailed)?;
            let outcome = match (&mut *reply).await {
                Ok(outcome) => outcome,
                Err(_) => {
                    self.shutdown_reply.take();
                    let _joined = self.join().await;
                    return Err(OwnedRunnerError::WorkerFailed);
                }
            };
            self.shutdown_reply.take();
            self.shutdown_outcome = Some(outcome);
        }
        self.join().await?;
        self.shutdown_outcome
            .take()
            .ok_or(OwnedRunnerError::WorkerFailed)?
            .map_err(OwnedRunnerError::from)
    }

    /// Joins a runner that reached Finished through shutdown, channel closure or runtime failure.
    pub async fn join(&mut self) -> Result<(), OwnedRunnerError> {
        #[cfg(test)]
        if let Some((entered, release)) = self.join_gate.clone() {
            entered.notify_one();
            release.notified().await;
            self.join_gate.take();
        }
        if let Some(worker) = self.worker.as_mut()
            && (&mut *worker).await.is_err()
        {
            self.worker.take();
            return Err(OwnedRunnerError::WorkerFailed);
        }
        self.worker.take();
        Ok(())
    }
}

impl Drop for OwnedChildRunner {
    fn drop(&mut self) {
        self.normal.take();
        self.control.take();
        self.session.take();
        if let Some(worker) = self.worker.take() {
            worker.abort();
        }
    }
}

mod actor;
mod identity;
mod update;

pub(crate) use identity::next_runner_generation;
pub use identity::{RunnerGeneration, RunnerIdentity, WakeHint};
pub use update::OwnedRunnerUpdate;
