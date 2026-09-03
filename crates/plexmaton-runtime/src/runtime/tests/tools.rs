use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use plexmaton_agent::{
    AdmissionRefusal, Input, ModelEvent, RequestItem, StopReason, ToolCall, ToolOutcome,
};
use plexmaton_command::MAX_MODEL_OUTPUT_BYTES;
use plexmaton_core::{ApprovalDecision, ApprovalId, AttentionRequest, SessionEvent, ToolCallId};
use rustix::{io::Errno, process::Pid};

use super::{FakeDriver, Script, agent_id, complete_usage, finish_active};
use crate::{LiveRuntime, NativeToolCatalog, runtime::ModelDriver};

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
                Err(error) => panic!("create test workspace: {error}"),
            }
        }
    }

    fn catalog(&self) -> NativeToolCatalog {
        NativeToolCatalog::open(&self.0, "TEST_KEY", "/bin/false", "/bin/false", Vec::new())
            .unwrap_or_else(|error| panic!("open native catalog: {error}"))
    }
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0)
            .unwrap_or_else(|error| panic!("remove test workspace: {error}"));
    }
}

fn runtime(driver: Arc<dyn ModelDriver>, workspace: &TestWorkspace) -> LiveRuntime {
    LiveRuntime::with_driver(
        agent_id(),
        "Plexmaton".to_owned(),
        driver,
        workspace.catalog(),
    )
}

fn called(id: &str, name: &str, arguments: serde_json::Value) -> ModelEvent {
    ModelEvent::Called(ToolCall {
        call_id: ToolCallId::new(id)
            .unwrap_or_else(|error| panic!("fixture tool call id: {error}")),
        name: name.to_owned(),
        arguments: arguments.to_string(),
    })
}

async fn submit(runtime: &mut LiveRuntime, text: &str) {
    let _announced = runtime.try_next_event();
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: text.to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("submit fixture turn: {error}"));
}

async fn next_approval(runtime: &mut LiveRuntime) -> ApprovalId {
    loop {
        let event = match runtime.try_next_event() {
            Some(event) => event,
            None => tokio::time::timeout(Duration::from_secs(5), runtime.next_event())
                .await
                .unwrap_or_else(|_| panic!("approval did not arrive"))
                .unwrap_or_else(|error| panic!("receive approval event: {error}"))
                .unwrap_or_else(|| panic!("runtime ended before approval")),
        };
        if let SessionEvent::AttentionRequested {
            request: AttentionRequest::Approval { approval_id, .. },
            ..
        } = event.event
        {
            return approval_id;
        }
    }
}

