//! Composition-owned status command and immutable snapshot scheduling.

mod config;
mod process;
mod snapshot;
#[cfg(test)]
mod tests;

use plexmaton_provider::ResolvedModel;
use plexmaton_runtime::LiveRuntime;
use plexmaton_tui::{StatusLineText, Workspace};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub(crate) use config::StatusLineConfig;
use process::Failure;
pub(super) use snapshot::Dimensions;

struct Active {
    task: JoinHandle<Result<StatusLineText, Failure>>,
    cancel: CancellationToken,
    generation: u64,
}

#[derive(Debug)]
pub(super) enum Update {
    Capture,
    Output(Result<StatusLineText, Failure>),
}

pub(super) struct StatusLine {
    config: StatusLineConfig,
    model: ResolvedModel,
    credential_envs: Vec<String>,
    cwd: PathBuf,
    active: Option<Active>,
    due: Option<Instant>,
    generation: u64,
    last_input: Option<Vec<u8>>,
    force_refresh: bool,
    cleanup_failed: bool,
}

impl StatusLine {
    pub fn new(config: StatusLineConfig, model: ResolvedModel, cwd: PathBuf) -> Self {
        Self {
            config,
            credential_envs: vec![model.api_key_env().to_owned()],
            model,
            cwd,
            active: None,
            due: Some(Instant::now()),
            generation: 0,
            last_input: None,
            force_refresh: true,
            cleanup_failed: false,
        }
    }

    pub fn with_provider_credentials(mut self, names: impl Iterator<Item = String>) -> Self {
        self.credential_envs.extend(names);
        self.credential_envs.sort();
        self.credential_envs.dedup();
        self
    }

    /// Capture semantic status/accounting changes, not text streaming deltas (STL-1).
    pub fn observe(&mut self, event: &plexmaton_core::ConversationEvent) {
        if matches!(
            event,
            plexmaton_core::ConversationEvent::AgentCreated { .. }
                | plexmaton_core::ConversationEvent::AgentStatusChanged { .. }
                | plexmaton_core::ConversationEvent::TurnUsageUpdated { .. }
        ) {
            self.mark_dirty();
        }
    }

    pub fn mark_dirty(&mut self) {
        if self.cleanup_failed {
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        if self.active.is_some() {
            // Refreshes invalidate the result, not the process. Let bounded work finish and
            // coalesce changes into one capture of the latest facts and terminal dimensions.
            self.last_input = None;
        }
        let next = Instant::now() + Duration::from_millis(300);
        self.due = Some(self.due.map_or(next, |prior| prior.min(next)));
    }

    pub fn capture(
        &mut self,
        runtime: &LiveRuntime,
        dimensions: Dimensions,
        workspace: &mut Workspace,
    ) {
        let cwd = self.cwd.to_string_lossy();
        let snapshot = snapshot::Snapshot::capture(
            runtime,
            runtime.configured_model().unwrap_or(&self.model),
            &cwd,
            dimensions,
        );
        let result = serde_json::to_vec(&snapshot);
        let input = match result {
            Ok(input) => input,
            Err(_) => {
                workspace.set_status_line_error(Failure::Snapshot.to_string());
                self.schedule_refresh();
                return;
            }
        };
        if !self.force_refresh && self.last_input.as_ref() == Some(&input) {
            self.schedule_refresh();
            return;
        }
        self.force_refresh = false;
        self.last_input = Some(input.clone());
        let config = self.config.clone();
        let cwd = self.cwd.clone();
        let credential_envs = self.credential_envs.clone();
        let cancel = CancellationToken::new();
        let child_cancel = cancel.clone();
        self.active = Some(Active {
            generation: self.generation,
            cancel,
            task: tokio::spawn(async move {
                process::execute(&config, input, &cwd, &credential_envs, child_cancel).await
            }),
        });
    }

    fn schedule_refresh(&mut self) {
        self.due = self
            .config
            .refresh_ms
            .map(|ms| Instant::now() + Duration::from_millis(ms));
    }

    pub async fn next(&mut self) -> Update {
        loop {
            if let Some(active) = &mut self.active {
                let generation = active.generation;
                let result = (&mut active.task).await.unwrap_or(Err(Failure::Cleanup));
                self.active = None;
                if matches!(result, Err(Failure::Cleanup)) {
                    self.cleanup_failed = true;
                    self.due = None;
                    return Update::Output(result);
                }
                if generation != self.generation {
                    continue;
                }
                self.schedule_refresh();
                return Update::Output(result);
            }
            if let Some(due) = self.due {
                tokio::time::sleep_until(tokio::time::Instant::from_std(due)).await;
                self.due = None;
                return Update::Capture;
            }
            std::future::pending::<()>().await;
        }
    }

    pub fn apply(&mut self, output: Result<StatusLineText, Failure>, workspace: &mut Workspace) {
        match output {
            Ok(text) => workspace.set_status_line(text, self.config.max_rows),
            Err(error) => {
                self.last_input = None;
                workspace.set_status_line_error(error.to_string());
            }
        }
        // An explicitly configured refresh may update Git or a script clock without new facts.
        self.force_refresh = self.config.refresh_ms.is_some();
    }

    pub async fn shutdown(&mut self) -> anyhow::Result<()> {
        if self.cleanup_failed {
            anyhow::bail!("status-line cleanup failed");
        }
        if let Some(active) = &mut self.active {
            active.cancel.cancel();
            let result = (&mut active.task).await?;
            self.active = None;
            if matches!(result, Err(Failure::Cleanup)) {
                anyhow::bail!("status-line cleanup failed");
            }
        }
        Ok(())
    }
}

impl Drop for StatusLine {
    fn drop(&mut self) {
        if let Some(active) = &self.active {
            active.cancel.cancel();
            active.task.abort();
        }
    }
}
