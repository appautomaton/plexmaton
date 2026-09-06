use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, Ordering},
    },
};

use futures_util::future::BoxFuture;
use plexmaton_agent::{
    AssistantBlock, AssistantOutput, CompactionAttemptFinished, CompactionFailure,
    CompactionOutcome, ContextAtomValue, ModelCall, RequestAttemptId, RequestAttemptOwner,
    RequestAttemptTerminal, RequestAttemptTerminalState, RequestCost, RequestDispatchedOutcome,
    StopReason, ToolCall,
};
use plexmaton_core::TranscriptItemId;
use plexmaton_provider::{
    CompactionInput, FunctionTool, ModelRegistry, ResolvedModel, request_environment,
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::*;
use crate::RuntimeUpdate;
use crate::runtime::ModelDriver;

const MODEL_CONFIG: &str = r#"
active_model = { provider = "local", model = "test" }

[providers.local]
base_url = "http://127.0.0.1:1/v1"
api_key_env = "TEST_KEY"
api = "openai_responses"

[providers.local.models.test]
id = "test"
display_name = "Test"
reasoning_effort = "none"
context_window_tokens = 8192
max_output_tokens = 1024
output_reserve_tokens = 512
"#;

pub(super) enum SummaryScript {
    Complete(String),
    Fail(CompactionFailure),
    WaitForCancellation(Arc<AtomicBool>),
}

pub(super) struct CompactionDriver {
    agent: Arc<FakeDriver>,
    model: ResolvedModel,
    tools: Arc<[FunctionTool]>,
    environment: RequestEnvironment,
    enabled: AtomicBool,
    summaries: Arc<Mutex<VecDeque<SummaryScript>>>,
    summary_calls: Arc<StdMutex<Vec<CompactionInput>>>,
}

impl CompactionDriver {
    pub(super) fn new(
        agent_scripts: impl IntoIterator<Item = Script>,
        summary_scripts: impl IntoIterator<Item = SummaryScript>,
    ) -> Arc<Self> {
        let model = ModelRegistry::parse(MODEL_CONFIG)
            .unwrap_or_else(|error| panic!("compaction model: {error}"))
            .active_model()
            .clone();
        let tools: Arc<[FunctionTool]> = Arc::from([]);
        let environment = request_environment(&model, &tools, Some(model.max_output_tokens()));
        Arc::new(Self {
            agent: FakeDriver::new(agent_scripts),
            model,
            tools,
            environment,
            enabled: AtomicBool::new(false),
            summaries: Arc::new(Mutex::new(summary_scripts.into_iter().collect())),
            summary_calls: Arc::new(StdMutex::new(Vec::new())),
        })
    }

    pub(super) fn enable(&self) {
        self.enabled.store(true, Ordering::SeqCst);
    }

    pub(super) fn summary_call_count(&self) -> usize {
        self.summary_calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    fn summary_requests(&self) -> Vec<plexmaton_agent::ModelRequest> {
        self.summary_calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .map(|input| input.request().clone())
            .collect()
    }

    pub(super) async fn agent_calls(&self) -> Vec<ModelCall> {
        self.agent.calls().await
    }
}

impl ModelDriver for CompactionDriver {
    fn request_environment(&self) -> &RequestEnvironment {
        &self.environment
    }

    fn budget_inputs(&self) -> Option<(&ResolvedModel, &[FunctionTool])> {
        self.enabled
            .load(Ordering::SeqCst)
            .then_some((&self.model, &self.tools))
    }

    fn drive(
        &self,
        attempt_id: RequestAttemptId,
        call: ModelCall,
        signals: mpsc::Sender<ModelSignal>,
        cancellation: CancellationToken,
    ) -> BoxFuture<'static, ModelTerminalReport> {
        self.agent.drive(attempt_id, call, signals, cancellation)
    }

    fn summarize(
        &self,
        attempt_id: RequestAttemptId,
        input: CompactionInput,
        _max_summary_bytes: usize,
        cancellation: CancellationToken,
    ) -> BoxFuture<'static, CompactionAttemptFinished> {
        self.summary_calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(input.clone());
        let scripts = Arc::clone(&self.summaries);
        async move {
            let script = scripts
                .lock()
                .await
                .pop_front()
                .unwrap_or(SummaryScript::Fail(CompactionFailure::Unavailable));
            let mode = plexmaton_agent::CompactionInputMode::Verbatim;
            match script {
                SummaryScript::Complete(text) => finished_summary(
                    attempt_id,
                    mode,
                    RequestDispatchedOutcome::Completed {
                        stop_reason: StopReason::EndOfTurn,
                    },
                    CompactionOutcome::Complete {
                        output: summary_output(text),
                    },
                    true,
                ),
                SummaryScript::Fail(kind) => {
                    let outcome = match kind {
                        CompactionFailure::ContextTooLong => {
                            RequestDispatchedOutcome::ContextTooLong
                        }
                        CompactionFailure::Cancelled | CompactionFailure::TimedOut => {
                            RequestDispatchedOutcome::Cancelled
                        }
                        CompactionFailure::TransportFailed => {
                            RequestDispatchedOutcome::TransportFailed
                        }
                        CompactionFailure::ProviderFailed | CompactionFailure::Unavailable => {
                            RequestDispatchedOutcome::ProviderFailed
                        }
                        CompactionFailure::Refused => RequestDispatchedOutcome::Completed {
                            stop_reason: StopReason::Refused,
                        },
                        CompactionFailure::OutputLimit => RequestDispatchedOutcome::Completed {
                            stop_reason: StopReason::OutputLimit,
                        },
                        CompactionFailure::EmptyOutput
                        | CompactionFailure::OutputTooLarge
                        | CompactionFailure::ToolCallOutput
                        | CompactionFailure::Malformed
                        | CompactionFailure::NoProgress => RequestDispatchedOutcome::Malformed,
                    };
                    finished_summary(
                        attempt_id,
                        mode,
                        outcome,
                        CompactionOutcome::Failed { kind, output: None },
                        false,
                    )
                }
                SummaryScript::WaitForCancellation(cancelled) => {
                    cancellation.cancelled().await;
                    cancelled.store(true, Ordering::SeqCst);
                    finished_summary(
                        attempt_id,
                        mode,
                        RequestDispatchedOutcome::Cancelled,
                        CompactionOutcome::Failed {
                            kind: CompactionFailure::Cancelled,
                            output: None,
                        },
                        false,
                    )
                }
            }
        }
        .boxed()
    }
}