async fn allow_once(runtime: &mut LiveRuntime, approval_id: ApprovalId) {
    runtime
        .submit(
            agent_id(),
            Input::ApprovalDecided {
                approval_id,
                decision: ApprovalDecision::AllowOnce,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("approve fixture tool: {error}"));
}

fn succeeded_output<'a>(request: &'a plexmaton_agent::ModelCall, call_id: &str) -> &'a str {
    request
        .request
        .items
        .iter()
        .find_map(|item| match item {
            RequestItem::ToolResult {
                call_id: found,
                outcome: ToolOutcome::Succeeded { output },
            } if found.as_str() == call_id => Some(output.as_str()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("next model request has no success for {call_id}"))
}

fn tool_results(request: &plexmaton_agent::ModelCall) -> Vec<(&str, &ToolOutcome)> {
    request
        .request
        .items
        .iter()
        .filter_map(|item| match item {
            RequestItem::ToolResult { call_id, outcome } => Some((call_id.as_str(), outcome)),
            _ => None,
        })
        .collect()
}

/// LIVE-1 and APV-3: one observation store survives read, approval, edit, and continuation.
#[tokio::test]
async fn file_observation_survives_the_runtime_boundary_into_an_approved_edit() {
    let workspace = TestWorkspace::new("read-edit");
    std::fs::write(workspace.0.join("note.txt"), "old value\n")
        .unwrap_or_else(|error| panic!("write fixture: {error}"));
    let driver = FakeDriver::new([
        Script::Events(vec![
            called(
                "read-1",
                "read_file",
                serde_json::json!({"path":"note.txt", "offset":null, "limit":null}),
            ),
            complete_usage(10, 2),
            ModelEvent::Stopped(StopReason::ToolCalls),
        ]),
        Script::Events(vec![
            called(
                "edit-1",
                "edit_file",
                serde_json::json!({
                    "path":"note.txt",
                    "observation":"obs-0000000000000001",
                    "edits":[{"old_text":"old value", "new_text":"new value"}]
                }),
            ),
            complete_usage(20, 3),
            ModelEvent::Stopped(StopReason::ToolCalls),
        ]),
        Script::Events(vec![
            ModelEvent::TextDelta("finished".to_owned()),
            complete_usage(30, 4),
            ModelEvent::Stopped(StopReason::EndOfTurn),
        ]),
    ]);
    let mut runtime = runtime(driver.clone(), &workspace);
    submit(&mut runtime, "update the note").await;
    let approval = next_approval(&mut runtime).await;
    allow_once(&mut runtime, approval).await;
    let _events = finish_active(&mut runtime).await;

    assert_eq!(
        std::fs::read_to_string(workspace.0.join("note.txt"))
            .unwrap_or_else(|error| panic!("read edited fixture: {error}")),
        "new value\n"
    );
    let calls = driver.calls().await;
    assert_eq!(calls.len(), 3);
    assert!(succeeded_output(&calls[1], "read-1").contains("obs-0000000000000001"));
    assert!(succeeded_output(&calls[2], "edit-1").contains("edits_applied"));
}

/// LIVE-1: command capture stays bounded in both the record and exact next request.
#[tokio::test]
async fn maximal_command_result_stays_bounded_in_the_next_model_request() {
    let workspace = TestWorkspace::new("command-bound");
    let driver = FakeDriver::new([
        Script::Events(vec![
            called(
                "command-1",
                "exec_command",
                serde_json::json!({
                    "cmd":"head -c 1048576 /dev/zero; head -c 1048576 /dev/zero >&2",
                    "timeout_ms":5000
                }),
            ),
            complete_usage(8, 2),
            ModelEvent::Stopped(StopReason::ToolCalls),
        ]),
        Script::Events(vec![
            ModelEvent::TextDelta("checked".to_owned()),
            complete_usage(9, 2),
            ModelEvent::Stopped(StopReason::EndOfTurn),
        ]),
    ]);
    let mut runtime = runtime(driver.clone(), &workspace);
    submit(&mut runtime, "run the check").await;
    let approval = next_approval(&mut runtime).await;
    allow_once(&mut runtime, approval).await;
    let _events = finish_active(&mut runtime).await;

    let calls = driver.calls().await;
    assert_eq!(calls.len(), 2);
    let result = succeeded_output(&calls[1], "command-1");
    assert!(result.len() <= MAX_MODEL_OUTPUT_BYTES);
    assert!(result.contains("stdout_bytes: 1048576"));
    assert!(result.contains("stderr_bytes: 1048576"));
}

/// LIVE-1, LOOP-3 and APV-5: denial pays only its slot, never runs it, and sibling results keep
/// the order in which the model requested them.
#[tokio::test]
async fn denied_command_has_no_side_effect_and_keeps_model_order_with_a_read_sibling() {
    let workspace = TestWorkspace::new("denied-command");
    std::fs::write(workspace.0.join("note.txt"), "safe value\n")
        .unwrap_or_else(|error| panic!("write fixture: {error}"));
    let driver = FakeDriver::new([
        Script::Events(vec![
            called(
                "command-1",
                "exec_command",
                serde_json::json!({
                    "cmd":"printf 'ran' > should-not-exist",
                    "timeout_ms":5000
                }),
            ),
            called(
                "read-1",
                "read_file",
                serde_json::json!({"path":"note.txt", "offset":null, "limit":null}),
            ),
            complete_usage(8, 2),
            ModelEvent::Stopped(StopReason::ToolCalls),
        ]),
        Script::Events(vec![
            ModelEvent::TextDelta("finished".to_owned()),
            complete_usage(9, 2),
            ModelEvent::Stopped(StopReason::EndOfTurn),
        ]),
    ]);
    let mut runtime = runtime(driver.clone(), &workspace);
    submit(&mut runtime, "inspect without running the command").await;
    let approval_id = next_approval(&mut runtime).await;
    runtime
        .submit(
            agent_id(),
            Input::ApprovalDecided {
                approval_id,
                decision: ApprovalDecision::Deny,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("deny fixture tool: {error}"));
    let _events = finish_active(&mut runtime).await;

    assert!(
        !workspace.0.join("should-not-exist").exists(),
        "a denied command crossed the execution boundary"
    );
    let calls = driver.calls().await;
    assert_eq!(calls.len(), 2);
    let results = tool_results(&calls[1]);
    assert_eq!(results.len(), 2);
    assert!(matches!(results[0], ("command-1", ToolOutcome::Denied)));
    assert!(matches!(
        results[1],
        ("read-1", ToolOutcome::Succeeded { output }) if output.contains("safe value")
    ));
}

/// LIVE-1 and LOOP-2: a catalog refusal remains a tool result, pays the batch debt, and opens the
/// next model step instead of failing the runtime.
#[tokio::test]
async fn unknown_tool_is_a_typed_result_and_the_turn_continues() {
    let workspace = TestWorkspace::new("unknown-tool");
    let driver = FakeDriver::new([
        Script::Events(vec![
            called("unknown-1", "not_registered", serde_json::json!({})),
            complete_usage(8, 2),
            ModelEvent::Stopped(StopReason::ToolCalls),
        ]),
        Script::Events(vec![
            ModelEvent::TextDelta("recovered".to_owned()),
            complete_usage(9, 2),
            ModelEvent::Stopped(StopReason::EndOfTurn),
        ]),
    ]);
    let mut runtime = runtime(driver.clone(), &workspace);
    submit(&mut runtime, "try the unavailable tool").await;
    let _events = finish_active(&mut runtime).await;

    let calls = driver.calls().await;
    assert_eq!(calls.len(), 2);
    assert!(matches!(
        tool_results(&calls[1]).as_slice(),
        [(
            "unknown-1",
            ToolOutcome::AdmissionRefused {
                reason: AdmissionRefusal::UnknownTool
            }
        )]
    ));
}

/// LIVE-3: cancelling repeated `next_event` polls never drops the retained command owner.
#[tokio::test]
async fn cancelled_next_event_keeps_command_work_owned_until_interrupt_joins_it() {
    let workspace = TestWorkspace::new("command-cancel");
    let driver = FakeDriver::new([Script::Events(vec![
        called(
            "command-1",
            "exec_command",
            serde_json::json!({
                "cmd":"trap '' TERM; printf '%s' $$ > command.pid; while :; do :; done",
                "timeout_ms":5000
            }),
        ),
        complete_usage(8, 2),
        ModelEvent::Stopped(StopReason::ToolCalls),
    ])]);
    let mut runtime = runtime(driver.clone(), &workspace);
    submit(&mut runtime, "start the command").await;
    let approval = next_approval(&mut runtime).await;
    allow_once(&mut runtime, approval).await;

    let pid_path = workspace.0.join("command.pid");
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
    .unwrap_or_else(|_| panic!("command did not reach its readiness barrier"));
    let pid = Pid::from_raw(
        raw.trim()
            .parse()
            .unwrap_or_else(|error| panic!("parse command pid: {error}")),
    )
    .unwrap_or_else(|| panic!("command pid must be positive"));

    runtime
        .submit(agent_id(), Input::Interrupted)
        .await
        .unwrap_or_else(|error| panic!("interrupt command: {error}"));
    assert!(!runtime.has_active_work());
    assert_eq!(
        rustix::process::test_kill_process_group(pid),
        Err(Errno::SRCH)
    );
    assert_eq!(
        driver.calls().await.len(),
        1,
        "interrupt starts no next step"
    );
}
