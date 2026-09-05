//! Owned, bounded discovery/loading work. No model dispatch is part of session selection.
use super::*;
use plexmaton_core::SessionId;
use plexmaton_provider::ResolvedModel;
use plexmaton_session_store::SessionDirectory;
use plexmaton_tui::{SessionChoice, SessionPickerStatus};
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
}

pub(super) enum Update {
    Listed(Vec<SessionChoice>, bool),
    Opened(Box<OpenedSession>),
    Failed(SessionPickerStatus),
}

pub(super) struct SessionPicker {
    pub current: Option<PersistedSession>,
    launcher: Launcher,
    job: Option<JoinHandle<Update>>,
    cancel: CancellationToken,
}

impl SessionPicker {
    pub fn new(launcher: Launcher) -> Self {
        Self {
            launcher,
            current: None,
            job: None,
            cancel: CancellationToken::new(),
        }
    }

    pub fn execute_command(
        &mut self,
        workspace: &mut Workspace,
        runtime: &LiveRuntime,
        command: Command,
    ) {
        match command {
            Command::Config => {
                workspace.show_configuration(configuration_summary(&self.launcher.model))
            }
            Command::Resume => self.open(workspace),
            Command::New => {
                workspace.open_session_picker();
                let selection = if self.current.is_some() {
                    SessionSelection::Automatic
                } else {
                    SessionSelection::Ephemeral
                };
                self.start(selection, runtime, workspace);
            }
        }
    }

    pub fn open(&mut self, workspace: &mut Workspace) {
        workspace.open_session_picker();
        if self.job.is_some() {
            workspace.set_session_picker_status(SessionPickerStatus::Busy);
            return;
        }
        self.cancel = CancellationToken::new();
        let cancel = self.cancel.clone();
        let root = self.launcher.root.clone();
        self.job = Some(tokio::task::spawn_blocking(move || {
            match listing::list(&root, &cancel) {
                Ok((entries, limited)) => Update::Listed(entries, limited),
                Err(_) => Update::Failed(SessionPickerStatus::ListFailed),
            }
        }));
    }

    pub fn select(&mut self, id: SessionId, runtime: &LiveRuntime, workspace: &mut Workspace) {
        self.start(SessionSelection::Resume(id), runtime, workspace);
    }

    fn start(
        &mut self,
        selection: SessionSelection,
        runtime: &LiveRuntime,
        workspace: &mut Workspace,
    ) {
        if self.job.is_some() {
            workspace.set_session_picker_status(SessionPickerStatus::Opening);
            return;
        }
        if runtime.has_active_work() {
            workspace.set_session_picker_status(SessionPickerStatus::Busy);
            return;
        }
        if workspace.has_unsent_input() {
            workspace.set_session_picker_status(SessionPickerStatus::DraftPresent);
            return;
        }
        if let SessionSelection::Resume(id) = &selection
            && self
                .current
                .as_ref()
                .is_some_and(|current| &current.id == id)
        {
            workspace.close_session_picker();
            return;
        }
        workspace.set_session_picker_status(SessionPickerStatus::Opening);
        self.cancel = CancellationToken::new();
        let launcher = self.launcher.clone();
        let agent = runtime.agent_id().clone();
        let cancel = self.cancel.clone();
        self.job = Some(tokio::spawn(async move {
            match launcher.open(selection, agent, cancel).await {
                Ok(opened) => Update::Opened(Box::new(opened)),
                Err(_) => Update::Failed(SessionPickerStatus::OpenFailed),
            }
        }));
    }

    pub fn observe_closed(&self, workspace: &Workspace) {
        if !workspace.session_picker_open() {
            self.cancel.cancel();
        }
    }

    pub async fn next(&mut self) -> Update {
        let Some(job) = &mut self.job else {
            return std::future::pending().await;
        };
        let update = job
            .await
            .unwrap_or(Update::Failed(SessionPickerStatus::OpenFailed));
        self.job = None;
        update
    }

    pub async fn apply(
        &mut self,
        update: Update,
        runtime: &mut LiveRuntime,
        workspace: &mut Workspace,
    ) -> anyhow::Result<bool> {
        let accepted = !self.cancel.is_cancelled() && workspace.session_picker_open();
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
                workspace.set_session_choices(entries, limited)
            }
            Update::Failed(status) if accepted => workspace.set_session_picker_status(status),
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
                workspace.close_session_picker();
                workspace.replace_projection(events);
                if let Some(feedback) = restoration_feedback(opened.recovery) {
                    workspace.report_session_recovery(feedback);
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
        selection: SessionSelection,
        agent: AgentId,
        cancel: CancellationToken,
    ) -> anyhow::Result<OpenedSession> {
        let key = resolve_api_key(&self.model, std::env::var_os(self.model.api_key_env()))?;
        self.open_with_key(selection, agent, cancel, key).await
    }

    async fn open_with_key(
        self,
        selection: SessionSelection,
        agent: AgentId,
        cancel: CancellationToken,
        key: plexmaton_provider::ApiKey,
    ) -> anyhow::Result<OpenedSession> {
        let model = self.model.clone();
        let root = self.root.clone();
        let selected = selection.clone();
        let (journal, tools, key) = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            anyhow::ensure!(!cancel.is_cancelled(), "session load cancelled");
            let tools = NativeToolCatalog::open(
                self.workspace,
                self.model.api_key_env(),
                self.ripgrep,
                self.driver,
                vec![OsString::from(INTERNAL_RG_DRIVER)],
            )?;
            let journal = match selected {
                SessionSelection::Resume(id) => {
                    Some(SessionDirectory::under(self.root)?.resume(&id)?)
                }
                _ => None,
            };
            anyhow::ensure!(!cancel.is_cancelled(), "session load cancelled");
            Ok((journal, tools, key))
        })
        .await
        .context("join session file reader")??;
        let Some(journal) = journal else {
            return open_selected_session(&root, selection, agent, model, key, tools).await;
        };
        let persisted = PersistedSession {
            id: journal.journal().session_id().clone(),
            path: journal.path().to_path_buf(),
        };
        let (runtime, recovery) =
            LiveRuntime::openai_with_resumed_journal(agent, model, key, tools, journal).await?;
        Ok(OpenedSession {
            runtime,
            recovery: Some(recovery),
            persisted: Some(persisted),
        })
    }
}
