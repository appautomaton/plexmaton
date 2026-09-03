//! Opt-in Tier 5 smoke against the developer's selected local OpenAI-compatible proxy.

use std::{fs, time::Duration};

use plexmaton_agent::Input;
use plexmaton_command::COMMAND_TOOL_NAME;
use plexmaton_core::{AgentId, SessionEvent, TokenUsage};
use plexmaton_file_tools::{EDIT_TOOL_NAME, READ_TOOL_NAME};
use plexmaton_runtime::{LiveRuntime, NativeToolCatalog};

#[path = "live_local/support.rs"]
mod support;

use support::{
    COMMAND_MARKER, EXACT_COMMAND, FIXTURE_FILE, IsolatedWorkspace, LIVE_TURN_TIMEOUT,
    LiveToolEvidence, MODEL_MARKER, SHUTDOWN_TIMEOUT, assert_empty_report, drive_live_tool_turn,
    live_profile_and_key, observe_output,
};

/// LIVE-1, LIVE-4 and LIVE-6 at the real transport boundary; never part of the default gate.
#[tokio::test]
#[ignore = "requires PLEXMATON_HOME and its selected local proxy credential"]
async fn one_local_request_streams_text_and_reported_usage() {
    let (profile, key) = live_profile_and_key();
    let agent_id =
        AgentId::new("live-smoke").unwrap_or_else(|error| panic!("test agent id: {error}"));
    let workspace = std::env::current_dir()
        .unwrap_or_else(|error| panic!("resolve live test workspace: {error}"));
    let tools = NativeToolCatalog::open(
        &workspace,
        profile.api_key_env(),
        "/bin/false",
        "/bin/false",
        Vec::new(),
    )
    .unwrap_or_else(|error| panic!("build live test catalog: {error}"));
    let mut runtime = LiveRuntime::openai(agent_id.clone(), "Live smoke", profile, key, tools)
        .unwrap_or_else(|error| panic!("build runtime: {error}"));
    let _announced = runtime.try_next_event();

    runtime
        .submit(
            agent_id,
            Input::Submitted {
                text: "Reply with one short sentence naming the color of a clear daytime sky."
                    .to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("submit live request: {error}"));

    let mut evidence = LiveToolEvidence::default();
    while runtime.has_active_work() {
        let event = tokio::time::timeout(Duration::from_secs(120), runtime.next_event())
            .await
            .unwrap_or_else(|_| panic!("local provider timed out"))
            .unwrap_or_else(|error| panic!("receive runtime event: {error}"));
        if let Some(envelope) = event {
            let _unhandled = observe_output(&mut evidence, envelope.event);
        }
    }
    while let Some(envelope) = runtime.try_next_event() {
        let _unhandled = observe_output(&mut evidence, envelope.event);
    }
    runtime
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown live runtime: {error}"));

    assert!(
        !evidence.assistant_text.trim().is_empty(),
        "the live model streamed no text; warnings={:?}, usage={:?}",
        evidence.warnings,
        evidence.usage
    );
    assert!(
        matches!(
            evidence.usage,
            Some(TokenUsage::Complete(_) | TokenUsage::Partial(_))
        ),
        "the live endpoint omitted usable token counts: {:?}",
        evidence.usage
    );
    assert!(
        evidence.warnings.is_empty(),
        "the live turn warned instead of completing"
    );
}

/// LIVE-1, LIVE-3, LIVE-4, APV-3 and APV-4 at the real tool transport boundary.
#[tokio::test]
#[ignore = "requires PLEXMATON_HOME and its selected local proxy credential"]
async fn real_model_completes_read_observed_edit_and_command_with_exact_approvals() {
    let workspace = IsolatedWorkspace::new();
    fs::write(workspace.0.join(FIXTURE_FILE), "before\n")
        .unwrap_or_else(|error| panic!("write live tool fixture: {error}"));
    let (profile, key) = live_profile_and_key();
    let agent_id =
        AgentId::new("live-native-tools").unwrap_or_else(|error| panic!("test agent id: {error}"));
    let tools = NativeToolCatalog::open(
        &workspace.0,
        profile.api_key_env(),
        "/bin/false",
        "/bin/false",
        Vec::new(),
    )
    .unwrap_or_else(|error| panic!("build live native catalog: {error}"));
    let mut runtime =
        LiveRuntime::openai(agent_id.clone(), "Live native tools", profile, key, tools)
            .unwrap_or_else(|error| panic!("build live runtime: {error}"));
    let _announced = runtime.try_next_event();
    let prompt = format!(
        "Complete this deterministic integration task with exactly one tool call per step. \
         First call read_file for {FIXTURE_FILE} with null offset and limit. Use its returned \
         observation in edit_file to replace the exact text `before` with `after`. After that edit \
         succeeds, call exec_command with cmd exactly `{EXACT_COMMAND}` and timeout_ms 30000. Do \
         not call search, create_file, or any other command. Only after the command exits zero and \
         its stdout contains {COMMAND_MARKER}, finish with exactly `{MODEL_MARKER} \
         {COMMAND_MARKER}`. Do not merely describe the operations."
    );
    let report = runtime
        .submit(agent_id.clone(), Input::Submitted { text: prompt })
        .await
        .unwrap_or_else(|error| panic!("submit live tool task: {error}"));
    assert_empty_report(&report, "initial submission").unwrap_or_else(|error| panic!("{error}"));

    let mut evidence = LiveToolEvidence::default();
    let turn = tokio::time::timeout(
        LIVE_TURN_TIMEOUT,
        drive_live_tool_turn(&mut runtime, &agent_id, &workspace.0, &mut evidence),
    )
    .await;
    let shutdown = tokio::time::timeout(SHUTDOWN_TIMEOUT, runtime.shutdown()).await;
    while let Some(envelope) = runtime.try_next_event() {
        if let SessionEvent::RuntimeWarning { message, .. } = envelope.event {
            evidence.warnings.push(message);
        }
    }

    let shutdown_report = shutdown
        .unwrap_or_else(|_| panic!("live runtime shutdown timed out"))
        .unwrap_or_else(|error| panic!("shutdown live runtime: {error}"));
    assert_empty_report(&shutdown_report, "shutdown").unwrap_or_else(|error| panic!("{error}"));
    turn.unwrap_or_else(|_| panic!("live native tool turn exceeded {LIVE_TURN_TIMEOUT:?}"))
        .unwrap_or_else(|error| panic!("live native tool turn failed: {error}"));

    assert_eq!(
        fs::read_to_string(workspace.0.join(FIXTURE_FILE))
            .unwrap_or_else(|error| panic!("read edited live fixture: {error}")),
        "after\n"
    );
    assert_eq!(
        evidence.approvals,
        [EDIT_TOOL_NAME, COMMAND_TOOL_NAME],
        "the exact protected calls were not each approved once"
    );
    assert_eq!(evidence.approvals_resolved, 2);
    assert!(
        evidence.failed_tools.is_empty(),
        "native tool calls did not all succeed: {:?}",
        evidence.failed_tools
    );
    assert_eq!(
        evidence.succeeded_tools,
        [READ_TOOL_NAME, EDIT_TOOL_NAME, COMMAND_TOOL_NAME],
        "native tools did not succeed exactly once in order"
    );
    assert!(
        evidence
            .assistant_text
            .contains(&format!("{MODEL_MARKER} {COMMAND_MARKER}")),
        "the model did not confirm the command result: {:?}",
        evidence.assistant_text
    );
    assert!(
        evidence
            .usage
            .as_ref()
            .and_then(TokenUsage::counts)
            .is_some_and(|counts| counts.total > 0),
        "the live tool turn reported no usable token count: {:?}",
        evidence.usage
    );
    assert_eq!(
        evidence.warnings,
        Vec::<String>::new(),
        "the live tool turn emitted runtime warnings"
    );
}
