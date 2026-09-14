//! The root composition that turns the collaboration backend into a running product surface.
//!
//! Everything below already existed and had no caller: the ledger, the owner, the child factory and
//! the four typed tools were reachable only from tests, so `delegate` never appeared in a model's
//! tools and the roster had nothing to list. This module is the one place that binds them to the
//! executable's own conversation, and the one place that turns their activity into the events the
//! TUI already knows how to draw.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use anyhow::Context as _;
use plexmaton_agent::collaboration::{CollaborationItemRef, CollaborationLimits, MailEndpoint};
use plexmaton_core::{
    AgentId, AgentStatus, CollaborationId, CollaborationItemId, ConversationEvent, ConversationId,
    TranscriptItemId, TurnId,
};
use plexmaton_runtime::{
    CollaborationIngressOutcome, CollaborationWriter, DelegatedChildFactory, LiveRuntime,
    MainCollaborationIngress, OwnedCollaboration, OwnedCollaborationActivity, OwnedRunnerUpdate,
    RuntimeUpdate, SchedulerLimits,
};
use plexmaton_session_store::{DelegatedConversationDirectory, collaboration::CollaborationFile};

/// Children a root may run at once. Delegation history is unbounded; concurrency is not.
const RUNNERS: usize = 4;

/// Prefix of every inclusion this composition root admits for its own conversation.
const ROOT_INCLUSION: &str = "root-inclusion-";

/// One root's collaboration log, its owner, and the Main tool lane bound to this executable.
pub(crate) struct Collaboration {
    owner: OwnedCollaboration,
    /// Children on the roster and the short name each was given, keyed by the conversation a
    /// runner update names. A second `AgentCreated` for one agent is a reduce error, and resume and
    /// a settlement both announce, so the map is also what makes announcing idempotent.
    announced: BTreeMap<ConversationId, AgentId>,
    /// Root inclusions issued so far, which name each one. Identity must be stable across a retry
    /// and distinct across turns, and a counter seeded from the log is both.
    ///
    /// Seeded rather than restarted: a counter that begins at zero every launch renames a resumed
    /// session's first inclusion after the original session's first one. The log then recognises
    /// the item, hands back that earlier admission, and the new letter is delivered inside a turn
    /// frozen before it existed — the root answers, having never been shown what arrived.
    delivered: u64,
    /// Mail reached the log while the root was mid-turn. Nothing else will settle on its own, so
    /// without this the letter waits forever for an activity that never comes.
    undelivered: bool,
    /// Where the root receives mail, taken at bind time because binding consumes the proof.
    root: Option<MailEndpoint>,
    /// Letters already on screen. The snapshot is the whole correspondence every time, and resume
    /// replays it from the log, so the projection has to be the part that knows what is new.
    shown: BTreeSet<CollaborationItemRef>,
    /// Where a child's own journal lives, so its history can be read back after a restart.
    children: DelegatedConversationDirectory,
    /// The task each delegation was last drawn with. The log keeps only the current task, so the
    /// text itself is what separates "already on screen" from "Main changed it".
    assigned: BTreeMap<ConversationId, String>,
}

/// Opens or reopens the log for one root conversation and hands back its unbound Main tool lane.
///
/// The caller must install the lane on its tool catalog before building the runtime, then call
/// [`Collaboration::bind`] with that runtime: the lane carries no authorship until it is sealed to
/// the exact runtime instance that owns it (CTL-1).
pub(crate) fn open(
    plexmaton_home: &Path,
    conversation: &ConversationId,
) -> anyhow::Result<(Collaboration, MainCollaborationIngress)> {
    let children = DelegatedConversationDirectory::under(plexmaton_home)
        .context("open the delegated session directory")?;
    let path = plexmaton_home
        .join("collaborations")
        .join(format!("{}.jsonl", conversation.as_str()));
    let id = CollaborationId::new(conversation.as_str())
        .context("derive a collaboration identity from the conversation")?;
    let file = if path.exists() {
        CollaborationFile::open(&path).context("open this conversation's collaboration log")?
    } else {
        CollaborationFile::create(&path, id, CollaborationLimits::default())
            .context("create this conversation's collaboration log")?
    };
    // How many root inclusions this log already holds, so the counter continues rather than
    // restarts. Rejected: a per-launch clock reading — a rollback repeats one, and a clock before
    // the epoch collapsed every launch onto the same value, which is the collision it was added to
    // prevent. The log is the only thing that actually knows.
    let delivered = u64::try_from(
        file.ledger()
            .records()
            .iter()
            .filter(|record| record.id.as_str().starts_with(ROOT_INCLUSION))
            .count(),
    )
    .unwrap_or(u64::MAX);
    let writer = CollaborationWriter::spawn(file).context("start the collaboration writer")?;
    let limits = SchedulerLimits::new(RUNNERS)
        .context("child runner capacity must be nonzero")
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let mut owner = OwnedCollaboration::new(writer, limits);
    let ingress = owner
        .open_main_ingress()
        .map_err(|error| anyhow::anyhow!("open the Main collaboration lane: {error}"))?;
    Ok((
        Collaboration {
            owner,
            announced: BTreeMap::new(),
            children,
            delivered,
            undelivered: false,
            root: None,
            shown: BTreeSet::new(),
            assigned: BTreeMap::new(),
        },
        ingress,
    ))
}

