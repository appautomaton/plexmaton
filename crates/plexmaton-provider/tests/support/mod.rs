use std::convert::Infallible;

use futures_util::stream;
use plexmaton_agent::{
    Agent, Effect, Input, ModelEvent, ModelRequest, ToolCall, ToolDefinitionRevision, ToolOutcome,
};
use plexmaton_core::{AgentId, TokenUsage, ToolCapability, ToolDefinitionId};
use plexmaton_provider::{
    DecodeLimits, FunctionTool, ModelApi, ModelRegistry, ResolvedModel, drive_sse,
};
use serde_json::json;

pub async fn decode_fixture(
    profile: &ResolvedModel,
    fixture: &str,
    chunk_sizes: &[usize],
) -> Vec<ModelEvent> {
    let chunks = chunks(fixture.as_bytes(), chunk_sizes);
    let source = stream::iter(chunks.into_iter().map(Ok::<_, Infallible>));
    let mut events = Vec::new();
    drive_sse(
        &plexmaton_agent::RequestAttemptId::new("fixture-attempt")
            .unwrap_or_else(|error| panic!("attempt: {error}")),
        profile,
        source,
        DecodeLimits::production(),
        |event| {
            events.push(event);
            std::future::ready(())
        },
    )
    .await
    .unwrap_or_else(|error| panic!("fixture should decode: {error}"));
    events
}

pub fn open_agent(text: &str) -> (Agent, ModelRequest) {
    let mut agent = Agent::new(
        AgentId::new("fixture-agent").unwrap_or_else(|error| panic!("fixture agent id: {error}")),
    );
    let reaction = agent.handle_at(
        Input::Submitted {
            text: text.to_owned(),
        },
        plexmaton_agent::UnixMillis::EPOCH,
    );
    let [Effect::CallModel(request)] = reaction.effects.as_slice() else {
        panic!(
            "submission should open one model request: {:?}",
            reaction.effects
        );
    };
    (agent, request.request.clone())
}

/// Drives semantic output through admission and execution into the loop's next request.
/// Usage is asserted on decoded events; request accounting belongs to runtime terminal handling.
pub fn complete_tool_step(agent: &mut Agent, events: &[ModelEvent], output: &str) -> ModelRequest {
    let mut effects = Vec::new();
    for event in events
        .iter()
        .filter(|event| !matches!(event, ModelEvent::Usage(_)))
    {
        let step_id = agent
            .active_model_step()
            .unwrap_or_else(|| panic!("fixture expected an open provider step"));
        let reaction = agent.handle_at(
            Input::Streamed {
                step_id,
                event: event.clone(),
            },
            plexmaton_agent::UnixMillis::EPOCH,
        );
        assert!(reaction.undelivered_model.is_empty());
        effects.extend(reaction.effects);
    }
    let mut effects = effects.into_iter();
    let Some(Effect::AdmitTool(request)) = effects.next() else {
        panic!("tool step should request one admission");
    };
    assert!(effects.next().is_none());
    let call = request.requested().clone();
    let admitted = request
        .admit(
            ToolDefinitionId::new("read-file-v1")
                .unwrap_or_else(|error| panic!("fixture definition id: {error}")),
            ToolDefinitionRevision::new(1).unwrap_or_else(|| panic!("fixture revision")),
            [ToolCapability::FileRead],
            call.arguments.clone(),
            "read README.md".to_owned(),
            None,
        )
        .unwrap_or_else(|error| panic!("fixture admission: {error:?}"));
    let admitted_reaction = agent.handle_at(
        Input::ToolAdmissionResolved(admitted),
        plexmaton_agent::UnixMillis::EPOCH,
    );
    assert!(matches!(
        admitted_reaction.effects.as_slice(),
        [Effect::RunTool { call: running, .. }] if running.requested() == &call
    ));

    let finished = agent.handle_at(
        Input::ToolFinished {
            call_id: call.call_id,
            result: plexmaton_agent::ToolExecutionResult::new(
                ToolOutcome::Succeeded {
                    output: output.to_owned(),
                },
                None,
            ),
        },
        plexmaton_agent::UnixMillis::EPOCH,
    );
    let [Effect::CallModel(request)] = finished.effects.as_slice() else {
        panic!(
            "tool result should open the next model step: {:?}",
            finished.effects
        );
    };
    request.request.clone()
}

