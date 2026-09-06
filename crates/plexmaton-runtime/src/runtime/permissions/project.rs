//! Project transactions serialize mutations; the memory lock is never held across storage I/O.
use std::sync::Arc;

use plexmaton_agent::{
    AdmittedToolCall, ApprovalPolicy, PermissionChangeError, PermissionConfiguration,
    PermissionGrant, PermissionGrantOrigin, PermissionPreparationRequest, PermissionSnapshot,
    ProjectPermissions, ToolAuthorization,
};
use plexmaton_core::{PermissionAction, PermissionGrantId, PermissionIntent, PermissionScope};
use plexmaton_permission_store::{
    PermissionStoreError, PermissionTransaction, ProjectPermissionSnapshot, ProjectPermissionStore,
};

use super::{CodingSessionPermissions, permits_authorization};

impl CodingSessionPermissions {
    /// Attaches and validates a personal project store before terminal ownership, or on a worker.
    /// Every later mutation and dispatch observes this same external source under its stable lock.
    pub fn with_project_store(
        mut self,
        store: ProjectPermissionStore,
    ) -> Result<Self, PermissionChangeError> {
        self.project = Some(store);
        self.refresh(&|| false)?;
        Ok(self)
    }

    /// Blocking external refresh for an owned worker; snapshot() itself remains memory-only.
    pub fn refresh(
        &self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Arc<PermissionSnapshot>, PermissionChangeError> {
        let _transaction = self.project_transaction(cancelled)?;
        self.state
            .lock()
            .map_err(|_| PermissionChangeError::Unavailable)
            .map(|state| state.snapshot())
    }

    pub(super) fn prepare_grant(
        &self,
        request: &PermissionPreparationRequest,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<PermissionGrantId, PermissionChangeError> {
        let transaction = self.project_transaction(cancelled)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| PermissionChangeError::Unavailable)?;
        if cancelled() {
            return Err(PermissionChangeError::Unavailable);
        }
        if request.scope() == PermissionScope::Session {
            return state.remember(
                request.revision(),
                request.matcher().clone(),
                request.admitted(),
                &ApprovalPolicy::default(),
            );
        }
        state.validate_remember(
            request.revision(),
            request.matcher(),
            request.admitted(),
            &ApprovalPolicy::default(),
        )?;
        drop(state);
        let transaction = transaction.ok_or(PermissionChangeError::Unavailable)?;
        let revision = transaction.snapshot().revision.clone();
        let id = PermissionGrantId::new(format!("project-grant-{}", uuid::Uuid::now_v7()))
            .unwrap_or_else(|_| unreachable!("a UUID produces a nonempty grant id"));
        let grant = PermissionGrant {
            id: id.clone(),
            matcher: request.matcher().clone(),
            origin: PermissionGrantOrigin::Approval,
        };
        let committed = transaction
            .grant(&revision, grant, cancelled)
            .map_err(|error| self.store_failure(error))?;
        self.observe_project(committed.snapshot())?;
        Ok(id)
    }

    /// Applies a control on an owned worker. Storage waiting and writes never hold the memory lock.
    pub fn apply_control(
        &self,
        intent: &PermissionIntent,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), PermissionChangeError> {
        let transaction = self.project_transaction(cancelled)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| PermissionChangeError::Unavailable)?;
        if cancelled() {
            return Err(PermissionChangeError::Unavailable);
        }
        let snapshot = state.snapshot();
        if &intent.expected != snapshot.revision() {
            return Err(PermissionChangeError::StaleRevision);
        }
        if matches!(
            intent.action,
            PermissionAction::TrustProjectConfiguration(_) | PermissionAction::RevokeProjectTrust
        ) {
            let ProjectPermissions::Ready {
                configuration,
                trusted_config,
                ..
            } = snapshot.project()
            else {
                return Err(PermissionChangeError::Unavailable);
            };
            let fingerprint = match intent.action {
                PermissionAction::TrustProjectConfiguration(fingerprint) => {
                    let config = configuration
                        .as_ref()
                        .filter(|config| config.fingerprint() == fingerprint)
                        .ok_or(PermissionChangeError::StaleRevision)?;
                    if !config
                        .rules()
                        .iter()
                        .any(|rule| rule.action == plexmaton_agent::PermissionRuleAction::Allow)
                    {
                        return Err(PermissionChangeError::NotFound);
                    }
                    Some(fingerprint)
                }
                PermissionAction::RevokeProjectTrust if trusted_config.is_some() => None,
                _ => return Err(PermissionChangeError::NotFound),
            };
            drop(state);
            let transaction = transaction.ok_or(PermissionChangeError::Unavailable)?;
            let revision = transaction.snapshot().revision.clone();
            let committed = transaction
                .trust_config(&revision, fingerprint, cancelled)
                .map_err(|error| self.store_failure(error))?;
            return self.observe_project(committed.snapshot());
        }
        if let PermissionAction::Revoke(id) = &intent.action
            && snapshot
                .project_grants()
                .iter()
                .any(|grant| &grant.id == id)
        {
            drop(state);
            let transaction = transaction.ok_or(PermissionChangeError::Unavailable)?;
            let revision = transaction.snapshot().revision.clone();
            let committed = transaction
                .revoke(&revision, id.clone(), cancelled)
                .map_err(|error| self.store_failure(error))?;
            return self.observe_project(committed.snapshot());
        }
        state.apply_control(intent)
    }