impl Collaboration {
    /// Seals Main authorship to the runtime that holds the lane, and gives it a child factory.
    ///
    /// A runtime that is not user-owned has no Main identity to offer, so this reports the refusal
    /// rather than leaving a lane that would accept a `delegate` call it could never attribute.
    pub(crate) fn bind(
        &mut self,
        runtime: &LiveRuntime,
        factory: DelegatedChildFactory,
    ) -> anyhow::Result<()> {
        let identity = runtime
            .main_collaboration_identity()
            .context("this runtime cannot hold Main collaboration authorship")?;
        self.root = Some(identity.endpoint().clone());
        self.owner
            .bind_main_runtime(identity)
            .map_err(|error| anyhow::anyhow!("bind Main collaboration authorship: {error}"))?;
        self.owner
            .bind_child_factory(factory)
            .map_err(|error| anyhow::anyhow!("bind the delegated child factory: {error}"))?;
        Ok(())
    }

    /// Rebuilds every selector and child capability a previous run created (CTL-1).
    ///
    /// Resume does not wake a child (CHB-3); this only makes existing delegations addressable again
    /// and puts them back on the roster, with the correspondence they already exchanged.
    pub(crate) async fn restore(&mut self, runtime: &mut LiveRuntime) -> anyhow::Result<()> {
        // A conversation that once read its mail carries that turn's collaboration atom in its
        // journal. Nothing else hands the admission back after a restart, and a reference that
        // cannot resolve refuses every later turn the user types.
        self.owner
            .restore_root_context(runtime)
            .await
            .map_err(|error| {
                anyhow::anyhow!("restore this root's collaboration context: {error}")
            })?;
        self.sync_roster(runtime, AgentStatus::Idle).await?;
        self.replay_children(runtime)?;
        // Restoring draws the whole correspondence and answers none of it: a letter the root
        // already replied to before it exited must not earn a second turn every launch.
        let _restored = self.show_mail(runtime).await?;
        Ok(())
    }

    /// Reads back what each child did in an earlier process, from the child's own journal.
    ///
    /// A live child streams its work through its runner and [`Self::project_runner`] forwards it.
    /// A resumed one has no runner and never will until something addresses it, so its history is
    /// only in its journal — and without this a conversation reopened tomorrow shows the ask and
    /// the answer with the work between them missing. Reading is not waking (CHB-3): the journal is
    /// opened, projected and closed without constructing a runtime or dispatching anything.
    fn replay_children(&mut self, runtime: &mut LiveRuntime) -> anyhow::Result<()> {
        let children: Vec<(ConversationId, AgentId)> = self
            .announced
            .iter()
            .map(|(conversation, agent)| (conversation.clone(), agent.clone()))
            .collect();
        for (conversation, agent_id) in children {
            // A child whose journal is missing or held elsewhere keeps its roster row and its
            // correspondence; only the work between them is unavailable.
            let Ok(file) = self.children.resume(&conversation) else {
                continue;
            };
            let journal = file.journal();
            let Ok(projection) = journal.project(journal.selected_head()) else {
                continue;
            };
            for envelope in projection.events() {
                let mut event = envelope.event.clone();
                if !forwarded(&event) {
                    continue;
                }
                *event.agent_mut() = agent_id.clone();
                runtime.project_delegated(event);
            }
        }
        Ok(())
    }

