//! The root composition that turns the collaboration backend into a running product surface.
//!
//! Everything below already existed and had no caller: the ledger, the owner, the child factory and
//! the four typed tools were reachable only from tests, so `delegate` never appeared in a model's
//! tools and the roster had nothing to list. This module is the one place that binds them to the
//! executable's own conversation, and the one place that turns their activity into the events the
//! TUI already knows how to draw.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::{Path, PathBuf},
};

use anyhow::Context as _;
use plexmaton_agent::collaboration::{CollaborationLimits, MailEndpoint};
use plexmaton_core::{
    AgentId, AgentStatus, ApprovalId, AttentionId, CollaborationId, CollaborationItemId,
    ConversationEvent, ConversationId, TranscriptItemId, TurnId,
};
use plexmaton_runtime::{
    CollaborationRuntimeStamp, CollaborationWriter, DelegatedChildFactory, DispatchReport,
    LiveRuntime, MainCollaborationIngress, OwnedChildControlSnapshot, OwnedCollaboration,
    OwnedCollaborationActivity, OwnedSchedulingError, OwnedShutdownReport, RunnerGeneration,
    SchedulerLimits, UserInputTarget,
};
use plexmaton_session_store::{DelegatedConversationDirectory, collaboration::CollaborationFile};

mod attention;
mod child_control;
pub(crate) use child_control::undelivered_reason;
mod history;
mod pending;
mod placement;
use pending::PendingRootProjection;
pub(crate) use pending::RootProjectionProgress;
#[cfg(debug_assertions)]
mod process_cut;
#[cfg(debug_assertions)]
pub(crate) use process_cut::PendingHandoffProcessCut;

#[cfg(test)]
mod tests;

/// Children a root may run at once. Delegation history is unbounded; concurrency is not.
const RUNNERS: usize = 4;

/// Prefix of every inclusion this composition root admits for its own conversation.
const ROOT_INCLUSION: &str = "root-inclusion-";

#[derive(Clone, Debug, Eq, PartialEq)]
struct LiveApprovalRoute {
    attention_id: AttentionId,
    generation: RunnerGeneration,
}

/// One root's collaboration log, its owner, and the Main tool lane bound to this executable.
pub(crate) struct Collaboration {
    owner: OwnedCollaboration,
    collaboration: CollaborationId,
    /// Durable root identity. A session picker may replace only the runtime, so every projection
    /// checks this before it can consume root-owned activity.
    conversation: ConversationId,
    /// The process-local runtime instance that consumed this composition's Main capability.
    bound_runtime: Option<CollaborationRuntimeStamp>,
    /// One selected root projection retained across a racing journal acknowledgement. The owner
    /// keeps every later activity in its bounded lanes until this exact fact is resolved.
    pending_projection: Option<PendingRootProjection>,
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
    /// Log records already observed for root-turn admission. Display readiness is session-local,
    /// so this cannot also stand in for the two independently anchored transcript rows.
    observed: BTreeSet<CollaborationItemId>,
    /// Shared transcript rows already projected, keyed by their stable per-session identity.
    shown: BTreeSet<TranscriptItemId>,
    /// Sessions with a canonical row waiting for their own durable placement link.
    pending_placement: BTreeSet<AgentId>,
    /// Exact journal prefix a passively reopened child will replay once when it is activated.
    replayed_prefix: BTreeMap<ConversationId, VecDeque<ConversationEvent>>,
    /// Where a child's own journal lives, so its history can be read back after a restart.
    children: DelegatedConversationDirectory,
    /// Authenticated input targets retained separately from display-only controller snapshots.
    user_targets: BTreeMap<ConversationId, UserInputTarget>,
    /// Latest authenticated controller projection for each canonical child Conversation.
    controls: BTreeMap<ConversationId, OwnedChildControlSnapshot>,
    /// Approval routes issued only by newly admitted live child requests in this process.
    live_approvals: BTreeMap<(AgentId, ApprovalId), LiveApprovalRoute>,
}

