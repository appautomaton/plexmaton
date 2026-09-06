use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use plexmaton_agent::{
    ConversationMetadata, Input, JournalEntryPayload, JournalRecord, ModelEvent,
    ModelOutputPosition, StopReason, ToolCall, UndeliveredReason,
};
use plexmaton_core::{AgentId, ConversationId, ToolCallId, ToolCallStatus};
use plexmaton_session_store::StoreError;
use tokio::sync::Notify;

use super::{FakeDriver, Script, agent_id};
use crate::runtime::{LiveRuntime, journal::JournalStore};
use crate::{CleanupFailure, NativeToolCatalog, PersistenceFailure, RuntimeError, RuntimeUpdate};

struct ControlledStore {
    records: Arc<Mutex<Vec<JournalRecord>>>,
    gate: Arc<Gate>,
    fail_at: Arc<AtomicUsize>,
    fail_unknown: Arc<AtomicBool>,
    attempts: Arc<AtomicUsize>,
    block_at: Arc<AtomicUsize>,
    dropped: Arc<AtomicBool>,
    fail_text: Arc<Mutex<Option<String>>>,
    panic_at: Arc<AtomicUsize>,
    block_payload: Arc<Mutex<Option<BlockPayload>>>,
    cancel_gate: Arc<Gate>,
}

#[derive(Clone, Copy)]
enum BlockPayload {
    ToolRequested,
    PermissionDecision,
    ToolStatus(ToolCallStatus),
    RequestAuthorized,
    RequestFinished,
    CompactionAuthorized,
    CompactionFinished,
    CompactionCheckpoint,
    AgentAuthorized,
}

impl Drop for ControlledStore {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

struct Gate {
    blocked: Mutex<bool>,
    released: Condvar,
    entered: Notify,
}

impl Gate {
    fn new() -> Self {
        Self {
            blocked: Mutex::new(false),
            released: Condvar::new(),
            entered: Notify::new(),
        }
    }

    fn arm(&self) {
        *self
            .blocked
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
    }

    fn wait(&self) {
        self.wait_for(Duration::from_secs(5));
    }

    fn wait_for(&self, timeout: Duration) {
        let blocked = self
            .blocked
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *blocked {
            self.entered.notify_one();
        }
        // A failed assertion can drop the runtime before the test releases this gate.
        // Its writer joins synchronously, so an async timeout alone cannot bound cleanup.
        let (blocked, _) = self
            .released
            .wait_timeout_while(blocked, timeout, |blocked| *blocked)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(!*blocked, "test journal gate was not released");
    }

    fn release(&self) {
        let mut blocked = self
            .blocked
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *blocked = false;
        self.released.notify_all();
    }
}

struct ReleaseGateOnDrop<'a>(&'a Gate);

impl Drop for ReleaseGateOnDrop<'_> {
    fn drop(&mut self) {
        self.0.release();
    }
}

#[test]
#[should_panic(expected = "test journal gate was not released")]
fn an_unreleased_test_gate_fails_instead_of_hanging_writer_cleanup() {
    let gate = Gate::new();
    gate.arm();
    gate.wait_for(Duration::ZERO);
}

impl JournalStore for ControlledStore {
    fn append(&mut self, record: JournalRecord) -> Result<(), StoreError> {
        let attempt = self.attempts.fetch_add(1, Ordering::SeqCst) + 1;
        if self.block_at.load(Ordering::SeqCst) == attempt {
            self.gate.wait();
        }
        let blocked_payload = *self
            .block_payload
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let blocks_payload = blocked_payload.is_some_and(|payload| payload.matches(&record));
        if blocks_payload {
            self.gate.wait();
        }
        if matches!(&record, JournalRecord::AppendEntry { entry, .. }
        if matches!(&entry.payload, JournalEntryPayload::ToolCallChanged {
            status: ToolCallStatus::Cancelled, ..
        })) {
            self.cancel_gate.wait();
        }
        assert_ne!(
            self.panic_at.load(Ordering::SeqCst),
            attempt,
            "injected journal writer panic"
        );
        if self.fail_at.load(Ordering::SeqCst) == attempt {
            return if self.fail_unknown.load(Ordering::SeqCst) {
                Err(StoreError::Io {
                    operation: "append",
                    source: std::io::Error::other("injected uncertain write"),
                })
            } else {
                Err(StoreError::WriterPoisoned)
            };
        }
        if matches!(
            &record,
            JournalRecord::AppendEntry { entry, .. }
                if matches!(
                    &entry.payload,
                    JournalEntryPayload::TurnStarted { text, .. }
                        | JournalEntryPayload::SteeringAccepted { text, .. }
                        if self
                            .fail_text
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .as_deref()
                            == Some(text)
                )
        ) {
            return Err(StoreError::WriterPoisoned);
        }
        self.records
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(record);
        Ok(())
    }
}