    /// Puts every canonical delegation this root owns on the roster, announcing only the new ones.
    ///
    /// Both resume and a fresh settlement land here, because a delegation's identity on the roster
    /// is its child conversation and only the registry knows which conversation a target addresses.
    async fn sync_roster(
        &mut self,
        runtime: &mut LiveRuntime,
        status: AgentStatus,
    ) -> anyhow::Result<()> {
        let targets = self
            .owner
            .register_collaboration_targets()
            .await
            .map_err(|error| anyhow::anyhow!("register delegated targets: {error}"))?;
        let children: Vec<ConversationId> = targets
            .iter()
            .map(|target| target.worker().conversation.clone())
            .collect();
        for child in children {
            self.announce(runtime, child, status)?;
        }
        self.show_tasks(runtime).await
    }

    /// Draws what Main has asked of each child, on both sides, whenever the ask changes.
    ///
    /// A task is the other direction of the same conversation: `delegate` and `update_task` are how
    /// Main addresses a child, and until this ran nothing Main sent downward appeared anywhere. The
    /// log keeps one current task per delegation rather than a history, so the revision is the only
    /// thing that says whether this is the ask already drawn or a new one.
    async fn show_tasks(&mut self, runtime: &mut LiveRuntime) -> anyhow::Result<()> {
        let targets = self
            .owner
            .register_collaboration_targets()
            .await
            .map_err(|error| anyhow::anyhow!("register delegated targets: {error}"))?;
        for target in &targets {
            let child = target.worker().conversation.clone();
            let Some(to) = self.announced.get(&child).cloned() else {
                continue;
            };
            let Ok(view) = self
                .owner
                .delegation_view(target.delegation().clone())
                .await
            else {
                continue;
            };
            let revision = view.revision.0;
            let task = view.task.as_str().to_owned();
            // Compared by text, because `HandoffCompleted` advances the same revision counter and
            // would otherwise redraw an ask nobody changed.
            if self.assigned.get(&child) == Some(&task) {
                continue;
            }
            self.assigned.insert(child, task.clone());
            // Both ends named the way the roster names them, as mail is. Reading the delegator
            // straight out of the log works only while the root's agent id is a constant.
            let Some(from) = self.endpoint_name(&view.delegator) else {
                continue;
            };
            for (owner, side) in [(from.clone(), "out"), (to.clone(), "in")] {
                let item_id = TranscriptItemId::new(format!(
                    "task-{}-r{revision}-{side}",
                    target.delegation().as_str()
                ))
                .context("build a task entry identity")?;
                runtime.project_delegated(ConversationEvent::TaskAssigned {
                    agent_id: owner,
                    item_id,
                    from: from.clone(),
                    to: to.clone(),
                    task: task.clone(),
                });
            }
        }
        Ok(())
    }

    /// The next thing the collaboration owner did, for the caller's select loop.
    pub(crate) async fn next(&mut self) -> Option<OwnedCollaborationActivity> {
        self.owner.next_activity().await
    }

    /// Projects one settled activity into the conversation events the TUI already draws.
    ///
    /// The collaboration log is the durable record; these events are a projection over it, which is
    /// why nothing here writes to the session journal.
    pub(crate) async fn apply(
        &mut self,
        runtime: &mut LiveRuntime,
        activity: OwnedCollaborationActivity,
    ) -> anyhow::Result<()> {
        match activity {
            OwnedCollaborationActivity::Ingress(settlement) => match settlement.result() {
                Ok(CollaborationIngressOutcome::Delegated { .. }) => {
                    self.sync_roster(runtime, AgentStatus::Running).await
                }
                // Only a letter that arrived earns the root a turn; waking it over its own
                // outgoing letter is how it ends up answering itself. The letter is drawn first,
                // and answering it is left to the caller, which starts that turn only once these
                // events are on screen.
                Ok(CollaborationIngressOutcome::MailAccepted) => {
                    self.undelivered |= self.show_mail(runtime).await?;
                    Ok(())
                }
                // Main changing what it asked of a child is the other direction of the same
                // conversation, and is drawn the moment it settles rather than at the next restart.
                Ok(CollaborationIngressOutcome::TaskUpdated) => self.show_tasks(runtime).await,
                Ok(_) | Err(_) => Ok(()),
            },
            // A child's own transcript belongs to its own journal, but its lifecycle belongs on the
            // roster: an agent stuck at `running` forever is the panel lying about what it knows.
            OwnedCollaborationActivity::Runner(update) => self.project_runner(runtime, update),
        }
    }

    /// Gives the root a turn over whatever its inbox holds, once it is free to take one.
    ///
    /// Mail almost always lands while the root is still finishing the turn that sent the work, and
    /// a busy conversation cannot open a second turn. Nothing else settles afterwards, so the
    /// retry has to hang off the root's own progress rather than the collaboration owner's.
    pub(crate) async fn deliver_pending(
        &mut self,
        runtime: &mut LiveRuntime,
    ) -> anyhow::Result<()> {
        if self.undelivered {
            self.deliver_to_root(runtime).await
        } else {
            Ok(())
        }
    }

