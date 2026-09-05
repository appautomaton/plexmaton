use std::sync::Arc;

use plexmaton_agent::{
    Agent, ApprovalPolicy, Input, JournalEntryPayload, JournalRecord, ModelEvent,
    ModelOutputPosition, RequestAttemptTerminalState, StopReason, ToolCall, TurnBudget,
    TurnFinishedAt, UnixMillis,
};
use plexmaton_core::{HeadName, SessionEvent, SessionId, TokenUsage, ToolCallId};
use plexmaton_session_store::JournalFile;

use super::{
    FakeDriver, Script, agent_id, complete_usage, finish_active, runtime_with_clock, text_delta,
    tools::TestWorkspace, usage_value,
};
use crate::LiveRuntime;
use crate::runtime::clock::FixedWallClock;

/// TIM-1/JRN-3: runtime clocks supply session and turn chronology; reducers read no clock.
#[tokio::test]
async fn runtime_clock_values_reach_session_and_turn_chronology() {
    let driver = FakeDriver::new([Script::Events(vec![ModelEvent::Stopped(
        StopReason::EndOfTurn,
    )])]);
    let observed_at = UnixMillis::new(1_788_000_000_123);
    let mut runtime = runtime_with_clock(driver, Arc::new(FixedWallClock(observed_at)));
    let _announced = runtime.try_next_event();
    assert_eq!(runtime.agent.journal().created_at_unix_ms(), observed_at);

    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "timed".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("submission: {error}"));
    let _events = finish_active(&mut runtime).await;

    assert!(
        runtime
            .agent
            .journal()
            .records()
            .iter()
            .any(|record| matches!(
                record,
                JournalRecord::AppendEntry { entry, .. }
                    if matches!(
                        entry.payload,
                        JournalEntryPayload::TurnStarted {
                            accepted_at,
                            opened_at,
                            ..
                        } if accepted_at == observed_at && opened_at == observed_at
                    )
            ))
    );
    assert!(
        runtime
            .agent
            .journal()
            .records()
            .iter()
            .any(|record| matches!(
                record,
                JournalRecord::TurnFinished { fact, .. }
                    if fact.at == TurnFinishedAt::Observed {
                        completed_at: observed_at
                    }
            ))
    );
}

async fn journal_runtime(workspace: &TestWorkspace, driver: Arc<FakeDriver>) -> LiveRuntime {
    let session_id =
        SessionId::new("request-audit").unwrap_or_else(|error| panic!("session identity: {error}"));
    let file = JournalFile::create(
        workspace.0.join("session.jsonl"),
        session_id,
        UnixMillis::new(100),
    )
    .unwrap_or_else(|error| panic!("create audit journal: {error}"));
    LiveRuntime::with_driver_store_and_clock(
        agent_id(),
        "Plexmaton".to_owned(),
        driver,
        workspace.catalog(),
        file.journal().metadata().clone(),
        Box::new(file),
        Arc::new(FixedWallClock(UnixMillis::new(101))),
    )
    .await
    .unwrap_or_else(|error| panic!("create journaled runtime: {error}"))
}

/// PRV-5/TIM-5/JRN-8: provider failure survives JSONL without becoming transport failure or retry.
#[tokio::test]
async fn provider_failure_reopens_as_the_same_non_retryable_outcome() {
    let workspace = TestWorkspace::new("provider-failure-audit");
    let error = plexmaton_agent::ModelError::ProviderFailed {
        message: "provider returned HTTP 529".into(),
    };
    let driver = FakeDriver::new([Script::Fail(error.clone())]);
    let mut runtime = journal_runtime(&workspace, driver.clone()).await;
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "hello".into(),
            },
        )
        .await
        .expect("submit");
    let events = finish_active(&mut runtime).await;
    assert!(events.iter().any(|envelope| matches!(
        &envelope.event,
        SessionEvent::RuntimeError { message, .. } if message == &error.message()
    )));
    assert!(runtime.retry_candidate().is_none());
    assert_eq!(driver.calls().await.len(), 1);
    let live = runtime.agent.journal().clone();
    runtime.shutdown().await.expect("shutdown");
    drop(runtime);

    let file = JournalFile::open(workspace.0.join("session.jsonl")).expect("reopen");
    assert_eq!(file.journal().records(), live.records());
    let attempts = file.journal().request_attempts().collect::<Vec<_>>();
    assert_eq!(attempts.len(), 1);
    assert!(matches!(
        attempts[0].terminal().map(|terminal| terminal.terminal()),
        Some(RequestAttemptTerminalState::Dispatched {
            outcome: plexmaton_agent::RequestDispatchedOutcome::ProviderFailed,
            ..
        })
    ));
    let resumed = Agent::from_journal(
        agent_id(),
        file.journal().clone(),
        TurnBudget::default(),
        ApprovalPolicy::default(),
    )
    .expect("resume");
    assert!(!resumed.is_running());
    assert!(resumed.retry_candidate().is_none());
}

