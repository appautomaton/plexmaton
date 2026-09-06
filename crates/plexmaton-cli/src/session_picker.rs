//! Owned, bounded discovery/loading work. No model dispatch is part of session selection.
use super::*;
use plexmaton_core::ConversationId;
use plexmaton_provider::ResolvedModel;
use plexmaton_session_store::ConversationDirectory;
use plexmaton_tui::{ConversationChoice, ConversationPickerStatus};
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
    Opened(Box<OpenedConversation>),
    Failed(ConversationPickerStatus),
}

pub(super) struct ConversationPicker {
    pub current: Option<PersistedConversation>,
    launcher: Launcher,
    job: Option<JoinHandle<Update>>,
    cancel: JobCancellation,
}

impl ConversationPicker {
    pub fn new(launcher: Launcher) -> Self {
        Self {
            launcher,
            current: None,
            job: None,
            cancel: JobCancellation::new(),
        }
    }

    pub fn configuration(&self) -> ConfigurationSummary {
        configuration_summary(&self.launcher.model)
    }

    pub fn new_conversation(&mut self, workspace: &mut Workspace, runtime: &LiveRuntime) {
        workspace.open_conversation_picker();
        let selection = if self.current.is_some() {
            ConversationSelection::Automatic
        } else {
            ConversationSelection::Ephemeral
        };
        self.start(selection, runtime, workspace);
    }

    pub fn open(&mut self, workspace: &mut Workspace) {
        workspace.open_conversation_picker();
        if self.job.is_some() {
            workspace.set_conversation_picker_status(ConversationPickerStatus::Busy);
            return;
        }
        self.cancel = JobCancellation::new();
        let cancel = self.cancel.task.clone();
        let root = self.launcher.root.clone();
        self.job = Some(tokio::task::spawn_blocking(move || {
            match listing::list(&root, &cancel) {
                Ok((entries, limited)) => Update::Listed(entries, limited),
                Err(_) => Update::Failed(ConversationPickerStatus::ListFailed),
            }
        }));
    }

    pub fn select(&mut self, id: ConversationId, runtime: &LiveRuntime, workspace: &mut Workspace) {
        self.start(ConversationSelection::Resume(id), runtime, workspace);
    }

    fn start(
        &mut self,
        selection: ConversationSelection,
        runtime: &LiveRuntime,
        workspace: &mut Workspace,
    ) {
        if self.job.is_some() {
            workspace.set_conversation_picker_status(ConversationPickerStatus::Opening);
            return;
        }
        if runtime.has_active_work() {
            workspace.set_conversation_picker_status(ConversationPickerStatus::Busy);
            return;
        }
        if workspace.has_unsent_input() {
            workspace.set_conversation_picker_status(ConversationPickerStatus::DraftPresent);
            return;
        }
        if let ConversationSelection::Resume(id) = &selection
            && self
                .current
                .as_ref()
                .is_some_and(|current| &current.id == id)
        {
            workspace.close_conversation_picker();
            return;
        }
        workspace.set_conversation_picker_status(ConversationPickerStatus::Opening);
        self.cancel = JobCancellation::new();
        let launcher = self.launcher.clone();
        let agent = runtime.agent_id().clone();
        let cancel = self.cancel.clone();
        self.job = Some(tokio::spawn(async move {
            match launcher.open(selection, agent, cancel).await {
                Ok(opened) => Update::Opened(Box::new(opened)),
                Err(_) => Update::Failed(ConversationPickerStatus::OpenFailed),
            }
        }));
    }

    pub fn observe_closed(&self, workspace: &Workspace) {
        if !workspace.conversation_picker_open() {
            self.cancel.cancel();
        }
    }

    pub async fn next(&mut self) -> Update {
        let Some(job) = &mut self.job else {
            return std::future::pending().await;
        };
        let update = job
            .await
            .unwrap_or(Update::Failed(ConversationPickerStatus::OpenFailed));
        self.job = None;
        update
    }

