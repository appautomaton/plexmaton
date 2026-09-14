//! The root composition that turns the collaboration backend into a running product surface.
//!
//! Everything below already existed and had no caller: the ledger, the owner, the child factory and
//! the four typed tools were reachable only from tests, so `delegate` never appeared in a model's
//! tools and the roster had nothing to list. This module is the one place that binds them to the
//! executable's own conversation, and the one place that turns their activity into the events the
//! TUI already knows how to draw.

use std::{collections::BTreeSet, path::Path};

use anyhow::Context as _;
use plexmaton_agent::collaboration::CollaborationLimits;
use plexmaton_core::{
    AgentId, AgentStatus, CollaborationId, CollaborationItemId, ConversationEvent,
    ConversationEventEnvelope, ConversationId, TurnId,
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
    /// Children already on the roster, keyed by the conversation a runner update names. A second
    /// `AgentCreated` for one agent is a reduce error, and resume and a settlement both announce.
    announced: BTreeSet<ConversationId>,
    /// Root inclusions issued so far, which name each one. Identity must be stable across a retry
    /// and distinct across turns, and a counter is both without consulting the log.
    delivered: u64,
    /// Mail reached the log while the root was mid-turn. Nothing else will settle on its own, so
    /// without this the letter waits forever for an activity that never comes.
    undelivered: bool,
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
    let mut owner = OwnedCollaboration::new(writer, limits);
    let ingress = owner
        .open_main_ingress()
        .map_err(|error| anyhow::anyhow!("open the Main collaboration lane: {error}"))?;
    Ok((
        Collaboration {
            owner,
            announced: BTreeSet::new(),
            delivered: 0,
            undelivered: false,
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
    /// and puts them back on the roster.
    pub(crate) async fn restore(
        &mut self,
        runtime: &mut LiveRuntime,
    ) -> anyhow::Result<Vec<ConversationEventEnvelope>> {
        self.sync_roster(runtime, AgentStatus::Idle).await
    }

    /// Puts every canonical delegation this root owns on the roster, announcing only the new ones.
    ///
    /// Both resume and a fresh settlement land here, because a delegation's identity on the roster
    /// is its child conversation and only the registry knows which conversation a target addresses.
    async fn sync_roster(
        &mut self,
        runtime: &mut LiveRuntime,
        status: AgentStatus,
    ) -> anyhow::Result<Vec<ConversationEventEnvelope>> {
        let targets = self
            .owner
            .register_collaboration_targets()
            .await
            .map_err(|error| anyhow::anyhow!("register delegated targets: {error}"))?;
        let children: Vec<ConversationId> = targets
            .iter()
            .map(|target| target.worker().conversation.clone())
            .collect();
        let mut events = Vec::new();
        for child in children {
            events.extend(self.announce(runtime, child, status)?);
        }
        Ok(events)
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
    ) -> anyhow::Result<Vec<ConversationEventEnvelope>> {
        match activity {
            OwnedCollaborationActivity::Ingress(settlement) => match settlement.result() {
                Ok(CollaborationIngressOutcome::Delegated { .. }) => {
                    self.sync_roster(runtime, AgentStatus::Running).await
                }
                // Mail a child sends has no wake of its own: a wake addresses an owned runner and
                // the root is not one, it is the user's conversation. Without this the delegation
                // is one-way — work goes out and no answer ever comes back.
                Ok(CollaborationIngressOutcome::MailAccepted) => {
                    self.deliver_to_root(runtime).await
                }
                Ok(_) | Err(_) => Ok(Vec::new()),
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
    ) -> anyhow::Result<Vec<ConversationEventEnvelope>> {
        if self.undelivered {
            self.deliver_to_root(runtime).await
        } else {
            Ok(Vec::new())
        }
    }

    async fn deliver_to_root(
        &mut self,
        runtime: &mut LiveRuntime,
    ) -> anyhow::Result<Vec<ConversationEventEnvelope>> {
        let turn = TurnId::new(format!("turn-root-mail-{}", self.delivered))
            .context("build a root collaboration turn identity")?;
        let Ok((boundary, previous)) = runtime.collaboration_boundary(turn) else {
            return Ok(Vec::new());
        };
        let item = CollaborationItemId::new(format!("root-inclusion-{}", self.delivered))
            .context("build a root inclusion identity")?;
        let Ok(resolved) = self.owner.admit_root_turn(item, boundary, previous).await else {
            return Ok(Vec::new());
        };
        self.delivered = self.delivered.saturating_add(1);
        self.undelivered = false;
        runtime
            .start_collaboration_turn(resolved)
            .await
            .context("give the root its delegated mail")?;
        Ok(Vec::new())
    }

    /// Moves one child's roster status, which is all of a runner update the root can show today.
    fn project_runner(
        &mut self,
        runtime: &mut LiveRuntime,
        update: OwnedRunnerUpdate,
    ) -> anyhow::Result<Vec<ConversationEventEnvelope>> {
        let (identity, status) = match &update {
            // A child reports its own lifecycle in its own conversation's events. Re-addressing
            // that one event is how the roster learns a child stopped; nothing else in a child's
            // stream belongs to the root, because its transcript is its own conversation's.
            OwnedRunnerUpdate::Runtime { identity, update } => (
                identity,
                match update.as_ref() {
                    RuntimeUpdate::Event(envelope) => match &envelope.event {
                        ConversationEvent::AgentStatusChanged { status, .. } => *status,
                        _ => return Ok(Vec::new()),
                    },
                    RuntimeUpdate::Report(_) => return Ok(Vec::new()),
                    RuntimeUpdate::Finished => AgentStatus::Completed,
                },
            ),
            OwnedRunnerUpdate::Failed { identity, .. }
            | OwnedRunnerUpdate::WorkerFailed { identity }
            | OwnedRunnerUpdate::WakeRejected { identity, .. } => (identity, AgentStatus::Failed),
            _ => return Ok(Vec::new()),
        };
        if !self.announced.contains(&identity.endpoint().conversation) {
            return Ok(Vec::new());
        }
        let agent_id = AgentId::new(identity.endpoint().conversation.as_str())
            .context("build a delegated agent identity")?;
        Ok(runtime.project_delegated(ConversationEvent::AgentStatusChanged { agent_id, status }))
    }

    /// Settles retained work before joining every runner and then the writer.
    pub(crate) async fn shutdown(&mut self) {
        if self.owner.begin_shutdown().await.is_ok() {
            let _ = self.owner.finish_shutdown().await;
        }
    }

    /// The root numbers the event, because the roster shares one sequence with the conversation:
    /// a second counter here arrives stale and the projection drops it.
    fn announce(
        &mut self,
        runtime: &mut LiveRuntime,
        child: ConversationId,
        status: AgentStatus,
    ) -> anyhow::Result<Vec<ConversationEventEnvelope>> {
        if !self.announced.insert(child.clone()) {
            return Ok(Vec::new());
        }
        let agent_id = AgentId::new(child.as_str()).context("build a delegated agent identity")?;
        // The runtime chose this child, so the roster shows what it is rather than who it is: the
        // delegate tool carries no name, and a target selector is not one (CTL-1).
        Ok(runtime.project_delegated(ConversationEvent::AgentCreated {
            agent_id,
            label: "Delegated".to_owned(),
            status,
        }))
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
