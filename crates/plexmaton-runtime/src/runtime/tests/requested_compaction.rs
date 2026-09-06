//! CPL-9: a compaction the user asked for, admitted only while idle, with no step to continue.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use plexmaton_agent::{
    CompactionFailure, ContextAtomValue, Input, ModelEvent, StopReason, UndeliveredReason,
};
use plexmaton_core::{ApprovalDecision, ConversationEvent};
use tokio::sync::Notify;

use super::{
    Script, agent_id,
    compaction::{CompactionDriver, SummaryScript, large_answer},
    complete_usage, finish_active, runtime, text_delta,
    tools::{TestWorkspace, called, next_approval},
};
use crate::{
    CompactionRequest, CompactionRequestRefusal, DispatchReport, LiveRuntime,
    RequestedCompactionOutcome, RuntimeUpdate,
};

fn large_turn() -> Script {
    Script::Events(vec![
        text_delta(&large_answer()),
        ModelEvent::Stopped(StopReason::EndOfTurn),
    ])
}

/// One completed turn, then the budget is switched on so a request can be planned.
async fn seeded(driver: &Arc<CompactionDriver>) -> LiveRuntime {
    let mut runtime = runtime(driver.clone());
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "seed".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("seed submit: {error}"));
    let _events = finish_active(&mut runtime).await;
    driver.enable();
    runtime
}

async fn start(runtime: &mut LiveRuntime) -> plexmaton_agent::CompactionId {
    match runtime
        .request_compaction(agent_id())
        .await
        .unwrap_or_else(|error| panic!("request compaction: {error}"))
    {
        CompactionRequest::Started { id } => id,
        CompactionRequest::Refused(refusal) => panic!("idle request was refused: {refusal:?}"),
    }
}

async fn refusal(runtime: &mut LiveRuntime) -> CompactionRequestRefusal {
    match runtime
        .request_compaction(agent_id())
        .await
        .unwrap_or_else(|error| panic!("request compaction: {error}"))
    {
        CompactionRequest::Refused(refusal) => refusal,
        CompactionRequest::Started { id } => panic!("busy request started {id:?}"),
    }
}

/// Drives the runtime until nothing is owned, keeping every event and non-event report.
async fn settle(runtime: &mut LiveRuntime) -> (Vec<ConversationEvent>, Vec<DispatchReport>) {
    let mut events = Vec::new();
    let mut reports = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        while runtime.has_active_work() {
            match runtime
                .next_update()
                .await
                .unwrap_or_else(|error| panic!("runtime update: {error}"))
            {
                RuntimeUpdate::Event(event) => events.push(event.event),
                RuntimeUpdate::Report(report) => reports.push(report),
                RuntimeUpdate::Finished => break,
            }
        }
        while let Some(event) = runtime.try_next_event() {
            events.push(event.event);
        }
        let report = runtime.take_report();
        if !report.is_empty() {
            reports.push(report);
        }
    })
    .await
    .unwrap_or_else(|_| panic!("requested compaction did not settle"));
    (events, reports)
}

fn outcome(reports: &[DispatchReport]) -> Option<&RequestedCompactionOutcome> {
    reports
        .iter()
        .find_map(|report| report.requested_compaction.as_ref())
}

fn has_checkpoint(runtime: &LiveRuntime) -> bool {
    runtime.agent.journal().records().iter().any(|record| {
        matches!(
            record,
            plexmaton_agent::JournalRecord::AppendEntry { entry, .. }
                if matches!(entry.payload, plexmaton_agent::JournalEntryPayload::CompactionCheckpoint { .. })
        )
    })
}

fn attempt_failed_with(runtime: &LiveRuntime, kind: CompactionFailure) -> bool {
    runtime.agent.journal().records().iter().any(|record| {
        matches!(
            record,
            plexmaton_agent::JournalRecord::CompactionAttemptFinished { fact, .. }
                if fact.outcome().failure() == Some(kind)
        )
    })
}

