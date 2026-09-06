//! One retained permission-control worker. The TUI receives acknowledged projections only.
use plexmaton_core::{PermissionChangeError, PermissionIntent, PermissionStateView};
use plexmaton_runtime::CodingSessionPermissions;
use plexmaton_tui::Workspace;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub(super) struct PermissionUpdate {
    view: Result<PermissionStateView, PermissionChangeError>,
    changed: Option<Result<(), PermissionChangeError>>,
}

pub(super) struct PermissionControls {
    owner: CodingSessionPermissions,
    job: Option<JoinHandle<PermissionUpdate>>,
    cancel: CancellationToken,
}

impl PermissionControls {
    pub fn new(owner: CodingSessionPermissions) -> Self {
        Self {
            owner,
            job: None,
            cancel: CancellationToken::new(),
        }
    }

    pub fn open(&mut self, workspace: &mut Workspace) {
        workspace.open_permissions();
        self.start(None);
    }

    pub fn apply(&mut self, intent: PermissionIntent) {
        self.start(Some(intent));
    }

    /// The current view, for a place that opened or asked again (PER-7).
    pub fn refresh(&mut self) {
        self.start(None);
    }

    fn start(&mut self, intent: Option<PermissionIntent>) {
        if self.job.is_some() {
            return;
        }
        self.cancel = CancellationToken::new();
        let cancel = self.cancel.clone();
        let owner = self.owner.clone();
        self.job = Some(tokio::task::spawn_blocking(move || {
            let changed = intent.map(|intent| {
                if cancel.is_cancelled() {
                    Err(PermissionChangeError::Unavailable)
                } else {
                    owner.apply_control(&intent, &|| cancel.is_cancelled())
                }
            });
            let view = owner
                .refresh(&|| cancel.is_cancelled())
                .map(|view| view.control_view())
                .map_err(|_| PermissionChangeError::Unavailable);
            PermissionUpdate { view, changed }
        }));
    }

    pub fn observe_closed(&self, workspace: &Workspace) {
        if !workspace.permissions_open() {
            self.cancel.cancel();
        }
    }

    pub async fn next(&mut self) -> PermissionUpdate {
        let Some(job) = &mut self.job else {
            return std::future::pending().await;
        };
        let result = job.await.unwrap_or(PermissionUpdate {
            view: Err(PermissionChangeError::Unavailable),
            changed: None,
        });
        self.job = None;
        result
    }

    pub fn publish(update: PermissionUpdate, workspace: &mut Workspace) {
        workspace.update_permissions(update.view, update.changed);
    }

    pub async fn shutdown(&mut self) -> anyhow::Result<()> {
        self.cancel.cancel();
        if let Some(job) = self.job.take() {
            let _completion = job.await?;
        }
        Ok(())
    }
}