    async fn deliver_to_root(&mut self, runtime: &mut LiveRuntime) -> anyhow::Result<()> {
        let turn = TurnId::new(format!("turn-root-mail-{}", self.delivered))
            .context("build a root collaboration turn identity")?;
        // A refusal here is almost always "the root is mid-turn", which resolves on its own. The
        // flag is what makes that true: without it the letter is dropped and never retried.
        let Ok((boundary, previous)) = runtime.collaboration_boundary(turn) else {
            self.undelivered = true;
            return Ok(());
        };
        let item = CollaborationItemId::new(format!("{ROOT_INCLUSION}{}", self.delivered))
            .context("build a root inclusion identity")?;
        let Ok(resolved) = self.owner.admit_root_turn(item, boundary, previous).await else {
            self.undelivered = true;
            return Ok(());
        };
        self.delivered = self.delivered.saturating_add(1);
        self.undelivered = false;
        runtime
            .start_collaboration_turn(resolved)
            .await
            .context("give the root its delegated mail")?;
        Ok(())
    }

    /// Draws every letter of this root's correspondence not on screen yet, and says whether any of
    /// them arrived for the root (CMP-1).
    ///
    /// Both directions, because the inspector is the inspected agent's own conversation and shows
    /// incoming as well as outgoing mail. Rejected: keeping only what arrived, which is what made
    /// every letter Main sent invisible; filing a letter under its producer alone, which left the
    /// root answering a question the user could see no trace of having been asked; and writing a
    /// `MailDelivered` journal entry, which survives restart for free but gives one letter two
    /// durable homes — the roadmap locks revisions to reconcile against the collaboration log.
    async fn show_mail(&mut self, runtime: &mut LiveRuntime) -> anyhow::Result<bool> {
        let Some(root) = self.root.clone() else {
            return Ok(false);
        };
        // A root that has never been written to is not an endpoint the ledger knows, and an empty
        // inbox is not a failure worth reporting to the session loop.
        let Ok(snapshot) = self.owner.mail_snapshot(root.clone()).await else {
            return Ok(false);
        };
        let mut arrived = false;
        for mail in snapshot.items() {
            if self.shown.contains(mail.reference()) {
                continue;
            }
            let envelope = mail.envelope();
            // Both directions. The snapshot is the root's whole correspondence, and keeping only
            // what arrived is what made every letter Main sent invisible. Each end is named the way
            // the roster names it, and a session the roster has never heard of is one the user
            // cannot be shown either end of.
            let (Some(from), Some(to)) = (
                self.endpoint_name(&envelope.from),
                self.endpoint_name(&envelope.to),
            ) else {
                continue;
            };
            arrived |= envelope.to.conversation == root.conversation;
            self.shown.insert(mail.reference().clone());
            // One transcript item per side, over one mail identity: an item belongs to exactly one
            // conversation, and the sender's copy and the recipient's are different items.
            for (owner, side) in [(from.clone(), "out"), (to.clone(), "in")] {
                let item_id = TranscriptItemId::new(format!(
                    "mail-{}-{side}",
                    mail.reference().item.as_str()
                ))
                .context("build a mail entry identity")?;
                runtime.project_delegated(ConversationEvent::MailDelivered {
                    agent_id: owner,
                    item_id,
                    mail_id: envelope.id.clone(),
                    from: from.clone(),
                    to: to.clone(),
                    summary: envelope.summary.as_str().to_owned(),
                });
            }
        }
        Ok(arrived)
    }

    /// Names either end of a letter the way the roster names it, root included.
    fn endpoint_name(&self, endpoint: &MailEndpoint) -> Option<AgentId> {
        if Some(&endpoint.conversation) == self.root.as_ref().map(|root| &root.conversation) {
            return self.root.as_ref().map(|root| root.agent.clone());
        }
        self.announced.get(&endpoint.conversation).cloned()
    }

