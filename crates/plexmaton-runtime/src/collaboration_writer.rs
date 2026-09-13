//! Bounded asynchronous ownership of one blocking collaboration file.

use std::sync::Arc;
use std::thread::{self, JoinHandle};

use plexmaton_agent::collaboration::{
    CollaborationItemRef, CollaborationMailProjection, DelegationView, ItemReceipt, MailEndpoint,
    Preparation, ResolvedTurnAdmission, TurnBoundary,
};
use plexmaton_core::{CollaborationId, CollaborationItemId, DelegationId};
use plexmaton_session_store::collaboration::{
    CollaborationAttempt, CollaborationFile, CollaborationStoreError, DelegatedConversationControl,
    ExecutionReservation, ExecutionTicket,
};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

const COMMAND_CAPACITY: usize = 1;

/// Exact normal-lane request retained if scheduling cannot reach its collaboration owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScheduledTurnRequest {
    delegation: DelegationId,
    item: CollaborationItemId,
    boundary: TurnBoundary,
    previous: Option<CollaborationItemRef>,
}

pub(crate) struct SessionMailSourceSnapshot {
    pub(crate) mail: CollaborationMailProjection,
    pub(crate) turns: Vec<Arc<ResolvedTurnAdmission>>,
}

impl ScheduledTurnRequest {
    /// Captures one proposed canonical turn admission before authority is reserved.
    #[must_use]
    pub fn new(
        delegation: DelegationId,
        item: CollaborationItemId,
        boundary: TurnBoundary,
        previous: Option<CollaborationItemRef>,
    ) -> Self {
        Self {
            delegation,
            item,
            boundary,
            previous,
        }
    }

    /// Delegation whose one execution slot this request must reserve.
    #[must_use]
    pub const fn delegation(&self) -> &DelegationId {
        &self.delegation
    }

    /// Exact child session boundary proposed before durable turn admission.
    #[must_use]
    pub const fn boundary(&self) -> &TurnBoundary {
        &self.boundary
    }
}

/// One admitted child turn carrying all non-cloneable authority needed by its runner.
pub(crate) struct PreparedChildExecution {
    resolved: Arc<ResolvedTurnAdmission>,
    reservation: ExecutionReservation,
    ticket: ExecutionTicket,
}

impl PreparedChildExecution {
    /// Frozen canonical context admitted for this exact child turn.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn resolved(&self) -> &Arc<ResolvedTurnAdmission> {
        &self.resolved
    }

    /// Transfers the admission and its one-shot authority to the owned child runner.
    #[must_use]
    pub(crate) fn into_parts(
        self,
    ) -> (
        ExecutionReservation,
        ExecutionTicket,
        Arc<ResolvedTurnAdmission>,
    ) {
        (self.reservation, self.ticket, self.resolved)
    }
}

