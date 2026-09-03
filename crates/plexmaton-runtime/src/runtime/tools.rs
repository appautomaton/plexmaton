//! Retained native-tool workers and their per-call cancellation ownership.

use std::{
    collections::BTreeMap,
    panic::{self, AssertUnwindSafe},
    thread::{self, JoinHandle},
};

use futures_util::{FutureExt as _, StreamExt as _, future::BoxFuture, stream::FuturesUnordered};
use plexmaton_agent::{
    AdmissionOutcome, AdmissionRefusal, AdmissionRequest, AdmittedToolCall, ToolOutcome,
};
use plexmaton_core::ToolCallId;

use crate::{
    RuntimeError,
    native::{NativeCancellation, NativeToolCatalog},
};

type ToolFuture = BoxFuture<'static, ToolCompletion>;

pub(super) struct ToolTasks {
    catalog: NativeToolCatalog,
    pending: FuturesUnordered<ToolFuture>,
    active: BTreeMap<ToolCallId, ActiveTool>,
}

impl ToolTasks {
    pub(super) fn new(catalog: NativeToolCatalog) -> Self {
        Self {
            catalog,
            pending: FuturesUnordered::new(),
            active: BTreeMap::new(),
        }
    }

    pub(super) fn start_admission(
        &mut self,
        request: AdmissionRequest,
    ) -> Result<(), RuntimeError> {
        let call_id = request.requested().call_id.clone();
        self.ensure_available(&call_id)?;
        let cancellation = NativeCancellation::new();
        let future = self.catalog.admit(request, cancellation.clone());
        let completion_id = call_id.clone();
        let resolution_id = call_id.clone();
        let future = AssertUnwindSafe(future)
            .catch_unwind()
            .map(move |result| {
                let outcome = match result {
                    Ok(outcome) => outcome,
                    Err(_) => AdmissionOutcome::Refused {
                        call_id: resolution_id,
                        reason: AdmissionRefusal::DefinitionUnavailable,
                    },
                };
                ToolCompletion {
                    call_id: completion_id,
                    phase: ToolPhase::Admission,
                    resolution: ToolResolution::Admission(outcome),
                }
            })
            .boxed();
        let (completion, worker) = run_on_worker(future, call_id.clone(), ToolPhase::Admission);
        self.active.insert(
            call_id,
            ActiveTool {
                phase: ToolPhase::Admission,
                cancellation,
                worker,
            },
        );
        self.pending.push(completion);
        Ok(())
    }

    pub(super) fn start_execution(&mut self, call: AdmittedToolCall) -> Result<(), RuntimeError> {
        let call_id = call.requested().call_id.clone();
        self.ensure_available(&call_id)?;
        let cancellation = NativeCancellation::new();
        let future = self.catalog.execute(call, cancellation.clone());
        let completion_id = call_id.clone();
        let resolution_id = call_id.clone();
        let future = AssertUnwindSafe(future)
            .catch_unwind()
            .map(move |result| {
                let outcome = match result {
                    Ok(outcome) => outcome,
                    Err(_) => ToolOutcome::Failed {
                        message: "worker: native tool future terminated unexpectedly".to_owned(),
                    },
                };
                ToolCompletion {
                    call_id: completion_id,
                    phase: ToolPhase::Execution,
                    resolution: ToolResolution::Execution {
                        call_id: resolution_id,
                        outcome,
                    },
                }
            })
            .boxed();
        let (completion, worker) = run_on_worker(future, call_id.clone(), ToolPhase::Execution);
        self.active.insert(
            call_id,
            ActiveTool {
                phase: ToolPhase::Execution,
                cancellation,
                worker,
            },
        );
        self.pending.push(completion);
        Ok(())
    }

    pub(super) async fn next(&mut self) -> Result<Option<ToolResolution>, RuntimeError> {
        let Some(completion) = self.pending.next().await else {
            return if self.active.is_empty() {
                Ok(None)
            } else {
                Err(RuntimeError::ToolTaskLost)
            };
        };
        let Some(mut owner) = self.active.remove(&completion.call_id) else {
            return Err(RuntimeError::UnexpectedToolCompletion(completion.call_id));
        };
        if owner.phase != completion.phase {
            return Err(RuntimeError::UnexpectedToolCompletion(completion.call_id));
        }
        let worker_failed = owner
            .worker
            .take()
            .is_some_and(|worker| worker.join().is_err());
        let completion = if worker_failed {
            failed_completion(completion.call_id, completion.phase)
        } else {
            completion
        };
        Ok(Some(completion.resolution.for_call(completion.call_id)))
    }