impl BlockPayload {
    fn matches(self, record: &JournalRecord) -> bool {
        match self {
            Self::PermissionDecision => {
                matches!(record, JournalRecord::AppendEntry { entry, .. } if matches!(&entry.payload, JournalEntryPayload::ToolPermissionDecided { audit, .. } if audit.user.is_some()))
            }
            Self::ToolRequested => matches!(
                record,
                JournalRecord::AppendEntry { entry, .. }
                    if matches!(&entry.payload, JournalEntryPayload::ToolCallRequested { .. })
            ),
            Self::ToolStatus(expected) => matches!(
                record,
                JournalRecord::AppendEntry { entry, .. }
                    if matches!(
                        &entry.payload,
                        JournalEntryPayload::ToolCallChanged { status, .. } if *status == expected
                    )
            ),
            Self::RequestAuthorized => {
                matches!(record, JournalRecord::RequestAttemptAuthorized { .. })
            }
            Self::RequestFinished => {
                matches!(record, JournalRecord::RequestAttemptFinished { .. })
            }
            Self::CompactionAuthorized => matches!(
                record,
                JournalRecord::RequestAttemptAuthorized { fact, .. }
                    if matches!(fact.owner(), plexmaton_agent::RequestAttemptOwner::Compaction { .. })
            ),
            Self::AgentAuthorized => matches!(
                record,
                JournalRecord::RequestAttemptAuthorized { fact, .. }
                    if matches!(fact.owner(), plexmaton_agent::RequestAttemptOwner::AgentStep { .. })
            ),
            Self::CompactionFinished => {
                matches!(record, JournalRecord::CompactionAttemptFinished { .. })
            }
            Self::CompactionCheckpoint => matches!(
                record,
                JournalRecord::AppendEntry { entry, .. }
                    if matches!(entry.payload, JournalEntryPayload::CompactionCheckpoint { .. })
            ),
        }
    }
}

struct StoreControl {
    records: Arc<Mutex<Vec<JournalRecord>>>,
    gate: Arc<Gate>,
    fail_at: Arc<AtomicUsize>,
    fail_unknown: Arc<AtomicBool>,
    attempts: Arc<AtomicUsize>,
    block_at: Arc<AtomicUsize>,
    dropped: Arc<AtomicBool>,
    fail_text: Arc<Mutex<Option<String>>>,
    panic_at: Arc<AtomicUsize>,
    block_payload: Arc<Mutex<Option<BlockPayload>>>,
    cancel_gate: Arc<Gate>,
}

impl StoreControl {
    fn pair() -> (Self, ControlledStore) {
        let records = Arc::new(Mutex::new(Vec::new()));
        let gate = Arc::new(Gate::new());
        let fail_at = Arc::new(AtomicUsize::new(0));
        let fail_unknown = Arc::new(AtomicBool::new(false));
        let attempts = Arc::new(AtomicUsize::new(0));
        let block_at = Arc::new(AtomicUsize::new(0));
        let dropped = Arc::new(AtomicBool::new(false));
        let fail_text = Arc::new(Mutex::new(None));
        let panic_at = Arc::new(AtomicUsize::new(0));
        let block_payload = Arc::new(Mutex::new(None));
        let cancel_gate = Arc::new(Gate::new());
        (
            Self {
                records: Arc::clone(&records),
                gate: Arc::clone(&gate),
                fail_at: Arc::clone(&fail_at),
                fail_unknown: Arc::clone(&fail_unknown),
                attempts: Arc::clone(&attempts),
                block_at: Arc::clone(&block_at),
                dropped: Arc::clone(&dropped),
                fail_text: Arc::clone(&fail_text),
                panic_at: Arc::clone(&panic_at),
                block_payload: Arc::clone(&block_payload),
                cancel_gate: Arc::clone(&cancel_gate),
            },
            ControlledStore {
                records,
                gate,
                fail_at,
                fail_unknown,
                attempts,
                block_at,
                dropped,
                fail_text,
                panic_at,
                block_payload,
                cancel_gate,
            },
        )
    }