    pub async fn apply(
        &mut self,
        update: Update,
        runtime: &mut LiveRuntime,
        workspace: &mut Workspace,
    ) -> anyhow::Result<bool> {
        let accepted = !self.cancel.is_cancelled() && workspace.conversation_picker_open();
        match update {
            Update::Listed(mut entries, limited) if accepted => {
                for entry in &mut entries {
                    if self
                        .current
                        .as_ref()
                        .is_some_and(|current| current.id == entry.id)
                    {
                        entry.title = format!("[current] {}", entry.title);
                    }
                }
                workspace.set_conversation_choices(entries, limited)
            }
            Update::Failed(status) if accepted => workspace.set_conversation_picker_status(status),
            Update::Opened(mut opened) => {
                if !accepted || runtime.has_active_work() || workspace.has_unsent_input() {
                    surface_shutdown_report(opened.runtime.shutdown().await?)?;
                    return Ok(false);
                }
                if let Err(error) = runtime
                    .shutdown()
                    .await
                    .map_err(anyhow::Error::from)
                    .and_then(surface_shutdown_report)
                {
                    return match opened
                        .runtime
                        .shutdown()
                        .await
                        .map_err(anyhow::Error::from)
                        .and_then(surface_shutdown_report)
                    {
                        Ok(()) => Err(error),
                        Err(cleanup) => {
                            Err(error.context(format!("candidate cleanup also failed: {cleanup}")))
                        }
                    };
                }
                let events = std::iter::from_fn(|| opened.runtime.try_next_event()).collect();
                workspace.close_conversation_picker();
                workspace.replace_projection(events);
                skills::sync_choices(&opened.runtime, workspace);
                for diagnostic in opened.runtime.skill_diagnostics() {
                    workspace.report_skill_diagnostic(diagnostic);
                }
                if let Some(feedback) = restoration_feedback(opened.recovery) {
                    workspace.report_conversation_recovery(feedback);
                }
                *runtime = opened.runtime;
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
        if let Some(job) = self.job.take()
            && let Update::Opened(mut opened) = job.await.context("join session loader")?
        {
            surface_shutdown_report(opened.runtime.shutdown().await?)?;
        }
        Ok(())
    }
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
        let model = self.model.clone();
        let root = self.root.clone();
        let selected = selection.clone();
        let (journal, tools, key) = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            anyhow::ensure!(!cancel.is_cancelled(), "session load cancelled");
            let project_root = project_config::discover_project_root(&self.workspace)?;
            let tools = NativeToolCatalog::open(
                &self.workspace,
                self.model.api_key_env(),
                self.ripgrep,
                self.driver,
                vec![OsString::from(INTERNAL_RG_DRIVER)],
            )?
            .with_skill_roots(&self.root, &project_root, &cancel.files)?;
            let journal = match selected {
                ConversationSelection::Resume(id) => {
                    Some(ConversationDirectory::under(self.root)?.resume(&id)?)
                }
                _ => None,
            };
            anyhow::ensure!(!cancel.is_cancelled(), "session load cancelled");
            Ok((journal, tools, key))
        })
        .await
        .context("join session file reader")??;
        let Some(journal) = journal else {
            let mut opened =
                open_selected_conversation(&root, selection, agent, model, key, tools).await?;
            opened.runtime.use_coding_session(permissions)?;
            return Ok(opened);
        };
        let persisted = PersistedConversation {
            id: journal.journal().conversation_id().clone(),
            path: journal.path().to_path_buf(),
        };
        let (mut runtime, recovery) =
            LiveRuntime::provider_with_resumed_journal(agent, model, key, tools, journal).await?;
        runtime.use_coding_session(permissions)?;
        Ok(OpenedConversation {
            runtime,
            recovery: Some(recovery),
            persisted: Some(persisted),
        })
    }
}
