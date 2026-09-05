use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, Ordering},
    },
};

use futures_util::{FutureExt, future::BoxFuture};
use plexmaton_agent::{
    DispatchedRequestTiming, ElapsedMillis, Input, ModelCall, ModelDeliveryRefusal, ModelError,
    ModelEvent, ModelOutputPosition, ModelStepId, ProviderCodecId, ProviderCodecRevision,
    ProviderModelFamilyId, ProviderReplayOwnerId, ReplayCompatibility, RequestAttemptId,
    RequestAttemptTerminal, RequestAttemptTerminalState, RequestCost, RequestDispatchedOutcome,
    RequestEnvironment, RequestEnvironmentFingerprint, StopReason, UnixMillis,
};
use plexmaton_core::{
    AgentId, AgentStatus, SessionEvent, SessionEventEnvelope, TokenCounts, TokenUsage,
};
use tokio::sync::{Mutex, Notify, mpsc};
use tokio_util::sync::CancellationToken;

use super::{
    LiveRuntime, ModelCompletion, ModelDriver, ModelOutput, ModelSignal, ModelTerminalReport,
    WaitOutcome,
};
use crate::NativeToolCatalog;

enum Script {
    Events(Vec<ModelEvent>),
    Fail(ModelError),
    OutputThenFail(ModelEvent, ModelError),
    WaitForCancellation {
        started: Arc<Notify>,
        finished: Arc<AtomicBool>,
    },
    TerminalReady(Arc<Notify>),
    TerminalThenWaitForCancellation {
        ready: Arc<Notify>,
        finished: Arc<AtomicBool>,
    },
    EndWithoutTerminal,
}

struct FakeDriver {
    scripts: Arc<Mutex<VecDeque<Script>>>,
    calls: Arc<StdMutex<Vec<ModelCall>>>,
    environment: RequestEnvironment,
}

impl FakeDriver {
    fn new(scripts: impl IntoIterator<Item = Script>) -> Arc<Self> {
        Arc::new(Self {
            scripts: Arc::new(Mutex::new(scripts.into_iter().collect())),
            calls: Arc::new(StdMutex::new(Vec::new())),
            environment: test_request_environment(),
        })
    }