/// Failure of the asynchronous collaboration owner without loss of caller-owned input.
#[derive(Debug, Error)]
pub enum CollaborationWriterError {
    /// The worker no longer accepts commands that carry no durable mutation.
    #[error("collaboration writer is closed")]
    Closed,
    /// The single command slot is occupied; a query was not accepted.
    #[error("collaboration writer command lane is busy")]
    Busy,
    /// The worker stopped without answering an accepted command.
    #[error("collaboration writer stopped unexpectedly")]
    WorkerFailed,
    /// A mutation was not accepted by the channel; the exact attempt remains available.
    #[error("collaboration writer closed before accepting an admission")]
    AdmissionClosed { attempt: Box<CollaborationAttempt> },
    /// The command slot was full; the exact mutation remains caller-owned.
    #[error("collaboration writer is busy; admission was not accepted")]
    AdmissionBusy { attempt: Box<CollaborationAttempt> },
    /// One accepted admission must settle before another mutation can enter.
    #[error("a collaboration admission is already in progress")]
    AdmissionInProgress { attempt: Box<CollaborationAttempt> },
    /// Logical turn admission must reserve execution through [`crate::OwnedCollaboration::schedule`].
    #[error("turn admission requires the scheduling boundary")]
    TurnRequiresScheduling { attempt: Box<CollaborationAttempt> },
    /// Handoff must close and join its child through [`crate::OwnedCollaboration`].
    #[error("Handoff requires the owned collaboration boundary")]
    HandoffRequiresOwnership { attempt: Box<CollaborationAttempt> },
    /// The worker panicked after accepting a mutation; canonical reopen must reconcile it.
    #[error("collaboration writer failed while admitting an accepted mutation")]
    AdmissionWorkerFailed { attempt: Box<CollaborationAttempt> },
    /// The file rejected or could not acknowledge the exact mutation.
    #[error("collaboration admission failed: {source}")]
    Admission {
        source: CollaborationStoreError,
        attempt: Box<CollaborationAttempt>,
    },
    /// Scheduling failed before authority could reach a runner.
    #[error("collaboration scheduling failed: {source}")]
    Schedule {
        source: CollaborationStoreError,
        request: Box<ScheduledTurnRequest>,
    },
    /// The scheduling command was not accepted; its exact request remains available.
    #[error("collaboration writer closed before accepting a schedule request")]
    ScheduleClosed { request: Box<ScheduledTurnRequest> },
    /// The command slot was full; the exact schedule request remains caller-owned.
    #[error("collaboration writer is busy; schedule request was not accepted")]
    ScheduleBusy { request: Box<ScheduledTurnRequest> },
    /// The worker panicked after accepting a schedule request.
    #[error("collaboration writer failed while preparing an accepted schedule request")]
    ScheduleWorkerFailed { request: Box<ScheduledTurnRequest> },
    /// One accepted scheduling command still owns its cancellation-safe reply.
    #[error("a collaboration schedule request is already in progress")]
    ScheduleInProgress { request: Box<ScheduledTurnRequest> },
    /// A proposed Handoff is not a canonical mutation for the current ledger state.
    #[error("collaboration Handoff preflight failed: {source}")]
    HandoffPreflight {
        source: CollaborationStoreError,
        attempt: Box<CollaborationAttempt>,
    },
    /// A read-only authority query failed inside the current file owner.
    #[error(transparent)]
    Store(#[from] CollaborationStoreError),
}

impl CollaborationWriterError {
    /// Exact mutation that remains caller-owned or requires canonical reconciliation.
    #[must_use]
    pub fn attempt(&self) -> Option<&CollaborationAttempt> {
        match self {
            Self::AdmissionClosed { attempt }
            | Self::AdmissionBusy { attempt }
            | Self::AdmissionInProgress { attempt }
            | Self::TurnRequiresScheduling { attempt }
            | Self::HandoffRequiresOwnership { attempt }
            | Self::AdmissionWorkerFailed { attempt }
            | Self::Admission { attempt, .. }
            | Self::HandoffPreflight { attempt, .. } => Some(attempt),
            _ => None,
        }
    }

