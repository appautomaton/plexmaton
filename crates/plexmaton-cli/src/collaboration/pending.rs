//! Root-bound ownership of one selected collaboration projection.

use plexmaton_agent::collaboration::{CollaborationItemRef, MailEndpoint};
use plexmaton_core::{AgentId, ConversationEvent, ConversationId};
use plexmaton_runtime::{
    CollaborationIngressOutcome, DelegatedProjectionRefusal, LiveRuntime,
    OwnedCollaborationActivity, OwnedRunnerUpdate, OwnedSchedulingError, OwnedStopReport,
    RunnerGeneration, RuntimeUpdate,
};

use super::{AgentStatus, Collaboration, forwarded};

/// Whether the current root runtime advanced one retained projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RootProjectionProgress {
    Idle,
    Applied,
    PendingCommit,
    RequiresReopen,
    ConversationMismatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RecoverySource {
    ChildJournal,
    None,
}

/// The exact root-facing work selected from the bounded owner before a runtime barrier was known.
#[derive(Debug)]
pub(super) enum PendingRootProjection {
    RefreshLog {
        sync_running_roster: bool,
        orphaned_link: Option<OrphanedIngressLink>,
    },
    RunnerEvent {
        child: ConversationId,
        generation: RunnerGeneration,
        event: Box<ConversationEvent>,
        recovery: RecoverySource,
    },
    StopSettled {
        agent: AgentId,
        outcome: Box<Result<OwnedStopReport, OwnedSchedulingError>>,
    },
}

#[derive(Clone, Debug)]
pub(super) struct OrphanedIngressLink {
    pub(super) caller: MailEndpoint,
    pub(super) reference: CollaborationItemRef,
}

impl PendingRootProjection {
    pub(super) fn transient_description(&self) -> Option<String> {
        match self {
            Self::RefreshLog { .. }
            | Self::RunnerEvent {
                recovery: RecoverySource::ChildJournal,
                ..
            } => None,
            Self::StopSettled { .. } => None,
            Self::RunnerEvent {
                child,
                generation,
                event,
                recovery: RecoverySource::None,
            } => Some(format!(
                "child {child} generation {} produced {event:?}",
                generation.get()
            )),
        }
    }
}

impl Collaboration {
    /// Whether the select loop may take one more activity from this root's bounded owner.
    pub(crate) fn can_poll(&self, runtime: &LiveRuntime) -> bool {
        self.pending_projection.is_none()
            && self.matches_runtime(runtime)
            && runtime.delegated_projection_refusal()
                != Some(DelegatedProjectionRefusal::PersistenceFailed)
    }

    pub(crate) fn has_pending_projection(&self) -> bool {
        self.pending_projection.is_some()
    }

    /// Takes the one retained Stop settlement after the owner has delivered it.
    ///
    /// The report is deliberately consumed here, once, rather than being turned into a transcript
    /// event or a status-line message. Its existing dispatch-report path is responsible for
    /// returning any child-owned input to the child's composer.
    pub(crate) fn take_stop_settlement(
        &mut self,
    ) -> Option<(AgentId, Result<OwnedStopReport, OwnedSchedulingError>)> {
        let projection = self.pending_projection.take()?;
        match projection {
            PendingRootProjection::StopSettled { agent, outcome, .. } => Some((agent, *outcome)),
            other => {
                self.pending_projection = Some(other);
                None
            }
        }
    }

    /// Converts one opaque owner activity into the smallest retryable root projection.
    ///
    /// Successful ingress is reconstructed from its durable log. Runner events retain only their
    /// mapped semantic event and source identity. Activity with no root projection is consumed.
    pub(crate) fn stage(&mut self, activity: OwnedCollaborationActivity) -> anyhow::Result<bool> {
        anyhow::ensure!(
            self.pending_projection.is_none(),
            "one root projection is already retained"
        );
        self.pending_projection = match activity {
            OwnedCollaborationActivity::Ingress(settlement) if settlement.result().is_ok() => {
                let orphaned_link = if settlement.reply_delivered() {
                    None
                } else {
                    Some(OrphanedIngressLink {
                        caller: settlement.caller().cloned().ok_or_else(|| {
                            anyhow::anyhow!("settled collaboration ingress has no caller endpoint")
                        })?,
                        reference: settlement
                            .result()
                            .as_ref()
                            .expect("successful ingress checked above")
                            .reference()
                            .clone(),
                    })
                };
                Some(PendingRootProjection::RefreshLog {
                    sync_running_roster: matches!(
                        settlement.result(),
                        Ok(result)
                            if matches!(result.outcome(), CollaborationIngressOutcome::Delegated { .. })
                    ),
                    orphaned_link,
                })
            }
            OwnedCollaborationActivity::Ingress(_) => None,
            OwnedCollaborationActivity::Runner(update) => self.prepare_runner(update),
        };
        Ok(self.pending_projection.is_some())
    }