    fn block_after(&self, additional_attempts: usize) {
        self.block_at.store(
            self.attempts.load(Ordering::SeqCst) + additional_attempts,
            Ordering::SeqCst,
        );
        self.gate.arm();
    }

    fn fail_after(&self, additional_attempts: usize, outcome_unknown: bool) {
        self.fail_unknown.store(outcome_unknown, Ordering::SeqCst);
        self.fail_at.store(
            self.attempts.load(Ordering::SeqCst) + additional_attempts,
            Ordering::SeqCst,
        );
    }

    fn fail_on_text(&self, text: &str) {
        *self
            .fail_text
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(text.to_owned());
    }

    fn panic_after(&self, additional_attempts: usize) {
        self.panic_at.store(
            self.attempts.load(Ordering::SeqCst) + additional_attempts,
            Ordering::SeqCst,
        );
    }

    fn block_on_payload(&self, payload: BlockPayload) {
        *self
            .block_payload
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(payload);
        self.gate.arm();
    }
}

async fn runtime<D: super::ModelDriver>(
    controlled: ControlledStore,
    driver: Arc<D>,
) -> LiveRuntime {
    let clock = Arc::new(
        crate::runtime::clock::SystemWallClock::new()
            .unwrap_or_else(|error| panic!("test wall clock: {error}")),
    );
    runtime_with_clock(controlled, driver, clock).await
}

async fn runtime_with_clock<D: super::ModelDriver>(
    controlled: ControlledStore,
    driver: Arc<D>,
    clock: Arc<dyn crate::runtime::clock::WallClock>,
) -> LiveRuntime {
    let created_at_unix_ms = clock.now();
    let workspace =
        std::env::current_dir().unwrap_or_else(|error| panic!("resolve workspace: {error}"));
    let tools = NativeToolCatalog::open(
        &workspace,
        "TEST_KEY",
        "/bin/false",
        "/bin/false",
        Vec::new(),
    )
    .unwrap_or_else(|error| panic!("open tools: {error}"));
    LiveRuntime::with_driver_store_and_clock(
        agent_id(),
        "Plexmaton".to_owned(),
        driver,
        tools,
        ConversationMetadata::new(
            ConversationId::new("session-durable")
                .unwrap_or_else(|error| panic!("session id: {error}")),
            created_at_unix_ms,
        ),
        Box::new(controlled),
        clock,
    )
    .await
    .unwrap_or_else(|error| panic!("open durable runtime: {error}"))
}

fn submission() -> Input {
    Input::Submitted {
        text: "keep this exact draft".to_owned(),
    }
}

async fn await_report(runtime: &mut LiveRuntime) -> crate::DispatchReport {
    loop {
        match tokio::time::timeout(Duration::from_secs(5), runtime.next_update())
            .await
            .unwrap_or_else(|_| panic!("runtime did not surface its dispatch report"))
            .unwrap_or_else(|error| panic!("drive runtime report: {error}"))
        {
            RuntimeUpdate::Event(_) => {}
            RuntimeUpdate::Report(report) => return report,
            RuntimeUpdate::Finished => panic!("runtime finished before returning ownership"),
        }
    }
}

async fn drive_until_store_blocks(runtime: &mut LiveRuntime, control: &StoreControl) {
    drive_until_gate(runtime, &control.gate).await;
}

async fn drive_until_gate(runtime: &mut LiveRuntime, gate: &Gate) {
    let deadline = tokio::time::sleep(Duration::from_secs(5));
    tokio::pin!(deadline);
    loop {
        let entered = gate.entered.notified();
        let blocked = {
            let update = runtime.next_update();
            tokio::pin!(update);
            tokio::select! {
                result = &mut update => {
                    match result.unwrap_or_else(|error| panic!("drive tool barrier: {error}")) {
                        RuntimeUpdate::Event(_) => false,
                        RuntimeUpdate::Report(report) => {
                            panic!("unexpected report before tool barrier: {report:?}")
                        }
                        RuntimeUpdate::Finished => panic!("runtime finished before tool barrier"),
                    }
                }
                () = entered => true,
                () = &mut deadline => {
                    gate.release();
                    panic!("runtime did not reach the store barrier");
                }
            }
        };
        if blocked {
            return;
        }
    }
}

mod barriers;
mod cancellation;
mod failures;
mod permissions;
mod retry;
