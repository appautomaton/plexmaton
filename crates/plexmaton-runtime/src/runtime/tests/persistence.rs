use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use plexmaton_agent::{
    Input, JournalEntryPayload, JournalRecord, ModelEvent, StopReason, ToolCall, UndeliveredReason,
};
use plexmaton_core::{AgentId, SessionId, ToolCallId, ToolCallStatus};
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
}

#[derive(Clone, Copy)]
enum BlockPayload {
    ToolRequested,
    ToolStatus(ToolCallStatus),
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
        let mut blocked = self
            .blocked
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *blocked {
            self.entered.notify_one();
        }
        while *blocked {
            blocked = self
                .released
                .wait(blocked)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
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

impl JournalStore for ControlledStore {
    fn append(&mut self, record: JournalRecord) -> Result<(), StoreError> {
        let attempt = self.attempts.fetch_add(1, Ordering::SeqCst) + 1;
        if self.block_at.load(Ordering::SeqCst) == attempt {
            self.gate.wait();
        }
        let blocks_payload = match *self
            .block_payload
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
        {
            Some(BlockPayload::ToolRequested) => matches!(
                &record,
                JournalRecord::AppendEntry { entry, .. }
                    if matches!(&entry.payload, JournalEntryPayload::ToolCallRequested { .. })
            ),
            Some(BlockPayload::ToolStatus(expected)) => matches!(
                &record,
                JournalRecord::AppendEntry { entry, .. }
                    if matches!(
                        &entry.payload,
                        JournalEntryPayload::ToolCallChanged { status, .. } if *status == expected
                    )
            ),
            None => false,
        };
        if blocks_payload {
            self.gate.wait();
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
                    JournalEntryPayload::Message { text, .. }
                        | JournalEntryPayload::TurnStarted { text, .. }
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

async fn runtime(controlled: ControlledStore, driver: Arc<FakeDriver>) -> LiveRuntime {
    let clock = Arc::new(
        crate::runtime::clock::SystemWallClock::new()
            .unwrap_or_else(|error| panic!("test wall clock: {error}")),
    );
    runtime_with_clock(controlled, driver, clock).await
}

async fn runtime_with_clock(
    controlled: ControlledStore,
    driver: Arc<FakeDriver>,
    clock: Arc<dyn crate::runtime::clock::WallClock>,
) -> LiveRuntime {
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
        SessionId::new("session-durable").unwrap_or_else(|error| panic!("session id: {error}")),
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
    loop {
        let entered = control.gate.entered.notified();
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
