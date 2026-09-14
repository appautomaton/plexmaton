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
use plexmaton_agent::collaboration::{
    CollaborationItemRef, CollaborationLimits, MailDirection, MailEndpoint,
};
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

/// One root's collaboration log, its owner, and the Main tool lane bound to this executable.
pub(crate) struct Collaboration {
    owner: OwnedCollaboration,
    /// Children on the roster and the short name each was given, keyed by the conversation a
    /// runner update names. A second `AgentCreated` for one agent is a reduce error, and resume and
    /// a settlement both announce, so the map is also what makes announcing idempotent.
    announced: BTreeMap<ConversationId, AgentId>,
    /// Root inclusions issued so far, which name each one together with [`Collaboration::run`].
    /// Identity must be stable across a retry and distinct across turns, and a counter is both
    /// without consulting the log.
    delivered: u64,
    /// Distinguishes this process's root inclusions from the ones a previous run already appended.
    ///
    /// The counter restarts at zero every launch, so on its own it renames the first letter of a
    /// resumed session after the first letter of the original one. The log then recognises the
    /// item, hands back that earlier admission, and the new letter is delivered inside a turn
    /// frozen before it existed — the root answers, having never been shown what arrived.
    run: String,
    /// Mail reached the log while the root was mid-turn. Nothing else will settle on its own, so
    /// without this the letter waits forever for an activity that never comes.
    undelivered: bool,
    /// Where the root receives mail, taken at bind time because binding consumes the proof.
    root: Option<MailEndpoint>,
    /// Letters already on screen. The snapshot is the whole correspondence every time, and resume
    /// replays it from the log, so the projection has to be the part that knows what is new.
    shown: BTreeSet<CollaborationItemRef>,
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
    let writer = CollaborationWriter::spawn(file).context("start the collaboration writer")?;
    let limits = SchedulerLimits::new(RUNNERS)
        .context("child runner capacity must be nonzero")
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    // A clock reading is enough to separate one launch from the next: the journal takes a writer
    // lock, so two processes never hold this conversation at the same instant.
    let run = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos())
        .to_string();
    let mut owner = OwnedCollaboration::new(writer, limits);
    let ingress = owner
        .open_main_ingress()
        .map_err(|error| anyhow::anyhow!("open the Main collaboration lane: {error}"))?;
    Ok((
        Collaboration {
            owner,
            announced: BTreeMap::new(),
            delivered: 0,
            run,
            undelivered: false,
            root: None,
            shown: BTreeSet::new(),
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
        self.show_mail(runtime).await
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
                // Mail a child sends has no wake of its own: a wake addresses an owned runner and
                // the root is not one, it is the user's conversation. Without this the delegation
                // is one-way — work goes out and no answer ever comes back.
                // The letter is drawn, and answering it is left to the caller, which starts that
                // turn only after these events are on screen. Starting it here would number the
                // letter before the turn and then emit it after, and the projection drops an event
                // that arrives behind one already applied.
                Ok(CollaborationIngressOutcome::MailAccepted) => {
                    self.undelivered = true;
                    self.show_mail(runtime).await
                }
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
        let turn = TurnId::new(format!("turn-root-mail-{}-{}", self.run, self.delivered))
            .context("build a root collaboration turn identity")?;
        // A refusal here is almost always "the root is mid-turn", which resolves on its own. The
        // flag is what makes that true: without it the letter is dropped and never retried.
        let Ok((boundary, previous)) = runtime.collaboration_boundary(turn) else {
            self.undelivered = true;
            return Ok(());
        };
        let item =
            CollaborationItemId::new(format!("root-inclusion-{}-{}", self.run, self.delivered))
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

    /// Draws every letter the root has been sent that is not on screen yet (CMP-1).
    ///
    /// Each letter reaches both conversations, because the inspector is the inspected agent's own
    /// conversation and it shows incoming as well as outgoing mail: the child's side says what it
    /// sent, the root's says what arrived. Rejected: filing a letter only under its producer, which
    /// left the root answering a question the user could see no trace of having been asked; and
    /// writing a `MailDelivered` journal entry, which survives restart for free but gives one letter
    /// two durable homes — the roadmap locks revisions to reconcile against the collaboration log.
    async fn show_mail(&mut self, runtime: &mut LiveRuntime) -> anyhow::Result<()> {
        let Some(root) = self.root.clone() else {
            return Ok(());
        };
        // A root that has never been written to is not an endpoint the ledger knows, and an empty
        // inbox is not a failure worth reporting to the session loop.
        let Ok(snapshot) = self.owner.mail_snapshot(root).await else {
            return Ok(());
        };
        for mail in snapshot.items() {
            if mail.direction() != MailDirection::Incoming || self.shown.contains(mail.reference())
            {
                continue;
            }
            let envelope = mail.envelope();
            // The sender is named the way the roster names it. A child's own agent id names nothing
            // the panel has ever heard of, and a letter from a session that is not on the roster is
            // one the user cannot be shown either end of.
            let Some(from) = self.announced.get(&envelope.from.conversation).cloned() else {
                continue;
            };
            let to = envelope.to.agent.clone();
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
        Ok(())
    }

    /// Moves one child's roster status, which is all of a runner update the root can show today.
    fn project_runner(
        &mut self,
        runtime: &mut LiveRuntime,
        update: OwnedRunnerUpdate,
    ) -> anyhow::Result<()> {
        let (identity, status) = match &update {
            // A child reports its own lifecycle in its own conversation's events. Re-addressing
            // that one event is how the roster learns a child stopped; nothing else in a child's
            // stream belongs to the root, because its transcript is its own conversation's.
            OwnedRunnerUpdate::Runtime { identity, update } => (
                identity,
                match update.as_ref() {
                    RuntimeUpdate::Event(envelope) => match &envelope.event {
                        ConversationEvent::AgentStatusChanged { status, .. } => *status,
                        _ => return Ok(()),
                    },
                    RuntimeUpdate::Report(_) => return Ok(()),
                    RuntimeUpdate::Finished => AgentStatus::Completed,
                },
            ),
            OwnedRunnerUpdate::Failed { identity, .. }
            | OwnedRunnerUpdate::WorkerFailed { identity }
            | OwnedRunnerUpdate::WakeRejected { identity, .. } => (identity, AgentStatus::Failed),
            _ => return Ok(()),
        };
        let Some(agent_id) = self
            .announced
            .get(&identity.endpoint().conversation)
            .cloned()
        else {
            return Ok(());
        };
        runtime.project_delegated(ConversationEvent::AgentStatusChanged { agent_id, status });
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
