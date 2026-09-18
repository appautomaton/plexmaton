//! Owned, bounded discovery/loading work. No model dispatch is part of session selection.
use super::*;
use plexmaton_core::ConversationId;
use plexmaton_provider::ResolvedModel;
use plexmaton_session_store::ConversationDirectory;
use plexmaton_tui::{ConversationChoice, ConversationPickerStatus, SwitchRefusal};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

mod listing;
#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(super) struct Launcher {
    pub root: PathBuf,
    pub workspace: PathBuf,
    pub model: ResolvedModel,
    pub models: plexmaton_provider::ModelRegistry,
    pub ripgrep: PathBuf,
    pub driver: PathBuf,
    pub permissions: plexmaton_runtime::CodingSessionPermissions,
}

#[derive(Clone, Default)]
struct JobCancellation {
    task: CancellationToken,
    files: plexmaton_file_tools::FileCancellation,
}

impl JobCancellation {
    fn new() -> Self {
        Self::default()
    }

    fn cancel(&self) {
        self.task.cancel();
        self.files.cancel();
    }

    fn is_cancelled(&self) -> bool {
        self.task.is_cancelled() || self.files.is_cancelled()
    }
}

pub(super) enum Update {
    Listed(Vec<ConversationChoice>, bool),
    ListFailed,
    Opened(Box<OpenedConversation>),
    OpenFailed,
}

/// What the one job is doing, so a join failure names the right outcome and a second request
/// knows what it is waiting behind.
#[derive(Clone, Debug, Eq, PartialEq)]
enum JobKind {
    Listing,
    /// Opening, and the children this switch was confirmed to stop on the way (SPK-2).
    ///
    /// They travel with the job rather than staying on the picker because the confirmation is
    /// consumed when the request is made: by the time the candidate lands, the only question left
    /// is whether the runtime and the draft still admit a switch, and neither is about a child.
    Opening {
        stopping: Vec<AgentId>,
    },
}

/// A switch already offered with what it would cost, waiting to be chosen a second time.
///
/// The request is kept, not just the fact that something was offered, so that choosing a different
/// row is a different question rather than an accidental confirmation of the first one.
#[derive(Clone, Debug, Eq, PartialEq)]
struct OfferedSwitch {
    request: ConfirmableRequest,
    children: Vec<AgentId>,
}

/// Which switch was offered. `/new` has no row to choose again, so its second gesture is the
/// command itself; the two share one mechanism so neither can drift into its own rules.
#[derive(Clone, Debug, Eq, PartialEq)]
enum ConfirmableRequest {
    New,
    Saved(ConversationId),
}

impl ConfirmableRequest {
    fn of(selection: &ConversationSelection) -> Self {
        match selection {
            ConversationSelection::Resume(id) => Self::Saved(id.clone()),
            _ => Self::New,
        }
    }
}

pub(super) struct ConversationPicker {
    pub current: Option<PersistedConversation>,
    launcher: Launcher,
    job: Option<(JobKind, JoinHandle<Update>)>,
    cancel: JobCancellation,
    /// One offered switch, or none. Cleared by every other rung of `start`'s ladder.
    offered: Option<OfferedSwitch>,
    /// Children the landed switch was confirmed to stop, carried from its job to `apply`.
    stopping: Vec<AgentId>,
}

impl ConversationPicker {
    pub fn new(launcher: Launcher) -> Self {
        Self {
            launcher,
            current: None,
            job: None,
            cancel: JobCancellation::new(),
            offered: None,
            stopping: Vec::new(),
        }
    }

    pub fn models(&self) -> &plexmaton_provider::ModelRegistry {
        &self.launcher.models
    }

    pub fn configuration(&self) -> ConfigurationSummary {
        configuration_summary(&self.launcher.model)
    }

    pub fn new_conversation(&mut self, workspace: &mut Workspace, runtime: &LiveRuntime) {
        let selection = if self.current.is_some() {
            ConversationSelection::Automatic
        } else {
            ConversationSelection::Ephemeral
        };
        self.start(selection, runtime, workspace);
    }

    /// Lists for `/resume`. A listing already running delivers to the rows; a withdrawn one is
    /// listed again when it lands (see `apply`).
    pub fn open(&mut self, workspace: &mut Workspace) {
        workspace.open_conversation_picker();
        match self.job {
            Some((JobKind::Listing, _)) => return,
            Some((JobKind::Opening { .. }, _)) => {
                workspace.set_conversation_picker_status(ConversationPickerStatus::Opening);
                return;
            }
            None => {}
        }
        self.cancel = JobCancellation::new();
        let cancel = self.cancel.task.clone();
        let root = self.launcher.root.clone();
        let job = tokio::task::spawn_blocking(move || match listing::list(&root, &cancel) {
            Ok((entries, limited)) => Update::Listed(entries, limited),
            Err(_) => Update::ListFailed,
        });
        self.job = Some((JobKind::Listing, job));
    }

