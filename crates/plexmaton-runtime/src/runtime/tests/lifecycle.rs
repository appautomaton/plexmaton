use std::{
    future,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use futures_util::{FutureExt as _, future::BoxFuture};
use plexmaton_agent::{Input, ModelCall, ModelEvent, ModelOutputPosition, StopReason, ToolCall};
use plexmaton_core::{ApprovalDecision, AttentionRequest, SessionEvent, ToolCallId};
use rustix::{io::Errno, process::Pid};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::{FakeDriver, Script, agent_id, complete_usage, test_request_environment};
use crate::{
    LiveRuntime, NativeToolCatalog,
    runtime::{ModelDriver, ModelOutput, ModelSignal, ModelTerminalReport},
};

const STUBBORN_COMMAND: &str = "trap '' TERM; printf '%s' $$ > command.pid; while :; do :; done";

struct TestWorkspace(PathBuf);

impl TestWorkspace {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        loop {
            let serial = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "plexmaton-runtime-{label}-{}-{serial}",
                std::process::id()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("create lifecycle workspace: {error}"),
            }
        }
    }

    fn catalog(&self) -> NativeToolCatalog {
        NativeToolCatalog::open(&self.0, "TEST_KEY", "/bin/false", "/bin/false", Vec::new())
            .unwrap_or_else(|error| panic!("open lifecycle catalog: {error}"))
    }
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0)
            .unwrap_or_else(|error| panic!("remove lifecycle workspace: {error}"));
    }
}

fn runtime(driver: Arc<dyn ModelDriver>, workspace: &TestWorkspace) -> LiveRuntime {
    LiveRuntime::with_driver(
        agent_id(),
        "Plexmaton".to_owned(),
        driver,
        workspace.catalog(),
    )
    .unwrap_or_else(|error| panic!("construct runtime: {error}"))
}

fn command_call() -> ModelEvent {
    ModelEvent::Called {
        position: ModelOutputPosition::new(0, 0),
        call: ToolCall {
            call_id: ToolCallId::new("command-lifecycle")
                .unwrap_or_else(|error| panic!("fixture call id: {error}")),
            name: "exec_command".to_owned(),
            arguments: serde_json::json!({
                "cmd": STUBBORN_COMMAND,
                "timeout_ms": 5000
            })
            .to_string(),
        },
    }
}

async fn start_stubborn_command(runtime: &mut LiveRuntime, workspace: &Path) -> Pid {
    let _announced = runtime.try_next_event();
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "start the lifecycle fixture".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("submit lifecycle turn: {error}"));
    let approval_id = loop {
        let envelope = match runtime.try_next_event() {
            Some(envelope) => envelope,
            None => tokio::time::timeout(Duration::from_secs(5), runtime.next_event())
                .await
                .unwrap_or_else(|_| panic!("lifecycle approval did not arrive"))
                .unwrap_or_else(|error| panic!("receive lifecycle event: {error}"))
                .unwrap_or_else(|| panic!("runtime ended before lifecycle approval")),
        };
        if let SessionEvent::AttentionRequested {
            request: AttentionRequest::Approval { approval_id, .. },
            ..
        } = envelope.event
        {
            break approval_id;
        }
    };
    runtime
        .submit(
            agent_id(),
            Input::ApprovalDecided {
                approval_id,
                decision: ApprovalDecision::AllowOnce,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("approve lifecycle command: {error}"));

    let pid_path = workspace.join("command.pid");
    let raw = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(raw) = std::fs::read_to_string(&pid_path)
                && !raw.trim().is_empty()
            {
                break raw;
            }
            let _cancelled_poll =
                tokio::time::timeout(Duration::from_millis(10), runtime.next_event()).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("lifecycle command did not publish its pid"));
    Pid::from_raw(
        raw.trim()
            .parse()
            .unwrap_or_else(|error| panic!("parse lifecycle pid: {error}")),
    )
    .unwrap_or_else(|| panic!("lifecycle pid must be positive"))
}