    /// Exact scheduling input that never reached a runner.
    #[must_use]
    pub fn schedule_request(&self) -> Option<&ScheduledTurnRequest> {
        match self {
            Self::Schedule { request, .. }
            | Self::ScheduleClosed { request }
            | Self::ScheduleBusy { request }
            | Self::ScheduleWorkerFailed { request }
            | Self::ScheduleInProgress { request } => Some(request),
            _ => None,
        }
    }
}

enum Command {
    Admit {
        attempt: CollaborationAttempt,
        reply: oneshot::Sender<Result<ItemReceipt, CollaborationWriterError>>,
    },
    DelegatedControl {
        delegation: DelegationId,
        reply: oneshot::Sender<Result<DelegatedConversationControl, CollaborationWriterError>>,
    },
    DelegationView {
        delegation: DelegationId,
        reply: oneshot::Sender<Result<DelegationView, CollaborationWriterError>>,
    },
    DelegatedControls {
        reply: oneshot::Sender<Result<Vec<DelegatedConversationControl>, CollaborationWriterError>>,
    },
    ProjectMail {
        endpoint: MailEndpoint,
        reply: oneshot::Sender<Result<CollaborationMailProjection, CollaborationWriterError>>,
    },
    ProjectSessionMail {
        endpoint: MailEndpoint,
        references: Vec<CollaborationItemRef>,
        reply: oneshot::Sender<Result<SessionMailSourceSnapshot, CollaborationWriterError>>,
    },
    ResolveContext {
        references: Vec<CollaborationItemRef>,
        reply: oneshot::Sender<Result<Vec<Arc<ResolvedTurnAdmission>>, CollaborationWriterError>>,
    },
    Schedule {
        request: ScheduledTurnRequest,
        reply: oneshot::Sender<Result<PreparedChildExecution, CollaborationWriterError>>,
    },
    RequireQuiescent {
        reply: oneshot::Sender<Result<(), CollaborationWriterError>>,
    },
    PreflightHandoff {
        attempt: CollaborationAttempt,
        reply: oneshot::Sender<Result<DelegationId, CollaborationWriterError>>,
    },
    #[cfg(test)]
    Hold {
        entered: std::sync::mpsc::Sender<()>,
        release: std::sync::mpsc::Receiver<()>,
    },
    #[cfg(test)]
    PanicAfterAdmission {
        attempt: CollaborationAttempt,
        reply: oneshot::Sender<Result<ItemReceipt, CollaborationWriterError>>,
    },
}

/// One joined worker that exclusively owns a blocking collaboration file (SCH-1).
pub struct CollaborationWriter {
    collaboration: CollaborationId,
    sender: Option<mpsc::Sender<Command>>,
    worker: Option<JoinHandle<()>>,
    finished: Option<oneshot::Receiver<bool>>,
    pending_admission: Option<PendingAdmission>,
    pending_schedule:
        Option<oneshot::Receiver<Result<PreparedChildExecution, CollaborationWriterError>>>,
    worker_failed: bool,
}

struct PendingAdmission {
    attempt: CollaborationAttempt,
    result: oneshot::Receiver<Result<ItemReceipt, CollaborationWriterError>>,
}

impl CollaborationWriter {
    /// Transfers one already-open file to its sole asynchronous owner.
    pub fn spawn(file: CollaborationFile) -> Result<Self, CollaborationWriterError> {
        let collaboration = file.ledger().id().clone();
        let (sender, mut receiver) = mpsc::channel(COMMAND_CAPACITY);
        let (finished_tx, finished) = oneshot::channel();
        let worker = thread::Builder::new()
            .name("plexmaton-collaboration".into())
            .stack_size(512 * 1024)
            .spawn(move || {
                let mut file = file;
                let mut failed = false;
                while let Some(command) = receiver.blocking_recv() {
                    if worker::process_command(&mut file, command) {
                        failed = true;
                        worker::reject_queued_commands(&mut receiver);
                        break;
                    }
                }
                let _owner_gone = finished_tx.send(failed).is_err();
            })
            .map_err(|_| CollaborationWriterError::WorkerFailed)?;
        Ok(Self {
            collaboration,
            sender: Some(sender),
            worker: Some(worker),
            finished: Some(finished),
            pending_admission: None,
            pending_schedule: None,
            worker_failed: false,
        })
    }

    /// Reconstructs the canonical reference acknowledged by this exact file owner.
    pub(crate) fn item_reference(&self, receipt: &ItemReceipt) -> CollaborationItemRef {
        CollaborationItemRef {
            collaboration: self.collaboration.clone(),
            item: receipt.id.clone(),
            sequence: receipt.sequence,
        }
    }

    #[cfg(test)]
    pub(crate) fn hold_for_test(
        &self,
    ) -> (std::sync::mpsc::Receiver<()>, std::sync::mpsc::Sender<()>) {
        let (entered, observed) = std::sync::mpsc::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        self.sender
            .as_ref()
            .expect("test writer is open")
            .try_send(Command::Hold {
                entered,
                release: blocked,
            })
            .expect("test writer accepts hold command");
        (observed, release)
    }

    /// Waits without occupying command capacity until a previously full lane can accept work.
    pub(crate) async fn wait_writable(&self) -> Result<(), CollaborationWriterError> {
        let sender = self
            .sender
            .as_ref()
            .ok_or(CollaborationWriterError::Closed)?;
        let permit = sender
            .reserve()
            .await
            .map_err(|_| CollaborationWriterError::Closed)?;
        drop(permit);
        Ok(())
    }