/// [`open`], off the thread the terminal loop is drawn on.
///
/// Opening reads the whole ledger to seed its delivered counter, takes the log's writer lock and
/// spawns the writer thread. At startup that cost is paid before there is a frame to miss, but the
/// picker opens a conversation while one is on screen, and its loader task shares a thread with the
/// interaction loop — so the file work belongs in the blocking pool (SPK-3).
pub(crate) async fn open_off_thread(
    plexmaton_home: PathBuf,
    conversation: ConversationId,
) -> anyhow::Result<(Collaboration, MainCollaborationIngress)> {
    tokio::task::spawn_blocking(move || open(&plexmaton_home, &conversation))
        .await
        .context("join the collaboration opener")?
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
        CollaborationFile::create(&path, id.clone(), CollaborationLimits::default())
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
            collaboration: id,
            conversation: conversation.clone(),
            bound_runtime: None,
            pending_projection: None,
            announced: BTreeMap::new(),
            children,
            user_targets: BTreeMap::new(),
            controls: BTreeMap::new(),
            live_approvals: BTreeMap::new(),
            delivered,
            undelivered: false,
            root: None,
            observed: BTreeSet::new(),
            shown: BTreeSet::new(),
            pending_placement: BTreeSet::new(),
            replayed_prefix: BTreeMap::new(),
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
        self.ensure_root_conversation(runtime)?;
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
        self.bound_runtime = Some(runtime.collaboration_runtime_stamp());
        Ok(())
    }

    /// Rebuilds every selector and child capability a previous run created (CTL-1).
    ///
    /// Resume does not wake a child (CHB-3); this only makes existing delegations addressable again
    /// and puts them back on the roster, with the correspondence they already exchanged.
    pub(crate) async fn restore(&mut self, runtime: &mut LiveRuntime) -> anyhow::Result<()> {
        self.ensure_bound_runtime(runtime)?;
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
        let records = self
            .owner
            .records()
            .await
            .context("read the collaboration log for restoration")?;
        self.replay_children(runtime, &records).await?;
        let source = runtime
            .collaboration_session_source()
            .context("capture this root's selected collaboration session")?;
        let placements = self
            .session_placements(&source.endpoint().agent, source.journal(), &records)
            .await?;
        // Restoring draws the whole correspondence and answers none of it: a letter the root
        // already replied to before it exited must not earn a second turn every launch.
        let root = self
            .root
            .as_ref()
            .context("restored collaboration has no Main endpoint")?
            .agent
            .clone();
        self.project_session_rows(runtime, &records, &root, &placements, true, false)?;
        self.observe_records(&records);
        runtime.rebuild_delegated_projection().map_err(|error| {
            anyhow::anyhow!("rebuild the durably placed collaboration projection: {error:?}")
        })?;
        Ok(())
    }

    /// The next thing the collaboration owner did, for the caller's select loop.
    pub(crate) async fn next(&mut self) -> Option<OwnedCollaborationActivity> {
        self.owner.next_activity().await
    }

    /// Admits a focused child's Stop through the collaboration owner without waiting for it.
    ///
    /// The workspace names children by their local roster identity. The owner names them by their
    /// durable Conversation, so this lookup is the only translation at the CLI boundary. A missing
    /// or resumed child returns a typed owner refusal; callers deliberately consume that refusal
    /// because a failed child control action must never be redirected to the root runtime (INV-7,
    /// SCH-2, SCH-4).
    pub(crate) fn begin_child_stop(&mut self, agent: &AgentId) -> Result<(), OwnedSchedulingError> {
        let conversation = self
            .announced
            .iter()
            .find_map(|(conversation, announced)| (announced == agent).then_some(conversation))
            .ok_or(OwnedSchedulingError::UnknownRunner)?;
        self.owner.begin_stop(conversation)
    }

    /// Gives the root a turn over whatever arrived, once it is free to take one.
    ///
    /// Mail almost always lands while the root is still finishing the turn that sent the work, and
    /// a busy conversation cannot open a second turn. Nothing else settles afterwards, so the
    /// retry has to hang off the root's own progress rather than the collaboration owner's.
    pub(crate) async fn deliver_pending(
        &mut self,
        runtime: &mut LiveRuntime,
    ) -> anyhow::Result<()> {
        if !self.matches_runtime(runtime) {
            return Ok(());
        }
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
            .start_collaboration_turn(std::sync::Arc::clone(&resolved))
            .await
            .context("give the root its delegated mail")?;
        Ok(())
    }

    /// Names either end of a collaboration fact the way the roster names it, root included.
    fn name(&self, endpoint: &MailEndpoint) -> Option<AgentId> {
        if Some(&endpoint.conversation) == self.root.as_ref().map(|root| &root.conversation) {
            return self.root.as_ref().map(|root| root.agent.clone());
        }
        self.announced.get(&endpoint.conversation).cloned()
    }

    /// Settles retained work before joining every runner and then the writer.
    ///
    /// Durable pending projections are rebuilt from the collaboration log or child journal on
    /// reopen. A transient runner outcome has no such source and therefore becomes an observable
    /// shutdown failure instead of disappearing with this process.
    pub(crate) async fn shutdown(&mut self) -> anyhow::Result<()> {
        // A child already committed this request or resolution, but the root may quit before its
        // own projection is writable. Admit the reference now so passive reopen can join the two
        // canonical sources instead of treating a graceful shutdown as an orphan.
        let attention_failure = self
            .admit_pending_attention_for_shutdown()
            .await
            .err()
            .map(|error| format!("admit pending Attention during shutdown: {error}"));
        let transient = self
            .pending_projection
            .as_ref()
            .and_then(PendingRootProjection::transient_description);
        self.owner
            .begin_shutdown()
            .await
            .map_err(|error| anyhow::anyhow!("begin collaboration shutdown: {error}"))?;
        let mut drained_attention_failures = Vec::new();
        while let Some(update) = self.owner.next_update().await {
            if let Err(error) = self.admit_shutdown_attention_update(&update).await {
                drained_attention_failures.push(format!("admit shutdown Attention: {error}"));
            }
        }
        let mut failures = Vec::new();
        if let Some(failure) = attention_failure {
            failures.push(failure);
        }
        failures.extend(drained_attention_failures);
        match self.owner.finish_shutdown().await {
            Ok(report) => collect_shutdown_failures(&report, &mut failures),
            Err(failure) => {
                collect_shutdown_failures(failure.report(), &mut failures);
                failures.push(format!("join collaboration owner: {failure}"));
            }
        }
        if let Some(transient) = transient {
            failures.push(format!("unresolved root projection: {transient}"));
        }
        if failures.is_empty() {
            Ok(())
        } else {
            anyhow::bail!(failures.join("; "))
        }
    }

    fn matches_runtime(&self, runtime: &LiveRuntime) -> bool {
        runtime.conversation_id() == &self.conversation
            && self.bound_runtime.as_ref() == Some(&runtime.collaboration_runtime_stamp())
    }

    fn ensure_root_conversation(&self, runtime: &LiveRuntime) -> anyhow::Result<()> {
        anyhow::ensure!(
            runtime.conversation_id() == &self.conversation,
            "collaboration root {} cannot bind conversation {}",
            self.conversation,
            runtime.conversation_id()
        );
        Ok(())
    }

    fn ensure_bound_runtime(&self, runtime: &LiveRuntime) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.matches_runtime(runtime),
            "collaboration root {} cannot project through this runtime instance",
            self.conversation
        );
        Ok(())
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
        let event = ConversationEvent::AgentCreated {
            agent_id: agent_id.clone(),
            label: format!("Delegated {position}"),
            status,
        };
        runtime
            .project_delegated_roster(&event)
            .context("project delegated roster entry")?;
        self.announced.insert(child, agent_id);
        Ok(())
    }
}