    /// Forgets a standing offer, and takes its question off the workspace's last row.
    ///
    /// One call for both halves. The offer is what `start` consults; the armed note is what the
    /// user reads. Clearing one without the other leaves a sentence on screen asking for a gesture
    /// nothing is waiting for — the failure a single entry point exists to make unrepresentable.
    pub fn forget_offer(&mut self, workspace: &mut Workspace) {
        self.offered = None;
        workspace.disarm_switch();
    }

    pub fn select(&mut self, id: ConversationId, runtime: &LiveRuntime, workspace: &mut Workspace) {
        self.start(ConversationSelection::Resume(id), runtime, workspace);
    }

    /// The one ladder a switch descends, and the one place an offer is made, kept or forgotten.
    ///
    /// Every rung above the child question also invalidates a standing offer, because each of them
    /// means the user asked something else: a draft appeared, the root started working, a different
    /// row was chosen. Only the same request, asked again with the same children still working,
    /// consumes it (SPK-2).
    fn start(
        &mut self,
        selection: ConversationSelection,
        runtime: &LiveRuntime,
        workspace: &mut Workspace,
    ) {
        let refusal = if self.job.is_some() {
            // Too early to be a different intent: the offer survives a request that never ran.
            Some(SwitchRefusal::RequestInFlight)
        } else if runtime.has_active_work() {
            Some(SwitchRefusal::Busy)
        } else if workspace.has_unsent_input() {
            Some(SwitchRefusal::DraftPresent)
        } else {
            None
        };
        if let Some(refusal) = refusal {
            if refusal != SwitchRefusal::RequestInFlight {
                self.forget_offer(workspace);
            }
            workspace.report_switch_refusal(refusal);
            return;
        }
        if let ConversationSelection::Resume(id) = &selection
            && self
                .current
                .as_ref()
                .is_some_and(|current| &current.id == id)
        {
            self.forget_offer(workspace);
            workspace.close_conversation_picker();
            return;
        }
        let request = ConfirmableRequest::of(&selection);
        let working = workspace.working_delegates();
        let stopping = if working.is_empty() {
            // Nothing to lose, so nothing to ask. A child restored without being woken is idle and
            // reaches here, which is why resuming a conversation full of finished work is silent.
            self.forget_offer(workspace);
            Vec::new()
        } else {
            let children: Vec<AgentId> = working.iter().map(|(id, _)| id.clone()).collect();
            match self.offered.take() {
                Some(offer) if offer.request == request && offer.children == children => {
                    workspace.disarm_switch();
                    offer.children
                }
                _ => {
                    let child = working
                        .first()
                        .map(|(_, label)| label.clone())
                        .unwrap_or_default();
                    self.offered = Some(OfferedSwitch { request, children });
                    workspace.arm_switch(child);
                    return;
                }
            }
        };
        workspace.begin_conversation_switch();
        self.cancel = JobCancellation::new();
        let launcher = self.launcher.clone();
        let agent = runtime.agent_id().clone();
        let cancel = self.cancel.clone();
        let job = tokio::spawn(async move {
            match launcher.open(selection, agent, cancel).await {
                Ok(opened) => Update::Opened(Box::new(opened)),
                Err(_) => Update::OpenFailed,
            }
        });
        self.job = Some((JobKind::Opening { stopping }, job));
    }

    pub fn observe_closed(&mut self, workspace: &Workspace) {
        if !workspace.conversation_picker_open() {
            self.cancel.cancel();
            // The rows the offer referred to are gone. The armed row is left alone: a withdrawn
            // listing is not the user answering, and `/new` has no listing to withdraw at all.
            self.offered = None;
        }
    }

    pub async fn next(&mut self) -> Update {
        let Some((kind, job)) = &mut self.job else {
            return std::future::pending().await;
        };
        let update = job.await.unwrap_or(match kind {
            JobKind::Listing => Update::ListFailed,
            JobKind::Opening { .. } => Update::OpenFailed,
        });
        // What the user confirmed travels from the request that made it to the frame that acts on
        // it, so `apply` never has to ask the question a second time.
        self.stopping = match self.job.take() {
            Some((JobKind::Opening { stopping }, _)) => stopping,
            _ => Vec::new(),
        };
        update
    }

