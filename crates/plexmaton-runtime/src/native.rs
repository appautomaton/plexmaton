//! Runtime-owned composition of native file, command and skill tools.

use std::{
    ffi::{OsStr, OsString},
    path::Path,
    sync::Arc,
};

use futures_util::future::{BoxFuture, FutureExt as _};
use plexmaton_agent::{
    AdmissionOutcome, AdmissionRefusal, AdmissionRequest, AdmittedToolCall, MAX_TOOL_OUTCOME_BYTES,
    ToolExecutionResult, ToolOutcome, bounded_tool_text,
};
use plexmaton_command::{
    COMMAND_DEFINITION_ID, COMMAND_DESCRIPTION, COMMAND_TOOL_NAME, CommandTool,
    CommandToolConfigurationError, command_parameters_schema,
};
use plexmaton_file_tools::{
    CREATE_TOOL_NAME, EDIT_TOOL_NAME, FileCancellation, FileTools, PathError, READ_TOOL_NAME,
    SEARCH_TOOL_NAME,
};
use plexmaton_provider::{FunctionTool, FunctionToolError};
use thiserror::Error;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::{
    ChildCollaborationIngress, CollaborationIngressOutcome, CollaborationToolScope,
    MainCollaborationIngress,
    collaboration_tools::{
        admit_collaboration_tool, is_collaboration_call, is_collaboration_tool_name,
        parse_admitted_collaboration_tool,
    },
};

mod collaboration;
mod skill;
pub(crate) use skill::{
    ExplicitSkillError, SkillReadTask, explicit_skill_name, origin as skill_source,
    selected_skill_matches,
};

