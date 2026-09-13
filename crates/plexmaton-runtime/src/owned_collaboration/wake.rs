//! Advisory wake ownership and canonical rescheduling.

use plexmaton_agent::collaboration::CollaborationError;
use plexmaton_agent::collaboration::TurnBoundary;
use plexmaton_core::ConversationId;
use plexmaton_session_store::collaboration::CollaborationStoreError;
use std::fmt;
use thiserror::Error;

use super::*;

/// Whether a bounded wake hint created a new retained owner obligation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WakeAdmission {
    Accepted,
    Coalesced,
}

/// Why an advisory wake could not enter the current live owner.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum WakeRefusal {
    #[error("scheduling owner is shutting down")]
    ShuttingDown,
    #[error("the addressed child has no owned runner")]
    UnknownRunner,
    #[error("the wake does not name the current runner generation")]
    StaleRunner,
    #[error("Handoff has closed normal admission for this delegation")]
    HandoffPending,
    #[error("the selected provider has no typed collaboration representation")]
    ProviderUnsupported,
    #[error("an accepted collaboration admission must settle before wake")]
    AdmissionPending,
    #[error("Stop has closed wake admission for this child")]
    StopPending,
    #[error("the child runner is closed")]
    RunnerClosed,
}

/// Immediate wake refusal that returns the exact content-free hint to its caller.
#[derive(Debug)]
pub struct WakeFailure {
    reason: WakeRefusal,
    hint: WakeHint,
}

impl WakeFailure {
    #[must_use]
    pub const fn reason(&self) -> &WakeRefusal {
        &self.reason
    }

    #[must_use]
    pub fn into_hint(self) -> WakeHint {
        self.hint
    }
}

impl fmt::Display for WakeFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.reason.fmt(formatter)
    }
}

impl std::error::Error for WakeFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.reason)
    }
}

pub(super) struct PendingWake {
    pub(super) hint: WakeHint,
    pub(super) queued: bool,
    pub(super) deferred: bool,
    pub(super) ready: Option<ScheduledTurnRequest>,
}

impl OwnedCollaboration {
    /// Retains one bounded content-free wake for the exact live runner generation.
    pub fn wake(&mut self, hint: WakeHint) -> Result<WakeAdmission, WakeFailure> {
        let refusal = if self.shutting_down {
            Some(WakeRefusal::ShuttingDown)
        } else if self.writer.has_pending_admission() {
            Some(WakeRefusal::AdmissionPending)
        } else {
            let conversation = &hint.runner().endpoint().conversation;
            match self.runners.get(conversation) {
                None => Some(WakeRefusal::UnknownRunner),
                Some(slot) if slot.runner.identity() != hint.runner() => {
                    Some(WakeRefusal::StaleRunner)
                }
                Some(slot) if slot.finished => Some(WakeRefusal::RunnerClosed),
                Some(_)
                    if self.pending_stop.as_ref().is_some_and(|pending| {
                        pending.conversation == hint.runner().endpoint().conversation
                    }) =>
                {
                    Some(WakeRefusal::StopPending)
                }
                Some(slot) if self.handoff_closed.contains(&slot.delegation) => {
                    Some(WakeRefusal::HandoffPending)
                }
                Some(slot) if !slot.runner.supports_collaboration() => {
                    Some(WakeRefusal::ProviderUnsupported)
                }
                Some(_) => None,
            }
        };
        if let Some(reason) = refusal {
            return Err(WakeFailure { reason, hint });
        }

        let conversation = hint.runner().endpoint().conversation.clone();
        if let Some(pending) = self.wakes.get_mut(&conversation) {
            pending.deferred = false;
            return Ok(WakeAdmission::Coalesced);
        }
        let queued = match self
            .runners
            .get(&conversation)
            .expect("validated wake retains its runner")
            .runner
            .queue_wake(hint.clone())
        {
            Ok(()) => true,
            Err(ReserveStartError::Busy) => false,
            Err(ReserveStartError::Closed) => {
                return Err(WakeFailure {
                    reason: WakeRefusal::RunnerClosed,
                    hint,
                });
            }
        };
        self.wakes.insert(
            conversation,
            PendingWake {
                hint,
                queued,
                deferred: false,
                ready: None,
            },
        );
        Ok(WakeAdmission::Accepted)
    }