    pub async fn apply(
        &mut self,
        update: Update,
        runtime: &mut LiveRuntime,
        collaboration: &mut Option<collaboration::Collaboration>,
        workspace: &mut Workspace,
    ) -> anyhow::Result<bool> {
        let accepted = !self.cancel.is_cancelled() && workspace.conversation_picker_open();
        match update {
            // A listing withdrawn and asked for again before it landed is listed afresh.
            Update::Listed(..) | Update::ListFailed if !accepted => {
                if workspace.conversation_picker_open() {
                    self.open(workspace);
                }
            }
            Update::Listed(mut entries, limited) => {
                for entry in &mut entries {
                    if self
                        .current
                        .as_ref()
                        .is_some_and(|current| current.id == entry.id)
                    {
                        entry.title = format!("[current] {}", entry.title);
                    }
                }
                workspace.set_conversation_choices(entries, limited);
            }
            Update::ListFailed => {
                workspace.set_conversation_picker_status(ConversationPickerStatus::ListFailed);
            }
            Update::OpenFailed if accepted => {
                workspace.report_switch_refusal(SwitchRefusal::OpenFailed);
            }
            Update::Opened(mut opened) => {
                let refusal = if runtime.has_active_work() {
                    Some(SwitchRefusal::Busy)
                } else if workspace.has_unsent_input() {
                    Some(SwitchRefusal::DraftPresent)
                } else {
                    None
                };
                if !accepted || refusal.is_some() {
                    discard(&mut opened).await?;
                    if let Some(refusal) = refusal.filter(|_| accepted) {
                        workspace.report_switch_refusal(refusal);
                    }
                    return Ok(false);
                }
                // The outgoing collaboration is moved out rather than flagged: `shutdown` is not
                // idempotent, and the exit path at loop end calls it unconditionally on whatever is
                // still here. Joined before the runtime, in the order the exit path uses.
                if let Some(mut previous) = collaboration.take() {
                    // What the user was told this would cost, charged before anything is joined.
                    // A child that finished on its own between the confirmation and here is already
                    // gone, and the owner answers that with a typed refusal rather than an error.
                    for child in std::mem::take(&mut self.stopping) {
                        match previous.stop_child_for_switch(&child).await {
                            Ok(())
                            | Err(plexmaton_runtime::OwnedSchedulingError::UnknownRunner) => {}
                            Err(error) => {
                                return Err(anyhow::anyhow!("stop {child}: {error}"));
                            }
                        }
                    }
                    previous.shutdown().await?;
                }
                if let Err(error) = runtime
                    .shutdown()
                    .await
                    .map_err(anyhow::Error::from)
                    .and_then(surface_shutdown_report)
                {
                    return match discard(&mut opened).await {
                        Ok(()) => Err(error),
                        Err(cleanup) => {
                            Err(error.context(format!("candidate cleanup also failed: {cleanup}")))
                        }
                    };
                }
                let events = std::iter::from_fn(|| opened.runtime.try_next_event()).collect();
                workspace.close_conversation_picker();
                workspace.replace_projection(events);
                if let Some(model) = opened.runtime.configured_model() {
                    workspace.set_model(configuration_summary(model));
                    workspace
                        .set_effort_choices(model.allowed_reasoning_efforts().map(<[_]>::to_vec));
                }
                skills::sync_choices(&opened.runtime, workspace);
                for diagnostic in opened.runtime.skill_diagnostics() {
                    workspace.report_skill_diagnostic(diagnostic);
                }
                if let Some(feedback) = restoration_feedback(opened.recovery) {
                    workspace.report_conversation_recovery(feedback);
                }
                *runtime = opened.runtime;
                *collaboration = opened.collaboration;
                self.current = opened.persisted;
                retry::sync_actions(runtime, workspace);
                return Ok(true);
            }
            _ => {}
        }
        Ok(false)
    }

    pub async fn shutdown(&mut self) -> anyhow::Result<()> {
        self.cancel.cancel();
        if let Some((_, job)) = self.job.take()
            && let Update::Opened(mut opened) = job.await.context("join session loader")?
        {
            discard(&mut opened).await?;
        }
        Ok(())
    }
}

/// Releases a candidate nobody took: its collaboration log's writer lock, then its runtime.
///
/// One function for all three places a candidate can be dropped — refused, cancelled, or cleaned up
/// after the current runtime failed to close — because a candidate whose log stayed locked cannot be
/// opened again, and the three sites had already drifted once when only the runtime existed.
async fn discard(opened: &mut OpenedConversation) -> anyhow::Result<()> {
    if let Some(collaboration) = opened.collaboration.as_mut() {
        collaboration.shutdown().await?;
    }
    surface_shutdown_report(opened.runtime.shutdown().await?)
}