pub fn complete_answer(agent: &mut Agent, events: &[ModelEvent]) {
    for event in events
        .iter()
        .filter(|event| !matches!(event, ModelEvent::Usage(_)))
    {
        let step_id = agent
            .active_model_step()
            .unwrap_or_else(|| panic!("fixture expected an open provider step"));
        let reaction = agent.handle_at(
            Input::Streamed {
                step_id,
                event: event.clone(),
            },
            plexmaton_agent::UnixMillis::EPOCH,
        );
        assert!(reaction.undelivered_model.is_empty());
        assert!(
            reaction.effects.is_empty(),
            "a final answer should request no work: {:?}",
            reaction.effects
        );
    }
    assert!(!agent.is_running());
}

fn chunks(bytes: &[u8], sizes: &[usize]) -> Vec<Vec<u8>> {
    let mut chunks = Vec::new();
    let mut offset = 0;
    let mut index = 0;
    while offset < bytes.len() {
        let size = sizes[index % sizes.len()];
        let end = offset.saturating_add(size).min(bytes.len());
        chunks.push(bytes[offset..end].to_vec());
        offset = end;
        index += 1;
    }
    chunks
}

pub fn called(events: &[ModelEvent]) -> ToolCall {
    events
        .iter()
        .find_map(|event| match event {
            ModelEvent::Called { call, .. } => Some(call.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("fixture should contain one tool call"))
}

pub fn visible_text(events: &[ModelEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            ModelEvent::TextDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect()
}

pub fn reasoning_text(events: &[ModelEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            ModelEvent::ReasoningDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect()
}

pub fn reported_usage(events: &[ModelEvent]) -> &TokenUsage {
    events
        .iter()
        .find_map(|event| match event {
            ModelEvent::Usage(usage) => Some(usage),
            _ => None,
        })
        .unwrap_or_else(|| panic!("fixture should contain one usage report"))
}

pub fn read_tool() -> FunctionTool {
    FunctionTool::new(
        "read_file",
        "Read one workspace file",
        json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"],
            "additionalProperties": false,
        }),
    )
    .unwrap_or_else(|error| panic!("fixture tool should be valid: {error}"))
}

pub fn profile(api: ModelApi) -> ResolvedModel {
    let api = match api {
        ModelApi::OpenaiResponses => "openai_responses",
        ModelApi::OpenaiChatCompletions => "openai_chat_completions",
        ModelApi::AnthropicMessages => "anthropic_messages",
        ModelApi::GoogleGenerateContent => "google_generate_content",
    };
    let effort = if api == "openai_responses" {
        "xhigh"
    } else {
        "high"
    };
    let source = format!(
        r#"
active_model = {{ provider = "local", model = "luna" }}

[providers.local]
base_url = "http://127.0.0.1:8317/v1"
api_key_env = "PLEXMATON_LOCAL_API_KEY"

[providers.local.models.luna]
api = "{api}"
id = "gpt-5.6-luna"
reasoning_effort = "{effort}"
context_window_tokens = 272000
max_output_tokens = 128000
output_reserve_tokens = 16384
"#
    );
    ModelRegistry::parse(&source)
        .unwrap_or_else(|error| panic!("fixture profile should parse: {error}"))
        .active_model()
        .clone()
}

pub trait ResultTestExt<T, E> {
    fn unwrap_err_or_else(self) -> E;
}

impl<T, E> ResultTestExt<T, E> for Result<T, E>
where
    E: std::fmt::Debug,
{
    fn unwrap_err_or_else(self) -> E {
        match self {
            Ok(_) => panic!("fixture unexpectedly succeeded"),
            Err(error) => error,
        }
    }
}