/// TIM-2/TIM-3/TIM-4/JRN-7: real writer acknowledgement and JSONL reload preserve the exact
/// attempt facts and both projections, without persisting streamed usage as another authority.
#[tokio::test]
async fn request_attempts_reopen_with_identical_accounting_and_context() {
    let workspace = TestWorkspace::new("request-audit");
    let driver = FakeDriver::new([Script::Events(vec![
        text_delta("retained answer"),
        complete_usage(100, 7),
        ModelEvent::Stopped(StopReason::EndOfTurn),
    ])]);
    let mut runtime = journal_runtime(&workspace, driver).await;
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "hello".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("submit durable request: {error}"));
    let events = finish_active(&mut runtime).await;
    let main = HeadName::new("main").unwrap_or_else(|error| panic!("head: {error}"));
    let live = runtime.agent.journal().clone();
    runtime
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown: {error}"));
    drop(runtime);

    let file = JournalFile::open(workspace.0.join("session.jsonl"))
        .unwrap_or_else(|error| panic!("reopen request audit: {error}"));
    assert_eq!(file.journal().records(), live.records());
    let projection = file
        .journal()
        .project(&main)
        .unwrap_or_else(|error| panic!("replay audit: {error:?}"));
    let live_projection = live
        .project(&main)
        .unwrap_or_else(|error| panic!("live projection: {error:?}"));
    assert_eq!(projection.request(), live_projection.request());
    assert_eq!(projection.events(), live_projection.events());
    // Streaming text precedes its canonical completion; accounting retains its own event order.
    for accounting in [false, true] {
        let select = |events: &[plexmaton_core::SessionEventEnvelope]| {
            events
                .iter()
                .filter(|envelope| {
                    matches!(envelope.event, SessionEvent::TurnUsageUpdated { .. }) == accounting
                })
                .map(|envelope| envelope.event.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(select(projection.events()), select(&events));
    }
    let attempts = projection.request_attempts();
    assert_eq!(attempts.len(), 1);
    assert_eq!(
        attempts[0].authorization().authorized_at(),
        UnixMillis::new(101)
    );
    assert!(
        matches!(attempts[0].terminal().map(|terminal| terminal.terminal()),
        Some(RequestAttemptTerminalState::Dispatched { usage, .. })
            if usage == &usage_value(100, 7))
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.event, SessionEvent::TurnUsageUpdated { .. }))
            .count(),
        1
    );
    assert_eq!(
        file.journal()
            .records()
            .iter()
            .filter(|record| matches!(record, JournalRecord::RequestAttemptFinished { .. }))
            .count(),
        1
    );
}

/// TIM-5/JRN-5: losing the runtime after authorization leaves an unknown attempt; recovery never
/// fills in dispatch, usage or timing and never starts a replacement request.
#[tokio::test]
async fn dropped_runtime_reopens_authorization_without_a_fabricated_terminal() {
    let workspace = TestWorkspace::new("orphan-request");
    let driver = FakeDriver::new([
        Script::Events(vec![
            ModelEvent::Called {
                position: ModelOutputPosition::new(0, 0),
                call: ToolCall {
                    call_id: ToolCallId::new("unknown-tool")
                        .unwrap_or_else(|error| panic!("call identity: {error}")),
                    name: "unknown_tool".to_owned(),
                    arguments: "{}".to_owned(),
                },
            },
            complete_usage(100, 7),
            ModelEvent::Stopped(StopReason::ToolCalls),
        ]),
        Script::EndWithoutTerminal,
    ]);
    let mut runtime = journal_runtime(&workspace, Arc::clone(&driver)).await;
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "pending".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("authorize request: {error}"));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while driver.calls().await.len() < 2 {
            runtime
                .next_update()
                .await
                .unwrap_or_else(|error| panic!("drive first request: {error}"));
        }
    })
    .await
    .unwrap_or_else(|_| panic!("second request never authorized"));
    let attempts: Vec<_> = runtime
        .agent
        .journal()
        .request_attempts()
        .cloned()
        .collect();
    assert_eq!(attempts.len(), 2);
    assert!(attempts[0].terminal().is_some());
    assert!(attempts[1].terminal().is_none());
    drop(runtime);

    let file = JournalFile::open(workspace.0.join("session.jsonl"))
        .unwrap_or_else(|error| panic!("reopen orphaned request: {error}"));
    let main = HeadName::new("main").unwrap_or_else(|error| panic!("head: {error}"));
    let projection = file
        .journal()
        .project(&main)
        .unwrap_or_else(|error| panic!("project crash prefix: {error:?}"));
    assert!(
        matches!(projection.events().iter().rev().find_map(|envelope| match &envelope.event {
        SessionEvent::TurnUsageUpdated { usage, .. } => Some(usage), _ => None,
    }), Some(TokenUsage::Partial(counts)) if counts.total == 107)
    );
    let mut agent = Agent::from_journal(
        agent_id(),
        file.journal().clone(),
        TurnBudget::default(),
        ApprovalPolicy::default(),
    )
    .unwrap_or_else(|error| panic!("restore agent: {error:?}"));
    let recovered = agent
        .recover_after_process_death_at(UnixMillis::new(999))
        .unwrap_or_else(|| panic!("orphaned turn needs recovery"));
    assert!(recovered.effects.is_empty());
    assert_eq!(
        agent
            .journal()
            .request_attempts()
            .cloned()
            .collect::<Vec<_>>(),
        attempts
    );
    assert!(
        matches!(recovered.events.iter().find_map(|envelope| match &envelope.event {
        SessionEvent::TurnUsageUpdated { usage, .. } => Some(usage), _ => None,
    }), Some(TokenUsage::Partial(counts)) if counts.total == 107)
    );
    assert_eq!(
        agent
            .journal()
            .records()
            .iter()
            .filter(|record| matches!(record, JournalRecord::RequestAttemptFinished { .. }))
            .count(),
        1
    );
    assert!(
        agent
            .recover_after_process_death_at(UnixMillis::new(1000))
            .is_none()
    );
    assert_eq!(driver.calls().await.len(), 2);
}
