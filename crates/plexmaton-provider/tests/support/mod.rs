use std::convert::Infallible;

use futures_util::stream;
use plexmaton_agent::{
    AdmissionOutcome, AdmittedToolCall, Agent, Effect, Input, ModelEvent, ModelRequest, ToolCall,
    ToolDefinitionRevision, ToolOutcome,
};
use plexmaton_core::{AgentId, TokenUsage, ToolCapability, ToolDefinitionId};
use plexmaton_provider::{
    DecodeLimits, FunctionTool, Protocol, ProviderConfig, ProviderProfile, drive_sse,
};
use serde_json::json;

pub async fn decode_fixture(
    profile: &ProviderProfile,
    fixture: &str,
    chunk_sizes: &[usize],
) -> Vec<ModelEvent> {
    let chunks = chunks(fixture.as_bytes(), chunk_sizes);
    let source = stream::iter(chunks.into_iter().map(Ok::<_, Infallible>));
    let mut events = Vec::new();
    drive_sse(
        profile.protocol(),
        source,
        DecodeLimits::for_profile(profile),
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
    let reaction = agent.handle(Input::Submitted {
        text: text.to_owned(),
    });
    let [Effect::CallModel(request)] = reaction.effects.as_slice() else {
        panic!(
            "submission should open one model request: {:?}",
            reaction.effects
        );
    };
    (agent, request.request.clone())
}

/// Drives decoded provider events through admission and execution into the loop's next request.
pub fn complete_tool_step(agent: &mut Agent, events: &[ModelEvent], output: &str) -> ModelRequest {
    let mut effects = Vec::new();
    for event in events {
        let step_id = agent
            .active_model_step()
            .unwrap_or_else(|| panic!("fixture expected an open provider step"));
        effects.extend(
            agent
                .handle(Input::Streamed {
                    step_id,
                    event: event.clone(),
                })
                .effects,
        );
    }
    let [Effect::AdmitTool(call)] = effects.as_slice() else {
        panic!("tool step should request one admission: {effects:?}");
    };
    let call = call.clone();
    let admitted = AdmittedToolCall::new(
        call.clone(),
        ToolDefinitionId::new("read-file-v1")
            .unwrap_or_else(|error| panic!("fixture definition id: {error}")),
        ToolDefinitionRevision::new(1).unwrap_or_else(|| panic!("fixture revision")),
        [ToolCapability::FileRead],
        call.arguments.clone(),
        "read README.md".to_owned(),
    )
    .unwrap_or_else(|error| panic!("fixture admission: {error:?}"));
    let admitted_reaction = agent.handle(Input::ToolAdmissionResolved(AdmissionOutcome::Admitted(
        admitted,
    )));
    assert!(matches!(
        admitted_reaction.effects.as_slice(),
        [Effect::RunTool(running)] if running.requested() == &call
    ));

    let finished = agent.handle(Input::ToolFinished {
        call_id: call.call_id,
        outcome: ToolOutcome::Succeeded {
            output: output.to_owned(),
        },
    });
    let [Effect::CallModel(request)] = finished.effects.as_slice() else {
        panic!(
            "tool result should open the next model step: {:?}",
            finished.effects
        );
    };
    request.request.clone()
}

pub fn complete_answer(agent: &mut Agent, events: &[ModelEvent]) {
    for event in events {
        let step_id = agent
            .active_model_step()
            .unwrap_or_else(|| panic!("fixture expected an open provider step"));
        let reaction = agent.handle(Input::Streamed {
            step_id,
            event: event.clone(),
        });
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
            ModelEvent::Called(call) => Some(call.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("fixture should contain one tool call"))
}

pub fn visible_text(events: &[ModelEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            ModelEvent::TextDelta(text) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

pub fn reasoning_text(events: &[ModelEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            ModelEvent::ReasoningDelta(text) => Some(text.as_str()),
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

pub fn profile(protocol: Protocol) -> ProviderProfile {
    let protocol = match protocol {
        Protocol::Responses => "responses",
        Protocol::ChatCompletions => "chat_completions",
    };
    let effort = if protocol == "responses" {
        "xhigh"
    } else {
        "high"
    };
    let source = format!(
        r#"
active_provider = "local_luna"

[providers.local_luna]
kind = "openai_compatible"
protocol = "{protocol}"
base_url = "http://127.0.0.1:8317/v1"
model = "gpt-5.6-luna"
api_key_env = "PLEXMATON_LOCAL_API_KEY"
reasoning_effort = "{effort}"
"#
    );
    ProviderConfig::parse(&source)
        .unwrap_or_else(|error| panic!("fixture profile should parse: {error}"))
        .active()
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