fn summary_output(text: String) -> AssistantOutput {
    AssistantOutput::new(
        vec![AssistantBlock::Text {
            item_id: TranscriptItemId::new("summary-output")
                .unwrap_or_else(|error| panic!("summary item: {error}")),
            text,
        }],
        None,
    )
    .unwrap_or_else(|error| panic!("summary output: {error}"))
}

fn finished_summary(
    attempt_id: RequestAttemptId,
    mode: plexmaton_agent::CompactionInputMode,
    request_outcome: RequestDispatchedOutcome,
    outcome: CompactionOutcome,
    output: bool,
) -> CompactionAttemptFinished {
    let terminal = RequestAttemptTerminal::new(
        attempt_id,
        RequestAttemptTerminalState::Dispatched {
            timing: request_timing(output),
            outcome: request_outcome,
            usage: TokenUsage::Unavailable,
            cost: RequestCost::Unavailable,
        },
    )
    .unwrap_or_else(|error| panic!("summary terminal: {error}"));
    CompactionAttemptFinished::new(terminal, mode, outcome)
        .unwrap_or_else(|error| panic!("summary finish: {error}"))
}

pub(super) fn large_answer() -> String {
    // Cross the soft limit while leaving room for the complete CPL-2 summary instruction.
    "history ".repeat(3_200)
}

pub(super) fn hard_answer() -> String {
    "history ".repeat(4_000)
}

async fn seed_large_history(runtime: &mut LiveRuntime) {
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "seed".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("seed submit: {error}"));
    let _events = finish_active(runtime).await;
}