    /// Shows what a child is doing, in the root's projection, under the name the roster gave it.
    ///
    /// A child is a separate conversation: it numbers its own events and names itself by its own
    /// agent identity, so its envelopes cannot be forwarded as they are — the sequence would arrive
    /// stale beside the root's own, and the name would belong to nobody the panel has heard of.
    /// Each fact is re-addressed and renumbered instead.
    ///
    /// What is forwarded is the child's work: what it said, what it ran, and what went wrong. Its
    /// own `AgentCreated` is not, because the roster already announced it under a different name;
    /// nor are its letters and tasks, which the collaboration log already draws on both sides and
    /// which would otherwise appear twice.
    fn project_runner(
        &mut self,
        runtime: &mut LiveRuntime,
        update: OwnedRunnerUpdate,
    ) -> anyhow::Result<()> {
        let (identity, event) = match update {
            OwnedRunnerUpdate::Runtime { identity, update } => match *update {
                RuntimeUpdate::Event(envelope) => (identity, Some(envelope.event)),
                RuntimeUpdate::Report(_) => return Ok(()),
                RuntimeUpdate::Finished => (identity, None),
            },
            OwnedRunnerUpdate::Failed { identity, .. }
            | OwnedRunnerUpdate::WorkerFailed { identity }
            | OwnedRunnerUpdate::WakeRejected { identity, .. } => (identity, None),
            _ => return Ok(()),
        };
        let Some(agent_id) = self
            .announced
            .get(&identity.endpoint().conversation)
            .cloned()
        else {
            return Ok(());
        };
        // A runner that ended without saying so leaves the roster claiming it is still working.
        let Some(mut event) = event else {
            runtime.project_delegated(ConversationEvent::AgentStatusChanged {
                agent_id,
                status: AgentStatus::Failed,
            });
            return Ok(());
        };
        if !forwarded(&event) {
            return Ok(());
        }
        *event.agent_mut() = agent_id;
        runtime.project_delegated(event);
        Ok(())
    }

    /// Settles retained work before joining every runner and then the writer.
    pub(crate) async fn shutdown(&mut self) {
        if self.owner.begin_shutdown().await.is_ok() {
            let _ = self.owner.finish_shutdown().await;
        }
    }

    /// Puts one child on the roster under a short name, once.
    ///
    /// The name is the position this root delegated it in, not its conversation: a conversation id
    /// is forty characters of identity, and the moment mail names its sender on screen that spends
    /// the whole row. The runtime chose this child, so the name says what it is rather than who it
    /// is — the delegate tool carries none, and a target selector is not one (CTL-1). The root
    /// numbers the event too, because the roster shares one sequence with the conversation: a
    /// second counter here arrives stale and the projection drops it.
    fn announce(
        &mut self,
        runtime: &mut LiveRuntime,
        child: ConversationId,
        status: AgentStatus,
    ) -> anyhow::Result<()> {
        if self.announced.contains_key(&child) {
            return Ok(());
        }
        let position = self.announced.len().saturating_add(1);
        let agent_id = AgentId::new(format!("delegated-{position}"))
            .context("build a delegated agent identity")?;
        self.announced.insert(child, agent_id.clone());
        runtime.project_delegated(ConversationEvent::AgentCreated {
            agent_id,
            label: format!("Delegated {position}"),
            status,
        });
        Ok(())
    }
}

/// Builds the factory that creates children, using the root's own model and workspace tools.
///
/// CHB-1's floor is applied inside the factory; the catalog handed here is the root's, narrowed
/// there rather than trusted to be narrow already.
pub(crate) fn child_factory(
    plexmaton_home: &Path,
    model: plexmaton_provider::ResolvedModel,
    key: plexmaton_provider::ApiKey,
    tools: plexmaton_runtime::NativeToolCatalog,
) -> anyhow::Result<DelegatedChildFactory> {
    let directory = DelegatedConversationDirectory::under(plexmaton_home)
        .context("open the delegated session directory")?;
    Ok(DelegatedChildFactory::new(directory, model, key, tools))
}

/// Whether one of a child's own facts belongs in the root's view of it.
///
/// Its transcript, its tools and its failures are what the user opened the child to read. Its
/// identity and its correspondence are already on screen under names this root chose, and its
/// token usage has no surface to appear on.
const fn forwarded(event: &ConversationEvent) -> bool {
    matches!(
        event,
        ConversationEvent::AgentStatusChanged { .. }
            | ConversationEvent::TranscriptItemStarted { .. }
            | ConversationEvent::TranscriptDelta { .. }
            | ConversationEvent::TranscriptItemFinalized { .. }
            | ConversationEvent::ToolCallChanged { .. }
            | ConversationEvent::ArtifactAnnounced { .. }
            | ConversationEvent::RuntimeWarning { .. }
            | ConversationEvent::RuntimeError { .. }
    )
}