    async fn calls(&self) -> Vec<ModelCall> {
        self.calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl ModelDriver for FakeDriver {
    fn request_environment(&self) -> &RequestEnvironment {
        &self.environment
    }

    fn drive(
        &self,
        attempt_id: RequestAttemptId,
        call: ModelCall,
        signals: mpsc::Sender<ModelSignal>,
        cancellation: CancellationToken,
    ) -> BoxFuture<'static, ModelTerminalReport> {
        let scripts = Arc::clone(&self.scripts);
        self.calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(call.clone());
        async move {
            let script = scripts
                .lock()
                .await
                .pop_front()
                .unwrap_or(Script::EndWithoutTerminal);
            match script {
                Script::Events(events) => {
                    let mut usage = TokenUsage::Unavailable;
                    let mut stop = None;
                    let mut emitted_output = false;
                    for event in events {
                        match event {
                            ModelEvent::Usage(report) => usage = report,
                            ModelEvent::Stopped(reason) => stop = Some(reason),
                            event => {
                                let output = ModelOutput::from_event(event).unwrap_or_else(|_| {
                                    unreachable!("terminal events were matched separately")
                                });
                                emitted_output |= output.is_first_output();
                                if signals
                                    .send(ModelSignal {
                                        attempt_id: attempt_id.clone(),
                                        step_id: call.step_id.clone(),
                                        output,
                                    })
                                    .await
                                    .is_err()
                                {
                                    return cancelled_report(attempt_id, call.step_id, usage);
                                }
                            }
                        }
                    }
                    match stop {
                        Some(reason) => completed_report(
                            attempt_id,
                            call.step_id,
                            reason,
                            usage,
                            emitted_output,
                        ),
                        None => failed_report(
                            attempt_id,
                            call.step_id,
                            ModelError::Transport {
                                message: "provider future ended without terminal output".to_owned(),
                            },
                            usage,
                            emitted_output,
                        ),
                    }
                }
                Script::OutputThenFail(event, error) => {
                    let output =
                        ModelOutput::from_event(event).expect("nonterminal fixture output");
                    signals
                        .send(ModelSignal {
                            attempt_id: attempt_id.clone(),
                            step_id: call.step_id.clone(),
                            output,
                        })
                        .await
                        .expect("runtime retains receiver");
                    failed_report(
                        attempt_id,
                        call.step_id,
                        error,
                        TokenUsage::Unavailable,
                        true,
                    )
                }
                Script::Fail(error) => failed_report(
                    attempt_id,
                    call.step_id,
                    error,
                    TokenUsage::Unavailable,
                    false,
                ),
                Script::WaitForCancellation { started, finished } => {
                    let output = ModelOutput::from_event(text_delta("partial"))
                        .unwrap_or_else(|_| unreachable!("text is nonterminal model output"));
                    let _closed = signals
                        .send(ModelSignal {
                            attempt_id: attempt_id.clone(),
                            step_id: call.step_id.clone(),
                            output,
                        })
                        .await;
                    started.notify_one();
                    cancellation.cancelled().await;
                    finished.store(true, Ordering::SeqCst);
                    cancelled_report(attempt_id, call.step_id, TokenUsage::Unavailable)
                }
                Script::TerminalReady(ready) => {
                    ready.notify_one();
                    completed_report(
                        attempt_id,
                        call.step_id,
                        StopReason::EndOfTurn,
                        usage_value(8, 2),
                        false,
                    )
                }
                Script::TerminalThenWaitForCancellation { ready, finished } => {
                    ready.notify_one();
                    cancellation.cancelled().await;
                    finished.store(true, Ordering::SeqCst);
                    completed_report(
                        attempt_id,
                        call.step_id,
                        StopReason::EndOfTurn,
                        TokenUsage::Unavailable,
                        false,
                    )
                }
                Script::EndWithoutTerminal => failed_report(
                    attempt_id,
                    call.step_id,
                    ModelError::Transport {
                        message: "provider future ended without terminal output".to_owned(),
                    },
                    TokenUsage::Unavailable,
                    false,
                ),
            }
        }
        .boxed()
    }
}

fn test_request_environment() -> RequestEnvironment {
    let compatibility = ReplayCompatibility::new(
        ProviderReplayOwnerId::new("runtime-test")
            .unwrap_or_else(|error| panic!("provider owner: {error:?}")),
        ProviderCodecId::new("runtime-test").unwrap_or_else(|error| panic!("codec id: {error:?}")),
        ProviderCodecRevision::new(1).unwrap_or_else(|error| panic!("codec revision: {error:?}")),
        ProviderModelFamilyId::new("runtime-test")
            .unwrap_or_else(|error| panic!("model family: {error:?}")),
    );
    RequestEnvironment::new(compatibility, RequestEnvironmentFingerprint::new([0; 32]))
}

fn request_timing(emitted_output: bool) -> DispatchedRequestTiming {
    DispatchedRequestTiming::new(
        UnixMillis::EPOCH,
        Some(ElapsedMillis::new(0)),
        emitted_output.then(|| ElapsedMillis::new(0)),
        ElapsedMillis::new(0),
    )
    .unwrap_or_else(|error| panic!("request timing: {error}"))
}

fn terminal_report(
    attempt_id: RequestAttemptId,
    step_id: ModelStepId,
    outcome: RequestDispatchedOutcome,
    usage: TokenUsage,
    completion: ModelCompletion,
    emitted_output: bool,
) -> ModelTerminalReport {
    let terminal = RequestAttemptTerminal::new(
        attempt_id,
        RequestAttemptTerminalState::Dispatched {
            timing: request_timing(emitted_output),
            outcome,
            usage,
            cost: RequestCost::Unavailable,
        },
    )
    .unwrap_or_else(|error| panic!("request terminal: {error}"));
    ModelTerminalReport::new(step_id, terminal, completion)
}

fn completed_report(
    attempt_id: RequestAttemptId,
    step_id: ModelStepId,
    reason: StopReason,
    usage: TokenUsage,
    emitted_output: bool,
) -> ModelTerminalReport {
    terminal_report(
        attempt_id,
        step_id,
        RequestDispatchedOutcome::Completed {
            stop_reason: reason,
        },
        usage,
        ModelCompletion::Stopped(reason),
        emitted_output,
    )
}

fn failed_report(
    attempt_id: RequestAttemptId,
    step_id: ModelStepId,
    error: ModelError,
    usage: TokenUsage,
    emitted_output: bool,
) -> ModelTerminalReport {
    let outcome = match &error {
        ModelError::Transport { .. } => RequestDispatchedOutcome::TransportFailed,
        ModelError::RateLimited { .. } => RequestDispatchedOutcome::RateLimited,
        ModelError::ContextTooLong => RequestDispatchedOutcome::ContextTooLong,
        ModelError::Malformed { .. } => RequestDispatchedOutcome::Malformed,
    };
    terminal_report(
        attempt_id,
        step_id,
        outcome,
        usage,
        ModelCompletion::Failed(error),
        emitted_output,
    )
}

fn cancelled_report(
    attempt_id: RequestAttemptId,
    step_id: ModelStepId,
    usage: TokenUsage,
) -> ModelTerminalReport {
    terminal_report(
        attempt_id,
        step_id,
        RequestDispatchedOutcome::Cancelled,
        usage,
        ModelCompletion::Cancelled,
        true,
    )
}

mod cancellation;
mod lifecycle;
mod persistence;
mod presentation;
mod retry;
mod timing;
mod tools;

fn agent_id() -> AgentId {
    AgentId::new("agent-live").unwrap_or_else(|error| panic!("fixture agent: {error}"))
}

fn runtime(driver: Arc<dyn ModelDriver>) -> LiveRuntime {
    runtime_with_clock(
        driver,
        Arc::new(
            super::clock::SystemWallClock::new()
                .unwrap_or_else(|error| panic!("test wall clock: {error}")),
        ),
    )
}

pub(super) fn runtime_with_clock(
    driver: Arc<dyn ModelDriver>,
    clock: Arc<dyn super::clock::WallClock>,
) -> LiveRuntime {
    let workspace =
        std::env::current_dir().unwrap_or_else(|error| panic!("resolve test workspace: {error}"));
    let tools = NativeToolCatalog::open(
        &workspace,
        "TEST_KEY",
        "/bin/false",
        "/bin/false",
        Vec::new(),
    )
    .unwrap_or_else(|error| panic!("open test tool catalog: {error}"));
    LiveRuntime::with_driver_and_clock(agent_id(), "Plexmaton".to_owned(), driver, tools, clock)
}

fn complete_usage(input: u64, output: u64) -> ModelEvent {
    ModelEvent::Usage(usage_value(input, output))
}

fn usage_value(input: u64, output: u64) -> TokenUsage {
    TokenUsage::Complete(TokenCounts {
        input,
        cached_input: Some(0),
        cache_write_input: Some(0),
        output,
        reasoning_output: Some(0),
        total: input + output,
    })
}

fn text_delta(delta: &str) -> ModelEvent {
    ModelEvent::TextDelta {
        position: ModelOutputPosition::new(0, 0),
        delta: delta.to_owned(),
    }
}

fn take_ready(runtime: &mut LiveRuntime, events: &mut Vec<SessionEventEnvelope>) {
    while let Some(event) = runtime.try_next_event() {
        events.push(event);
    }
}

async fn finish_active(runtime: &mut LiveRuntime) -> Vec<SessionEventEnvelope> {
    let mut events = Vec::new();
    take_ready(runtime, &mut events);
    while runtime.has_active_work() {
        if let Some(event) = runtime
            .next_event()
            .await
            .unwrap_or_else(|error| panic!("runtime event: {error}"))
        {
            events.push(event);
        }
        take_ready(runtime, &mut events);
    }
    events
}

/// LIVE-1 and LIVE-4: one runtime drives sequential real loop turns and publishes the provider's
/// checked per-turn usage beside their streamed semantic text.
#[tokio::test]
async fn sequential_turns_stream_and_report_their_own_usage() {
    let driver = FakeDriver::new([
        Script::Events(vec![
            text_delta("first answer"),
            complete_usage(10, 3),
            ModelEvent::Stopped(StopReason::EndOfTurn),
        ]),
        Script::Events(vec![
            text_delta("second answer"),
            complete_usage(20, 4),
            ModelEvent::Stopped(StopReason::EndOfTurn),
        ]),
    ]);
    let mut runtime = runtime(driver);
    let _announced = runtime.try_next_event();

    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "first".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("first submission: {error}"));
    let first = finish_active(&mut runtime).await;
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "second".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("second submission: {error}"));
    let second = finish_active(&mut runtime).await;

    assert!(first.iter().any(|envelope| matches!(
        &envelope.event,
        SessionEvent::TranscriptDelta { text, .. } if text == "first answer"
    )));
    assert!(second.iter().any(|envelope| matches!(
        &envelope.event,
        SessionEvent::TranscriptDelta { text, .. } if text == "second answer"
    )));
    assert!(first.iter().any(|envelope| matches!(
        &envelope.event,
        SessionEvent::TurnUsageUpdated {
            usage: TokenUsage::Complete(counts),
            ..
        } if counts.total == 13
    )));
    assert!(second.iter().any(|envelope| matches!(
        &envelope.event,
        SessionEvent::TurnUsageUpdated {
            usage: TokenUsage::Complete(counts),
            ..
        } if counts.total == 24
    )));
}

