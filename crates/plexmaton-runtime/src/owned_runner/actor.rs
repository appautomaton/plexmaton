//! Owned child actor loop and cooperative cleanup.

use std::panic::{AssertUnwindSafe, resume_unwind};

use futures_util::FutureExt as _;

use super::*;
use crate::RuntimeUpdate;

pub(super) async fn run_owned_child(
    mut runtime: LiveRuntime,
    identity: RunnerIdentity,
    normal: mpsc::Receiver<NormalCommand>,
    control: mpsc::Receiver<ControlCommand>,
    query: mpsc::Receiver<QueryCommand>,
    updates: mpsc::Sender<OwnedRunnerUpdate>,
) {
    let result = AssertUnwindSafe(run_owned_child_loop(
        &mut runtime,
        identity,
        normal,
        control,
        query,
        updates,
    ))
    .catch_unwind()
    .await;
    if let Err(payload) = result {
        let _cleanup = runtime.shutdown().await;
        resume_unwind(payload);
    }
}

async fn run_owned_child_loop(
    runtime: &mut LiveRuntime,
    identity: RunnerIdentity,
    mut normal: mpsc::Receiver<NormalCommand>,
    mut control: mpsc::Receiver<ControlCommand>,
    mut query: mpsc::Receiver<QueryCommand>,
    updates: mpsc::Sender<OwnedRunnerUpdate>,
) {
    let mut pending_update = None;
    loop {
        if let Some(update) = pending_update.take() {
            match flush_pending_update(
                runtime,
                &mut normal,
                &mut control,
                &mut query,
                &updates,
                update,
            )
            .await
            {
                PendingUpdateAction::Retain(update) => pending_update = Some(update),
                PendingUpdateAction::Delivered => {}
                PendingUpdateAction::Close { update, reply } => {
                    close_owned_child(runtime, identity, normal, updates, Some(update), reply)
                        .await;
                    return;
                }
                PendingUpdateAction::ReceiverClosed => {
                    let _shutdown = runtime.shutdown().await;
                    return;
                }
            }
            continue;
        }

        let accept_normal = !runtime.has_active_work();
        let next_normal = async {
            if accept_normal {
                normal.recv().await
            } else {
                std::future::pending().await
            }
        };

        tokio::select! {
            biased;
            command = control.recv() => {
                if let ControlAction::Close(reply) =
                    handle_control(command, runtime, &mut normal).await
                {
                    close_owned_child(runtime, identity, normal, updates, None, reply).await;
                    return;
                }
            }
            command = query.recv() => {
                handle_query(command, runtime);
            }
            update = runtime.next_update() => {
                match update {
                    Ok(update) => {
                        let finished = matches!(update, RuntimeUpdate::Finished);
                        pending_update = Some(OwnedRunnerUpdate::Runtime {
                            identity: identity.clone(),
                            update: Box::new(update),
                        });
                        if finished {
                            close_owned_child(
                                runtime,
                                identity,
                                normal,
                                updates,
                                pending_update.take(),
                                None,
                            ).await;
                            return;
                        }
                    }
                    Err(error) => {
                        close_owned_child(
                            runtime,
                            identity.clone(),
                            normal,
                            updates,
                            Some(OwnedRunnerUpdate::Failed { identity, error }),
                            None,
                        ).await;
                        return;
                    }
                }
            }
            command = next_normal => {
                match command {
                    Some(NormalCommand::Start { execution, reply }) => {
                        let (reservation, ticket, resolved) = execution.into_parts();
                        let result = runtime.start_delegated_turn(reservation, ticket, resolved).await;
                        let _caller_gone = reply.send(result).is_err();
                    }
                    Some(NormalCommand::WakeSnapshot { hint }) => {
                        pending_update = Some(match runtime.collaboration_boundary(hint.turn().clone()) {
                            Ok((boundary, previous)) => OwnedRunnerUpdate::WakeReady {
                                identity: identity.clone(),
                                hint,
                                boundary: Box::new(boundary),
                                previous,
                            },
                            Err(error) => OwnedRunnerUpdate::WakeFailed {
                                identity: identity.clone(),
                                hint,
                                error,
                            },
                        });
                    }
                    None => {
                        close_owned_child(runtime, identity, normal, updates, None, None).await;
                        return;
                    }
                }
            }
        }
    }
}