    /// Admits one turn and reserves its exact execution authority before returning it.
    #[cfg(test)]
    pub(crate) async fn schedule(
        &mut self,
        request: ScheduledTurnRequest,
    ) -> Result<PreparedChildExecution, CollaborationWriterError> {
        self.begin_schedule(request)?;
        self.finish_schedule().await
    }

    /// Admits one schedule command while retaining its reply across caller cancellation.
    pub(crate) fn begin_schedule(
        &mut self,
        request: ScheduledTurnRequest,
    ) -> Result<(), CollaborationWriterError> {
        if self.pending_schedule.is_some() {
            return Err(CollaborationWriterError::ScheduleInProgress {
                request: Box::new(request),
            });
        }
        let (reply, result) = oneshot::channel();
        let Some(sender) = &self.sender else {
            return Err(CollaborationWriterError::ScheduleClosed {
                request: Box::new(request),
            });
        };
        let command = Command::Schedule { request, reply };
        match sender.try_send(command) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(Command::Schedule { request, .. })) => {
                return Err(CollaborationWriterError::ScheduleBusy {
                    request: Box::new(request),
                });
            }
            Err(mpsc::error::TrySendError::Closed(Command::Schedule { request, .. })) => {
                return Err(CollaborationWriterError::ScheduleClosed {
                    request: Box::new(request),
                });
            }
            Err(_) => unreachable!("schedule try_send returns its schedule command"),
        }
        self.pending_schedule = Some(result);
        Ok(())
    }

    /// Finishes the one accepted schedule command; cancellation leaves the reply retained.
    pub(crate) async fn finish_schedule(
        &mut self,
    ) -> Result<PreparedChildExecution, CollaborationWriterError> {
        let result = self
            .pending_schedule
            .as_mut()
            .ok_or(CollaborationWriterError::Closed)?;
        let outcome = match (&mut *result).await {
            Ok(outcome) => outcome,
            Err(_) => {
                self.pending_schedule.take();
                self.worker_failed = true;
                return Err(CollaborationWriterError::WorkerFailed);
            }
        };
        self.pending_schedule.take();
        outcome
    }

    /// Validates a Handoff against the canonical ledger without requiring current quiescence.
    pub(crate) async fn preflight_handoff(
        &self,
        attempt: CollaborationAttempt,
    ) -> Result<DelegationId, CollaborationWriterError> {
        let (reply, result) = oneshot::channel();
        let Some(sender) = &self.sender else {
            return Err(CollaborationWriterError::AdmissionClosed {
                attempt: Box::new(attempt),
            });
        };
        let command = Command::PreflightHandoff { attempt, reply };
        match sender.try_send(command) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(Command::PreflightHandoff { attempt, .. })) => {
                return Err(CollaborationWriterError::AdmissionBusy {
                    attempt: Box::new(attempt),
                });
            }
            Err(mpsc::error::TrySendError::Closed(Command::PreflightHandoff {
                attempt, ..
            })) => {
                return Err(CollaborationWriterError::AdmissionClosed {
                    attempt: Box::new(attempt),
                });
            }
            Err(_) => unreachable!("preflight try_send returns its Handoff command"),
        }
        result
            .await
            .map_err(|_| CollaborationWriterError::WorkerFailed)?
    }
}

mod admission;
mod projection;
mod shutdown;
mod worker;

#[cfg(test)]
mod tests {
    use plexmaton_agent::HeadRevision;
    use plexmaton_agent::collaboration::{
        CollaborationEvent, CollaborationLimits, CollaborationText, DelegationRevision,
        MailEndpoint,
    };
    use plexmaton_core::{AgentId, CollaborationId, ConversationId, HeadName, TurnId};

    use super::*;

    struct Directory(std::path::PathBuf);