    pub(super) async fn cancel_and_join(&mut self) -> Result<(), RuntimeError> {
        for owner in self.active.values() {
            owner.cancellation.cancel();
        }
        while !self.pending.is_empty() {
            let _discarded = self.next().await?;
        }
        if self.active.is_empty() {
            Ok(())
        } else {
            Err(RuntimeError::ToolTaskLost)
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.pending.is_empty() && self.active.is_empty()
    }

    fn ensure_available(&self, call_id: &ToolCallId) -> Result<(), RuntimeError> {
        if self.active.contains_key(call_id) {
            return Err(RuntimeError::ToolAlreadyActive(call_id.clone()));
        }
        Ok(())
    }
}

impl Drop for ToolTasks {
    fn drop(&mut self) {
        for owner in self.active.values_mut() {
            owner.cancellation.cancel();
        }
        for owner in self.active.values_mut() {
            if let Some(worker) = owner.worker.take() {
                let _worker_panicked_after_cancellation = worker.join().is_err();
            }
        }
    }
}

struct ActiveTool {
    phase: ToolPhase,
    cancellation: NativeCancellation,
    worker: Option<JoinHandle<()>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ToolPhase {
    Admission,
    Execution,
}

struct ToolCompletion {
    call_id: ToolCallId,
    phase: ToolPhase,
    resolution: ToolResolution,
}

fn run_on_worker(
    future: ToolFuture,
    call_id: ToolCallId,
    phase: ToolPhase,
) -> (ToolFuture, Option<JoinHandle<()>>) {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let worker_call_id = call_id.clone();
    let worker = thread::Builder::new()
        .name("plexmaton-tool".to_owned())
        .stack_size(512 * 1024)
        .spawn(move || {
            let completion = panic::catch_unwind(AssertUnwindSafe(|| {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .ok()
                    .map(|runtime| runtime.block_on(future))
            }))
            .ok()
            .flatten()
            .unwrap_or_else(|| failed_completion(worker_call_id, phase));
            let _runtime_gone = sender.send(completion);
        })
        .ok();
    let completion = async move {
        receiver
            .await
            .unwrap_or_else(|_| failed_completion(call_id, phase))
    }
    .boxed();
    (completion, worker)
}

fn failed_completion(call_id: ToolCallId, phase: ToolPhase) -> ToolCompletion {
    let resolution = match phase {
        ToolPhase::Admission => ToolResolution::Admission(AdmissionOutcome::Refused {
            call_id: call_id.clone(),
            reason: AdmissionRefusal::DefinitionUnavailable,
        }),
        ToolPhase::Execution => ToolResolution::Execution {
            call_id: call_id.clone(),
            outcome: ToolOutcome::Failed {
                message: "worker: native tool worker terminated unexpectedly".to_owned(),
            },
        },
    };
    ToolCompletion {
        call_id,
        phase,
        resolution,
    }
}

pub(super) enum ToolResolution {
    Admission(AdmissionOutcome),
    Execution {
        call_id: ToolCallId,
        outcome: ToolOutcome,
    },
}

impl ToolResolution {
    fn for_call(self, expected: ToolCallId) -> Self {
        match self {
            Self::Admission(AdmissionOutcome::Admitted(call))
                if call.requested().call_id != expected =>
            {
                Self::Admission(AdmissionOutcome::Refused {
                    call_id: expected,
                    reason: AdmissionRefusal::DefinitionUnavailable,
                })
            }
            Self::Admission(AdmissionOutcome::Refused { call_id, .. }) if call_id != expected => {
                Self::Admission(AdmissionOutcome::Refused {
                    call_id: expected,
                    reason: AdmissionRefusal::DefinitionUnavailable,
                })
            }
            Self::Execution {
                call_id: returned, ..
            } if returned != expected => Self::Execution {
                call_id: expected,
                outcome: ToolOutcome::Failed {
                    message: "worker: tool completion identity changed".to_owned(),
                },
            },
            resolution => resolution,
        }
    }
}
