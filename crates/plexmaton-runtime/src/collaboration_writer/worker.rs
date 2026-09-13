//! Collaboration-file command execution and failure recovery.

use super::*;
use std::panic::{AssertUnwindSafe, catch_unwind};

pub(super) fn process_command(file: &mut CollaborationFile, command: Command) -> bool {
    match command {
        Command::Admit { attempt, reply } => {
            let recovery = attempt.clone();
            send_admission_result(
                catch_unwind(AssertUnwindSafe(|| admit(file, attempt))),
                recovery,
                reply,
            )
        }
        Command::DelegatedControl { delegation, reply } => send_caught(
            catch_unwind(AssertUnwindSafe(|| {
                file.delegated_control(&delegation)
                    .map_err(CollaborationWriterError::from)
            })),
            reply,
        ),
        Command::DelegationView { delegation, reply } => send_caught(
            catch_unwind(AssertUnwindSafe(|| {
                file.ledger()
                    .delegation(&delegation)
                    .cloned()
                    .ok_or(plexmaton_agent::collaboration::CollaborationError::UnknownDelegation)
                    .map_err(CollaborationStoreError::from)
                    .map_err(CollaborationWriterError::from)
            })),
            reply,
        ),
        Command::DelegatedControls { reply } => send_caught(
            catch_unwind(AssertUnwindSafe(|| {
                let delegations = file
                    .ledger()
                    .records()
                    .iter()
                    .filter_map(|record| match &record.event {
                        plexmaton_agent::collaboration::CollaborationEvent::DelegationCreated {
                            delegation,
                            ..
                        } => Some(delegation.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                delegations
                    .iter()
                    .map(|delegation| file.delegated_control(delegation))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(CollaborationWriterError::from)
            })),
            reply,
        ),
        Command::ProjectMail { endpoint, reply } => send_caught(
            catch_unwind(AssertUnwindSafe(|| {
                file.project_mail(&endpoint)
                    .map_err(CollaborationWriterError::Store)
            })),
            reply,
        ),
        Command::ProjectSessionMail {
            endpoint,
            references,
            reply,
        } => send_caught(
            catch_unwind(AssertUnwindSafe(|| {
                let mail = file
                    .project_mail(&endpoint)
                    .map_err(CollaborationWriterError::Store)?;
                let turns = file
                    .resolve_turns(&references)
                    .map_err(CollaborationWriterError::Store)?;
                Ok(SessionMailSourceSnapshot { mail, turns })
            })),
            reply,
        ),
        Command::ResolveContext { references, reply } => send_caught(
            catch_unwind(AssertUnwindSafe(|| {
                file.resolve_turns(&references)
                    .map_err(CollaborationWriterError::Store)
            })),
            reply,
        ),
        Command::Schedule { request, reply } => {
            let recovery = request.clone();
            send_schedule_result(
                catch_unwind(AssertUnwindSafe(|| schedule(file, request))),
                recovery,
                reply,
            )
        }
        Command::RequireQuiescent { reply } => send_caught(
            catch_unwind(AssertUnwindSafe(|| {
                file.require_quiescent()
                    .map_err(CollaborationWriterError::from)
            })),
            reply,
        ),
        Command::PreflightHandoff { attempt, reply } => {
            let recovery = attempt.clone();
            match catch_unwind(AssertUnwindSafe(|| preflight_handoff(file, &attempt))) {
                Ok(Ok(delegation)) => {
                    let _reply_cancelled = reply.send(Ok(delegation)).is_err();
                    false
                }
                Ok(Err(source)) => {
                    let _reply_cancelled = reply
                        .send(Err(CollaborationWriterError::HandoffPreflight {
                            source,
                            attempt: Box::new(recovery),
                        }))
                        .is_err();
                    false
                }
                Err(_) => {
                    let _reply_cancelled = reply
                        .send(Err(CollaborationWriterError::AdmissionWorkerFailed {
                            attempt: Box::new(recovery),
                        }))
                        .is_err();
                    true
                }
            }
        }
        #[cfg(test)]
        Command::Hold { entered, release } => {
            let _test_gone = entered.send(()).is_err();
            let _test_gone = release.recv().is_err();
            false
        }
        #[cfg(test)]
        Command::PanicAfterAdmission { attempt, reply } => {
            let recovery = attempt.clone();
            let panicked = catch_unwind(AssertUnwindSafe(|| {
                let _outcome = admit(file, attempt);
                panic!("injected collaboration worker panic after admission");
            }));
            debug_assert!(panicked.is_err());
            let _reply_cancelled = reply
                .send(Err(CollaborationWriterError::AdmissionWorkerFailed {
                    attempt: Box::new(recovery),
                }))
                .is_err();
            true
        }
    }
}

fn send_caught<T>(
    result: thread::Result<Result<T, CollaborationWriterError>>,
    reply: oneshot::Sender<Result<T, CollaborationWriterError>>,
) -> bool {
    match result {
        Ok(result) => {
            let _reply_cancelled = reply.send(result).is_err();
            false
        }
        Err(_) => {
            let _reply_cancelled = reply
                .send(Err(CollaborationWriterError::WorkerFailed))
                .is_err();
            true
        }
    }
}

fn send_admission_result(
    result: thread::Result<Result<ItemReceipt, CollaborationWriterError>>,
    recovery: CollaborationAttempt,
    reply: oneshot::Sender<Result<ItemReceipt, CollaborationWriterError>>,
) -> bool {
    match result {
        Ok(result) => {
            let _reply_cancelled = reply.send(result).is_err();
            false
        }
        Err(_) => {
            let _reply_cancelled = reply
                .send(Err(CollaborationWriterError::AdmissionWorkerFailed {
                    attempt: Box::new(recovery),
                }))
                .is_err();
            true
        }
    }
}

fn send_schedule_result(
    result: thread::Result<Result<PreparedChildExecution, CollaborationWriterError>>,
    recovery: ScheduledTurnRequest,
    reply: oneshot::Sender<Result<PreparedChildExecution, CollaborationWriterError>>,
) -> bool {
    match result {
        Ok(result) => {
            let _reply_cancelled = reply.send(result).is_err();
            false
        }
        Err(_) => {
            let _reply_cancelled = reply
                .send(Err(CollaborationWriterError::ScheduleWorkerFailed {
                    request: Box::new(recovery),
                }))
                .is_err();
            true
        }
    }
}

pub(super) fn reject_queued_commands(receiver: &mut mpsc::Receiver<Command>) {
    while let Ok(command) = receiver.try_recv() {
        match command {
            Command::Admit { attempt, reply } => {
                let _reply_cancelled = reply
                    .send(Err(CollaborationWriterError::AdmissionWorkerFailed {
                        attempt: Box::new(attempt),
                    }))
                    .is_err();
            }
            Command::Schedule { request, reply } => {
                let _reply_cancelled = reply
                    .send(Err(CollaborationWriterError::ScheduleWorkerFailed {
                        request: Box::new(request),
                    }))
                    .is_err();
            }
            Command::DelegatedControl { reply, .. } => {
                let _reply_cancelled = reply
                    .send(Err(CollaborationWriterError::WorkerFailed))
                    .is_err();
            }
            Command::DelegationView { reply, .. } => {
                let _reply_cancelled = reply
                    .send(Err(CollaborationWriterError::WorkerFailed))
                    .is_err();
            }
            Command::DelegatedControls { reply } => {
                let _reply_cancelled = reply
                    .send(Err(CollaborationWriterError::WorkerFailed))
                    .is_err();
            }
            Command::ProjectMail { reply, .. } => {
                let _reply_cancelled = reply
                    .send(Err(CollaborationWriterError::WorkerFailed))
                    .is_err();
            }
            Command::ProjectSessionMail { reply, .. } => {
                let _reply_cancelled = reply
                    .send(Err(CollaborationWriterError::WorkerFailed))
                    .is_err();
            }
            Command::ResolveContext { reply, .. } => {
                let _reply_cancelled = reply
                    .send(Err(CollaborationWriterError::WorkerFailed))
                    .is_err();
            }
            Command::RequireQuiescent { reply } => {
                let _reply_cancelled = reply
                    .send(Err(CollaborationWriterError::WorkerFailed))
                    .is_err();
            }
            Command::PreflightHandoff { attempt, reply } => {
                let _reply_cancelled = reply
                    .send(Err(CollaborationWriterError::AdmissionWorkerFailed {
                        attempt: Box::new(attempt),
                    }))
                    .is_err();
            }
            #[cfg(test)]
            Command::Hold { .. } => {}
            #[cfg(test)]
            Command::PanicAfterAdmission { attempt, reply } => {
                let _reply_cancelled = reply
                    .send(Err(CollaborationWriterError::AdmissionWorkerFailed {
                        attempt: Box::new(attempt),
                    }))
                    .is_err();
            }
        }
    }
}

fn admit(
    file: &mut CollaborationFile,
    attempt: CollaborationAttempt,
) -> Result<ItemReceipt, CollaborationWriterError> {
    file.admit(attempt.id.clone(), attempt.event.clone())
        .map_err(|failure| {
            let (source, attempt) = failure.into_parts();
            CollaborationWriterError::Admission {
                source,
                attempt: Box::new(attempt),
            }
        })
}

fn schedule(
    file: &mut CollaborationFile,
    request: ScheduledTurnRequest,
) -> Result<PreparedChildExecution, CollaborationWriterError> {
    let result = schedule_inner(file, &request);
    result.map_err(|source| CollaborationWriterError::Schedule {
        source,
        request: Box::new(request),
    })
}

fn schedule_inner(
    file: &mut CollaborationFile,
    request: &ScheduledTurnRequest,
) -> Result<PreparedChildExecution, CollaborationStoreError> {
    let control = file.delegated_control(&request.delegation)?;
    if control.worker() != &request.boundary.recipient {
        return Err(CollaborationStoreError::InvalidExecutionTicket);
    }
    let reservation = control.reserve_execution()?;
    match file.ledger().prepare_turn(
        request.item.clone(),
        request.boundary.clone(),
        request.previous.clone(),
    )? {
        Preparation::Existing(_) => {}
        Preparation::Append(record) => {
            file.admit(record.id, record.event)
                .map_err(|failure| failure.into_parts().0)?;
        }
    }
    let reference = file.ledger().item_reference(&request.item)?;
    let resolved = file.ledger().resolve_turn(&reference)?;
    let ticket = file.execution_ticket(&request.delegation, &resolved)?;
    Ok(PreparedChildExecution {
        resolved,
        reservation,
        ticket,
    })
}

fn preflight_handoff(
    file: &CollaborationFile,
    attempt: &CollaborationAttempt,
) -> Result<DelegationId, CollaborationStoreError> {
    let delegation = match &attempt.event {
        plexmaton_agent::collaboration::CollaborationEvent::HandoffCompleted {
            delegation, ..
        } => delegation.clone(),
        _ => return Err(CollaborationStoreError::InvalidExecutionTicket),
    };
    file.ledger()
        .prepare(attempt.id.clone(), attempt.event.clone())?;
    Ok(delegation)
}