enum PendingUpdateAction {
    Retain(OwnedRunnerUpdate),
    Delivered,
    Close {
        update: OwnedRunnerUpdate,
        reply: Option<oneshot::Sender<Result<DispatchReport, RuntimeError>>>,
    },
    ReceiverClosed,
}

async fn flush_pending_update(
    runtime: &mut LiveRuntime,
    normal: &mut mpsc::Receiver<NormalCommand>,
    control: &mut mpsc::Receiver<ControlCommand>,
    query: &mut mpsc::Receiver<QueryCommand>,
    updates: &mpsc::Sender<OwnedRunnerUpdate>,
    update: OwnedRunnerUpdate,
) -> PendingUpdateAction {
    tokio::select! {
        biased;
        command = control.recv() => {
            match handle_control(command, runtime, normal).await {
                ControlAction::Continue => PendingUpdateAction::Retain(update),
                ControlAction::Close(reply) => PendingUpdateAction::Close { update, reply },
            }
        }
        command = query.recv() => {
            handle_query(command, runtime);
            PendingUpdateAction::Retain(update)
        }
        permit = updates.reserve() => {
            let Ok(permit) = permit else {
                return PendingUpdateAction::ReceiverClosed;
            };
            permit.send(update);
            PendingUpdateAction::Delivered
        }
    }
}

fn handle_query(command: Option<QueryCommand>, runtime: &LiveRuntime) {
    let Some(QueryCommand::SessionSource { reply }) = command else {
        return;
    };
    let _caller_gone = reply.send(runtime.collaboration_session_source()).is_err();
}

enum ControlAction {
    Continue,
    Close(Option<oneshot::Sender<Result<DispatchReport, RuntimeError>>>),
}

async fn handle_control(
    command: Option<ControlCommand>,
    runtime: &mut LiveRuntime,
    normal: &mut mpsc::Receiver<NormalCommand>,
) -> ControlAction {
    match command {
        Some(ControlCommand::Stop { reply }) => {
            drain_normal(normal);
            let agent = runtime.agent_id().clone();
            let result = runtime.submit(agent, Input::Interrupted).await;
            let _caller_gone = reply.send(result).is_err();
            ControlAction::Continue
        }
        Some(ControlCommand::Shutdown { reply }) => ControlAction::Close(Some(reply)),
        #[cfg(test)]
        Some(ControlCommand::Panic) => panic!("injected owned child actor panic"),
        None => ControlAction::Close(None),
    }
}

fn drain_normal(normal: &mut mpsc::Receiver<NormalCommand>) {
    while let Ok(command) = normal.try_recv() {
        if let NormalCommand::Start { reply, .. } = command {
            let _caller_gone = reply.send(Err(RuntimeError::DelegatedControlBusy)).is_err();
        }
    }
}

