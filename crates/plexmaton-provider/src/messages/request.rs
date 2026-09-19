//! Stateless Messages reconstruction from ordered canonical blocks.

use plexmaton_agent::{
    AssistantBlock, AssistantOutput, ContextAtom, ContextAtomValue, ModelRequest, ToolCall,
    ToolOutcome,
};
use plexmaton_core::ReasoningEffort;
use serde_json::{Value, json};

use crate::{
    EncodeError, FunctionTool, PromptCache, ResolvedModel,
    codec::tool_output,
    degrade::{self, Carried},
};

pub(crate) fn encode(
    model: &ResolvedModel,
    request: &ModelRequest,
    tools: &[FunctionTool],
    max_output_tokens: Option<u32>,
) -> Result<Value, EncodeError> {
    let mut messages = Vec::new();
    if !model.workspace_instructions().is_empty() {
        messages.push(json!({"role":"user", "content":[{"type":"text", "text":model.workspace_instructions()}]}));
    }
    for atom in &request.atoms {
        messages.extend(encode_atom(model, atom)?);
    }
    let mut body = json!({
        "model":model.wire_id(), "messages":messages, "stream":true,
        "max_tokens":max_output_tokens.unwrap_or(model.max_output_tokens()),
    });
    if !model.instructions().is_empty() {
        body["system"] = json!([{"type":"text","text":model.instructions()}]);
    }
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools.iter().map(|tool| json!({"name":tool.name(),"description":tool.description(),"input_schema":tool.parameters()})).collect());
        body["tool_choice"] = json!({"type":"auto"});
    }
    if model.prompt_cache() == PromptCache::Automatic {
        // PRV-6: the native Messages API applies this marker to its last cacheable block.
        // https://platform.claude.com/docs/en/build-with-claude/prompt-caching#automatic-caching
        body["cache_control"] = json!({"type":"ephemeral"});
    }
    match model.reasoning_effort() {
        ReasoningEffort::None => body["thinking"] = json!({"type":"disabled"}),
        _ => body["thinking"] = json!({"type":"adaptive","display":"summarized"}),
    }
    if !matches!(
        model.reasoning_effort(),
        ReasoningEffort::Default | ReasoningEffort::None
    ) {
        body["output_config"] = json!({"effort":model.reasoning_effort().as_str()});
    }
    Ok(body)
}

pub(crate) fn encode_atom(
    model: &ResolvedModel,
    atom: &ContextAtom,
) -> Result<Vec<Value>, EncodeError> {
    match atom.value() {
        ContextAtomValue::Collaboration(context) => Ok(vec![json!({
            "role":"user",
            "content":[{"type":"text", "text":crate::collaboration::collaboration_context(context)?}],
        })]),
        ContextAtomValue::User { text } | ContextAtomValue::CompactionSummary { text } => Ok(vec![
            json!({"role":"user", "content":[{"type":"text", "text":text}]}),
        ]),
        ContextAtomValue::Skill(activation) => Ok(vec![json!({
            "role":"user",
            "content":[{"type":"text", "text":crate::codec::skill_context(activation)}],
        })]),
        ContextAtomValue::Assistant(output) => Ok(assistant(model, output)?.into_iter().collect()),
        ContextAtomValue::ToolBatch(batch) => {
            let degraded = degrade::is_degraded(batch.assistant(), model);
            let content: Vec<_> = batch
                .results()
                .iter()
                .map(|result| {
                    json!({
                        "type":"tool_result",
                        "tool_use_id":degrade::atom_call_id(degraded, result.call_id()),
                        "content":tool_output(result.outcome()),
                        "is_error": !matches!(result.outcome(), ToolOutcome::Succeeded { .. }),
                    })
                })
                .collect();
            Ok(vec![
                assistant(model, batch.assistant())?.ok_or(EncodeError::InvalidReplayItem)?,
                json!({"role":"user", "content":content}),
            ])
        }
    }
}

/// Spells what survived a replay this model cannot use. PRV-3 owns what that is; this only writes
/// it in the dialect's own words.
fn degraded_assistant(carried: &[Carried<'_>]) -> Result<Option<Value>, EncodeError> {
    let mut content = Vec::new();
    for block in carried {
        content.push(match block {
            Carried::Text(text) => json!({"type":"text", "text":text}),
            Carried::Call(call) => json!({
                "type":"tool_use",
                "id":degrade::atom_call_id(true, &call.call_id),
                "name":call.name,
                "input":tool_input(call)?,
            }),
        });
    }
    Ok((!content.is_empty()).then(|| json!({"role":"assistant", "content":content})))
}

fn tool_input(call: &ToolCall) -> Result<Value, EncodeError> {
    let input: Value =
        serde_json::from_str(&call.arguments).map_err(|_| EncodeError::InvalidToolArguments)?;
    if !input.is_object() {
        return Err(EncodeError::InvalidToolArguments);
    }
    Ok(input)
}

fn assistant(
    model: &ResolvedModel,
    output: &AssistantOutput,
) -> Result<Option<Value>, EncodeError> {
    if let Some(carried) = degrade::degraded(output, model) {
        return degraded_assistant(&carried);
    }
    let mut content = Vec::new();
    for (index, block) in output.blocks().iter().enumerate() {
        let replay = output.replay().and_then(|replay| {
            replay
                .attachments()
                .iter()
                .find(|part| usize::from(part.block()) == index)
        });
        let item = match block {
            AssistantBlock::Reasoning { .. }
                if replay.is_none() && output.tool_calls().next().is_none() =>
            {
                // PRV-3: an interrupted summary stays in the journal, but is not a signed block.
                // A tool turn still requires every thinking block to be complete and unchanged.
                continue;
            }
            AssistantBlock::Text { text, .. } if replay.is_none() => {
                json!({"type":"text", "text":text})
            }
            AssistantBlock::ToolCall { call, .. } if replay.is_none() => {
                json!({
                    "type":"tool_use", "id":call.call_id.as_str(),
                    "name":call.name, "input":tool_input(call)?,
                })
            }
            AssistantBlock::Reasoning { .. } | AssistantBlock::ReplayOnly { .. } => {
                let replay = replay.ok_or(EncodeError::MissingThinkingSignature)?;
                let item: Value = serde_json::from_str(replay.payload())
                    .map_err(|_| EncodeError::InvalidReplayItem)?;
                match item.get("type").and_then(Value::as_str) {
                    Some("thinking")
                        if item
                            .get("signature")
                            .and_then(Value::as_str)
                            .is_some_and(|s| !s.is_empty()) =>
                    {
                        let text = match block {
                            AssistantBlock::Reasoning { text, .. } => text.as_str(),
                            _ => "",
                        };
                        if item.get("thinking").and_then(Value::as_str) != Some(text) {
                            return Err(EncodeError::InvalidReplayItem);
                        }
                    }
                    Some("redacted_thinking")
                        if matches!(block, AssistantBlock::ReplayOnly { .. })
                            && item
                                .get("data")
                                .and_then(Value::as_str)
                                .is_some_and(|s| !s.is_empty()) => {}
                    _ => return Err(EncodeError::InvalidReplayItem),
                }
                item
            }
            _ => return Err(EncodeError::InvalidReplayItem),
        };
        content.push(item);
    }
    Ok((!content.is_empty()).then(|| json!({"role":"assistant", "content":content})))
}