fn collect_shutdown_failures(report: &OwnedShutdownReport, failures: &mut Vec<String>) {
    for (identity, child) in report.runners() {
        if child != &DispatchReport::default() {
            failures.push(format!(
                "child {} generation {} retained shutdown results: {child:?}",
                identity.endpoint().conversation,
                identity.generation().get()
            ));
        }
    }
    if !report.settlements().is_empty() {
        failures.push(format!(
            "collaboration shutdown retained settlements: {:?}",
            report.settlements()
        ));
    }
}

/// Builds the factory that creates children, using the root's own model and workspace tools.
///
/// CHB-1's floor is applied inside the factory; the catalog handed here is the root's, narrowed
/// there rather than trusted to be narrow already.
/// Seals an opened conversation's collaboration to the runtime that will hold it (CTL-1).
///
/// Shared by both composition roots — the process's own startup and the picker's loader — because
/// the ordering is the contract, not a detail: the lane is installed before the runtime exists, and
/// authorship is sealed to that exact instance afterwards. Two copies of this would be two chances
/// to seal one of them to the wrong runtime.
pub(crate) fn seal(
    opened: &mut crate::session::OpenedConversation,
    plexmaton_home: &Path,
    model: plexmaton_provider::ResolvedModel,
    key: plexmaton_provider::ApiKey,
    tools: plexmaton_runtime::NativeToolCatalog,
) -> anyhow::Result<()> {
    let permissions = opened.runtime.coding_session();
    let Some(collaboration) = opened.collaboration.as_mut() else {
        return Ok(());
    };
    let factory = child_factory(plexmaton_home, model, key, tools, permissions)?;
    collaboration.bind(&opened.runtime, factory)
}