    /// The effect's authorization ordering point, called immediately before polling its executor.
    pub(in crate::runtime) fn authorize(
        &self,
        call: &AdmittedToolCall,
        authorization: &ToolAuthorization,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), PermissionChangeError> {
        let _transaction = self.project_transaction(cancelled)?;
        let state = self
            .state
            .lock()
            .map_err(|_| PermissionChangeError::Unavailable)?;
        if cancelled() {
            return Err(PermissionChangeError::Unavailable);
        }
        permits_authorization(&state.snapshot(), call, authorization)
    }

    fn project_transaction(
        &self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Option<PermissionTransaction<'_>>, PermissionChangeError> {
        let Some(store) = &self.project else {
            return Ok(None);
        };
        let transaction = store
            .transaction(cancelled)
            .map_err(|_| self.source_failure())?;
        let configuration = self
            .configuration
            .as_ref()
            .map(|source| source.load(cancelled))
            .transpose()
            .map_err(|_| self.source_failure())?
            .flatten();
        self.publish_project(transaction.snapshot(), configuration)?;
        Ok(Some(transaction))
    }

    fn observe_project(
        &self,
        project: &ProjectPermissionSnapshot,
    ) -> Result<(), PermissionChangeError> {
        let configuration = match self
            .state
            .lock()
            .map_err(|_| PermissionChangeError::Unavailable)?
            .snapshot()
            .project()
        {
            ProjectPermissions::Ready { configuration, .. } => configuration.clone(),
            _ => None,
        };
        self.publish_project(project, configuration)
    }

    fn publish_project(
        &self,
        project: &ProjectPermissionSnapshot,
        configuration: Option<PermissionConfiguration>,
    ) -> Result<(), PermissionChangeError> {
        self.state
            .lock()
            .map_err(|_| PermissionChangeError::Unavailable)?
            .observe_project(ProjectPermissions::Ready {
                can_remember: project.can_remember(),
                configuration,
                trusted_config: project.trusted_config,
                revision: project.revision.clone(),
                grants: project.grants.clone(),
            })
    }

    fn source_failure(&self) -> PermissionChangeError {
        if let Ok(mut state) = self.state.lock() {
            let _refused = state.observe_project(ProjectPermissions::Unavailable);
        }
        PermissionChangeError::Unavailable
    }

    fn store_failure(&self, error: PermissionStoreError) -> PermissionChangeError {
        let mapped = match error {
            PermissionStoreError::StaleRevision => PermissionChangeError::StaleRevision,
            PermissionStoreError::Capacity => PermissionChangeError::Capacity,
            PermissionStoreError::NotFound => PermissionChangeError::NotFound,
            _ => PermissionChangeError::Unavailable,
        };
        if mapped == PermissionChangeError::Unavailable
            && let Ok(mut state) = self.state.lock()
        {
            let _refused = state.observe_project(ProjectPermissions::Unavailable);
        }
        mapped
    }
}