/// CPL-9: an idle request owns one summarizer attempt, publishes a checkpoint, dispatches no
/// model step, and the next turn's request starts from the summary.
#[tokio::test]
async fn cpl_9_requested_compaction_publishes_a_checkpoint_and_dispatches_no_step() {
    let driver = CompactionDriver::new(
        [
            large_turn(),
            Script::Events(vec![ModelEvent::Stopped(StopReason::EndOfTurn)]),
        ],
        [SummaryScript::Complete("compact facts".repeat(8))],
    );
    let mut runtime = seeded(&driver).await;

    let id = start(&mut runtime).await;
    assert!(runtime.compaction.is_some());
    assert!(!runtime.agent.is_running());

    let (events, reports) = settle(&mut runtime).await;
    assert_eq!(
        outcome(&reports),
        Some(&RequestedCompactionOutcome::Published { id })
    );
    assert_eq!(driver.summary_call_count(), 1);
    assert_eq!(
        driver.agent_calls().await.len(),
        1,
        "no model step was dispatched"
    );
    assert!(has_checkpoint(&runtime));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ConversationEvent::RuntimeError { .. }))
    );

    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "next".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("next turn: {error}"));
    let _events = finish_active(&mut runtime).await;
    let calls = driver.agent_calls().await;
    assert_eq!(calls.len(), 2);
    assert!(matches!(
        calls[1].request.atoms.first().map(|atom| atom.value()),
        Some(ContextAtomValue::CompactionSummary { .. })
    ));
}

/// CPL-9: a running step refuses the request by name, and the refusal plans nothing.
#[tokio::test]
async fn cpl_9_a_running_step_refuses_the_request() {
    let started = Arc::new(Notify::new());
    let finished = Arc::new(AtomicBool::new(false));
    let driver = CompactionDriver::new(
        [
            Script::Events(vec![
                text_delta("short"),
                ModelEvent::Stopped(StopReason::EndOfTurn),
            ]),
            Script::WaitForCancellation {
                started: Arc::clone(&started),
                finished: Arc::clone(&finished),
            },
        ],
        [],
    );
    let mut runtime = seeded(&driver).await;
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "hold the model".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("held turn: {error}"));
    // The provider future advances only while the runtime is polled; wait for its first output.
    while runtime.try_next_event().is_some() {}
    let _delta = tokio::time::timeout(Duration::from_secs(5), runtime.next_event())
        .await
        .unwrap_or_else(|_| panic!("held model produced no output"))
        .unwrap_or_else(|error| panic!("held model event: {error}"));
    started.notified().await;

    assert_eq!(
        refusal(&mut runtime).await,
        CompactionRequestRefusal::TurnActive
    );

    runtime
        .submit(agent_id(), Input::Interrupted)
        .await
        .unwrap_or_else(|error| panic!("interrupt held turn: {error}"));
    let _events = finish_active(&mut runtime).await;
    assert!(finished.load(Ordering::SeqCst));
    assert_eq!(driver.summary_call_count(), 0);
    assert_eq!(driver.agent_calls().await.len(), 2);
}

/// CPL-9: an owned compaction and a begun shutdown each refuse by name; neither refusal plans
/// a second attempt or dispatches a step.
#[tokio::test]
async fn cpl_9_an_owned_compaction_and_shutdown_refuse_the_request() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let driver = CompactionDriver::new(
        [large_turn()],
        [SummaryScript::WaitForCancellation(Arc::clone(&cancelled))],
    );
    let mut runtime = seeded(&driver).await;
    let _id = start(&mut runtime).await;

    assert_eq!(
        refusal(&mut runtime).await,
        CompactionRequestRefusal::CompactionActive
    );
    assert_eq!(
        driver.summary_call_count(),
        1,
        "the refusal planned nothing"
    );

    runtime
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown: {error}"));
    assert!(cancelled.load(Ordering::SeqCst));
    assert_eq!(
        refusal(&mut runtime).await,
        CompactionRequestRefusal::ShuttingDown
    );
    assert_eq!(driver.agent_calls().await.len(), 1);
}

/// CPL-9: a pending approval is named before the turn that owns it.
#[tokio::test]
async fn cpl_9_a_waiting_approval_refuses_the_request() {
    let workspace = TestWorkspace::new("requested-compaction-approval");
    let driver = CompactionDriver::new(
        [
            Script::Events(vec![
                called(
                    0,
                    "command-1",
                    "exec_command",
                    serde_json::json!({
                        "cmd":"printf 'ran' > should-not-exist",
                        "timeout_ms":5000
                    }),
                ),
                complete_usage(8, 2),
                ModelEvent::Stopped(StopReason::ToolCalls),
            ]),
            Script::Events(vec![ModelEvent::Stopped(StopReason::EndOfTurn)]),
        ],
        [],
    );
    let mut runtime = LiveRuntime::with_driver(
        agent_id(),
        "Plexmaton".to_owned(),
        driver.clone(),
        workspace.catalog(),
    )
    .unwrap_or_else(|error| panic!("construct runtime: {error}"));
    driver.enable();
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "run a command".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("tool turn: {error}"));
    let approval_id = next_approval(&mut runtime).await;

    assert_eq!(
        refusal(&mut runtime).await,
        CompactionRequestRefusal::ApprovalPending
    );

    runtime
        .submit(
            agent_id(),
            Input::ApprovalDecided {
                approval_id,
                decision: ApprovalDecision::Deny,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("deny: {error}"));
    let _events = finish_active(&mut runtime).await;
    assert_eq!(driver.summary_call_count(), 0);
    assert!(!workspace.0.join("should-not-exist").exists());
}