    impl Directory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "plexmaton-collaboration-writer-{}",
                uuid::Uuid::now_v7()
            ));
            std::fs::create_dir(&path).expect("create test directory");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                    .expect("protect test directory");
            }
            Self(path)
        }
    }

    impl Drop for Directory {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).expect("remove test directory");
        }
    }

    fn endpoint(name: &str) -> MailEndpoint {
        MailEndpoint {
            agent: AgentId::new(name).expect("agent"),
            conversation: ConversationId::new(format!("conversation-{name}"))
                .expect("conversation"),
        }
    }

    fn delegation() -> DelegationId {
        DelegationId::new("task").expect("delegation")
    }

    fn item(name: &str) -> CollaborationItemId {
        CollaborationItemId::new(name).expect("item")
    }

    fn file(directory: &Directory) -> CollaborationFile {
        let mut file = CollaborationFile::create(
            directory.0.join("collaboration.jsonl"),
            CollaborationId::new("writer").expect("collaboration"),
            CollaborationLimits::default(),
        )
        .expect("collaboration file");
        file.admit(
            item("create"),
            CollaborationEvent::DelegationCreated {
                delegation: delegation(),
                delegator: endpoint("main"),
                worker: endpoint("child"),
                task: CollaborationText::new("Inspect the parser").expect("task"),
            },
        )
        .expect("create delegation");
        file
    }

    fn update() -> CollaborationAttempt {
        CollaborationAttempt {
            id: item("update"),
            event: CollaborationEvent::TaskUpdated {
                delegation: delegation(),
                expected: DelegationRevision(0),
                author: endpoint("main"),
                task: CollaborationText::new("Inspect the parser boundary").expect("task"),
            },
        }
    }

    fn handoff() -> CollaborationAttempt {
        CollaborationAttempt {
            id: item("handoff"),
            event: CollaborationEvent::HandoffCompleted {
                delegation: delegation(),
                expected: DelegationRevision(0),
                author: endpoint("main"),
            },
        }
    }

    fn schedule_request() -> ScheduledTurnRequest {
        ScheduledTurnRequest::new(
            delegation(),
            item("turn"),
            TurnBoundary {
                recipient: endpoint("child"),
                head: HeadName::new("main").expect("head"),
                head_revision: HeadRevision::new(0),
                parent: None,
                turn: TurnId::new("turn").expect("turn"),
            },
            None,
        )
    }

    /// CTL-1: owner-side revision and endpoints come from the current canonical reduction.
    #[tokio::test]
    async fn ctl_1_writer_resolves_current_delegation_state_for_owner_ingress() {
        let directory = Directory::new();
        let mut writer = CollaborationWriter::spawn(file(&directory)).expect("spawn writer");
        let initial = writer
            .delegation_view(delegation())
            .await
            .expect("initial view");
        assert_eq!(initial.revision, DelegationRevision(0));
        assert_eq!(initial.delegator, endpoint("main"));
        assert_eq!(initial.worker, endpoint("child"));

        writer.admit(update()).await.expect("update task");
        let current = writer
            .delegation_view(delegation())
            .await
            .expect("current view");
        assert_eq!(current.revision, DelegationRevision(1));
        assert_eq!(current.task.as_str(), "Inspect the parser boundary");
        writer.shutdown().await.expect("join writer");
    }

    /// SCH-1: an accepted command survives reply cancellation and shutdown joins its file owner.
    #[tokio::test]
    async fn sch_1_writer_finishes_accepted_admission_after_reply_cancellation() {
        let directory = Directory::new();
        let path = directory.0.join("collaboration.jsonl");
        let mut writer = CollaborationWriter::spawn(file(&directory)).expect("spawn writer");
        let (reply, cancelled) = oneshot::channel();
        writer
            .sender
            .as_ref()
            .expect("open writer")
            .send(Command::Admit {
                attempt: update(),
                reply,
            })
            .await
            .expect("accept command");
        drop(cancelled);

        let mut retry = update();
        let receipt = loop {
            match writer.admit(retry).await {
                Ok(receipt) => break receipt,
                Err(CollaborationWriterError::AdmissionBusy { attempt }) => {
                    retry = *attempt;
                    tokio::task::yield_now().await;
                }
                Err(error) => panic!("exact retry: {error}"),
            }
        };
        assert_eq!(receipt.id, item("update"));
        writer.shutdown().await.expect("join writer");

        let reopened = CollaborationFile::open(path).expect("writer released file");
        assert_eq!(
            reopened
                .ledger()
                .delegation(&delegation())
                .expect("delegation")
                .task
                .as_str(),
            "Inspect the parser boundary"
        );
    }

    /// SCH-1/COL-4: a worker panic returns the exact accepted mutation for reopen reconciliation.
    #[tokio::test]
    async fn sch_1_worker_panic_retains_attempt_and_sticky_failure() {
        let directory = Directory::new();
        let path = directory.0.join("collaboration.jsonl");
        let mut writer = CollaborationWriter::spawn(file(&directory)).expect("spawn writer");
        let (reply, result) = oneshot::channel();
        writer
            .sender
            .as_ref()
            .expect("open writer")
            .try_send(Command::PanicAfterAdmission {
                attempt: update(),
                reply,
            })
            .expect("accept panic fixture");
        let failure = result
            .await
            .expect("panic reply")
            .expect_err("worker panic is typed");
        assert_eq!(failure.attempt(), Some(&update()));
        assert!(matches!(
            writer.shutdown().await,
            Err(CollaborationWriterError::WorkerFailed)
        ));
        assert!(matches!(
            writer.shutdown().await,
            Err(CollaborationWriterError::WorkerFailed)
        ));

        let mut reopened = CollaborationFile::open(path).expect("reopen after worker failure");
        reopened
            .admit(update().id, update().event)
            .expect("exact retry reconciles the append");
        assert_eq!(reopened.ledger().records().len(), 2);
    }

    /// SCH-4/CMP-1: a failed quiescence check still joins the worker and releases its file lock.
    #[tokio::test]
    async fn sch_4_writer_joins_after_a_poisoned_quiescence_check() {
        let directory = Directory::new();
        let path = directory.0.join("collaboration.jsonl");
        let mut writer = CollaborationWriter::spawn(file(&directory)).expect("spawn writer");

        assert!(matches!(
            writer
                .finish_shutdown(Err(CollaborationWriterError::Store(
                    CollaborationStoreError::WriterPoisoned,
                )))
                .await,
            Err(CollaborationWriterError::Store(
                CollaborationStoreError::WriterPoisoned
            ))
        ));
        let reopened = CollaborationFile::open(path).expect("failed shutdown released file lock");
        drop(reopened);
    }

    /// SCH-3: only the scheduling command may append a logical child turn admission.
    #[tokio::test]
    async fn sch_3_generic_admission_refuses_turns_without_mutating_the_file() {
        let directory = Directory::new();
        let path = directory.0.join("collaboration.jsonl");
        let file = file(&directory);
        let request = schedule_request();
        let Preparation::Append(record) = file
            .ledger()
            .prepare_turn(
                request.item.clone(),
                request.boundary.clone(),
                request.previous.clone(),
            )
            .expect("prepare turn fixture")
        else {
            panic!("fresh turn fixture")
        };
        let unchanged = std::fs::read(&path).expect("collaboration bytes");
        let mut writer = CollaborationWriter::spawn(file).expect("spawn writer");
        let refusal = writer
            .admit(CollaborationAttempt {
                id: record.id,
                event: record.event,
            })
            .await
            .expect_err("generic turn admission is forbidden");
        assert!(matches!(
            refusal,
            CollaborationWriterError::TurnRequiresScheduling { .. }
        ));
        let refusal = writer
            .admit(handoff())
            .await
            .expect_err("generic Handoff admission is forbidden");
        assert!(matches!(
            refusal,
            CollaborationWriterError::HandoffRequiresOwnership { .. }
        ));
        assert_eq!(std::fs::read(&path).expect("unchanged bytes"), unchanged);
        writer.shutdown().await.expect("join writer");
    }

    /// SCH-1/SCH-3: queue capacity is hard and rejected scheduling retains no authority.
    #[tokio::test]
    async fn sch_1_writer_channel_is_bounded_while_the_owner_is_blocked() {
        let directory = Directory::new();
        let mut writer = CollaborationWriter::spawn(file(&directory)).expect("spawn writer");
        let (entered, observed) = std::sync::mpsc::channel();
        let (release, held) = std::sync::mpsc::channel();
        writer
            .sender
            .as_ref()
            .expect("open writer")
            .try_send(Command::Hold {
                entered,
                release: held,
            })
            .expect("block worker");
        observed.recv().expect("worker entered hold");

        let (first_reply, first_result) = oneshot::channel();
        writer
            .sender
            .as_ref()
            .expect("open writer")
            .try_send(Command::DelegatedControl {
                delegation: delegation(),
                reply: first_reply,
            })
            .expect("fill one queue slot");
        let (overflow_reply, _overflow_result) = oneshot::channel();
        assert!(matches!(
            writer
                .sender
                .as_ref()
                .expect("open writer")
                .try_send(Command::DelegatedControl {
                    delegation: delegation(),
                    reply: overflow_reply,
                }),
            Err(mpsc::error::TrySendError::Full(_))
        ));
        let refusal = writer
            .admit(update())
            .await
            .expect_err("public ingress refuses full queue");
        assert!(matches!(
            refusal,
            CollaborationWriterError::AdmissionBusy { .. }
        ));
        release.send(()).expect("release worker");
        first_result
            .await
            .expect("control reply")
            .expect("delegated control");
        writer.shutdown().await.expect("join writer");
    }

    /// SCH-3: busy or post-Handoff authority fails before another turn can become durable.
    #[tokio::test]
    async fn sch_3_schedule_preflights_authority_before_turn_admission() {
        let directory = Directory::new();
        let path = directory.0.join("collaboration.jsonl");
        let mut writer = CollaborationWriter::spawn(file(&directory)).expect("spawn writer");
        let prepared = writer
            .schedule(schedule_request())
            .await
            .expect("prepare first execution");
        let after_first = std::fs::read(&path).expect("first admission bytes");
        let mut second = schedule_request();
        second.item = item("turn-two");
        second.boundary.turn = TurnId::new("turn-two").expect("turn");
        let refusal = match writer.schedule(second).await {
            Ok(_) => panic!("busy execution slot refuses before append"),
            Err(error) => error,
        };
        assert!(matches!(
            refusal,
            CollaborationWriterError::Schedule {
                source: CollaborationStoreError::ExecutionBusy,
                ..
            }
        ));
        assert_eq!(
            std::fs::read(&path).expect("unchanged busy bytes"),
            after_first
        );

        drop(prepared);
        writer.admit_handoff(handoff()).await.expect("handoff");
        let after_handoff = std::fs::read(&path).expect("handoff bytes");
        let mut post_handoff = schedule_request();
        post_handoff.item = item("turn-after-handoff");
        post_handoff.boundary.turn = TurnId::new("turn-after-handoff").expect("turn");
        assert!(matches!(
            writer.schedule(post_handoff).await,
            Err(CollaborationWriterError::Schedule { .. })
        ));
        assert_eq!(
            std::fs::read(&path).expect("unchanged post-handoff bytes"),
            after_handoff
        );
        writer.shutdown().await.expect("join writer");
    }

    /// SCH-3/SCH-4: a prepared turn blocks Handoff until its reservation is disposed.
    #[tokio::test]
    async fn sch_3_prepared_execution_retains_authority_until_disposed() {
        let directory = Directory::new();
        let mut writer = CollaborationWriter::spawn(file(&directory)).expect("spawn writer");
        let prepared = writer
            .schedule(schedule_request())
            .await
            .expect("prepare execution");
        assert_eq!(prepared.resolved().reference().item, item("turn"));

        let refusal = writer
            .admit_handoff(handoff())
            .await
            .expect_err("reservation blocks handoff");
        assert!(matches!(
            refusal,
            CollaborationWriterError::Admission {
                source: CollaborationStoreError::ControlNotQuiescent,
                ..
            }
        ));
        assert!(matches!(
            writer.shutdown().await,
            Err(CollaborationWriterError::Store(
                CollaborationStoreError::ControlNotQuiescent
            ))
        ));
        drop(prepared);
        writer
            .admit_handoff(handoff())
            .await
            .expect("handoff after drop");
        writer.shutdown().await.expect("join writer");
    }
}