async fn finish_compaction_runtime(runtime: &mut LiveRuntime) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while runtime.has_active_work() {
            let _ = runtime.next_update().await.unwrap_or_else(|error| {
                panic!("compaction runtime update: {error}")
            });
        }
        while runtime.try_next_event().is_some() {}
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "compaction runtime stalled: model={}, compaction={}, pending_commit={}, after_commit={}, pending_model={}, deferred_model={}, deferred_failure={}",
            runtime.active.is_some(),
            runtime.compaction.is_some(),
            runtime.pending_commit.is_some(),
            runtime.after_commit.is_some(),
            runtime.pending_model_start.is_some(),
            runtime.deferred_model_call.is_some(),
            runtime.deferred_compaction_failure.is_some(),
        )
    });
}

/// CPL-6/CPL-7/CPL-8: first-step pressure is summarized by a distinct durable owner, and the
/// checkpoint is installed before the pending agent step is refreshed and dispatched.
#[tokio::test]
async fn soft_pre_turn_compaction_uses_a_distinct_owner_and_refreshes_after_checkpoint() {
    let driver = CompactionDriver::new(
        [
            Script::Events(vec![
                text_delta(&large_answer()),
                ModelEvent::Stopped(StopReason::EndOfTurn),
            ]),
            Script::Events(vec![ModelEvent::Stopped(StopReason::EndOfTurn)]),
        ],
        [SummaryScript::Complete("compact facts".repeat(8))],
    );
    let mut runtime = runtime(driver.clone());
    seed_large_history(&mut runtime).await;
    driver.enable();

    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "continue".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("pressured submit: {error}"));
    assert_eq!(driver.agent_calls().await.len(), 1, "agent dispatch waits");
    assert!(runtime.compaction.is_some());
    let frozen = runtime
        .agent
        .journal()
        .project(runtime.agent.selected_head())
        .expect("frozen request")
        .request()
        .clone();
    let summaries = driver.summary_requests();
    let [summary] = summaries.as_slice() else {
        panic!("one summary request was dispatched");
    };
    assert_eq!(summary.session_id, frozen.session_id);
    assert_eq!(
        &summary.atoms[..frozen.atoms.len()],
        frozen.atoms.as_slice()
    );
    assert_eq!(summary.atoms.len(), frozen.atoms.len() + 1);
    assert!(matches!(
        summary.atoms.last().map(|atom| atom.value()),
        Some(ContextAtomValue::CompactionSummary { .. })
    ));

    finish_compaction_runtime(&mut runtime).await;
    let calls = driver.agent_calls().await;
    assert_eq!(calls.len(), 2);
    assert!(matches!(
        calls[1].request.atoms.first().map(|atom| atom.value()),
        Some(ContextAtomValue::CompactionSummary { .. })
    ));
    assert_eq!(driver.summary_call_count(), 1);
    let records = runtime.agent.journal().records();
    let authorization = records
        .iter()
        .position(|record| {
            matches!(
                record,
                plexmaton_agent::JournalRecord::RequestAttemptAuthorized { fact, .. }
                    if matches!(fact.owner(), RequestAttemptOwner::Compaction { .. })
            )
        })
        .expect("compaction authorization");
    let terminal = records
        .iter()
        .position(|record| {
            matches!(
                record,
                plexmaton_agent::JournalRecord::CompactionAttemptFinished { .. }
            )
        })
        .expect("compaction terminal");
    let checkpoint = records.iter().position(|record| matches!(
        record,
        plexmaton_agent::JournalRecord::AppendEntry { entry, .. }
            if matches!(entry.payload, plexmaton_agent::JournalEntryPayload::CompactionCheckpoint { .. })
    )).expect("checkpoint");
    assert!(authorization < terminal && terminal < checkpoint);
}

/// CPL-7: one empty-output typed context error can compact and retry the same semantic step; the
/// second error terminates normally instead of opening an unbounded recovery loop.
#[tokio::test]
async fn typed_context_error_recovers_the_same_step_once() {
    let driver = CompactionDriver::new(
        [
            Script::Events(vec![
                text_delta(&large_answer()),
                ModelEvent::Stopped(StopReason::EndOfTurn),
            ]),
            Script::Fail(ModelError::ContextTooLong),
            Script::Fail(ModelError::ContextTooLong),
        ],
        [SummaryScript::Complete("recovered context".repeat(8))],
    );
    let mut runtime = runtime(driver.clone());
    seed_large_history(&mut runtime).await;
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "trigger provider context error".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("context submit: {error}"));
    driver.enable();

    finish_compaction_runtime(&mut runtime).await;
    let calls = driver.agent_calls().await;
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[1].step_id, calls[2].step_id);
    assert_eq!(driver.summary_call_count(), 1);
    assert!(!runtime.agent.is_running());
}