pub(crate) fn child_factory(
    plexmaton_home: &Path,
    model: plexmaton_provider::ResolvedModel,
    key: plexmaton_provider::ApiKey,
    tools: plexmaton_runtime::NativeToolCatalog,
    permissions: plexmaton_runtime::CodingSessionPermissions,
) -> anyhow::Result<DelegatedChildFactory> {
    let directory = DelegatedConversationDirectory::under(plexmaton_home)
        .context("open the delegated session directory")?;
    Ok(DelegatedChildFactory::new(directory, model, key, tools).with_coding_session(permissions))
}

/// Which transcript entry a forwarded fact belongs to, when it names one.
fn item_of(event: &ConversationEvent) -> Option<TranscriptItemId> {
    match event {
        ConversationEvent::TranscriptItemStarted { item_id, .. }
        | ConversationEvent::TranscriptDelta { item_id, .. }
        | ConversationEvent::TranscriptItemFinalized { item_id, .. }
        | ConversationEvent::ToolCallChanged { item_id, .. }
        | ConversationEvent::TaskAssigned { item_id, .. }
        | ConversationEvent::MailDelivered { item_id, .. }
        | ConversationEvent::HandoffCompleted { item_id, .. }
        | ConversationEvent::ArtifactAnnounced { item_id, .. }
        | ConversationEvent::RuntimeWarning { item_id, .. }
        | ConversationEvent::RuntimeError { item_id, .. } => Some(item_id.clone()),
        _ => None,
    }
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
            | ConversationEvent::TaskAssigned { .. }
            | ConversationEvent::MailDelivered { .. }
            | ConversationEvent::HandoffCompleted { .. }
            | ConversationEvent::TranscriptItemStarted { .. }
            | ConversationEvent::TranscriptDelta { .. }
            | ConversationEvent::TranscriptItemFinalized { .. }
            | ConversationEvent::ToolCallChanged { .. }
            | ConversationEvent::AttentionRequested { .. }
            | ConversationEvent::AttentionResolved { .. }
            | ConversationEvent::ArtifactAnnounced { .. }
            | ConversationEvent::RuntimeWarning { .. }
            | ConversationEvent::RuntimeError { .. }
    )
}

mod projection;
