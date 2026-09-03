use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use plexmaton_agent::Input;
use plexmaton_command::COMMAND_TOOL_NAME;
use plexmaton_core::{
    AgentId, AgentStatus, ApprovalDecision, AttentionRequest, SessionEvent, TokenUsage,
    ToolCallStatus, ToolCapability, TranscriptRole,
};
use plexmaton_file_tools::{EDIT_TOOL_NAME, READ_TOOL_NAME};
use plexmaton_provider::{ApiKey, ProviderConfig, ProviderProfile, resolve_api_key, resolve_home};
use plexmaton_runtime::{DispatchReport, LiveRuntime};

pub(super) const LIVE_TURN_TIMEOUT: Duration = Duration::from_secs(300);
pub(super) const LIVE_EVENT_TIMEOUT: Duration = Duration::from_secs(120);
pub(super) const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);
pub(super) const FIXTURE_FILE: &str = "task.txt";
pub(super) const COMMAND_MARKER: &str = "SLICE10_COMMAND_OK";
pub(super) const MODEL_MARKER: &str = "SLICE10_MODEL_DONE";
pub(super) const EXACT_COMMAND: &str =
    "grep -qx 'after' task.txt && printf 'SLICE10_COMMAND_OK\\n'";

static NEXT_WORKSPACE: AtomicU64 = AtomicU64::new(1);

pub(super) struct IsolatedWorkspace(pub(super) PathBuf);

impl IsolatedWorkspace {
    pub(super) fn new() -> Self {
        loop {
            let serial = NEXT_WORKSPACE.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("plex-live-{}-{serial}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("create isolated live workspace {path:?}: {error}"),
            }
        }
    }
}

impl Drop for IsolatedWorkspace {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0)
            .unwrap_or_else(|error| panic!("remove isolated live workspace {:?}: {error}", self.0));
    }
}

#[derive(Default)]
pub(super) struct LiveToolEvidence {
    assistant_items: BTreeSet<String>,
    pub(super) assistant_text: String,
    pub(super) usage: Option<TokenUsage>,
    pub(super) warnings: Vec<String>,
    pub(super) succeeded_tools: Vec<String>,
    pub(super) failed_tools: Vec<(String, ToolCallStatus)>,
    pub(super) approvals: Vec<String>,
    pub(super) approvals_resolved: usize,
    turn_finished: bool,
}

pub(super) fn live_profile_and_key() -> (ProviderProfile, ApiKey) {
    let configured_home = std::env::var_os("PLEXMATON_HOME");
    let user_home = std::env::var_os("HOME").map(PathBuf::from);
    let root = resolve_home(configured_home.as_deref(), user_home.as_deref())
        .unwrap_or_else(|error| panic!("resolve test config root: {error}"));
    let source = fs::read_to_string(root.join("config.toml"))
        .unwrap_or_else(|error| panic!("read test config: {error}"));
    let config =
        ProviderConfig::parse(&source).unwrap_or_else(|error| panic!("parse test config: {error}"));
    let profile = config.active().clone();
    let key = resolve_api_key(&profile, std::env::var_os(profile.api_key_env()))
        .unwrap_or_else(|error| panic!("resolve test key: {error}"));
    (profile, key)
}

pub(super) async fn drive_live_tool_turn(
    runtime: &mut LiveRuntime,
    agent_id: &AgentId,
    workspace: &Path,
    evidence: &mut LiveToolEvidence,
) -> Result<(), String> {
    loop {
        let envelope = match runtime.try_next_event() {
            Some(envelope) => envelope,
            None if evidence.turn_finished && !runtime.has_active_work() => return Ok(()),
            None => tokio::time::timeout(LIVE_EVENT_TIMEOUT, runtime.next_event())
                .await
                .map_err(|_| format!("no runtime event arrived within {LIVE_EVENT_TIMEOUT:?}"))?
                .map_err(|error| format!("receive runtime event: {error}"))?
                .ok_or_else(|| "runtime ended before the live turn completed".to_owned())?,
        };
        observe_live_tool_event(runtime, agent_id, workspace, evidence, envelope.event).await?;
    }
}

