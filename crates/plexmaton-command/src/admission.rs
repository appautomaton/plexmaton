use std::{
    ffi::OsStr,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

use plexmaton_agent::{
    AdmissionOutcome, AdmissionRefusal, AdmissionRequest, AdmittedToolCall,
    MAX_REQUESTED_TOOL_ARGUMENT_BYTES, ToolDefinitionRevision, bounded_tool_text,
};
use plexmaton_core::{ToolCapability, ToolDefinitionId, ToolDetail};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    environment::CommandEnvironment,
    executor::execute,
    result::{CommandExecutionError, CommandOutput},
};

/// Model-visible command tool name.
pub const COMMAND_TOOL_NAME: &str = "exec_command";
/// Stable identity for the first command definition.
pub const COMMAND_DEFINITION_ID: &str = "native.exec-command.v1";
/// Provider-neutral description paired with the model parameter schema.
pub const COMMAND_DESCRIPTION: &str = "Run one noninteractive foreground POSIX shell command from \
the workspace root. timeout_ms is null for the 120000 ms default and may be at most 300000 ms.";
/// Timeout used when the model omits `timeout_ms`.
pub const DEFAULT_TIMEOUT_MS: u64 = 120_000;
/// Hard admission ceiling for a command timeout.
pub const MAX_TIMEOUT_MS: u64 = 300_000;
/// Maximum Unicode scalar values accepted in a model command.
pub const MAX_COMMAND_CHARACTERS: usize = 6 * 1024;
/// Maximum UTF-8 bytes accepted in a model command.
pub const MAX_COMMAND_BYTES: usize = MAX_COMMAND_CHARACTERS * 4;

const MAX_WORKSPACE_ROOT_BYTES: usize = 4 * 1024;

const DEFINITION_REVISION: u64 = 1;
const CAPABILITIES: [ToolCapability; 3] = [
    ToolCapability::FileRead,
    ToolCapability::FileWrite,
    ToolCapability::ProcessSpawn,
];

/// Returns the strict provider-neutral JSON Schema advertised for `exec_command`.
#[must_use]
pub fn command_parameters_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "cmd": {
                "type": "string",
                "minLength": 1,
                "maxLength": MAX_COMMAND_CHARACTERS,
                "description": "The noninteractive shell command to run."
            },
            "timeout_ms": {
                "type": ["integer", "null"],
                "minimum": 1,
                "maximum": MAX_TIMEOUT_MS,
                "description": "Execution timeout in milliseconds; null means 120000."
            }
        },
        "required": ["cmd", "timeout_ms"],
        "additionalProperties": false
    })
}

/// Configuration failure before a command tool can admit requests.
#[derive(Debug, Error)]
pub enum CommandToolConfigurationError {
    /// The configured root could not be resolved once to a stable absolute path.
    #[error("cannot canonicalize command workspace root {path}: {source}")]
    Canonicalize {
        /// Root supplied by the composition owner.
        path: PathBuf,
        /// Filesystem failure.
        #[source]
        source: std::io::Error,
    },
    /// The resolved path was not a directory.
    #[error("command workspace root is not a directory: {0}")]
    NotDirectory(PathBuf),
    /// The trusted canonical form requires a Unicode root path.
    #[error("command workspace root is not valid UTF-8: {0:?}")]
    NonUtf8Root(PathBuf),
    /// The trusted root would consume too much of the admitted-call bound after JSON escaping.
    #[error("command workspace root exceeds 4096 UTF-8 bytes: {0}")]
    RootTooLong(PathBuf),
}

/// One concrete command definition bound to a canonical workspace root.
///
/// The root fixes `cwd`; it is not a filesystem sandbox. An allowed shell can still
/// use absolute paths, `..`, symlinks, network clients, or any other host authority available to
/// the Plexmaton process. The broad declared capabilities make that limitation visible to policy.
#[derive(Clone)]
pub struct CommandTool {
    workspace_root: PathBuf,
    workspace_root_utf8: String,
    workspace_identity: WorkspaceIdentity,
    environment: CommandEnvironment,
    definition_id: ToolDefinitionId,
    definition_revision: ToolDefinitionRevision,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WorkspaceIdentity {
    device: u64,
    inode: u64,
}

impl CommandTool {
    /// Pins an execution root and excludes the selected provider API-key variable from commands.
    pub fn new(
        workspace_root: impl AsRef<Path>,
        api_key_environment: impl AsRef<OsStr>,
    ) -> Result<Self, CommandToolConfigurationError> {
        Self::open(
            workspace_root.as_ref(),
            CommandEnvironment::capture_excluding(api_key_environment.as_ref()),
        )
    }