/// CPL-7: provider context pressure fails the one exact-history summary request without changing
/// its input or starting a degraded retry.
#[tokio::test]
async fn summary_context_pressure_does_not_retry_with_changed_input() {
    let driver = CompactionDriver::new(
        [
            Script::Events(vec![
                text_delta(&large_answer()),
                ModelEvent::Stopped(StopReason::EndOfTurn),
            ]),
            Script::Events(vec![ModelEvent::Stopped(StopReason::EndOfTurn)]),
        ],
        [SummaryScript::Fail(CompactionFailure::ContextTooLong)],
    );
    let mut runtime = runtime(driver.clone());
    seed_large_history(&mut runtime).await;
    driver.enable();
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "next".into(),
            },
        )
        .await
        .expect("pressured submit");
    finish_compaction_runtime(&mut runtime).await;

    assert_eq!(driver.summary_call_count(), 1);
    assert_eq!(
        driver.agent_calls().await.len(),
        2,
        "soft failure falls back"
    );
    assert!(runtime.agent.journal().records().iter().all(|record| !matches!(
        record,
        plexmaton_agent::JournalRecord::AppendEntry { entry, .. }
            if matches!(entry.payload, plexmaton_agent::JournalEntryPayload::CompactionCheckpoint { .. })
    )));
}

/// CPL-7: transport and provider failures terminate the operation without another summary request.
#[tokio::test]
async fn non_context_summary_failure_does_not_retry() {
    let driver = CompactionDriver::new(
        [
            Script::Events(vec![
                text_delta(&large_answer()),
                ModelEvent::Stopped(StopReason::EndOfTurn),
            ]),
            Script::Events(vec![ModelEvent::Stopped(StopReason::EndOfTurn)]),
        ],
        [
            SummaryScript::Fail(CompactionFailure::ProviderFailed),
            SummaryScript::Complete("must remain unused".to_owned()),
        ],
    );
    let mut runtime = runtime(driver.clone());
    seed_large_history(&mut runtime).await;
    driver.enable();
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "next".into(),
            },
        )
        .await
        .expect("pressured submit");
    finish_compaction_runtime(&mut runtime).await;

    assert_eq!(driver.summary_call_count(), 1);
    assert_eq!(
        driver.agent_calls().await.len(),
        2,
        "soft fallback runs once"
    );
}

/// CPL-6: a collected summary tool call is only a failed compaction audit; it never enters the
/// agent's admission or execution machinery.
#[tokio::test]
async fn summary_tool_call_failure_never_dispatches_tool_work() {
    let driver = CompactionDriver::new(
        [
            Script::Events(vec![
                text_delta(&large_answer()),
                ModelEvent::Stopped(StopReason::EndOfTurn),
            ]),
            Script::Events(vec![ModelEvent::Stopped(StopReason::EndOfTurn)]),
        ],
        [SummaryScript::Fail(CompactionFailure::ToolCallOutput)],
    );
    let mut runtime = runtime(driver.clone());
    seed_large_history(&mut runtime).await;
    driver.enable();
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "next".into(),
            },
        )
        .await
        .expect("pressured submit");
    finish_compaction_runtime(&mut runtime).await;

    assert!(runtime.tools.is_empty());
    assert_eq!(driver.summary_call_count(), 1);
    assert!(runtime.agent.journal().records().iter().all(|record| !matches!(
        record,
        plexmaton_agent::JournalRecord::AppendEntry { entry, .. }
            if matches!(entry.payload, plexmaton_agent::JournalEntryPayload::ToolCallRequested { .. })
    )));
}

