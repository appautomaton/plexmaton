//! Stateless Responses request reconstruction.

use plexmaton_agent::{
    AssistantBlock, AssistantOutput, BlockReplay, ContextAtom, ContextAtomValue, ModelRequest,
};
use plexmaton_core::ServerToolAction;
use serde_json::{Value, json};

use super::replay::{MessagePartKind, ResponseReplay};

use crate::{
    FunctionTool, ResolvedModel, ServerTool,
    codec::{EncodeError, RESPONSES_CODEC_ID, tool_output},
    degrade::{self, Carried},
};

pub(crate) fn encode(
    model: &ResolvedModel,
    request: &ModelRequest,
    tools: &[FunctionTool],
    max_output_tokens: Option<u32>,
) -> Result<Value, EncodeError> {
    let input = encode_input(model, request)?;
    let mut tools: Vec<_> = tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "name": tool.name(),
                "description": tool.description(),
                "parameters": tool.parameters(),
                "strict": true,
            })
        })
        .collect();
    // PRV-6: configuration named the capability; this dialect owns the spelling. Hosted tools
    // follow the function tools so the order the owner reads in the body is the order declared.
    tools.extend(
        model
            .server_tools()
            .unwrap_or_default()
            .iter()
            .map(|tool| match tool {
                ServerTool::WebSearch => json!({"type": "web_search"}),
            }),
    );
    let mut body = json!({
        "model": model.wire_id(),
        "input": input,
        "stream": true,
        "store": false,
        "include": ["reasoning.encrypted_content"],
        "parallel_tool_calls": true,
    });
    if model.reasoning_effort() != plexmaton_core::ReasoningEffort::Default {
        body["reasoning"] = json!({"effort":model.reasoning_effort().as_str()});
    }
    if !model.instructions().is_empty() {
        body["instructions"] = Value::String(model.instructions().to_owned());
    }
    if model.prompt_cache() == crate::PromptCache::Automatic {
        body["prompt_cache_key"] = Value::String(crate::environment::session_cache_key(request));
    }
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
        body["tool_choice"] = Value::String("auto".to_owned());
    }
    if let Some(limit) = max_output_tokens {
        body["max_output_tokens"] = Value::from(limit);
    }
    Ok(body)
}

fn encode_input(model: &ResolvedModel, request: &ModelRequest) -> Result<Vec<Value>, EncodeError> {
    let mut input = Vec::new();
    if !model.workspace_instructions().is_empty() {
        input.push(json!({"role":"user", "content":model.workspace_instructions()}));
    }
    for atom in &request.atoms {
        input.extend(encode_atom(model, atom)?);
    }
    Ok(input)
}

pub(crate) fn encode_atom(
    model: &ResolvedModel,
    atom: &ContextAtom,
) -> Result<Vec<Value>, EncodeError> {
    let mut input = Vec::new();
    match atom.value() {
        ContextAtomValue::Collaboration(context) => {
            input.push(json!({
                "role": "user",
                "content": crate::collaboration::collaboration_context(context)?,
            }));
        }
        ContextAtomValue::User { text } | ContextAtomValue::CompactionSummary { text } => {
            input.push(json!({ "role": "user", "content": text }));
        }
        ContextAtomValue::Skill(activation) => {
            input.push(json!({
                "role": "user",
                "content": crate::codec::skill_context(activation),
            }));
        }
        ContextAtomValue::Assistant(output) => {
            encode_assistant(model, output, &mut input)?;
        }
        ContextAtomValue::ToolBatch(batch) => {
            encode_assistant(model, batch.assistant(), &mut input)?;
            let degraded = degrade::is_degraded(batch.assistant(), model);
            input.extend(batch.results().iter().map(|result| {
                json!({
                    "type": "function_call_output",
                    "call_id": degrade::atom_call_id(degraded, result.call_id()),
                    "output": tool_output(result.outcome()),
                })
            }));
        }
    }
    Ok(input)
}