    fn open(
        workspace_root: &Path,
        environment: CommandEnvironment,
    ) -> Result<Self, CommandToolConfigurationError> {
        let supplied = workspace_root;
        let root = std::fs::canonicalize(supplied).map_err(|source| {
            CommandToolConfigurationError::Canonicalize {
                path: supplied.to_path_buf(),
                source,
            }
        })?;
        if !root.is_dir() {
            return Err(CommandToolConfigurationError::NotDirectory(root));
        }
        let metadata = std::fs::metadata(&root).map_err(|source| {
            CommandToolConfigurationError::Canonicalize {
                path: root.clone(),
                source,
            }
        })?;
        let Some(root_utf8) = root.to_str() else {
            return Err(CommandToolConfigurationError::NonUtf8Root(root));
        };
        if root_utf8.len() > MAX_WORKSPACE_ROOT_BYTES {
            return Err(CommandToolConfigurationError::RootTooLong(root));
        }
        // These literals are reviewed definition data; changing either must change this module.
        let definition_id = ToolDefinitionId::new(COMMAND_DEFINITION_ID)
            .unwrap_or_else(|error| panic!("command definition id invariant: {error}"));
        let definition_revision = ToolDefinitionRevision::new(DEFINITION_REVISION)
            .unwrap_or_else(|| panic!("command definition revision must be non-zero"));
        Ok(Self {
            workspace_root: root.clone(),
            workspace_root_utf8: root_utf8.to_owned(),
            workspace_identity: WorkspaceIdentity {
                device: metadata.dev(),
                inode: metadata.ino(),
            },
            environment,
            definition_id,
            definition_revision,
        })
    }

    /// Returns the canonical directory fixed for every call this instance admits.
    #[must_use]
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Parses and canonicalizes one untrusted model request (APV-1, CMD-1).
    #[must_use]
    pub fn admit(&self, request: AdmissionRequest) -> AdmissionOutcome {
        if request.requested().name != COMMAND_TOOL_NAME {
            return request.refuse(AdmissionRefusal::UnknownTool);
        }
        if request.requested().arguments.len() > MAX_REQUESTED_TOOL_ARGUMENT_BYTES {
            return request.refuse(AdmissionRefusal::InvalidArguments);
        }
        let Ok(arguments) = serde_json::from_str::<ModelArguments>(&request.requested().arguments)
        else {
            return request.refuse(AdmissionRefusal::InvalidArguments);
        };
        if !valid_command(&arguments.cmd) {
            return request.refuse(AdmissionRefusal::InvalidArguments);
        }
        let timeout_ms = arguments.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);
        if !(1..=MAX_TIMEOUT_MS).contains(&timeout_ms) {
            return request.refuse(AdmissionRefusal::InvalidArguments);
        }
        let canonical = CanonicalArguments {
            cmd: arguments.cmd,
            timeout_ms,
            workspace_root: self.workspace_root_utf8.clone(),
        };
        let Ok(canonical_arguments) = serde_json::to_string(&canonical) else {
            return request.refuse(AdmissionRefusal::InvalidArguments);
        };
        let detail = approval_detail(&canonical);
        let invocation = invocation_detail(&canonical);
        let call_id = request.requested().call_id.clone();
        match request.admit(
            self.definition_id.clone(),
            self.definition_revision,
            CAPABILITIES,
            canonical_arguments,
            detail,
            Some(invocation),
        ) {
            Ok(outcome) => outcome,
            Err(_) => AdmissionOutcome::Refused {
                call_id,
                reason: AdmissionRefusal::InvalidArguments,
            },
        }
    }

