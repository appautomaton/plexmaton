//! PRV-3/JRN-3: real codec output crosses the on-disk journal boundary before replay.

use std::path::PathBuf;

use plexmaton_agent::{Agent, Effect, Input, ModelEvent, UnixMillis};
use plexmaton_core::{AgentId, HeadName};
use plexmaton_provider::{DecodeLimits, ModelRegistry, ProviderCodec, encode_request};
use plexmaton_session_store::JournalFile;
use serde_json::json;

#[path = "provider_replay/interrupted.rs"]
mod interrupted;

struct Scratch(PathBuf);
impl Scratch {
    fn new(label: &str) -> Self {
        for ordinal in 0..64 {
            let path = std::env::temp_dir().join(format!(
                "plexmaton-replay-{label}-{}-{ordinal}",
                std::process::id()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("isolated directory: {error}"),
            }
        }
        panic!("could not reserve an isolated replay-test directory")
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn deliver(agent: &mut Agent, event: ModelEvent) -> plexmaton_agent::Reaction {
    if matches!(event, ModelEvent::Usage(_)) {
        return plexmaton_agent::Reaction::default();
    }
    let step_id = agent
        .active_model_step()
        .unwrap_or_else(|| panic!("active step"));
    let reaction = agent.handle_at(Input::Streamed { step_id, event }, UnixMillis::EPOCH);
    assert!(reaction.undelivered_model.is_empty());
    reaction
}

/// PRV-3/JRN-3/JRN-5: phase loss or part merging after resume changes the actual next request.
#[test]
fn prv_3_responses_phase_and_parts_survive_jsonl_reopen() {
    let model = ModelRegistry::parse(
        r#"
active_model = { provider = "fixture", model = "model" }
[providers.fixture]
base_url = "http://127.0.0.1:1/v1"
api_key_env = "FIXTURE_KEY"
api = "openai_responses"
[providers.fixture.models.model]
id = "fixture-model"
reasoning_effort = "high"
context_window_tokens = 8192
max_output_tokens = 1024
output_reserve_tokens = 512
"#,
    )
    .unwrap_or_else(|error| panic!("model: {error}"));
    let model = model.active_model();
    let mut agent = Agent::new(AgentId::new("agent").unwrap_or_else(|error| panic!("id: {error}")));
    let _ = agent.handle_at(
        Input::Submitted {
            text: "Inspect the project.".to_owned(),
        },
        UnixMillis::EPOCH,
    );
    let items = [
        json!({"type":"message","id":"msg_commentary","role":"assistant","phase":"commentary","status":"completed",
            "content":[{"type":"output_text","text":"Inspecting.","annotations":[]}, {"type":"output_text","text":"One moment.","annotations":[]}]}),
        json!({"type":"reasoning","id":"rs_private","summary":[],"encrypted_content":"private-replay-sentinel"}),
        json!({"type":"message","id":"msg_final","role":"assistant","phase":"final_answer","status":"completed",
            "content":[{"type":"output_text","text":"Done.","annotations":[]}]}),
    ];
    let mut codec = ProviderCodec::new(
        &plexmaton_agent::RequestAttemptId::new("fixture-attempt")
            .unwrap_or_else(|error| panic!("attempt: {error}")),
        model,
        DecodeLimits::production(),
    );
    for (index, item) in items.iter().enumerate() {
        let event = json!({"type":"response.output_item.done","output_index":index,"item":item});
        for event in codec
            .push_sse("response.output_item.done", &event.to_string())
            .unwrap_or_else(|error| panic!("decode: {error}"))
        {
            deliver(&mut agent, event);
        }
    }
    let terminal =
        r#"{"type":"response.completed","response":{"status":"completed","usage":null}}"#;
    for event in codec
        .push_sse("response.completed", terminal)
        .unwrap_or_else(|error| panic!("terminal: {error}"))
    {
        deliver(&mut agent, event);
    }
    codec
        .finish()
        .unwrap_or_else(|error| panic!("finality: {error}"));
    let head = HeadName::new("main").unwrap_or_else(|error| panic!("head: {error}"));
    let before = agent
        .journal()
        .project(&head)
        .unwrap_or_else(|error| panic!("project: {error:?}"));
    let encoded = encode_request(model, before.request(), &[], None)
        .unwrap_or_else(|error| panic!("encode: {error}"));
    assert_eq!(
        encoded["input"].as_array().map(|input| &input[1..]),
        Some(items.as_slice())
    );
    assert!(!format!("{:?}", before.events()).contains("private-replay-sentinel"));
    assert!(!format!("{:?}", before.request()).contains("private-replay-sentinel"));

    let scratch = Scratch::new("responses");
    let path = scratch.0.join("session.jsonl");
    let mut file = JournalFile::create(
        &path,
        agent.journal().conversation_id().clone(),
        UnixMillis::EPOCH,
    )
    .unwrap_or_else(|error| panic!("create journal: {error}"));
    for record in agent.journal().records() {
        file.append(record.clone())
            .unwrap_or_else(|error| panic!("append: {error:?}"));
    }
    drop(file);
    let reopened = JournalFile::open(&path).unwrap_or_else(|error| panic!("reopen: {error}"));
    let after = reopened
        .journal()
        .project(&head)
        .unwrap_or_else(|error| panic!("project reopened: {error:?}"));
    assert_eq!(
        encode_request(model, after.request(), &[], None)
            .unwrap_or_else(|error| panic!("reencode: {error}")),
        encoded
    );
    assert_eq!(after.events(), before.events());
    let mut resumed = Agent::from_journal(
        AgentId::new("agent").unwrap_or_else(|error| panic!("agent: {error}")),
        reopened.journal().clone(),
        plexmaton_agent::TurnBudget::default(),
        plexmaton_agent::ApprovalPolicy::default(),
    )
    .unwrap_or_else(|error| panic!("resume: {error:?}"));
    let reaction = resumed.handle_at(
        Input::Submitted {
            text: "Continue.".to_owned(),
        },
        UnixMillis::EPOCH,
    );
    let [Effect::CallModel(call)] = reaction.effects.as_slice() else {
        panic!("next model request");
    };
    let next = encode_request(model, &call.request, &[], None)
        .unwrap_or_else(|error| panic!("next encode: {error}"));
    assert_eq!(next["prompt_cache_key"], encoded["prompt_cache_key"]);
    assert_eq!(next["prompt_cache_key"].as_str().map(str::len), Some(64));
    let prefix = encoded["input"]
        .as_array()
        .unwrap_or_else(|| panic!("input"));
    assert_eq!(
        &next["input"]
            .as_array()
            .unwrap_or_else(|| panic!("next input"))[..prefix.len()],
        prefix
    );
    let mut other = call.request.clone();
    other.session_id = plexmaton_core::ConversationId::new("another-session")
        .unwrap_or_else(|error| panic!("session: {error}"));
    assert_ne!(
        encode_request(model, &other, &[], None)
            .unwrap_or_else(|error| panic!("other encode: {error}"))["prompt_cache_key"],
        next["prompt_cache_key"]
    );
}

/// PRV-3: aborting a completed but undispatched call removes its attached replay too.
#[test]
fn prv_3_cancelled_calls_leave_no_dangling_replay() {
    use plexmaton_agent::{
        ModelOutputPosition, ProviderCodecId, ProviderCodecRevision, ProviderModelFamilyId,
        ProviderReplay, ProviderReplayOwnerId, ReplayCompatibility, ToolCall,
    };
    use plexmaton_core::ToolCallId;
    let mut agent = Agent::new(AgentId::new("agent").unwrap_or_else(|error| panic!("id: {error}")));
    let _ = agent.handle_at(
        Input::Submitted {
            text: "Use a tool.".to_owned(),
        },
        UnixMillis::EPOCH,
    );
    let compatibility = ReplayCompatibility::new(
        ProviderReplayOwnerId::new("fixture").unwrap_or_else(|error| panic!("owner: {error:?}")),
        ProviderCodecId::new("fixture").unwrap_or_else(|error| panic!("codec: {error:?}")),
        ProviderCodecRevision::new(1).unwrap_or_else(|_| panic!("revision")),
        ProviderModelFamilyId::new("fixture").unwrap_or_else(|error| panic!("model: {error:?}")),
    );
    let position = ModelOutputPosition::new(0, 0);
    deliver(
        &mut agent,
        ModelEvent::Called {
            position,
            call: ToolCall {
                call_id: ToolCallId::new("call").unwrap_or_else(|error| panic!("call: {error}")),
                name: "read_file".to_owned(),
                arguments: "{}".to_owned(),
            },
        },
    );
    deliver(
        &mut agent,
        ModelEvent::Replay {
            position,
            replay: ProviderReplay::new(compatibility, "signature".to_owned())
                .unwrap_or_else(|error| panic!("replay: {error:?}")),
        },
    );
    let reaction = agent.handle_at(Input::Interrupted, UnixMillis::EPOCH);
    assert!(!reaction.effects.iter().any(|effect| matches!(
        effect,
        Effect::AdmitTool(_) | Effect::RunTool { .. } | Effect::PreparePermission(_)
    )));
    let head = HeadName::new("main").unwrap_or_else(|error| panic!("head: {error}"));
    let projection = agent
        .journal()
        .project(&head)
        .unwrap_or_else(|error| panic!("project: {error:?}"));
    assert_eq!(projection.request().atoms.len(), 1);
    assert!(projection.recovery().is_none());
    assert!(!agent.is_running());
}

/// PRV-1/PRV-3/JRN-3: native tool conversations replay identically from actual JSONL bytes.
#[tokio::test]
async fn prv_3_native_tool_conversations_survive_jsonl_reopen() {
    use plexmaton_agent::{
        RequestAttemptId, ToolDefinitionRevision, ToolExecutionResult, ToolOutcome,
    };
    use plexmaton_core::{ToolCapability, ToolDefinitionId};
    use plexmaton_provider::{ModelApi, drive_sse};
    const M_TOOL: &str =
        include_str!("../../plexmaton-provider/tests/fixtures/messages_tool_call.sse");
    const M_ANSWER: &str =
        include_str!("../../plexmaton-provider/tests/fixtures/messages_final_answer.sse");
    const G_TOOL: &str =
        include_str!("../../plexmaton-provider/tests/fixtures/gemini_tool_call.sse");
    const G_ANSWER: &str =
        include_str!("../../plexmaton-provider/tests/fixtures/gemini_final_answer.sse");
    const R_TOOL: &str =
        include_str!("../../plexmaton-provider/tests/fixtures/responses_tool_call.sse");
    const R_ANSWER: &str =
        include_str!("../../plexmaton-provider/tests/fixtures/responses_final_answer.sse");
    for (api, tool, answer, label) in [
        (
            ModelApi::OpenaiResponses,
            R_TOOL,
            R_ANSWER,
            "responses-tools",
        ),
        (ModelApi::AnthropicMessages, M_TOOL, M_ANSWER, "messages"),
        (ModelApi::GoogleGenerateContent, G_TOOL, G_ANSWER, "gemini"),
    ] {
        let registry = ModelRegistry::parse(&format!(
            r#"
active_model = {{ provider = "fixture", model = "model" }}
[providers.fixture]
base_url = "http://127.0.0.1:1/v1"
api_key_env = "FIXTURE_KEY"
api = {}
[providers.fixture.models.model]
id = "fixture-model"
context_window_tokens = 8192
max_output_tokens = 1024
output_reserve_tokens = 512
"#,
            serde_json::to_string(&api).expect("API spelling")
        ))
        .expect("model");
        let model = registry.active_model();
        let mut agent = Agent::new(AgentId::new("agent").expect("agent"));
        let _ = agent.handle_at(
            Input::Submitted {
                text: "Read the file.".to_owned(),
            },
            UnixMillis::EPOCH,
        );
        for (ordinal, fixture) in [tool, answer].into_iter().enumerate() {
            let scope = RequestAttemptId::new(format!("native-attempt-{ordinal}")).expect("scope");
            let chunks = futures_util::stream::iter(
                fixture
                    .as_bytes()
                    .chunks(7)
                    .map(|chunk| Ok::<_, std::convert::Infallible>(chunk.to_vec())),
            );
            let mut events = Vec::new();
            drive_sse(&scope, model, chunks, DecodeLimits::production(), |event| {
                events.push(event);
                std::future::ready(())
            })
            .await
            .expect("fixture stream");
            let mut effects = Vec::new();
            for event in events {
                effects.extend(deliver(&mut agent, event).effects);
            }
            if ordinal == 0 {
                assert_eq!(effects.len(), 1);
                let Effect::AdmitTool(admission) = effects.pop().expect("admission") else {
                    panic!("tool admission");
                };
                let call = admission.requested().clone();
                let admitted = admission
                    .admit(
                        ToolDefinitionId::new("fixture-read").expect("definition"),
                        ToolDefinitionRevision::new(1).expect("revision"),
                        [ToolCapability::FileRead],
                        call.arguments.clone(),
                        "Read fixture".to_owned(),
                        None,
                    )
                    .expect("admit read");
                let reaction =
                    agent.handle_at(Input::ToolAdmissionResolved(admitted), UnixMillis::EPOCH);
                assert!(matches!(
                    reaction.effects.as_slice(),
                    [Effect::RunTool { .. }]
                ));
                let reaction = agent.handle_at(
                    Input::ToolFinished {
                        call_id: call.call_id,
                        result: ToolExecutionResult::new(
                            ToolOutcome::Succeeded {
                                output: "Plexmaton".to_owned(),
                            },
                            None,
                        ),
                    },
                    UnixMillis::EPOCH,
                );
                assert!(matches!(
                    reaction.effects.as_slice(),
                    [Effect::CallModel(_)]
                ));
            } else {
                assert!(effects.is_empty());
            }
        }
        assert!(!agent.is_running());
        let head = HeadName::new("main").expect("head");
        let before = agent.journal().project(&head).expect("project");
        let encoded = encode_request(model, before.request(), &[], None).expect("encode");
        let scratch = Scratch::new(label);
        let path = scratch.0.join("session.jsonl");
        let mut file = JournalFile::create(
            &path,
            agent.journal().conversation_id().clone(),
            UnixMillis::EPOCH,
        )
        .expect("create journal");
        for record in agent.journal().records() {
            file.append(record.clone()).expect("append journal");
        }
        drop(file);
        let reopened = JournalFile::open(&path).expect("reopen journal");
        let after = reopened
            .journal()
            .project(&head)
            .expect("reopened projection");
        assert_eq!(
            encode_request(model, after.request(), &[], None).expect("reencode"),
            encoded
        );
        assert_eq!(after.events(), before.events());
        let debug = format!("{:?}", after.request());
        for secret in [
            "signed-fixture",
            "redacted-fixture",
            "call-fixture",
            "answer-fixture",
        ] {
            assert!(!debug.contains(secret));
        }
    }
}