/// Failure to construct the one trusted native catalog before terminal ownership.
#[derive(Debug, Error)]
pub enum NativeToolSetupError {
    /// The workspace could not be pinned for descriptor-rooted file tools.
    #[error("cannot configure workspace file tools: {0}")]
    File(#[from] PathError),
    /// The workspace could not be fixed for foreground commands.
    #[error("cannot configure command tool: {0}")]
    Command(#[from] CommandToolConfigurationError),
    /// A first-party model schema was internally invalid.
    #[error("cannot publish native tool definition: {0}")]
    Definition(#[from] FunctionToolError),
    #[error("cannot discover skills: {0}")]
    Skills(#[from] plexmaton_skills::SkillError),
    #[error("skill catalog exceeds its encoded byte limit")]
    SkillCatalogTooLarge,
    #[error("native collaboration tools cannot be rebound to another runtime role")]
    CollaborationProfile,
}

/// The concrete trusted catalog used by one live runtime.
///
/// One mutable file owner retains the observation source of truth. Its operations serialize on an
/// async mutex before entering the blocking pool; command execution remains independently async.
#[derive(Clone)]
pub struct NativeToolCatalog {
    file: Arc<Mutex<FileTools>>,
    command: Arc<CommandTool>,
    api_key_environments: std::collections::BTreeSet<OsString>,
    definitions: Arc<[FunctionTool]>,
    skills: Option<Arc<plexmaton_skills::SkillCatalog>>,
    profile: NativeToolProfile,
    collaboration: Option<NativeCollaborationIngress>,
}

#[derive(Clone)]
enum NativeCollaborationIngress {
    Main(MainCollaborationIngress),
    Child(ChildCollaborationIngress),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeToolProfile {
    Full,
    ReadOnly,
    MainCollaboration,
    ChildCollaboration,
}

impl NativeToolProfile {
    fn allows(self, name: &str) -> bool {
        match self {
            Self::Full | Self::MainCollaboration => true,
            Self::ReadOnly => matches!(name, READ_TOOL_NAME | SEARCH_TOOL_NAME),
            Self::ChildCollaboration => {
                matches!(
                    name,
                    READ_TOOL_NAME | SEARCH_TOOL_NAME | crate::SEND_MAIL_TOOL_NAME
                )
            }
        }
    }

    fn allows_call(self, call: &AdmittedToolCall) -> bool {
        matches!(self, Self::Full | Self::MainCollaboration)
            || FileTools::is_inspection_call(call)
            || (self == Self::ChildCollaboration && is_collaboration_call(call))
    }

    const fn collaboration_scope(self) -> Option<CollaborationToolScope> {
        match self {
            Self::MainCollaboration => Some(CollaborationToolScope::Main),
            Self::ChildCollaboration => Some(CollaborationToolScope::Child),
            Self::Full | Self::ReadOnly => None,
        }
    }
}

/// Catalog-owned compiler for configured permission scopes, retaining the command execution context.
#[derive(Clone)]
pub struct NativePermissionCompiler {
    command: Arc<CommandTool>,
}

impl NativePermissionCompiler {
    /// Pins the known read/search pair for opt-in configured inspection policy.
    #[must_use]
    pub fn native_inspection(&self) -> plexmaton_agent::PermissionMatcher {
        let (read, search) = FileTools::inspection_permission_definitions();
        plexmaton_agent::PermissionMatcher::NativeInspection { read, search }
    }

    /// Pins the known create/edit pair; capability names never select preset membership.
    #[must_use]
    pub fn native_file_changes(&self) -> plexmaton_agent::PermissionMatcher {
        let (create, edit) = FileTools::permission_definitions();
        plexmaton_agent::PermissionMatcher::NativeFileChanges { create, edit }
    }

    /// Explicit literal tokens retain the same definition and captured context as admission.
    #[must_use]
    pub fn command_prefix(
        &self,
        arguments: Vec<String>,
    ) -> Option<plexmaton_agent::PermissionMatcher> {
        self.command.prefix_permission(arguments)
    }

    /// Exact source must satisfy native command admission bounds and retains captured context.
    #[must_use]
    pub fn exact_command(&self, source: &str) -> Option<plexmaton_agent::PermissionMatcher> {
        self.command.exact_permission(source)
    }
}

impl NativeToolCatalog {
    /// Shares only permission compilation; configuration readers cannot execute tools.
    #[must_use]
    pub fn permission_compiler(&self) -> NativePermissionCompiler {
        NativePermissionCompiler {
            command: Arc::clone(&self.command),
        }
    }
    /// Pins one workspace, its provider credential name, ripgrep, and descriptor driver command.
    pub fn open(
        workspace_root: impl AsRef<Path>,
        api_key_environment: impl AsRef<OsStr>,
        ripgrep: impl Into<std::path::PathBuf>,
        directory_driver: impl Into<std::path::PathBuf>,
        directory_driver_prefix: Vec<OsString>,
    ) -> Result<Self, NativeToolSetupError> {
        let workspace_root = workspace_root.as_ref();
        let api_key_environment = api_key_environment.as_ref().to_os_string();
        let file = FileTools::open_with_directory_driver_prefix(
            workspace_root,
            ripgrep,
            directory_driver,
            directory_driver_prefix,
        )?;
        let command = CommandTool::new(workspace_root, &api_key_environment)?;
        let mut definitions = Vec::with_capacity(5);
        for definition in FileTools::definitions() {
            definitions.push(FunctionTool::new(
                definition.name(),
                definition.description(),
                definition.parameters().clone(),
            )?);
        }
        definitions.push(FunctionTool::new(
            COMMAND_TOOL_NAME,
            COMMAND_DESCRIPTION,
            command_parameters_schema(),
        )?);
        Ok(Self {
            file: Arc::new(Mutex::new(file)),
            command: Arc::new(command),
            api_key_environments: [api_key_environment].into_iter().collect(),
            definitions: definitions.into(),
            skills: None,
            profile: NativeToolProfile::Full,
            collaboration: None,
        })
    }

    /// Permanently narrows this catalog to workspace inspection for a delegated child (CHB-1).
    #[must_use]
    pub fn into_read_only(mut self) -> Self {
        if self.profile == NativeToolProfile::ChildCollaboration {
            return self;
        }
        self.profile = NativeToolProfile::ReadOnly;
        self.skills = None;
        self.collaboration = None;
        self.definitions = FileTools::definitions()
            .into_iter()
            .filter(|definition| self.profile.allows(definition.name()))
            .map(|definition| {
                FunctionTool::new(
                    definition.name(),
                    definition.description(),
                    definition.parameters().clone(),
                )
                .unwrap_or_else(|error| {
                    unreachable!("validated native definition became invalid: {error}")
                })
            })
            .collect::<Vec<_>>()
            .into();
        self
    }

    /// Discovers optional skill roots before terminal ownership, or on a retained file worker.
    pub fn with_skill_roots(
        mut self,
        user_home: &Path,
        project_root: &Path,
        cancellation: &FileCancellation,
    ) -> Result<Self, NativeToolSetupError> {
        if matches!(
            self.profile,
            NativeToolProfile::ReadOnly | NativeToolProfile::ChildCollaboration
        ) {
            return Ok(self);
        }
        let catalog =
            plexmaton_skills::SkillCatalog::discover(user_home, project_root, cancellation)?;
        if !catalog.entries().is_empty() {
            let mut definitions = self.definitions.to_vec();
            definitions.push(skill::definition(&catalog)?);
            self.definitions = definitions.into();
        }
        self.skills = Some(Arc::new(catalog));
        Ok(self)
    }

    pub(crate) fn permission_workspace(&self) -> [u8; 32] {
        self.command.permission_workspace()
    }

    pub(crate) fn skills(&self) -> Option<Arc<plexmaton_skills::SkillCatalog>> {
        self.skills.as_ref().map(Arc::clone)
    }

    pub(crate) fn explicit_resource_authorized(
        &self,
        request: &AdmissionRequest,
        agent: &plexmaton_agent::Agent,
    ) -> bool {
        skill::explicit_resource_authorized(self.skills.as_deref(), request, agent)
    }

    /// Exclude every switchable provider credential before creating permission compilers.
    /// Tool ownership and captured environment values stay unchanged; only named keys are removed.
    #[must_use]
    pub fn with_provider_credentials<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        for name in names {
            let name = name.as_ref();
            Arc::make_mut(&mut self.command).exclude_environment(name);
            self.api_key_environments.insert(name.to_os_string());
        }
        self
    }

    pub(crate) fn excludes_api_key_environment(&self, expected: &str) -> bool {
        self.api_key_environments.contains(OsStr::new(expected))
    }

    pub(crate) fn provider_definitions(&self) -> Arc<[FunctionTool]> {
        Arc::clone(&self.definitions)
    }

    pub(crate) fn admit(
        &self,
        request: AdmissionRequest,
        cancellation: NativeCancellation,
        explicit_resource: bool,
    ) -> BoxFuture<'static, AdmissionOutcome> {
        let name = request.requested().name.as_str();
        if !self.profile.allows(name) {
            return async move { request.refuse(AdmissionRefusal::UnknownTool) }.boxed();
        }
        if is_collaboration_tool_name(name) {
            return match self.profile.collaboration_scope() {
                Some(scope) => async move { admit_collaboration_tool(scope, request) }.boxed(),
                None => async move { request.refuse(AdmissionRefusal::UnknownTool) }.boxed(),
            };
        }
        match name {
            skill::NAME => {
                let catalog = self.skills();
                async move { skill::admit(catalog.as_deref(), request, explicit_resource) }.boxed()
            }
            COMMAND_TOOL_NAME => {
                let command = Arc::clone(&self.command);
                async move { command.admit_with_cancellation(request, &|| cancellation.is_cancelled()) }.boxed()
            }
            READ_TOOL_NAME | SEARCH_TOOL_NAME | EDIT_TOOL_NAME | CREATE_TOOL_NAME => {
                let file = Arc::clone(&self.file);
                let call_id = request.requested().call_id.clone();
                async move {
                    let file = tokio::select! {
                        biased;
                        () = cancellation.async_token.cancelled() => {
                            return request.refuse(AdmissionRefusal::Cancelled);
                        }
                        file = file.lock_owned() => file,
                    };
                    let file_cancellation = cancellation.file.clone();
                    match tokio::task::spawn_blocking(move || {
                        file.admit(request, &file_cancellation)
                    })
                    .await
                    {
                        Ok(outcome) => outcome,
                        Err(_) => AdmissionOutcome::Refused {
                            call_id,
                            reason: AdmissionRefusal::DefinitionUnavailable,
                        },
                    }
                }
                .boxed()
            }
            _ => async move { request.refuse(AdmissionRefusal::UnknownTool) }.boxed(),
        }
    }

    pub(crate) fn execute(
        &self,
        call: AdmittedToolCall,
        cancellation: NativeCancellation,
    ) -> BoxFuture<'static, ToolExecutionResult> {
        if is_collaboration_call(&call) {
            let Some(scope) = self.profile.collaboration_scope() else {
                return async move {
                    bounded_failure(
                        "collaboration_profile",
                        "collaboration tool is unavailable in this runtime role",
                    )
                }
                .boxed();
            };
            let Ok(request) = parse_admitted_collaboration_tool(scope, &call) else {
                return async move {
                    bounded_failure(
                        "collaboration_definition",
                        "the admitted collaboration tool no longer matches this runtime role",
                    )
                }
                .boxed();
            };
            let Some(ingress) = self.collaboration.clone() else {
                return async move {
                    bounded_failure(
                        "collaboration_owner",
                        "the authenticated collaboration owner is unavailable",
                    )
                }
                .boxed();
            };
            #[cfg(debug_assertions)]
            collaboration::record_invocation(&call);
            let call_id = call.requested().call_id.clone();
            let cancellation = cancellation.async_token.child_token();
            return async move {
                let result = match ingress {
                    NativeCollaborationIngress::Main(ingress)
                        if scope == CollaborationToolScope::Main =>
                    {
                        ingress.execute(call_id, request, cancellation).await
                    }
                    NativeCollaborationIngress::Child(ingress)
                        if scope == CollaborationToolScope::Child =>
                    {
                        ingress.execute(call_id, request, cancellation).await
                    }
                    _ => Err(crate::CollaborationIngressRefusal::CapabilityMismatch),
                };
                match result {
                    Ok(result) => collaboration::tool_result(result),
                    Err(error) => bounded_failure("collaboration_ingress", &error.to_string()),
                }
            }
            .boxed();
        }
        if !self.profile.allows_call(&call) {
            return async move {
                bounded_failure(
                    "capability_floor",
                    "delegated child tool profile refused this execution",
                )
            }
            .boxed();
        }
        if call.definition_id().as_str() == skill::DEFINITION_ID {
            let catalog = self.skills();
            return async move {
                let Some(catalog) = catalog else {
                    return bounded_failure("skill_unavailable", "skills are not configured");
                };
                let result = tokio::task::spawn_blocking(move || {
                    skill::execute(&catalog, &call, &cancellation.file)
                })
                .await;
                bound_result(result.unwrap_or_else(|_| {
                    bounded_failure("skill_worker", "skill reader stopped unexpectedly")
                }))
            }
            .boxed();
        }
        if call.definition_id().as_str() == COMMAND_DEFINITION_ID {
            let command = Arc::clone(&self.command);
            return async move {
                let outcome = match command
                    .execute(&call, cancellation.async_token.child_token())
                    .await
                {
                    Ok(output) => ToolExecutionResult::new(
                        ToolOutcome::Succeeded {
                            output: output.to_model_text(),
                        },
                        Some(output.to_transcript_detail()),
                    ),
                    Err(error) => bounded_failure("command_execution", &error.to_string()),
                };
                bound_result(outcome)
            }
            .boxed();
        }

        let file = Arc::clone(&self.file);
        async move {
            let mut file = tokio::select! {
                biased;
                () = cancellation.async_token.cancelled() => {
                    return bounded_failure("cancelled", "file tool cancelled before execution");
                }
                file = file.lock_owned() => file,
            };
            let file_cancellation = cancellation.file.clone();
            let outcome =
                match tokio::task::spawn_blocking(move || file.execute(&call, &file_cancellation))
                    .await
                {
                    Ok(outcome) => outcome,
                    Err(_) => bounded_failure("worker", "file tool worker terminated unexpectedly"),
                };
            bound_result(outcome)
        }
        .boxed()
    }
}

/// Both cancellation mechanisms owned by one admission or execution future.
#[derive(Clone)]
pub(crate) struct NativeCancellation {
    async_token: CancellationToken,
    file: FileCancellation,
}

impl NativeCancellation {
    pub(crate) fn new() -> Self {
        Self {
            async_token: CancellationToken::new(),
            file: FileCancellation::new(),
        }
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.async_token.is_cancelled() || self.file.is_cancelled()
    }

    pub(crate) fn cancel(&self) {
        self.file.cancel();
        self.async_token.cancel();
    }
}

fn bound_result(result: ToolExecutionResult) -> ToolExecutionResult {
    let replacement = match result.outcome() {
        ToolOutcome::Succeeded { output } if output.len() > MAX_TOOL_OUTCOME_BYTES => Some((
            "result_too_large",
            "native tool result exceeded its hard byte bound",
        )),
        ToolOutcome::Failed { message } if message.len() > MAX_TOOL_OUTCOME_BYTES => Some((
            "failure_too_large",
            "native tool failure exceeded its hard byte bound",
        )),
        _ => None,
    };
    if let Some((kind, message)) = replacement {
        bounded_failure(kind, message)
    } else {
        result
    }
}

fn bounded_failure(kind: &str, message: &str) -> ToolExecutionResult {
    let available = MAX_TOOL_OUTCOME_BYTES
        .saturating_sub(kind.len())
        .saturating_sub(2);
    let mut end = message.len().min(available);
    while !message.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    let message = format!("{kind}: {}", &message[..end]);
    let presentation = Some(bounded_tool_text(&message, 0));
    ToolExecutionResult::new(ToolOutcome::Failed { message }, presentation)
}

#[cfg(test)]
mod tests {
    use plexmaton_agent::{
        AdmissionOutcome, AdmissionRequest, Agent, Effect, Input, ModelEvent, ModelOutputPosition,
        StopReason, ToolCall, ToolDefinitionRevision, ToolExecutionResult, ToolOutcome, UnixMillis,
    };
    use plexmaton_core::{AgentId, ToolCallId, ToolCapability, ToolDefinitionId};
    use serde_json::json;

    use super::{MAX_TOOL_OUTCOME_BYTES, NativeCancellation, NativeToolCatalog, bound_result};

    struct Directory(std::path::PathBuf);

    impl Directory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "plexmaton-read-only-tools-{}",
                uuid::Uuid::now_v7()
            ));
            std::fs::create_dir(&path).expect("create test directory");
            Self(path)
        }

        fn catalog(&self) -> NativeToolCatalog {
            NativeToolCatalog::open(&self.0, "TEST_KEY", "/bin/false", "/bin/false", Vec::new())
                .expect("open native catalog")
        }
    }

    impl Drop for Directory {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).expect("remove test directory");
        }
    }

    fn admission_request(name: &str, arguments: serde_json::Value) -> AdmissionRequest {
        let agent_id = AgentId::new("read-only-fixture").expect("agent");
        let mut agent = Agent::new(agent_id);
        agent.handle_at(
            Input::Submitted {
                text: "inspect the workspace".into(),
            },
            UnixMillis::EPOCH,
        );
        let step_id = agent.active_model_step().expect("active step");
        agent.handle_at(
            Input::Streamed {
                step_id: step_id.clone(),
                event: ModelEvent::Called {
                    position: ModelOutputPosition::new(0, 0),
                    call: ToolCall {
                        call_id: ToolCallId::new("read-only-call").expect("call"),
                        name: name.into(),
                        arguments: arguments.to_string(),
                    },
                },
            },
            UnixMillis::EPOCH,
        );
        let stopped = agent.handle_at(
            Input::Streamed {
                step_id,
                event: ModelEvent::Stopped(StopReason::ToolCalls),
            },
            UnixMillis::EPOCH,
        );
        let [effect] = stopped.effects.try_into().expect("one effect");
        let Effect::AdmitTool(request) = effect else {
            panic!("one admission request")
        };
        request
    }

    #[test]
    fn live_1_native_outcomes_are_bounded_before_the_loop_can_retain_them() {
        let result = bound_result(ToolExecutionResult::new(
            ToolOutcome::Succeeded {
                output: "x".repeat(MAX_TOOL_OUTCOME_BYTES + 1),
            },
            None,
        ));
        let ToolOutcome::Failed { message } = result.outcome() else {
            panic!("oversized output must become a bounded failure");
        };
        assert!(message.len() <= MAX_TOOL_OUTCOME_BYTES);
        assert!(message.contains("result_too_large"));
    }

    /// CHB-1: the child profile advertises and admits only reads, then rechecks execution.
    #[tokio::test]
    async fn chb_1_read_only_catalog_cannot_be_widened_by_an_admitted_call() {
        let directory = Directory::new();
        std::fs::write(directory.0.join("source.rs"), "fn main() {}\n")
            .expect("write readable fixture");
        let full = directory.catalog();
        let read_only = directory.catalog().into_read_only();
        assert_eq!(read_only.provider_definitions().len(), 2);

        for (name, arguments) in [
            (
                plexmaton_file_tools::READ_TOOL_NAME,
                json!({"path": "source.rs", "offset": null, "limit": null}),
            ),
            (
                plexmaton_file_tools::SEARCH_TOOL_NAME,
                json!({"pattern": "main", "path": null, "glob": null, "limit": null}),
            ),
        ] {
            let admission = read_only
                .admit(
                    admission_request(name, arguments),
                    NativeCancellation::new(),
                    false,
                )
                .await;
            let AdmissionOutcome::Admitted(call) = admission else {
                panic!("read-only catalog admits {name}")
            };
            assert!(plexmaton_file_tools::FileTools::is_inspection_call(&call));
            if name == plexmaton_file_tools::READ_TOOL_NAME {
                let result = read_only.execute(call, NativeCancellation::new()).await;
                assert!(matches!(
                    result.outcome(),
                    ToolOutcome::Succeeded { output } if output.contains("fn main()")
                ));
            }
        }
        for name in [
            plexmaton_file_tools::EDIT_TOOL_NAME,
            plexmaton_file_tools::CREATE_TOOL_NAME,
            plexmaton_command::COMMAND_TOOL_NAME,
            super::skill::NAME,
            "delegate",
        ] {
            assert!(matches!(
                read_only
                    .admit(
                        admission_request(name, json!({})),
                        NativeCancellation::new(),
                        false,
                    )
                    .await,
                AdmissionOutcome::Refused {
                    reason: plexmaton_agent::AdmissionRefusal::UnknownTool,
                    ..
                }
            ));
        }

        let AdmissionOutcome::Admitted(command) = full
            .admit(
                admission_request(
                    plexmaton_command::COMMAND_TOOL_NAME,
                    json!({"cmd": "touch forbidden", "timeout_ms": null}),
                ),
                NativeCancellation::new(),
                false,
            )
            .await
        else {
            panic!("full catalog admits the command fixture")
        };
        let result = read_only.execute(command, NativeCancellation::new()).await;
        assert!(matches!(
            result.outcome(),
            ToolOutcome::Failed { message } if message.contains("capability_floor")
        ));
        assert!(!directory.0.join("forbidden").exists());

        let AdmissionOutcome::Admitted(create) = full
            .admit(
                admission_request(
                    plexmaton_file_tools::CREATE_TOOL_NAME,
                    json!({"path": "created.rs", "content": "forbidden\n"}),
                ),
                NativeCancellation::new(),
                false,
            )
            .await
        else {
            panic!("full catalog admits the create fixture")
        };
        assert!(!plexmaton_file_tools::FileTools::is_inspection_call(
            &create
        ));
        let result = read_only.execute(create, NativeCancellation::new()).await;
        assert!(matches!(
            result.outcome(),
            ToolOutcome::Failed { message } if message.contains("capability_floor")
        ));
        assert!(!directory.0.join("created.rs").exists());

        let forged = admission_request(
            plexmaton_file_tools::READ_TOOL_NAME,
            json!({"path": "source.rs", "offset": null, "limit": null}),
        )
        .admit(
            ToolDefinitionId::new("foreign-process-v1").expect("definition"),
            ToolDefinitionRevision::new(1).expect("revision"),
            [ToolCapability::ProcessSpawn],
            "{}".into(),
            "forged read".into(),
            None,
        )
        .expect("bounded foreign admission");
        let AdmissionOutcome::Admitted(forged) = forged else {
            panic!("foreign catalog admitted fixture")
        };
        let result = read_only.execute(forged, NativeCancellation::new()).await;
        assert!(matches!(
            result.outcome(),
            ToolOutcome::Failed { message } if message.contains("capability_floor")
        ));

        let forged = admission_request(
            plexmaton_file_tools::READ_TOOL_NAME,
            json!({"path": "source.rs", "offset": null, "limit": null}),
        )
        .admit(
            ToolDefinitionId::new("foreign-read-v1").expect("definition"),
            ToolDefinitionRevision::new(1).expect("revision"),
            [ToolCapability::FileRead],
            "{}".into(),
            "foreign read".into(),
            None,
        )
        .expect("bounded foreign admission");
        let AdmissionOutcome::Admitted(forged) = forged else {
            panic!("foreign catalog admitted fixture")
        };
        let result = read_only.execute(forged, NativeCancellation::new()).await;
        assert!(matches!(
            result.outcome(),
            ToolOutcome::Failed { message } if message.contains("capability_floor")
        ));
    }
}
