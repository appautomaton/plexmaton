//! Coding Session ownership survives replacement of an individual Conversation runtime (PER-1).

use std::sync::{Arc, Mutex};

use plexmaton_agent::{
    AdmittedToolCall, ApprovalPolicy, Input, PermissionChangeError, PermissionPreparationOutcome,
    PermissionPreparationRequest, PermissionSnapshot, PolicyDecision, SessionPermissions,
    ToolAuthorization, ToolExecutionResult, ToolOutcome,
};
use plexmaton_core::CodingSessionId;

use crate::{LiveRuntime, NativeToolCatalog, RuntimeError};

mod configuration;
mod project;
pub use configuration::ProjectPermissionConfigurationSource;

/// One explicit shared owner for a coding Session's bounded, memory-only permission state.
///
/// The CLI retains this handle across `/new` and `/resume`. Dropping a Conversation runtime does
/// not revoke its Session; dropping the last handle ends the authority. No process global exists.
#[derive(Clone)]
pub struct CodingSessionPermissions {
    workspace: [u8; 32],
    state: Arc<Mutex<SessionPermissions>>,
    project: Option<plexmaton_permission_store::ProjectPermissionStore>,
    configuration: Option<Arc<dyn ProjectPermissionConfigurationSource>>,
}

impl CodingSessionPermissions {
    /// Begins a fresh coding Session bound to this catalog's physical working directory.
    #[must_use]
    pub fn new(tools: &NativeToolCatalog) -> Self {
        let id = CodingSessionId::new(format!("coding-{}", uuid::Uuid::now_v7()))
            .unwrap_or_else(|_| unreachable!("a UUID produces a nonempty identity"));
        let (create, edit) = plexmaton_file_tools::FileTools::permission_definitions();
        Self {
            workspace: tools.permission_workspace(),
            project: None,
            configuration: None,
            state: Arc::new(Mutex::new(
                SessionPermissions::new(id).with_native_file_changes(create, edit),
            )),
        }
    }

    pub(super) fn prepare(
        &self,
        request: PermissionPreparationRequest,
        cancellation: &crate::native::NativeCancellation,
    ) -> PermissionPreparationOutcome {
        let result = self.prepare_grant(&request, &|| cancellation.is_cancelled());
        match result {
            Ok(grant) => request.prepared(grant),
            Err(reason) => request.refused(reason),
        }
    }

    /// Revokes a temporary grant by its stable identity and the reviewed policy revision.
    pub fn revoke_session_grant(
        &self,
        revision: &plexmaton_agent::PermissionRevision,
        grant: &plexmaton_agent::PermissionGrantId,
    ) -> Result<(), PermissionChangeError> {
        self.apply_control(
            &plexmaton_core::PermissionIntent {
                expected: revision.clone(),
                action: plexmaton_core::PermissionAction::Revoke(grant.clone()),
            },
            &|| false,
        )
    }

    /// Current immutable view; a poisoned owner fails closed instead of resetting authority.
    pub fn snapshot(&self) -> Result<Arc<PermissionSnapshot>, RuntimeError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| RuntimeError::PermissionOwnerUnavailable)?
            .snapshot())
    }
}

impl LiveRuntime {
    /// Re-evaluates waiting calls after a user control completes, through the ordinary audit barrier.
    pub async fn permissions_changed(&mut self) -> Result<crate::DispatchReport, RuntimeError> {
        self.submit(self.agent_id.clone(), Input::PermissionsChanged)
            .await
    }
    /// Attaches the coding Session before this Conversation starts accepting work.
    pub fn use_coding_session(
        &mut self,
        owner: CodingSessionPermissions,
    ) -> Result<(), RuntimeError> {
        if self.has_active_work() || self.tools.permission_workspace() != owner.workspace {
            return Err(RuntimeError::PermissionOwnerMismatch);
        }
        self.agent.use_permission_snapshot(owner.snapshot()?);
        self.permissions = owner;
        Ok(())
    }

    pub(super) fn start_authorized_tool(
        &mut self,
        call: AdmittedToolCall,
        authorization: ToolAuthorization,
    ) -> Result<(), RuntimeError> {
        // An accepted control input owns cancellation even while the preceding audit waits.
        // Its ordinary agent transition will pay the call's result debt after this commit.
        if self.permission_dispatch_cancelled() {
            return Ok(());
        }
        let owner = self.permissions.clone();
        // Keep the bounded memory authority stable through the worker's dispatch handoff.
        let state = owner
            .state
            .lock()
            .map_err(|_| RuntimeError::PermissionOwnerUnavailable)?;
        let snapshot = state.snapshot();
        let checked = permits_authorization(&snapshot, &call, &authorization);
        if checked.is_ok() {
            return self
                .tools
                .start_execution(call, authorization, owner.clone());
        }
        let reason = checked
            .err()
            .unwrap_or(PermissionChangeError::StaleRevision);
        if self.pending_inputs.len() >= super::PENDING_INPUT_CAPACITY {
            return Err(RuntimeError::RuntimeInputQueueFull);
        }
        self.pending_inputs.push_back(super::PendingInput {
            input: Input::ToolFinished {
                call_id: call.requested().call_id.clone(),
                result: ToolExecutionResult::new(
                    ToolOutcome::PermissionRefused { reason },
                    Some(plexmaton_agent::bounded_tool_text(
                        "Permissions changed before execution; the tool did not run.",
                        0,
                    )),
                ),
            },
            observed_at: self.clock.now(),
            selected_skill: None,
            after: super::AfterCommit::None,
        });
        Ok(())
    }

    /// Retains the existing Session when constructing a replacement Conversation runtime.
    #[must_use]
    pub fn coding_session(&self) -> CodingSessionPermissions {
        self.permissions.clone()
    }

    pub(crate) fn refresh_permission_snapshot(&mut self) -> Result<(), RuntimeError> {
        self.agent
            .use_permission_snapshot(self.permissions.snapshot()?);
        Ok(())
    }

    pub(super) fn permission_dispatch_cancelled(&self) -> bool {
        self.shutdown_state != super::ShutdownState::Open
            || self
                .pending_inputs
                .iter()
                .any(|pending| pending.after == super::AfterCommit::Interrupt)
    }
}

fn permits_authorization(
    snapshot: &Arc<PermissionSnapshot>,
    call: &AdmittedToolCall,
    authorization: &ToolAuthorization,
) -> Result<(), PermissionChangeError> {
    let mut policy = ApprovalPolicy::default();
    policy.use_snapshot(Arc::clone(snapshot));
    let decision = policy.decide(call);
    if decision == PolicyDecision::Unavailable {
        return Err(PermissionChangeError::Unavailable);
    }
    let allowed = match authorization {
        ToolAuthorization::Once => decision != PolicyDecision::Forbidden,
        ToolAuthorization::Policy => decision == PolicyDecision::Allow,
        ToolAuthorization::Remembered { grant, .. } => {
            decision == PolicyDecision::Allow
                && snapshot
                    .grant(grant)
                    .is_some_and(|grant| grant.matcher.matches(call))
        }
    };
    if allowed {
        Ok(())
    } else {
        Err(PermissionChangeError::StaleRevision)
    }
}