/// Spells what survived a replay this model cannot use. PRV-3 owns what that is; this only writes
/// it in the dialect's own words.
fn degraded_assistant(carried: &[Carried<'_>], input: &mut Vec<Value>) {
    for block in carried {
        input.push(match block {
            Carried::Text(text) => json!({
                "type": "message", "role": "assistant",
                "content": [{"type": "output_text", "text": text}],
            }),
            Carried::Call(call) => json!({
                "type": "function_call",
                "call_id": degrade::atom_call_id(true, &call.call_id),
                "name": call.name,
                "arguments": call.arguments,
            }),
        });
    }
}

fn encode_assistant(
    model: &ResolvedModel,
    output: &AssistantOutput,
    input: &mut Vec<Value>,
) -> Result<(), EncodeError> {
    if let Some(carried) = degrade::degraded(output, model) {
        degraded_assistant(&carried, input);
        return Ok(());
    }
    debug_assert_eq!(
        model.replay_compatibility().codec().as_str(),
        RESPONSES_CODEC_ID
    );
    let mut attachments = output
        .replay()
        .map(|replay| replay.attachments().iter().peekable());

    for (index, block) in output.blocks().iter().enumerate() {
        let block_index = u16::try_from(index)
            .unwrap_or_else(|_| unreachable!("assistant block count is validated"));
        let replay = attachments.as_mut().and_then(|attachments| {
            (attachments
                .peek()
                .is_some_and(|item| item.block() == block_index))
            .then(|| {
                attachments
                    .next()
                    .unwrap_or_else(|| unreachable!("peeked attachment"))
            })
        });
        match block {
            AssistantBlock::Text { text, .. } => {
                if let Some(replay) = replay {
                    encode_text_replay(replay, text, input)?;
                } else {
                    input.push(json!({
                        "type": "message", "role": "assistant",
                        "content": [{"type": "output_text", "text": text}],
                    }));
                }
            }
            AssistantBlock::ReplayOnly { .. } => {
                let replay = replay.ok_or(EncodeError::InvalidReplayItem)?;
                let item: Value = serde_json::from_str(replay.payload())
                    .map_err(EncodeError::InvalidReplayJson)?;
                if item.get("type").and_then(Value::as_str) == Some("reasoning") {
                    input.push(decode_replay(replay)?);
                } else {
                    encode_text_replay(replay, "", input)?;
                }
            }
            AssistantBlock::Reasoning { .. } => {
                if let Some(replay) = replay {
                    input.push(decode_replay(replay)?);
                }
                // PRV-3: a summary interrupted before its replay item stays visible locally and
                // is left out of the wire, whether or not the turn went on to call a tool. There
                // is no signature here to be missing: a Responses reasoning item is identified by
                // the provider's own opaque id, and an input that omits one is well formed — the
                // API's ordering requirement runs the other way, binding a reasoning item that is
                // present to the item that followed it.
                //
                // Rejected: refusing the turn, as Messages does. That requirement is Anthropic's
                // and has a reason there — a `thinking` block beside `tool_use` must arrive
                // complete and signed — but it was applied to this dialect as well, where nothing
                // asks for it. One interrupted tool turn then made every later request in that
                // conversation unencodable, so a session could be read and never continued.
            }
            AssistantBlock::ToolCall { call, .. } => {
                let mut item = json!({"type":"function_call","call_id":call.call_id.as_str(),"name":call.name,"arguments":call.arguments});
                if let Some(replay) = replay {
                    let metadata: ResponseReplay = serde_json::from_str(replay.payload())
                        .map_err(|_| EncodeError::InvalidReplayItem)?;
                    let ResponseReplay::FunctionCall { id, status } = metadata else {
                        return Err(EncodeError::InvalidReplayItem);
                    };
                    if let Some(id) = id {
                        item["id"] = Value::String(id);
                    }
                    if let Some(status) = status {
                        item["status"] =
                            serde_json::to_value(status).map_err(EncodeError::InvalidReplayJson)?;
                    }
                }
                input.push(item);
            }
            AssistantBlock::ServerToolCall { call, .. } => {
                // The item is rebuilt from the record; the sidecar adds only the provider's own
                // identity and status, the way a function call's does.
                let kind = match call.tool {
                    ServerTool::WebSearch => "web_search_call",
                };
                let mut item = json!({
                    "type": kind,
                    "status": "completed",
                    "action": web_search_action(&call.action),
                });
                if let Some(replay) = replay {
                    let metadata: ResponseReplay = serde_json::from_str(replay.payload())
                        .map_err(|_| EncodeError::InvalidReplayItem)?;
                    let ResponseReplay::WebSearchCall { id, status } = metadata else {
                        return Err(EncodeError::InvalidReplayItem);
                    };
                    if let Some(id) = id {
                        item["id"] = Value::String(id);
                    }
                    if let Some(status) = status {
                        item["status"] =
                            serde_json::to_value(status).map_err(EncodeError::InvalidReplayJson)?;
                    }
                }
                input.push(item);
            }
        }
    }
    debug_assert!(
        attachments
            .as_mut()
            .is_none_or(|attachments| attachments.next().is_none()),
        "assistant replay anchors are validated"
    );
    Ok(())
}

fn decode_replay(replay: &BlockReplay) -> Result<Value, EncodeError> {
    let item: Value =
        serde_json::from_str(replay.payload()).map_err(EncodeError::InvalidReplayJson)?;
    if item.get("type").and_then(Value::as_str) != Some("reasoning")
        || item
            .get("encrypted_content")
            .and_then(Value::as_str)
            .is_none()
    {
        return Err(EncodeError::InvalidReplayItem);
    }
    Ok(item)
}

fn encode_text_replay(
    replay: &BlockReplay,
    text: &str,
    input: &mut Vec<Value>,
) -> Result<(), EncodeError> {
    let metadata: ResponseReplay =
        serde_json::from_str(replay.payload()).map_err(|_| EncodeError::InvalidReplayItem)?;
    let (id, phase, status, content_index, part, annotations) = match metadata {
        ResponseReplay::MessagePart {
            id,
            phase,
            status,
            content_index,
            part,
            annotations,
        } => (id, phase, status, content_index, part, annotations),
        ResponseReplay::WebSearchCall { .. } => return Err(EncodeError::InvalidReplayItem),
        ResponseReplay::EmptyMessage { id, phase, status } => {
            if !text.is_empty() {
                return Err(EncodeError::InvalidReplayItem);
            }
            let mut message = json!({"type":"message", "role":"assistant", "id":id, "content":[]});
            if let Some(phase) = phase {
                message["phase"] =
                    serde_json::to_value(phase).map_err(EncodeError::InvalidReplayJson)?;
            }
            if let Some(status) = status {
                message["status"] =
                    serde_json::to_value(status).map_err(EncodeError::InvalidReplayJson)?;
            }
            input.push(message);
            return Ok(());
        }
        ResponseReplay::FunctionCall { .. } => return Err(EncodeError::InvalidReplayItem),
    };
    let content = match part {
        MessagePartKind::OutputText => {
            json!({"type":"output_text", "text":text, "annotations":annotations})
        }
        MessagePartKind::Refusal => json!({"type":"refusal", "refusal":text}),
    };
    if content_index == 0 {
        let mut message =
            json!({"type":"message", "role":"assistant", "id":id, "content":[content]});
        if let Some(phase) = phase {
            message["phase"] =
                serde_json::to_value(phase).map_err(EncodeError::InvalidReplayJson)?;
        }
        if let Some(status) = status {
            message["status"] =
                serde_json::to_value(status).map_err(EncodeError::InvalidReplayJson)?;
        }
        input.push(message);
    } else {
        let prior = input.last_mut().ok_or(EncodeError::InvalidReplayItem)?;
        if prior["type"] != "message"
            || prior["id"] != id
            || serde_json::from_value::<Option<super::replay::MessagePhase>>(prior["phase"].clone())
                .map_err(EncodeError::InvalidReplayJson)?
                != phase
            || serde_json::from_value::<Option<super::replay::ItemStatus>>(prior["status"].clone())
                .map_err(|_| EncodeError::InvalidReplayItem)?
                != status
        {
            return Err(EncodeError::InvalidReplayItem);
        }
        let parts = prior["content"]
            .as_array_mut()
            .ok_or(EncodeError::InvalidReplayItem)?;
        if parts.len() != content_index {
            return Err(EncodeError::InvalidReplayItem);
        }
        parts.push(content);
    }
    Ok(())
}

/// The wire's spelling of what a search did. `query` is kept beside `queries` because both
/// spellings have been observed on live routes, and a replay should read as the item did.
fn web_search_action(action: &ServerToolAction) -> Value {
    match action {
        ServerToolAction::Search { queries } => {
            let mut value = json!({"type": "search", "queries": queries});
            if let Some(first) = queries.first() {
                value["query"] = Value::String(first.clone());
            }
            value
        }
        ServerToolAction::OpenPage { url } => json!({"type": "open_page", "url": url}),
        ServerToolAction::FindInPage { url, pattern } => {
            json!({"type": "find_in_page", "url": url, "pattern": pattern})
        }
    }
}