    /// Executes one matching, already-admitted call and returns only after process and pipe cleanup.
    ///
    /// The caller must drive this future to completion. Interruption is expressed through
    /// `cancellation`, not by dropping the future, because asynchronous cleanup cannot be proved by
    /// a detached `Drop` reaper (CMD-5 and CMD-6).
    pub async fn execute(
        &self,
        admitted: &AdmittedToolCall,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<CommandOutput, CommandExecutionError> {
        let arguments = self.validate_admitted(admitted)?;
        if !self.workspace_is_current() {
            return Err(CommandExecutionError::WorkspaceChanged);
        }
        execute(
            &self.workspace_root,
            arguments,
            &self.environment,
            cancellation,
        )
        .await
    }

    fn validate_admitted(
        &self,
        admitted: &AdmittedToolCall,
    ) -> Result<CanonicalArguments, CommandExecutionError> {
        if admitted.definition_id() != &self.definition_id
            || admitted.definition_revision() != self.definition_revision
            || admitted.requested().name != COMMAND_TOOL_NAME
            || !admitted.capabilities().iter().eq(CAPABILITIES)
        {
            return Err(CommandExecutionError::InvalidAdmittedCall);
        }
        let arguments: CanonicalArguments = serde_json::from_str(admitted.canonical_arguments())
            .map_err(|_| CommandExecutionError::InvalidAdmittedCall)?;
        if arguments.workspace_root != self.workspace_root_utf8
            || !valid_command(&arguments.cmd)
            || !(1..=MAX_TIMEOUT_MS).contains(&arguments.timeout_ms)
        {
            return Err(CommandExecutionError::InvalidAdmittedCall);
        }
        Ok(arguments)
    }

    fn workspace_is_current(&self) -> bool {
        std::fs::metadata(&self.workspace_root).is_ok_and(|metadata| {
            metadata.is_dir()
                && metadata.dev() == self.workspace_identity.device
                && metadata.ino() == self.workspace_identity.inode
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelArguments {
    cmd: String,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CanonicalArguments {
    pub(crate) cmd: String,
    pub(crate) timeout_ms: u64,
    pub(crate) workspace_root: String,
}

fn valid_command(command: &str) -> bool {
    !command.trim().is_empty()
        && command.chars().count() <= MAX_COMMAND_CHARACTERS
        && command.len() <= MAX_COMMAND_BYTES
        && !command.as_bytes().contains(&0)
}

fn approval_detail(arguments: &CanonicalArguments) -> String {
    const LIMIT: usize = plexmaton_agent::MAX_APPROVAL_DETAIL_BYTES;
    const ROOT_LIMIT: usize = 256;
    let quoted_root = serde_json::to_string(&arguments.workspace_root).unwrap_or_else(|error| {
        panic!("serializing an already-owned workspace root cannot fail: {error}")
    });
    let quoted_command = serde_json::to_string(&arguments.cmd).unwrap_or_else(|error| {
        panic!("serializing an already-owned command string cannot fail: {error}")
    });
    let root = bounded_head_tail(&quoted_root, ROOT_LIMIT);
    let prefix = "Command ";
    let context = format!(" · cwd {root} · timeout {} ms", arguments.timeout_ms);
    let command = bounded_head_tail(
        &quoted_command,
        LIMIT
            .saturating_sub(prefix.len())
            .saturating_sub(context.len()),
    );
    format!("{prefix}{command}{context}")
}

fn invocation_detail(arguments: &CanonicalArguments) -> ToolDetail {
    let quoted_root = serde_json::to_string(&arguments.workspace_root).unwrap_or_else(|error| {
        unreachable!("serializing an already-owned workspace root cannot fail: {error}")
    });
    let quoted_command = serde_json::to_string(&arguments.cmd).unwrap_or_else(|error| {
        unreachable!("serializing an already-owned command string cannot fail: {error}")
    });
    bounded_tool_text(
        &format!(
            "Command {quoted_command}\ncwd: {quoted_root}\ntimeout_ms: {}",
            arguments.timeout_ms
        ),
        0,
    )
}

fn bounded_head_tail(text: &str, limit: usize) -> String {
    const MARKER_RESERVE: usize = 64;
    if text.len() <= limit {
        return text.to_owned();
    }
    if limit <= MARKER_RESERVE {
        return "[detail omitted]"[..limit.min("[detail omitted]".len())].to_owned();
    }
    let retained = limit - MARKER_RESERVE;
    let mut head_end = retained / 2;
    while !text.is_char_boundary(head_end) {
        head_end -= 1;
    }
    let mut tail_start = text.len() - (retained - head_end);
    while !text.is_char_boundary(tail_start) {
        tail_start += 1;
    }
    let omitted = text.len() - head_end - (text.len() - tail_start);
    format!(
        "{}...[{omitted} bytes omitted]...{}",
        &text[..head_end],
        &text[tail_start..]
    )
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use plexmaton_agent::{
        AdmissionOutcome, AdmissionRefusal, AdmissionRequest, Agent, Effect, Input, ModelEvent,
        StopReason, ToolCall,
    };
    use plexmaton_core::{AgentId, ToolCallId, ToolCapability, ToolDetail};
    use tokio_util::sync::CancellationToken;

    use super::{
        CAPABILITIES, COMMAND_TOOL_NAME, CanonicalArguments, CommandTool, DEFAULT_TIMEOUT_MS,
        MAX_COMMAND_BYTES, MAX_COMMAND_CHARACTERS, MAX_TIMEOUT_MS, approval_detail,
        command_parameters_schema,
    };

    static NEXT_DIR: AtomicU64 = AtomicU64::new(1);

    struct TestDirectory(std::path::PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let serial = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "plexmaton-command-admission-{}-{serial}",
                std::process::id()
            ));
            std::fs::create_dir(&path)
                .unwrap_or_else(|error| panic!("create test workspace {path:?}: {error}"));
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0)
                .unwrap_or_else(|error| panic!("remove test workspace {:?}: {error}", self.0));
        }
    }

    fn request(name: &str, arguments: String) -> AdmissionRequest {
        let mut agent = Agent::new(
            AgentId::new("command-admission-fixture")
                .unwrap_or_else(|error| panic!("fixture agent id: {error}")),
        );
        let _submitted = agent.handle_at(
            Input::Submitted {
                text: "exercise command admission".to_owned(),
            },
            plexmaton_agent::UnixMillis::EPOCH,
        );
        let step_id = agent
            .active_model_step()
            .unwrap_or_else(|| panic!("fixture model step did not open"));
        let called = agent.handle_at(
            Input::Streamed {
                step_id: step_id.clone(),
                event: ModelEvent::Called(ToolCall {
                    call_id: ToolCallId::new("command-1")
                        .unwrap_or_else(|error| panic!("fixture call id: {error}")),
                    name: name.to_owned(),
                    arguments,
                }),
            },
            plexmaton_agent::UnixMillis::EPOCH,
        );
        assert!(called.effects.is_empty());
        let stopped = agent.handle_at(
            Input::Streamed {
                step_id,
                event: ModelEvent::Stopped(StopReason::ToolCalls),
            },
            plexmaton_agent::UnixMillis::EPOCH,
        );
        let mut effects = stopped.effects.into_iter();
        let Some(Effect::AdmitTool(request)) = effects.next() else {
            panic!("fixture did not emit an admission request");
        };
        assert!(effects.next().is_none());
        request
    }

    #[test]
    fn cmd_1_admission_is_strict_canonical_and_pins_the_workspace() {
        let directory = TestDirectory::new();
        let tool = CommandTool::new(&directory.0, "TEST_KEY")
            .unwrap_or_else(|error| panic!("command tool fixture: {error}"));
        let canonical_root = std::fs::canonicalize(&directory.0)
            .unwrap_or_else(|error| panic!("canonical fixture root: {error}"));
        assert_eq!(tool.workspace_root(), canonical_root);

        let AdmissionOutcome::Admitted(admitted) = tool.admit(request(
            COMMAND_TOOL_NAME,
            r#"{ "cmd": "printf hello" }"#.to_owned(),
        )) else {
            panic!("valid command must be admitted");
        };
        assert_eq!(
            admitted.capabilities().iter().collect::<Vec<_>>(),
            CAPABILITIES
        );
        assert_eq!(
            admitted.canonical_arguments(),
            format!(
                r#"{{"cmd":"printf hello","timeout_ms":{DEFAULT_TIMEOUT_MS},"workspace_root":{}}}"#,
                serde_json::to_string(
                    canonical_root
                        .to_str()
                        .unwrap_or_else(|| panic!("UTF-8 root"))
                )
                .unwrap_or_else(|error| panic!("serialize fixture root: {error}"))
            )
        );
        assert!(admitted.detail().len() <= plexmaton_agent::MAX_APPROVAL_DETAIL_BYTES);
        assert!(matches!(
            admitted.invocation(),
            Some(ToolDetail::Text { source, omitted_bytes: 0 })
                if source.starts_with("Command \"printf hello\"\ncwd: ")
                    && source.ends_with(&format!("\ntimeout_ms: {DEFAULT_TIMEOUT_MS}"))
                    && source.contains(
                        &serde_json::to_string(
                            canonical_root
                                .to_str()
                                .unwrap_or_else(|| panic!("UTF-8 root"))
                        )
                        .unwrap_or_else(|error| panic!("serialize fixture root: {error}"))
                    )
        ));
        assert_eq!(
            admitted.capabilities().iter().collect::<Vec<_>>(),
            vec![
                ToolCapability::FileRead,
                ToolCapability::FileWrite,
                ToolCapability::ProcessSpawn,
            ]
        );

        let long_command = format!("COMMAND-HEAD-{}-COMMAND-TAIL", "x".repeat(2_048));
        let AdmissionOutcome::Admitted(long) = tool.admit(request(
            COMMAND_TOOL_NAME,
            serde_json::json!({ "cmd": long_command }).to_string(),
        )) else {
            panic!("bounded long command must be admitted");
        };
        assert!(long.detail().len() <= plexmaton_agent::MAX_APPROVAL_DETAIL_BYTES);
        assert!(long.detail().contains("COMMAND-HEAD-"));
        assert!(long.detail().contains("-COMMAND-TAIL"));
        assert!(long.detail().contains("bytes omitted"));

        let AdmissionOutcome::Admitted(nullable_default) = tool.admit(request(
            COMMAND_TOOL_NAME,
            r#"{"cmd":"true","timeout_ms":null}"#.to_owned(),
        )) else {
            panic!("null timeout must select the canonical default");
        };
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(nullable_default.canonical_arguments())
                .unwrap_or_else(|error| panic!("canonical command arguments: {error}"))["timeout_ms"],
            DEFAULT_TIMEOUT_MS
        );

        assert!(matches!(
            tool.admit(request(
                COMMAND_TOOL_NAME,
                serde_json::json!({ "cmd": "x".repeat(MAX_COMMAND_CHARACTERS) }).to_string(),
            )),
            AdmissionOutcome::Admitted(_)
        ));
    }

    #[test]
    fn cmd_1_refuses_every_shape_outside_the_model_contract_and_hard_bounds() {
        let directory = TestDirectory::new();
        let tool = CommandTool::new(&directory.0, "TEST_KEY")
            .unwrap_or_else(|error| panic!("command tool fixture: {error}"));
        let cases = [
            (
                "other",
                r#"{"cmd":"true"}"#.to_owned(),
                AdmissionRefusal::UnknownTool,
            ),
            (
                COMMAND_TOOL_NAME,
                "{}".to_owned(),
                AdmissionRefusal::InvalidArguments,
            ),
            (
                COMMAND_TOOL_NAME,
                r#"{"cmd":"true","extra":1}"#.to_owned(),
                AdmissionRefusal::InvalidArguments,
            ),
            (
                COMMAND_TOOL_NAME,
                r#"{"cmd":"true","timeout_ms":0}"#.to_owned(),
                AdmissionRefusal::InvalidArguments,
            ),
            (
                COMMAND_TOOL_NAME,
                format!(r#"{{"cmd":"true","timeout_ms":{}}}"#, MAX_TIMEOUT_MS + 1),
                AdmissionRefusal::InvalidArguments,
            ),
            (
                COMMAND_TOOL_NAME,
                r#"{"cmd":"   "}"#.to_owned(),
                AdmissionRefusal::InvalidArguments,
            ),
            (
                COMMAND_TOOL_NAME,
                serde_json::json!({ "cmd": "x".repeat(MAX_COMMAND_CHARACTERS + 1) }).to_string(),
                AdmissionRefusal::InvalidArguments,
            ),
            (
                COMMAND_TOOL_NAME,
                serde_json::json!({ "cmd": "contains\0nul" }).to_string(),
                AdmissionRefusal::InvalidArguments,
            ),
        ];
        for (name, arguments, expected) in cases {
            assert_eq!(
                tool.admit(request(name, arguments)),
                AdmissionOutcome::Refused {
                    call_id: ToolCallId::new("command-1")
                        .unwrap_or_else(|error| panic!("fixture call id: {error}")),
                    reason: expected,
                }
            );
        }
    }

    #[test]
    fn cmd_1_model_schema_is_strict_nullable_and_exposes_timeout_bounds() {
        let schema = command_parameters_schema();
        assert_eq!(schema["required"], serde_json::json!(["cmd", "timeout_ms"]));
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["properties"]["cmd"]["type"], "string");
        assert_eq!(
            schema["properties"]["cmd"]["maxLength"],
            MAX_COMMAND_CHARACTERS
        );
        assert_eq!(
            schema["properties"]["timeout_ms"]["type"],
            serde_json::json!(["integer", "null"])
        );
        assert_eq!(schema["properties"]["timeout_ms"]["minimum"], 1);
        assert_eq!(
            schema["properties"]["timeout_ms"]["maximum"],
            MAX_TIMEOUT_MS
        );
    }

    #[test]
    fn cmd_1_multibyte_and_escaped_commands_fit_every_advertised_bound() {
        let directory = TestDirectory::new();
        let tool = CommandTool::new(&directory.0, "TEST_KEY")
            .unwrap_or_else(|error| panic!("command tool fixture: {error}"));
        let multibyte = "🦀".repeat(MAX_COMMAND_CHARACTERS);
        assert_eq!(multibyte.len(), MAX_COMMAND_BYTES);
        let AdmissionOutcome::Admitted(multibyte) = tool.admit(request(
            COMMAND_TOOL_NAME,
            serde_json::json!({ "cmd": multibyte, "timeout_ms": null }).to_string(),
        )) else {
            panic!("advertised multibyte edge must be admitted");
        };
        assert!(
            multibyte.canonical_arguments().len() <= plexmaton_agent::MAX_ADMITTED_ARGUMENT_BYTES
        );

        let escaped_command = format!("x{}", "\u{1}".repeat(MAX_COMMAND_CHARACTERS - 1));
        let escaped_arguments =
            serde_json::json!({ "cmd": escaped_command, "timeout_ms": null }).to_string();
        assert!(escaped_arguments.len() <= plexmaton_agent::MAX_REQUESTED_TOOL_ARGUMENT_BYTES);
        let AdmissionOutcome::Admitted(escaped) =
            tool.admit(request(COMMAND_TOOL_NAME, escaped_arguments))
        else {
            panic!("advertised escaping edge must be admitted");
        };
        assert!(
            escaped.canonical_arguments().len() <= plexmaton_agent::MAX_ADMITTED_ARGUMENT_BYTES
        );

        assert!(matches!(
            tool.admit(request(
                COMMAND_TOOL_NAME,
                serde_json::json!({ "cmd": "🦀".repeat(MAX_COMMAND_CHARACTERS + 1) }).to_string(),
            )),
            AdmissionOutcome::Refused {
                reason: AdmissionRefusal::InvalidArguments,
                ..
            }
        ));
    }

    #[test]
    fn cmd_1_approval_detail_leads_with_command_and_bounds_root_separately() {
        let arguments = CanonicalArguments {
            cmd: format!("COMMAND-HEAD-{}-COMMAND-TAIL", "c".repeat(2_048)),
            timeout_ms: DEFAULT_TIMEOUT_MS,
            workspace_root: format!("/ROOT-HEAD/{}/ROOT-TAIL", "r".repeat(1_024)),
        };
        let detail = approval_detail(&arguments);
        assert!(detail.len() <= plexmaton_agent::MAX_APPROVAL_DETAIL_BYTES);
        assert!(detail.starts_with("Command \"COMMAND-HEAD-"));
        assert!(detail.contains("ROOT-HEAD"));
        assert!(detail.contains("ROOT-TAIL"));
        assert!(detail.contains("COMMAND-HEAD"));
        assert!(detail.contains("COMMAND-TAIL"));
        assert_eq!(detail.matches("bytes omitted").count(), 2);
    }

    #[tokio::test]
    async fn cmd_1_changed_workspace_identity_is_refused_before_spawn() {
        let directory = TestDirectory::new();
        let tool = CommandTool::new(&directory.0, "TEST_KEY")
            .unwrap_or_else(|error| panic!("command tool fixture: {error}"));
        let AdmissionOutcome::Admitted(call) = tool.admit(request(
            COMMAND_TOOL_NAME,
            r#"{"cmd":"printf ran > must-not-exist"}"#.to_owned(),
        )) else {
            panic!("fixture command must be admitted");
        };
        let moved = directory.0.with_extension("original");
        std::fs::rename(&directory.0, &moved)
            .unwrap_or_else(|error| panic!("move original workspace: {error}"));
        std::fs::create_dir(&directory.0)
            .unwrap_or_else(|error| panic!("create replacement workspace: {error}"));

        let result = tool.execute(&call, CancellationToken::new()).await;
        let replacement_was_touched = directory.0.join("must-not-exist").exists();
        std::fs::remove_dir(&directory.0)
            .unwrap_or_else(|error| panic!("remove replacement workspace: {error}"));
        std::fs::rename(&moved, &directory.0)
            .unwrap_or_else(|error| panic!("restore original workspace: {error}"));

        assert!(matches!(
            result,
            Err(super::CommandExecutionError::WorkspaceChanged)
        ));
        assert!(!replacement_was_touched);
    }
}
