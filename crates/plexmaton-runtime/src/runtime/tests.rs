use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, Ordering},
    },
};

use futures_util::{FutureExt, future::BoxFuture};
use plexmaton_agent::{Input, ModelCall, ModelError, ModelEvent, ModelOutputPosition, StopReason};
use plexmaton_core::{
    AgentId, AgentStatus, SessionEvent, SessionEventEnvelope, TokenCounts, TokenUsage,
};
use tokio::sync::{Mutex, Notify, mpsc};
use tokio_util::sync::CancellationToken;

use super::{LiveRuntime, ModelDriver, ModelSignal, WaitOutcome};
use crate::NativeToolCatalog;

enum Script {
    Events(Vec<ModelEvent>),
    Fail(ModelError),
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
}

impl FakeDriver {
    fn new(scripts: impl IntoIterator<Item = Script>) -> Arc<Self> {
        Arc::new(Self {
            scripts: Arc::new(Mutex::new(scripts.into_iter().collect())),
            calls: Arc::new(StdMutex::new(Vec::new())),
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
    fn drive(
        &self,
        call: ModelCall,
        signals: mpsc::Sender<ModelSignal>,
        cancellation: CancellationToken,
    ) -> BoxFuture<'static, ()> {
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
                    for event in events {
                        let signal = if matches!(event, ModelEvent::Stopped(_)) {
                            ModelSignal::Terminal {
                                step_id: call.step_id.clone(),
                                event,
                            }
                        } else {
                            ModelSignal::Event {
                                step_id: call.step_id.clone(),
                                event,
                            }
                        };
                        if signals.send(signal).await.is_err() {
                            return;
                        }
                    }
                }
                Script::Fail(error) => {
                    let _closed = signals
                        .send(ModelSignal::Failed {
                            step_id: call.step_id,
                            error,
                        })
                        .await;
                }
                Script::WaitForCancellation { started, finished } => {
                    let _closed = signals
                        .send(ModelSignal::Event {
                            step_id: call.step_id,
                            event: text_delta("partial"),
                        })
                        .await;
                    started.notify_one();
                    cancellation.cancelled().await;
                    finished.store(true, Ordering::SeqCst);
                }
                Script::TerminalReady(ready) => {
                    let _closed = signals
                        .send(ModelSignal::Event {
                            step_id: call.step_id.clone(),
                            event: complete_usage(8, 2),
                        })
                        .await;
                    let _closed = signals
                        .send(ModelSignal::Terminal {
                            step_id: call.step_id,
                            event: ModelEvent::Stopped(StopReason::EndOfTurn),
                        })
                        .await;
                    ready.notify_one();
                }
                Script::TerminalThenWaitForCancellation { ready, finished } => {
                    let _closed = signals
                        .send(ModelSignal::Terminal {
                            step_id: call.step_id,
                            event: ModelEvent::Stopped(StopReason::EndOfTurn),
                        })
                        .await;
                    ready.notify_one();
                    cancellation.cancelled().await;
                    finished.store(true, Ordering::SeqCst);
                }
                Script::EndWithoutTerminal => {}
            }
        }
        .boxed()
    }
}

mod cancellation;
mod lifecycle;
mod persistence;
mod presentation;
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
    ModelEvent::Usage(TokenUsage::Complete(TokenCounts {
        input,
        cached_input: Some(0),
        cache_write_input: Some(0),
        output,
        reasoning_output: Some(0),
        total: input + output,
    }))
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

/// LIVE-2 and LIVE-3: when completion is already queued but the user's interrupt is admitted
/// first, cancellation wins once and both late terminal messages become typed non-deliveries.
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
        WaitOutcome::ModelEnded(Ok(()))
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
    assert_eq!(
        interrupted.undelivered_model.len() + late.undelivered_model.len(),
        2
    );
    runtime
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown: {error}"));
}