    /// Applies the retained fact only to its own healthy root runtime.
    ///
    /// The slot is cleared after all caller state changes succeed. The log/roster paths are
    /// idempotent, so an intermediate owner read failure leaves a safe retry.
    pub(crate) async fn drive_pending(
        &mut self,
        runtime: &mut LiveRuntime,
    ) -> anyhow::Result<RootProjectionProgress> {
        if self.pending_projection.is_none() {
            return Ok(RootProjectionProgress::Idle);
        }
        if !self.matches_runtime(runtime) {
            return Ok(RootProjectionProgress::ConversationMismatch);
        }
        match runtime.delegated_projection_refusal() {
            Some(DelegatedProjectionRefusal::PendingCommit) => {
                return Ok(RootProjectionProgress::PendingCommit);
            }
            Some(DelegatedProjectionRefusal::PersistenceFailed) => {
                return Ok(RootProjectionProgress::RequiresReopen);
            }
            None => {}
        }

        let refresh = match self
            .pending_projection
            .as_ref()
            .expect("pending projection checked above")
        {
            PendingRootProjection::RefreshLog {
                sync_running_roster,
                orphaned_link,
            } => Some((*sync_running_roster, orphaned_link.clone())),
            _ => None,
        };
        if let Some((sync_running_roster, orphaned_link)) = refresh {
            let refresh_child = match orphaned_link {
                Some(link) => self.persist_orphaned_link(runtime, link).await?,
                None => None,
            };
            if sync_running_roster {
                self.sync_roster(runtime, AgentStatus::Running).await?;
            }
            self.undelivered |= self.show(runtime).await?;
            if let Some(child) = refresh_child {
                self.refresh_child(runtime, &child).await?;
            }
            self.pending_projection.take();
            return Ok(RootProjectionProgress::Applied);
        }

        match self
            .pending_projection
            .as_ref()
            .expect("pending projection checked above")
        {
            PendingRootProjection::RefreshLog { .. } => unreachable!("refresh handled above"),
            PendingRootProjection::RunnerEvent { child, event, .. } => {
                runtime
                    .project_delegated(event)
                    .map_err(anyhow::Error::from)?;
                let child = child.clone();
                self.refresh_child(runtime, &child).await?;
            }
            // `apply_collaboration` takes this result through `take_stop_settlement` so its
            // report can return exact child-owned input. Keeping it staged is safe if a caller is
            // only advancing the root projection slot.
            PendingRootProjection::StopSettled { .. } => {
                return Ok(RootProjectionProgress::Idle);
            }
        }
        self.pending_projection.take();
        Ok(RootProjectionProgress::Applied)
    }

    fn prepare_runner(&mut self, update: OwnedRunnerUpdate) -> Option<PendingRootProjection> {
        let (identity, event, recovery) = match update {
            OwnedRunnerUpdate::StopSettled { identity, outcome } => {
                let agent = self
                    .announced
                    .get(&identity.endpoint().conversation)
                    .cloned()?;
                return Some(PendingRootProjection::StopSettled { agent, outcome });
            }
            OwnedRunnerUpdate::Runtime { identity, update } => match *update {
                RuntimeUpdate::Event(envelope) => {
                    (identity, Some(envelope.event), RecoverySource::ChildJournal)
                }
                RuntimeUpdate::Report(_) => return None,
                RuntimeUpdate::Finished => (identity, None, RecoverySource::None),
            },
            OwnedRunnerUpdate::Failed { identity, .. }
            | OwnedRunnerUpdate::WorkerFailed { identity }
            | OwnedRunnerUpdate::WakeRejected { identity, .. } => {
                (identity, None, RecoverySource::None)
            }
            _ => return None,
        };
        let agent_id = self
            .announced
            .get(&identity.endpoint().conversation)
            .cloned()?;
        let event = match event {
            Some(event) => event,
            None => ConversationEvent::AgentStatusChanged {
                agent_id: agent_id.clone(),
                status: AgentStatus::Failed,
            },
        };
        let event =
            self.prepare_forwarded_event(&identity.endpoint().conversation, &agent_id, event)?;
        Some(PendingRootProjection::RunnerEvent {
            child: identity.endpoint().conversation.clone(),
            generation: identity.generation(),
            event: Box::new(event),
            recovery,
        })
    }

    pub(super) fn prepare_forwarded_event(
        &mut self,
        conversation: &ConversationId,
        agent_id: &AgentId,
        mut event: ConversationEvent,
    ) -> Option<ConversationEvent> {
        if !forwarded(&event) {
            return None;
        }
        if let Some(prefix) = self.replayed_prefix.get_mut(conversation) {
            if prefix.front() == Some(&event) {
                prefix.pop_front();
                if prefix.is_empty() {
                    self.replayed_prefix.remove(conversation);
                }
                return None;
            }
            self.replayed_prefix.remove(conversation);
        }
        *event.agent_mut() = agent_id.clone();
        Some(event)
    }
}