/// TIM-3/LIVE-2: a stale retry cannot deliver output through a current step identity.
#[tokio::test]
async fn model_output_requires_both_the_active_attempt_and_step() {
    let mut runtime = runtime(FakeDriver::new([Script::EndWithoutTerminal]));
    let _announced = runtime.try_next_event();
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "begin".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("submission: {error}"));
    let active = runtime
        .active
        .as_ref()
        .unwrap_or_else(|| panic!("model owner was not started"));
    let expected = active.attempt_id.clone();
    let step_id = active.step_id.clone();
    let received = RequestAttemptId::new("stale-attempt")
        .unwrap_or_else(|error| panic!("stale attempt fixture: {error}"));
    let output = ModelOutput::from_event(text_delta("must not enter the transcript"))
        .unwrap_or_else(|_| panic!("text is a nonterminal output"));

    runtime
        .apply_signal(ModelSignal {
            attempt_id: received.clone(),
            step_id,
            output,
        })
        .await
        .unwrap_or_else(|error| panic!("refuse stale output: {error}"));
    let report = runtime.take_report();

    assert!(matches!(
        report.undelivered_model.as_slice(),
        [plexmaton_agent::UndeliveredModelInput {
            reason: ModelDeliveryRefusal::WrongAttempt {
                expected: actual_expected,
                received: actual_received,
            },
            ..
        }] if actual_expected == &expected && actual_received == &received
    ));
    assert!(runtime.pending.iter().all(|event| !matches!(
        &event.event,
        SessionEvent::TranscriptDelta { text, .. } if text == "must not enter the transcript"
    )));
    runtime
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown stale-output fixture: {error}"));
}