/// CPL-3/CPL-9: without a budget nothing can be planned; an empty history has nothing to
/// compact; and a summary that would not shrink a tiny request is refused at publication as
/// `NoProgress`. The refusals write no record.
#[tokio::test]
async fn cpl_9_planning_refusals_are_typed_and_write_nothing() {
    let empty = CompactionDriver::new([], []);
    let mut fresh = runtime(empty.clone());
    empty.enable();
    assert_eq!(
        refusal(&mut fresh).await,
        CompactionRequestRefusal::NothingToCompact
    );
    assert!(!fresh.has_active_work());

    let driver = CompactionDriver::new(
        [Script::Events(vec![
            text_delta("short"),
            ModelEvent::Stopped(StopReason::EndOfTurn),
        ])],
        [SummaryScript::Complete(
            "a summary no shorter than the history".to_owned(),
        )],
    );
    let mut runtime = runtime(driver.clone());
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "seed".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("seed submit: {error}"));
    let _events = finish_active(&mut runtime).await;
    let records = runtime.agent.journal().records().len();

    assert_eq!(
        refusal(&mut runtime).await,
        CompactionRequestRefusal::BudgetUnavailable
    );
    assert_eq!(runtime.agent.journal().records().len(), records);
    assert_eq!(driver.summary_call_count(), 0);

    driver.enable();
    let id = start(&mut runtime).await;
    let (_events, reports) = settle(&mut runtime).await;
    assert_eq!(
        outcome(&reports),
        Some(&RequestedCompactionOutcome::Failed {
            id,
            kind: CompactionFailure::NoProgress,
        })
    );
    assert!(!has_checkpoint(&runtime));
    assert_eq!(driver.agent_calls().await.len(), 1);
    assert!(!runtime.has_active_work());
}

/// CPL-9: interrupt cancels and joins the requested summarizer, records the cancelled attempt,
/// reports it, and dispatches no model step.
#[tokio::test]
async fn cpl_9_interrupt_cancels_a_requested_compaction_and_reports_it() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let driver = CompactionDriver::new(
        [large_turn()],
        [SummaryScript::WaitForCancellation(Arc::clone(&cancelled))],
    );
    let mut runtime = seeded(&driver).await;
    let id = start(&mut runtime).await;
    let held = runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "typed while compacting".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("submit during compaction: {error}"));
    assert!(held.undelivered.is_empty());

    let report = runtime
        .submit(agent_id(), Input::Interrupted)
        .await
        .unwrap_or_else(|error| panic!("interrupt compaction: {error}"));

    assert_eq!(
        report.undelivered.len(),
        1,
        "held text comes back with the interrupt"
    );
    assert_eq!(report.undelivered[0].text, "typed while compacting");
    assert_eq!(report.undelivered[0].reason, UndeliveredReason::Interrupted);
    assert!(cancelled.load(Ordering::SeqCst));
    assert!(runtime.compaction.is_none());
    assert_eq!(
        report.requested_compaction,
        Some(RequestedCompactionOutcome::Failed {
            id,
            kind: CompactionFailure::Cancelled,
        })
    );
    assert!(attempt_failed_with(&runtime, CompactionFailure::Cancelled));
    assert!(!has_checkpoint(&runtime));
    assert_eq!(driver.agent_calls().await.len(), 1);
    let (_events, _reports) = settle(&mut runtime).await;
    assert!(!runtime.has_active_work());
}

