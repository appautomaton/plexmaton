//! Canonical child provenance validation and bounded runner registration.

use std::fmt;

use thiserror::Error;

use super::*;
use crate::LiveRuntime;
use crate::owned_runner::next_runner_generation;

/// Why a still caller-owned runtime could not enter this scheduling owner.
#[derive(Debug, Error)]
pub enum RunnerRegistrationReason {
    #[error("scheduling owner is shutting down")]
    ShuttingDown,
    #[error("concurrent runner capacity is full")]
    Capacity,
    #[error("this child Conversation already has a runner")]
    Duplicate,
    #[error("runtime is not a bound delegated child")]
    NotDelegated,
    #[error("runtime provenance does not match this collaboration owner")]
    ProvenanceMismatch,
    #[error("runtime collaboration context could not be restored: {0}")]
    Runtime(#[source] RuntimeError),
    #[error("collaboration writer could not validate the child: {0}")]
    Writer(#[source] CollaborationWriterError),
    #[error("runtime could not enter task ownership")]
    Runner,
    #[error("runner generation space is exhausted")]
    GenerationExhausted,
    #[cfg(test)]
    #[error("injected runner registration failure")]
    Injected,
}

/// Registration refusal that preserves the live runtime and its journal owner.
pub struct RunnerRegistrationError {
    reason: RunnerRegistrationReason,
    runtime: Box<LiveRuntime>,
}

impl RunnerRegistrationError {
    #[must_use]
    pub const fn reason(&self) -> &RunnerRegistrationReason {
        &self.reason
    }

    #[must_use]
    pub fn into_runtime(self) -> LiveRuntime {
        *self.runtime
    }

    pub(crate) fn into_parts(self) -> (RunnerRegistrationReason, LiveRuntime) {
        (self.reason, *self.runtime)
    }
}

impl fmt::Debug for RunnerRegistrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RunnerRegistrationError")
            .field("reason", &self.reason)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for RunnerRegistrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.reason.fmt(formatter)
    }
}

impl std::error::Error for RunnerRegistrationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.reason)
    }
}

impl OwnedCollaboration {
    #[cfg(test)]
    pub(crate) fn fail_next_registration_for_test(&mut self) {
        self.fail_next_registration = true;
    }

    pub(crate) fn has_runner_capacity(&self) -> bool {
        !self.shutting_down && self.runners.len() < self.limits.runners()
    }

    pub(crate) fn live_runner_identity(
        &self,
        delegation: &DelegationId,
        endpoint: &MailEndpoint,
    ) -> Option<RunnerIdentity> {
        self.runners
            .get(&endpoint.conversation)
            .filter(|slot| {
                !slot.finished
                    && &slot.delegation == delegation
                    && slot.runner.identity().endpoint() == endpoint
            })
            .map(|slot| slot.runner.identity().clone())
    }

    /// Validates canonical provenance before moving a child into one bounded runner slot.
    pub async fn register(
        &mut self,
        mut runtime: LiveRuntime,
    ) -> Result<RunnerIdentity, RunnerRegistrationError> {
        #[cfg(test)]
        if std::mem::take(&mut self.fail_next_registration) {
            return Err(registration_error(
                RunnerRegistrationReason::Injected,
                runtime,
            ));
        }
        if self.shutting_down {
            return Err(registration_error(
                RunnerRegistrationReason::ShuttingDown,
                runtime,
            ));
        }
        if self.runners.len() >= self.limits.runners() {
            return Err(registration_error(
                RunnerRegistrationReason::Capacity,
                runtime,
            ));
        }
        let provenance = match runtime.delegated_provenance() {
            Ok(Some(provenance)) => provenance.clone(),
            _ => {
                return Err(registration_error(
                    RunnerRegistrationReason::NotDelegated,
                    runtime,
                ));
            }
        };
        if self.runners.contains_key(&provenance.worker().conversation) {
            return Err(registration_error(
                RunnerRegistrationReason::Duplicate,
                runtime,
            ));
        }
        let canonical = match self
            .writer
            .delegated_control(provenance.delegation().clone())
            .await
        {
            Ok(control) => control,
            Err(error) => {
                return Err(registration_error(
                    RunnerRegistrationReason::Writer(error),
                    runtime,
                ));
            }
        };
        if canonical.provenance() != &provenance {
            return Err(registration_error(
                RunnerRegistrationReason::ProvenanceMismatch,
                runtime,
            ));
        }
        let runtime_identity = runtime.child_collaboration_identity();
        #[cfg(not(test))]
        if runtime_identity.is_none() {
            return Err(registration_error(
                RunnerRegistrationReason::ProvenanceMismatch,
                runtime,
            ));
        }
        if runtime_identity.as_ref().is_some_and(|identity| {
            !self
                .ingress
                .as_ref()
                .is_some_and(|ingress| ingress.authenticates_child_runtime(identity, &provenance))
        }) {
            return Err(registration_error(
                RunnerRegistrationReason::ProvenanceMismatch,
                runtime,
            ));
        }
        let references = match runtime.collaboration_references() {
            Ok(references) => references,
            Err(error) => {
                return Err(registration_error(
                    RunnerRegistrationReason::Runtime(error),
                    runtime,
                ));
            }
        };
        let resolved = match self.writer.resolve_context(references).await {
            Ok(resolved) => resolved,
            Err(error) => {
                return Err(registration_error(
                    RunnerRegistrationReason::Writer(error),
                    runtime,
                ));
            }
        };
        if let Err(error) = runtime.restore_resolved_collaboration_context(resolved) {
            return Err(registration_error(
                RunnerRegistrationReason::Runtime(error),
                runtime,
            ));
        }
        let Some(generation) = next_runner_generation() else {
            return Err(registration_error(
                RunnerRegistrationReason::GenerationExhausted,
                runtime,
            ));
        };
        let runner = match OwnedChildRunner::spawn(runtime, generation) {
            Ok(runner) => runner,
            Err(error) => {
                return Err(registration_error(
                    RunnerRegistrationReason::Runner,
                    error.into_runtime(),
                ));
            }
        };
        let identity = runner.identity().clone();
        self.runners.insert(
            identity.endpoint().conversation.clone(),
            RunnerSlot {
                delegation: provenance.delegation().clone(),
                runner,
                finished: false,
                terminal_update: None,
                shutdown_requested: false,
                joined: false,
                shutdown_report: None,
                shutdown_error: None,
                input_unavailable: false,
            },
        );
        if let Some(runtime_identity) = runtime_identity {
            self.ingress
                .as_mut()
                .expect("authenticated child runtime has an ingress owner")
                .bind_child_runtime(runtime_identity);
        }
        Ok(identity)
    }
}

fn registration_error(
    reason: RunnerRegistrationReason,
    runtime: LiveRuntime,
) -> RunnerRegistrationError {
    RunnerRegistrationError {
        reason,
        runtime: Box::new(runtime),
    }
}