async fn observe_live_tool_event(
    runtime: &mut LiveRuntime,
    agent_id: &AgentId,
    workspace: &Path,
    evidence: &mut LiveToolEvidence,
    event: SessionEvent,
) -> Result<(), String> {
    let Some(event) = observe_output(evidence, event) else {
        return Ok(());
    };
    match event {
        SessionEvent::ToolCallChanged { label, status, .. } => match status {
            ToolCallStatus::Succeeded => evidence.succeeded_tools.push(label),
            ToolCallStatus::Failed | ToolCallStatus::Denied | ToolCallStatus::Cancelled => {
                evidence.failed_tools.push((label, status));
            }
            ToolCallStatus::Queued | ToolCallStatus::AwaitingApproval | ToolCallStatus::Running => {
            }
        },
        SessionEvent::AttentionRequested {
            request:
                AttentionRequest::Approval {
                    approval_id,
                    tool,
                    capabilities,
                    detail,
                    ..
                },
            ..
        } => {
            validate_exact_approval(workspace, evidence, &tool, &capabilities, &detail)?;
            let report = runtime
                .submit(
                    agent_id.clone(),
                    Input::ApprovalDecided {
                        approval_id,
                        decision: ApprovalDecision::AllowOnce,
                    },
                )
                .await
                .map_err(|error| format!("approve {tool}: {error}"))?;
            assert_empty_report(&report, &format!("approval for {tool}"))?;
            evidence.approvals.push(tool);
        }
        SessionEvent::AttentionResolved { .. } => evidence.approvals_resolved += 1,
        SessionEvent::AgentStatusChanged {
            status: AgentStatus::Idle,
            ..
        } => evidence.turn_finished = true,
        SessionEvent::AgentStatusChanged {
            status: AgentStatus::Failed | AgentStatus::Cancelled,
            ..
        } => return Err("agent stopped before completing the live tool turn".to_owned()),
        _ => {}
    }
    Ok(())
}

pub(super) fn observe_output(
    evidence: &mut LiveToolEvidence,
    event: SessionEvent,
) -> Option<SessionEvent> {
    match event {
        SessionEvent::TranscriptItemStarted {
            item_id,
            role: TranscriptRole::Assistant,
            ..
        } => {
            evidence.assistant_items.insert(item_id.to_string());
        }
        SessionEvent::TranscriptDelta { item_id, text, .. }
            if evidence.assistant_items.contains(item_id.as_str()) =>
        {
            evidence.assistant_text.push_str(&text);
        }
        SessionEvent::TurnUsageUpdated { usage, .. } => evidence.usage = Some(usage),
        SessionEvent::RuntimeWarning { message, .. } => evidence.warnings.push(message),
        event => return Some(event),
    }
    None
}

fn validate_exact_approval(
    workspace: &Path,
    evidence: &LiveToolEvidence,
    tool: &str,
    capabilities: &[ToolCapability],
    detail: &str,
) -> Result<(), String> {
    let (prerequisite, expected_capabilities, expected_detail) = match tool {
        EDIT_TOOL_NAME => (
            READ_TOOL_NAME,
            vec![ToolCapability::FileRead, ToolCapability::FileWrite],
            "edit task.txt (1 exact replacement)".to_owned(),
        ),
        COMMAND_TOOL_NAME => {
            let canonical = fs::canonicalize(workspace)
                .map_err(|error| format!("canonicalize live workspace: {error}"))?;
            let root = canonical
                .to_str()
                .ok_or_else(|| "live workspace path is not UTF-8".to_owned())?;
            (
                EDIT_TOOL_NAME,
                vec![
                    ToolCapability::FileRead,
                    ToolCapability::FileWrite,
                    ToolCapability::ProcessSpawn,
                ],
                format!(
                    "Command {} · cwd {} · timeout 30000 ms",
                    serde_json::to_string(EXACT_COMMAND)
                        .map_err(|error| format!("quote expected command: {error}"))?,
                    serde_json::to_string(root)
                        .map_err(|error| format!("quote live workspace: {error}"))?
                ),
            )
        }
        _ => return Err(format!("refusing unexpected live approval for {tool}")),
    };
    if !evidence
        .succeeded_tools
        .iter()
        .any(|name| name == prerequisite)
    {
        return Err(format!(
            "{tool} approval arrived before successful {prerequisite}"
        ));
    }
    if capabilities != expected_capabilities || detail != expected_detail {
        return Err(format!(
            "unexpected {tool} approval: capabilities={capabilities:?}, detail={detail:?}"
        ));
    }
    Ok(())
}

pub(super) fn assert_empty_report(report: &DispatchReport, context: &str) -> Result<(), String> {
    if report == &DispatchReport::default() {
        Ok(())
    } else {
        Err(format!("{context} returned undelivered work: {report:?}"))
    }
}