/// CPL-9/LIVE-3: shutdown cancels and joins the requested summarizer and returns only after it
/// and the journal owner are settled, with the outcome in the shutdown report.
#[tokio::test]
async fn cpl_9_shutdown_cancels_a_requested_compaction_and_dispatches_nothing() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let driver = CompactionDriver::new(
        [large_turn()],
        [SummaryScript::WaitForCancellation(Arc::clone(&cancelled))],
    );
    let mut runtime = seeded(&driver).await;
    let id = start(&mut runtime).await;
    let held = runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "typed while compacting".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("submit during compaction: {error}"));
    assert!(held.undelivered.is_empty());

    let report = runtime
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown: {error}"));

    assert_eq!(
        report.undelivered.len(),
        1,
        "held text comes back with shutdown"
    );
    assert_eq!(report.undelivered[0].text, "typed while compacting");
    assert_eq!(report.undelivered[0].reason, UndeliveredReason::Shutdown);
    assert!(cancelled.load(Ordering::SeqCst));
    assert!(!runtime.has_active_work());
    assert_eq!(
        report.requested_compaction,
        Some(RequestedCompactionOutcome::Failed {
            id,
            kind: CompactionFailure::Cancelled,
        })
    );
    assert!(!has_checkpoint(&runtime));
    assert_eq!(driver.agent_calls().await.len(), 1);
}

/// CPL-9: text typed while a requested compaction runs waits behind it, never reaches the
/// frozen summary request, and opens its turn from the checkpoint once it lands.
#[tokio::test]
async fn cpl_9_text_during_a_requested_compaction_waits_for_the_checkpoint() {
    let driver = CompactionDriver::new(
        [
            large_turn(),
            Script::Events(vec![ModelEvent::Stopped(StopReason::EndOfTurn)]),
        ],
        [SummaryScript::Complete("compact facts".repeat(8))],
    );
    let mut runtime = seeded(&driver).await;
    let id = start(&mut runtime).await;

    let held = runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "typed while compacting".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("submit during compaction: {error}"));
    assert!(held.undelivered.is_empty(), "text waits behind the request");
    assert!(runtime.compaction.is_some());
    assert!(
        !runtime.agent.is_running(),
        "the turn waits for the checkpoint"
    );
    assert_eq!(runtime.pending_inputs.len(), 1);

    let (_events, reports) = settle(&mut runtime).await;
    assert_eq!(
        outcome(&reports),
        Some(&RequestedCompactionOutcome::Published { id })
    );
    let calls = driver.agent_calls().await;
    assert_eq!(
        calls.len(),
        2,
        "the held text opened one turn after the checkpoint"
    );
    let atoms = &calls[1].request.atoms;
    assert!(matches!(
        atoms.first().map(|atom| atom.value()),
        Some(ContextAtomValue::CompactionSummary { .. })
    ));
    assert!(matches!(
        atoms.last().map(|atom| atom.value()),
        Some(ContextAtomValue::User { text }) if text == "typed while compacting"
    ));
    assert!(!runtime.has_active_work());
}

/// CPL-8/CPL-9: a failed or timed-out requested attempt reports its kind, shows it in the
/// conversation, publishes nothing and leaves the head where it was.
#[tokio::test]
async fn cpl_9_failed_and_timed_out_requests_report_their_kind_and_keep_the_head() {
    for (script, kind, timeout) in [
        (
            SummaryScript::Fail(CompactionFailure::ProviderFailed),
            CompactionFailure::ProviderFailed,
            crate::runtime::compaction::DEFAULT_COMPACTION_TIMEOUT,
        ),
        (
            SummaryScript::WaitForCancellation(Arc::new(AtomicBool::new(false))),
            CompactionFailure::TimedOut,
            Duration::ZERO,
        ),
    ] {
        let driver = CompactionDriver::new([large_turn()], [script]);
        let mut runtime = seeded(&driver).await;
        runtime.compaction_timeout = timeout;
        let head = runtime.agent.selected_head().clone();
        let revision = runtime
            .agent
            .journal()
            .head_revision(&head)
            .unwrap_or_else(|error| panic!("revision: {error:?}"));

        let id = start(&mut runtime).await;
        let (events, reports) = settle(&mut runtime).await;

        assert_eq!(
            outcome(&reports),
            Some(&RequestedCompactionOutcome::Failed { id, kind })
        );
        assert!(events.iter().any(|event| matches!(
            event,
            ConversationEvent::RuntimeError { message, .. } if *message == kind.to_string()
        )));
        assert!(attempt_failed_with(&runtime, kind));
        assert!(!has_checkpoint(&runtime));
        assert_eq!(runtime.agent.journal().head_revision(&head), Ok(revision));
        assert_eq!(driver.agent_calls().await.len(), 1);
        assert!(!runtime.has_active_work());
    }
}