impl Launcher {
    async fn open(
        self,
        selection: ConversationSelection,
        agent: AgentId,
        cancel: JobCancellation,
    ) -> anyhow::Result<OpenedConversation> {
        anyhow::ensure!(!cancel.is_cancelled(), "session load cancelled");
        let key = resolve_api_key(&self.model, std::env::var_os(self.model.api_key_env()))?;
        self.open_with_key(selection, agent, cancel, key).await
    }

    async fn open_with_key(
        self,
        selection: ConversationSelection,
        agent: AgentId,
        cancel: JobCancellation,
        key: plexmaton_provider::ApiKey,
    ) -> anyhow::Result<OpenedConversation> {
        let permissions = self.permissions.clone();
        let root = self.root.clone();
        let selected = selection.clone();
        let (journal, tools, key, model) =
            tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
                anyhow::ensure!(!cancel.is_cancelled(), "session load cancelled");
                let project_root = project_config::discover_project_root(&self.workspace)?;
                let model = agent_instructions::resolve_model(
                    &self.model,
                    &self.root,
                    &project_root,
                    &self.workspace,
                    &cancel.files,
                )?;
                let tools = NativeToolCatalog::open(
                    &self.workspace,
                    self.model.api_key_env(),
                    self.ripgrep,
                    self.driver,
                    vec![OsString::from(INTERNAL_RG_DRIVER)],
                )?
                .with_provider_credentials(self.models.models().map(|model| model.api_key_env()))
                .with_skill_roots(&self.root, &project_root, &cancel.files)?;
                let journal = match selected {
                    ConversationSelection::Resume(id) => {
                        Some(ConversationDirectory::under(self.root)?.resume(&id)?)
                    }
                    _ => None,
                };
                anyhow::ensure!(!cancel.is_cancelled(), "session load cancelled");
                Ok((journal, tools, key, model))
            })
            .await
            .context("join session file reader")??;
        // The child factory gets the catalog as it was before the Main lane narrowed it, exactly as
        // the process's own startup does: a child is read-only and has no delegation of its own.
        let child_tools = tools.clone();
        let child_model = model.clone();
        let child_key = key.clone();
        let mut opened = match journal {
            None => {
                let lane_root = root.clone();
                open_selected_conversation_with(
                    &root,
                    selection,
                    agent,
                    model,
                    key,
                    tools,
                    async |conversation, tools| {
                        collaboration_lane(lane_root, conversation, tools).await
                    },
                )
                .await?
            }
            // A resumed journal is opened before the runtime rather than by it, so this branch
            // installs the lane itself. It is the same lane, from the same function, because a
            // conversation that could delegate in one branch and not the other would be a switch
            // whose tools depended on which file the loader happened to find.
            Some(journal) => {
                let persisted = PersistedConversation {
                    id: journal.journal().conversation_id().clone(),
                    path: journal.path().to_path_buf(),
                };
                let (tools, collaboration) =
                    collaboration_lane(root.clone(), persisted.id.clone(), tools).await?;
                let (runtime, recovery) =
                    LiveRuntime::provider_with_resumed_journal(agent, model, key, tools, journal)
                        .await?;
                OpenedConversation {
                    runtime,
                    recovery: Some(recovery),
                    persisted: Some(persisted),
                    collaboration,
                }
            }
        };
        opened.runtime.use_coding_session(permissions)?;
        collaboration::seal(&mut opened, &root, child_model, child_key, child_tools)?;
        // Put this conversation's children back on its roster before it replaces the one on screen.
        // Reading a child's journal is not waking it (CHB-3), and doing it here keeps that file work
        // on the loader rather than on the frame that accepts the switch.
        if let Some(collaboration) = opened.collaboration.as_mut() {
            collaboration.restore(&mut opened.runtime).await?;
        }
        Ok(opened)
    }
}

/// One candidate conversation's Main collaboration lane, installed on the catalog it will use.
///
/// Shared by both of the loader's branches. An ephemeral candidate never reaches here: it has no
/// durable identity to name a log after, so `delegate` is absent from its tools by construction.
async fn collaboration_lane(
    root: PathBuf,
    conversation: ConversationId,
    tools: NativeToolCatalog,
) -> anyhow::Result<(NativeToolCatalog, Option<collaboration::Collaboration>)> {
    let (collaboration, ingress) = collaboration::open_off_thread(root, conversation).await?;
    let tools = tools
        .with_main_collaboration(ingress)
        .context("install the Main collaboration tools")?;
    Ok((tools, Some(collaboration)))
}
