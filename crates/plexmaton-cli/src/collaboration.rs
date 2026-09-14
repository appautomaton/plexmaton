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
    AgentId, AgentStatus, CollaborationId, ConversationEventEnvelope, ConversationId,
};
use plexmaton_runtime::{
    CollaborationIngressOutcome, CollaborationWriter, DelegatedChildFactory, LiveRuntime,
    MainCollaborationIngress, OwnedCollaboration, OwnedCollaborationActivity, SchedulerLimits,
};
use plexmaton_session_store::{DelegatedConversationDirectory, collaboration::CollaborationFile};

/// Children a root may run at once. Delegation history is unbounded; concurrency is not.
const RUNNERS: usize = 4;

/// One root's collaboration log, its owner, and the Main tool lane bound to this executable.
pub(crate) struct Collaboration {
    owner: OwnedCollaboration,
    /// Targets already on the roster. A second `AgentCreated` for one agent is a reduce error, and
    /// restore and a fresh settlement can name the same delegation if a resume races a retry.
    announced: BTreeSet<String>,
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
        let targets = self
            .owner
            .register_collaboration_targets()
            .await
            .map_err(|error| anyhow::anyhow!("restore delegated targets: {error}"))?;
        let selectors: Vec<String> = targets
            .iter()
            .map(|target| target.selector().as_str().to_owned())
            .collect();
        let mut events = Vec::new();
        for selector in &selectors {
            events.extend(self.announce(runtime, selector, AgentStatus::Idle)?);
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
    pub(crate) fn apply(
        &mut self,
        runtime: &mut LiveRuntime,
        activity: OwnedCollaborationActivity,
    ) -> anyhow::Result<Vec<ConversationEventEnvelope>> {
        match activity {
            OwnedCollaborationActivity::Ingress(settlement) => match settlement.result() {
                Ok(CollaborationIngressOutcome::Delegated { target }) => {
                    let target = target.as_str().to_owned();
                    self.announce(runtime, &target, AgentStatus::Running)
                }
                Ok(_) | Err(_) => Ok(Vec::new()),
            },
            // A runner update carries the child's own lifecycle. Its transcript stays in the child's
            // journal; the roster only needs to know the agent is still working.
            OwnedCollaborationActivity::Runner(_) => Ok(Vec::new()),
        }
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
        target: &str,
        status: AgentStatus,
    ) -> anyhow::Result<Vec<ConversationEventEnvelope>> {
        if !self.announced.insert(target.to_owned()) {
            return Ok(Vec::new());
        }
        let agent_id = AgentId::new(target).context("build a delegated agent identity")?;
        // The runtime chose this child, so the roster shows what it is rather than who it is: the
        // delegate tool carries no name, and a target selector is not one (CTL-1).
        Ok(runtime.announce_delegated(agent_id, "Delegated", status))
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