async fn close_owned_child(
    runtime: &mut LiveRuntime,
    identity: RunnerIdentity,
    mut normal: mpsc::Receiver<NormalCommand>,
    updates: mpsc::Sender<OwnedRunnerUpdate>,
    pending: Option<OwnedRunnerUpdate>,
    reply: Option<oneshot::Sender<Result<DispatchReport, RuntimeError>>>,
) {
    drain_normal(&mut normal);
    let shutdown = runtime.shutdown().await;
    let already_finished = matches!(&pending, Some(update) if update.is_finished());
    let (terminal_failure, pending) = match pending {
        Some(update @ OwnedRunnerUpdate::Failed { .. }) => (Some(update), None),
        other => (None, other),
    };
    let mut reply = reply;
    if let Some(update) = pending
        && updates.send(update).await.is_err()
    {
        if let Some(reply) = reply.take() {
            let _caller_gone = reply.send(shutdown).is_err();
        }
        return;
    }
    while let Some(event) = runtime.try_next_event() {
        if updates
            .send(OwnedRunnerUpdate::Runtime {
                identity: identity.clone(),
                update: Box::new(RuntimeUpdate::Event(event)),
            })
            .await
            .is_err()
        {
            if let Some(reply) = reply.take() {
                let _caller_gone = reply.send(shutdown).is_err();
            }
            return;
        }
    }
    match reply {
        Some(reply) => {
            if let Some(failure) = terminal_failure {
                if updates.send(failure).await.is_err() {
                    let _caller_gone = reply.send(shutdown).is_err();
                    return;
                }
                let _caller_gone = reply.send(shutdown).is_err();
                return;
            }
            if !already_finished
                && updates
                    .send(OwnedRunnerUpdate::Runtime {
                        identity,
                        update: Box::new(RuntimeUpdate::Finished),
                    })
                    .await
                    .is_err()
            {
                let _caller_gone = reply.send(shutdown).is_err();
                return;
            }
            let _caller_gone = reply.send(shutdown).is_err();
        }
        None => {
            let shutdown_failure = match shutdown {
                Ok(report) if !report.is_empty() => {
                    if updates
                        .send(OwnedRunnerUpdate::Runtime {
                            identity: identity.clone(),
                            update: Box::new(RuntimeUpdate::Report(report)),
                        })
                        .await
                        .is_err()
                    {
                        return;
                    }
                    None
                }
                Err(error) => Some(OwnedRunnerUpdate::Failed {
                    identity: identity.clone(),
                    error,
                }),
                Ok(_) => None,
            };
            if let Some(failure) = terminal_failure.or(shutdown_failure) {
                let _receiver_gone = updates.send(failure).await.is_err();
                return;
            }
            if !already_finished {
                let _receiver_gone = updates
                    .send(OwnedRunnerUpdate::Runtime {
                        identity,
                        update: Box::new(RuntimeUpdate::Finished),
                    })
                    .await
                    .is_err();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_agent::collaboration::MailEndpoint;
    use plexmaton_core::{AgentId, ConversationId};

    use super::*;
    use crate::NativeToolCatalog;
    use crate::runtime::tests::FakeDriver;

    /// SCH-2/SCH-4: runtime failure is the final update after cleanup output and joins safely.
    #[tokio::test]
    async fn sch_4_failed_runtime_emits_one_terminal_marker_after_cleanup() {
        let workspace = std::env::temp_dir().join(format!(
            "plexmaton-owned-runner-failure-{}",
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir(&workspace).expect("create workspace");
        let tools = NativeToolCatalog::open(
            &workspace,
            "TEST_KEY",
            "/bin/false",
            "/bin/false",
            Vec::new(),
        )
        .expect("native catalog");
        let agent = AgentId::new("failed-child").expect("agent");
        let runtime = LiveRuntime::with_root_driver_for_test(
            agent.clone(),
            "Failed child".into(),
            FakeDriver::new(Vec::<crate::runtime::tests::Script>::new()),
            tools,
        )
        .expect("runtime");
        let identity = RunnerIdentity {
            endpoint: MailEndpoint {
                agent,
                conversation: ConversationId::new("failed-conversation").expect("conversation"),
            },
            generation: RunnerGeneration::new(1).expect("generation"),
        };
        let (_normal, normal) = mpsc::channel(1);
        let (updates, mut received) = mpsc::channel(1);
        let failure_identity = identity.clone();
        let close = tokio::spawn(async move {
            let mut runtime = runtime;
            close_owned_child(
                &mut runtime,
                identity,
                normal,
                updates,
                Some(OwnedRunnerUpdate::Failed {
                    identity: failure_identity,
                    error: RuntimeError::ControlledByMain,
                }),
                None,
            )
            .await;
        });

        let mut terminal = None;
        while let Some(update) = received.recv().await {
            assert!(terminal.is_none(), "no update follows a terminal failure");
            if update.is_finished() {
                assert!(matches!(update, OwnedRunnerUpdate::Failed { .. }));
                terminal = Some(update);
            }
        }
        assert!(terminal.is_some());
        close.await.expect("join failed runtime close");
        std::fs::remove_dir_all(workspace).expect("remove workspace");
    }
}