    pub(super) fn queue_pending_wakes(&mut self) -> Option<OwnedRunnerUpdate> {
        let conversations = self
            .wakes
            .iter()
            .filter(|(_, pending)| !pending.queued && pending.ready.is_none())
            .map(|(conversation, _)| conversation.clone())
            .collect::<Vec<_>>();
        for conversation in conversations {
            let hint = self.wakes.get(&conversation)?.hint.clone();
            let result = self
                .runners
                .get(&conversation)
                .map(|slot| slot.runner.queue_wake(hint.clone()));
            match result {
                Some(Ok(())) => {
                    if let Some(pending) = self.wakes.get_mut(&conversation) {
                        pending.queued = true;
                    }
                }
                Some(Err(ReserveStartError::Busy)) => {}
                Some(Err(ReserveStartError::Closed)) | None => {
                    self.wakes.remove(&conversation);
                    return Some(OwnedRunnerUpdate::WakeRejected {
                        identity: hint.runner().clone(),
                        error: Box::new(OwnedSchedulingError::RunnerClosed),
                    });
                }
            }
        }
        None
    }

    pub(super) fn accept_wake_snapshot(
        &mut self,
        identity: &RunnerIdentity,
        hint: &WakeHint,
        boundary: TurnBoundary,
        previous: Option<plexmaton_agent::collaboration::CollaborationItemRef>,
    ) -> bool {
        let conversation = &identity.endpoint().conversation;
        let Some(pending) = self.wakes.get_mut(conversation) else {
            return false;
        };
        if identity != hint.runner() || pending.hint != *hint {
            return false;
        }
        let delegation = self
            .runners
            .get(conversation)
            .expect("accepted wake update retains its runner")
            .delegation
            .clone();
        pending.queued = false;
        pending.deferred = false;
        pending.ready = Some(ScheduledTurnRequest::new(
            delegation,
            hint.admission_id().clone(),
            boundary,
            previous,
        ));
        true
    }

    pub(super) fn discard_wake(&mut self, identity: &RunnerIdentity, hint: &WakeHint) -> bool {
        let conversation = &identity.endpoint().conversation;
        if self
            .wakes
            .get(conversation)
            .is_some_and(|pending| identity == hint.runner() && pending.hint == *hint)
        {
            self.wakes.remove(conversation);
            return true;
        }
        false
    }

    pub(super) async fn drive_ready_wake(&mut self) -> Option<OwnedRunnerUpdate> {
        let ready = self
            .wakes
            .iter()
            .filter_map(|(conversation, pending)| {
                if pending.deferred {
                    None
                } else {
                    pending.ready.as_ref().map(|request| {
                        (
                            conversation.clone(),
                            pending.hint.runner().clone(),
                            request.clone(),
                        )
                    })
                }
            })
            .collect::<Vec<_>>();
        for (conversation, identity, request) in ready {
            match self.schedule(request).await {
                Ok(report) => {
                    self.wakes.remove(&conversation);
                    return Some(OwnedRunnerUpdate::WakeScheduled {
                        identity,
                        report: Box::new(report),
                    });
                }
                Err(failure) if is_no_pending_items(failure.source()) => {
                    self.wakes.remove(&conversation);
                    return Some(OwnedRunnerUpdate::WakeIdle { identity });
                }
                Err(failure) if matches!(failure.source(), OwnedSchedulingError::RunnerBusy) => {
                    continue;
                }
                Err(failure) if is_owner_busy(failure.source()) => {
                    if let Some(pending) = self.wakes.get_mut(&conversation) {
                        pending.deferred = true;
                    }
                    return Some(OwnedRunnerUpdate::WakeDeferred { identity });
                }
                Err(failure) => {
                    self.wakes.remove(&conversation);
                    return Some(OwnedRunnerUpdate::WakeRejected {
                        identity,
                        error: failure.source,
                    });
                }
            }
        }
        None
    }

    pub(super) fn remove_wake(&mut self, conversation: &ConversationId) {
        self.wakes.remove(conversation);
    }

    pub(super) fn resume_deferred_wakes(&mut self) {
        for pending in self.wakes.values_mut() {
            pending.deferred = false;
        }
    }

    pub(super) fn has_deferred_wakes(&self) -> bool {
        self.wakes.values().any(|pending| pending.deferred)
    }

    pub(super) fn clear_wakes(&mut self) {
        self.wakes.clear();
    }
}

fn is_no_pending_items(error: &OwnedSchedulingError) -> bool {
    matches!(
        error,
        OwnedSchedulingError::Writer(CollaborationWriterError::Schedule {
            source: CollaborationStoreError::Rejected(CollaborationError::NoPendingItems),
            ..
        })
    )
}

fn is_owner_busy(error: &OwnedSchedulingError) -> bool {
    matches!(
        error,
        OwnedSchedulingError::ScheduleInProgress
            | OwnedSchedulingError::AdmissionInProgress
            | OwnedSchedulingError::StopInProgress
            | OwnedSchedulingError::Writer(
                CollaborationWriterError::Busy
                    | CollaborationWriterError::ScheduleBusy { .. }
                    | CollaborationWriterError::ScheduleInProgress { .. }
            )
    )
}