fn assert_group_gone(pid: Pid) {
    assert_eq!(
        rustix::process::test_kill_process_group(pid),
        Err(Errno::SRCH),
        "runtime returned while its command process group was still alive"
    );
}

/// LIVE-1 and CMD-6: even an abnormal direct drop cancels and joins the owned native worker.
#[tokio::test]
async fn dropping_an_active_runtime_joins_its_command_worker_and_process_group() {
    let workspace = TestWorkspace::new("drop-command");
    let driver = FakeDriver::new([Script::Events(vec![
        command_call(),
        complete_usage(8, 2),
        ModelEvent::Stopped(StopReason::ToolCalls),
    ])]);
    let mut runtime = runtime(driver, &workspace);
    let pid = start_stubborn_command(&mut runtime, &workspace.0).await;

    drop(runtime);

    assert_group_gone(pid);
}

/// LIVE-3: cancelling one shutdown poll leaves its state resumable by the next call.
#[tokio::test]
async fn cancelled_shutdown_can_be_called_again_to_finish_exact_cleanup() {
    let workspace = TestWorkspace::new("resume-shutdown");
    let driver = FakeDriver::new([Script::Events(vec![
        command_call(),
        complete_usage(8, 2),
        ModelEvent::Stopped(StopReason::ToolCalls),
    ])]);
    let mut runtime = runtime(driver, &workspace);
    let pid = start_stubborn_command(&mut runtime, &workspace.0).await;

    assert!(
        tokio::time::timeout(Duration::from_millis(20), runtime.shutdown())
            .await
            .is_err(),
        "the stubborn command did not hold the first shutdown poll open"
    );
    assert!(runtime.has_active_work());
    runtime
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("resume shutdown: {error}"));

    assert!(!runtime.has_active_work());
    assert_group_gone(pid);
}

struct PendingDriver {
    dropped: Arc<AtomicBool>,
    environment: plexmaton_agent::RequestEnvironment,
}

struct DropFlag(Arc<AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

impl ModelDriver for PendingDriver {
    fn request_environment(&self) -> &plexmaton_agent::RequestEnvironment {
        &self.environment
    }

    fn drive(
        &self,
        attempt_id: plexmaton_agent::RequestAttemptId,
        call: ModelCall,
        signals: mpsc::Sender<ModelSignal>,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'static, ModelTerminalReport> {
        let dropped = Arc::clone(&self.dropped);
        async move {
            let _drop_flag = DropFlag(dropped);
            let output = ModelOutput::from_event(ModelEvent::TextDelta {
                position: ModelOutputPosition::new(0, 0),
                delta: "started".to_owned(),
            })
            .unwrap_or_else(|_| unreachable!("text is nonterminal model output"));
            let _closed = signals
                .send(ModelSignal {
                    attempt_id,
                    step_id: call.step_id,
                    output,
                })
                .await;
            future::pending::<ModelTerminalReport>().await
        }
        .boxed()
    }
}

/// LIVE-1: the model operation is a retained future, not a spawned task left behind by Drop.
#[tokio::test]
async fn dropping_an_active_runtime_drops_the_exact_provider_future() {
    let workspace = TestWorkspace::new("drop-provider");
    let dropped = Arc::new(AtomicBool::new(false));
    let driver = Arc::new(PendingDriver {
        dropped: Arc::clone(&dropped),
        environment: test_request_environment(),
    });
    let mut runtime = runtime(driver, &workspace);
    let _announced = runtime.try_next_event();
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "start provider".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("submit provider fixture: {error}"));
    loop {
        let event = runtime
            .next_event()
            .await
            .unwrap_or_else(|error| panic!("start provider future: {error}"))
            .unwrap_or_else(|| panic!("runtime ended before provider future started"));
        if matches!(
            event.event,
            SessionEvent::TranscriptDelta { ref text, .. } if text == "started"
        ) {
            break;
        }
    }

    drop(runtime);

    assert!(dropped.load(Ordering::SeqCst));
}