/// LIVE-3 and LIVE-5: both stop paths settle semantic state, then cancellation joins the exact
/// in-flight task; absent terminal usage is visible as unavailable rather than zero.
#[tokio::test]
async fn interrupt_and_shutdown_cancel_and_join_the_exact_provider_task() {
    for shutdown in [false, true] {
        let started = Arc::new(Notify::new());
        let finished = Arc::new(AtomicBool::new(false));
        let driver = FakeDriver::new([Script::WaitForCancellation {
            started: Arc::clone(&started),
            finished: Arc::clone(&finished),
        }]);
        let mut runtime = runtime(driver);
        let _announced = runtime.try_next_event();
        runtime
            .submit(
                agent_id(),
                Input::Submitted {
                    text: "begin".to_owned(),
                },
            )
            .await
            .unwrap_or_else(|error| panic!("submission: {error}"));
        let mut events = Vec::new();
        take_ready(&mut runtime, &mut events);
        let first_stream = runtime
            .next_event()
            .await
            .unwrap_or_else(|error| panic!("first stream event: {error}"));
        events.extend(first_stream);
        take_ready(&mut runtime, &mut events);
        started.notified().await;

        if shutdown {
            runtime
                .shutdown()
                .await
                .unwrap_or_else(|error| panic!("shutdown: {error}"));
        } else {
            runtime
                .submit(agent_id(), Input::Interrupted)
                .await
                .unwrap_or_else(|error| panic!("interrupt: {error}"));
        }
        take_ready(&mut runtime, &mut events);

        assert!(!runtime.has_active_model());
        assert!(
            finished.load(Ordering::SeqCst),
            "the provider future settled before either stop path returned"
        );
        assert!(events.iter().any(|envelope| matches!(
            envelope.event,
            SessionEvent::TurnUsageUpdated {
                usage: TokenUsage::Unavailable,
                ..
            }
        )));
        assert!(events.iter().any(|envelope| matches!(
            envelope.event,
            SessionEvent::AgentStatusChanged {
                status: AgentStatus::Idle,
                ..
            }
        )));
    }
}

/// LIVE-2 and LIVE-3: when completion is ready but the user's interrupt is admitted first,
/// cancellation wins once and the retained terminal report cannot enter a later turn.
#[tokio::test]
async fn cancellation_wins_a_queued_completion_race_without_touching_a_later_turn() {
    let ready = Arc::new(Notify::new());
    let mut runtime = runtime(FakeDriver::new([
        Script::TerminalReady(Arc::clone(&ready)),
        Script::EndWithoutTerminal,
    ]));
    let _announced = runtime.try_next_event();
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "begin".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("submission: {error}"));
    assert!(matches!(
        runtime.wait_for_work().await,
        WaitOutcome::ModelEnded(Ok(_))
    ));
    assert!(ready.notified().now_or_never().is_some());

    let interrupted = runtime
        .submit(agent_id(), Input::Interrupted)
        .await
        .unwrap_or_else(|error| panic!("interrupt: {error}"));
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "later turn".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("later submission: {error}"));
    let pending_before = runtime.pending.len();
    while let Ok(signal) = runtime.signal_rx.try_recv() {
        runtime
            .apply_signal(signal)
            .await
            .unwrap_or_else(|error| panic!("apply late signal: {error}"));
    }
    let late = runtime.take_report();

    assert!(runtime.has_active_model());
    assert_eq!(runtime.pending.len(), pending_before);
    assert!(matches!(
        interrupted.undelivered_model.as_slice(),
        [plexmaton_agent::UndeliveredModelInput {
            reason: ModelDeliveryRefusal::NoActiveStep,
            ..
        }]
    ));
    assert!(late.undelivered_model.is_empty());
    runtime
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown: {error}"));
}