/// CPL-7: the owned deadline cancels and joins the exact summarizer future before soft fallback.
#[tokio::test]
async fn compaction_timeout_cancels_and_joins_before_continuation() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let driver = CompactionDriver::new(
        [
            Script::Events(vec![
                text_delta(&large_answer()),
                ModelEvent::Stopped(StopReason::EndOfTurn),
            ]),
            Script::Events(vec![ModelEvent::Stopped(StopReason::EndOfTurn)]),
        ],
        [SummaryScript::WaitForCancellation(Arc::clone(&cancelled))],
    );
    let mut runtime = runtime(driver.clone());
    seed_large_history(&mut runtime).await;
    runtime.compaction_timeout = std::time::Duration::ZERO;
    driver.enable();
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "next".into(),
            },
        )
        .await
        .expect("pressured submit");
    finish_compaction_runtime(&mut runtime).await;

    assert!(cancelled.load(Ordering::SeqCst));
    assert!(
        runtime
            .agent
            .journal()
            .records()
            .iter()
            .any(|record| matches!(
                record,
                plexmaton_agent::JournalRecord::CompactionAttemptFinished { fact, .. }
                    if fact.outcome().failure() == Some(CompactionFailure::TimedOut)
            ))
    );
}

/// CPL-7/CPL-8: hard pressure after a tool batch cannot fit the exact-history summary request, so
/// it fails visibly without dispatching a summarizer or the oversized next agent step.
#[tokio::test]
async fn post_tool_hard_pressure_preserves_history_and_dispatches_nothing() {
    let tool = ToolCall {
        call_id: plexmaton_core::ToolCallId::new("compact-after-tool")
            .unwrap_or_else(|error| panic!("tool id: {error}")),
        name: "unknown_tool".to_owned(),
        arguments: "{}".to_owned(),
    };
    let driver = CompactionDriver::new(
        [
            Script::Events(vec![
                text_delta(&hard_answer()),
                ModelEvent::Stopped(StopReason::EndOfTurn),
            ]),
            Script::Events(vec![
                ModelEvent::Called {
                    position: ModelOutputPosition::new(0, 0),
                    call: tool,
                },
                ModelEvent::Stopped(StopReason::ToolCalls),
            ]),
            Script::Events(vec![ModelEvent::Stopped(StopReason::EndOfTurn)]),
        ],
        [],
    );
    let mut runtime = runtime(driver.clone());
    seed_large_history(&mut runtime).await;
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "run a tool".to_owned(),
            },
        )
        .await
        .expect("tool turn submit");
    driver.enable();
    finish_compaction_runtime(&mut runtime).await;

    let calls = driver.agent_calls().await;
    assert_eq!(calls.len(), 2);
    assert_eq!(driver.summary_call_count(), 0);
    assert!(!runtime.agent.is_running());
    assert!(runtime.agent.journal().records().iter().all(|record| !matches!(
        record,
        plexmaton_agent::JournalRecord::AppendEntry { entry, .. }
            if matches!(entry.payload, plexmaton_agent::JournalEntryPayload::CompactionCheckpoint { .. })
    )));
}

/// CPL-7: context recovery is refused after any semantic model output because the open Step cannot
/// be safely replayed over already-collected positions.
#[tokio::test]
async fn context_error_after_output_does_not_start_compaction() {
    let driver = CompactionDriver::new(
        [
            Script::Events(vec![
                text_delta(&large_answer()),
                ModelEvent::Stopped(StopReason::EndOfTurn),
            ]),
            Script::OutputThenFail(text_delta("partial"), ModelError::ContextTooLong),
        ],
        [],
    );
    let mut runtime = runtime(driver.clone());
    seed_large_history(&mut runtime).await;
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "continue".into(),
            },
        )
        .await
        .expect("context turn submit");
    driver.enable();
    finish_compaction_runtime(&mut runtime).await;

    assert_eq!(driver.summary_call_count(), 0);
    assert!(!runtime.agent.is_running());
}

/// CPL-7/CPL-8: an unreducible context-error source follows the typed hard-failure continuation
/// instead of leaving a deferred action with no runtime owner.
#[tokio::test]
async fn context_recovery_planning_refusal_settles_the_open_step() {
    let driver = CompactionDriver::new([Script::Fail(ModelError::ContextTooLong)], []);
    let mut runtime = runtime(driver.clone());
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "only one atom".into(),
            },
        )
        .await
        .expect("context turn submit");
    driver.enable();
    finish_compaction_runtime(&mut runtime).await;

    assert_eq!(driver.summary_call_count(), 0);
    assert!(!runtime.agent.is_running());
    assert!(!runtime.has_active_work());
}

/// CPL-7: interrupt cancels and joins the retained summarizer, records its terminal audit, and
/// never starts the held agent request.
#[tokio::test]
async fn interrupt_cancels_and_joins_the_owned_compaction() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let driver = CompactionDriver::new(
        [Script::Events(vec![
            text_delta(&large_answer()),
            ModelEvent::Stopped(StopReason::EndOfTurn),
        ])],
        [SummaryScript::WaitForCancellation(Arc::clone(&cancelled))],
    );
    let mut runtime = runtime(driver.clone());
    seed_large_history(&mut runtime).await;
    driver.enable();
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "continue".into(),
            },
        )
        .await
        .expect("pressured submit");

    runtime
        .submit(agent_id(), Input::Interrupted)
        .await
        .expect("interrupt compaction");

    assert!(cancelled.load(Ordering::SeqCst));
    assert!(runtime.compaction.is_none());
    assert_eq!(driver.agent_calls().await.len(), 1);
    assert!(
        runtime
            .agent
            .journal()
            .records()
            .iter()
            .any(|record| matches!(
                record,
                plexmaton_agent::JournalRecord::CompactionAttemptFinished { fact, .. }
                    if fact.outcome().failure() == Some(CompactionFailure::Cancelled)
            ))
    );
}

/// CPL-7/LIVE-3: shutdown uses the same cancellation path and returns only after the summarizer
/// and journal owner are settled.
#[tokio::test]
async fn shutdown_cancels_and_joins_the_owned_compaction() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let driver = CompactionDriver::new(
        [Script::Events(vec![
            text_delta(&large_answer()),
            ModelEvent::Stopped(StopReason::EndOfTurn),
        ])],
        [SummaryScript::WaitForCancellation(Arc::clone(&cancelled))],
    );
    let mut runtime = runtime(driver.clone());
    seed_large_history(&mut runtime).await;
    driver.enable();
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "continue".into(),
            },
        )
        .await
        .expect("pressured submit");

    runtime.shutdown().await.expect("shutdown compaction");

    assert!(cancelled.load(Ordering::SeqCst));
    assert!(!runtime.has_active_work());
    assert_eq!(driver.agent_calls().await.len(), 1);
}

/// CPL-7/SKL-6: a waiting summarizer cannot hide a finished skill read or retain its failed draft.
#[tokio::test]
async fn skill_preparation_completes_while_compaction_is_waiting() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let driver = CompactionDriver::new(
        [Script::Events(vec![
            text_delta(&large_answer()),
            ModelEvent::Stopped(StopReason::EndOfTurn),
        ])],
        [SummaryScript::WaitForCancellation(Arc::clone(&cancelled))],
    );
    let mut runtime = runtime(driver.clone());
    seed_large_history(&mut runtime).await;
    driver.enable();
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "continue".into(),
            },
        )
        .await
        .expect("compaction");
    assert!(runtime.compaction.is_some());
    runtime
        .submit_skill(
            agent_id(),
            Input::Submitted {
                text: "$missing preserve my draft".into(),
            },
            "missing".into(),
        )
        .await
        .expect("prepare skill during compaction");
    let progress = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            match runtime.next_update().await.expect("runtime update") {
                RuntimeUpdate::Report(report) if !report.undelivered.is_empty() => break report,
                RuntimeUpdate::Event(_) | RuntimeUpdate::Report(_) => {}
                RuntimeUpdate::Finished => panic!("compaction must still be owned"),
            }
        }
    })
    .await;
    let still_running = runtime.compaction.is_some();
    runtime.shutdown().await.expect("cancel and join");
    let report = progress.expect("skill result must not wait for summary completion");
    assert!(still_running);
    assert_eq!(report.undelivered.len(), 1);
    assert_eq!(report.undelivered[0].text, "$missing preserve my draft");
    assert_eq!(report.undelivered[0].skill.as_deref(), Some("missing"));
    assert_eq!(
        report.undelivered[0].reason,
        plexmaton_agent::UndeliveredReason::SkillUnavailable
    );
    assert!(cancelled.load(Ordering::SeqCst));
    assert_eq!(driver.agent_calls().await.len(), 1);
}
