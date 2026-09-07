//! Stateless GenerateContent request encoding with part-local signatures.

use plexmaton_agent::{
    AssistantBlock, AssistantOutput, ContextAtom, ContextAtomValue, ModelRequest, ToolOutcome,
};
use plexmaton_core::ReasoningEffort;
use serde_json::{Value, json};

use super::wire::PartReplay;
use crate::{EncodeError, FunctionTool, ResolvedModel, codec::tool_output};

pub(crate) fn encode(
    model: &ResolvedModel,
    request: &ModelRequest,
    tools: &[FunctionTool],
    max_output_tokens: Option<u32>,
) -> Result<Value, EncodeError> {
    let mut contents = Vec::new();
    if !model.workspace_instructions().is_empty() {
        contents.push(json!({"role":"user", "parts":[{"text":model.workspace_instructions()}]}));
    }
    for atom in &request.atoms {
        contents.extend(encode_atom(model, atom)?);
    }
    let mut body = json!({"contents":contents, "generationConfig":{"maxOutputTokens":max_output_tokens.unwrap_or(model.max_output_tokens())}});
    if !model.instructions().is_empty() {
        body["systemInstruction"] = json!({"parts":[{"text":model.instructions()}]});
    }
    if !tools.is_empty() {
        let mut declarations = Vec::new();
        for tool in tools {
            if tool.name().len() > 128
                || !tool
                    .name()
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            {
                return Err(EncodeError::InvalidToolName);
            }
            declarations.push(json!({"name":tool.name(),"description":tool.description(),"parametersJsonSchema":tool.parameters()}));
        }
        body["tools"] = json!([{"functionDeclarations":declarations}]);
        body["toolConfig"] = json!({"functionCallingConfig":{"mode":"AUTO"}});
    }
    body["generationConfig"]["thinkingConfig"] = json!({"includeThoughts":true});
    if model.reasoning_effort() != ReasoningEffort::Default {
        body["generationConfig"]["thinkingConfig"]["thinkingLevel"] =
            json!(model.reasoning_effort().as_str().to_ascii_uppercase());
    }
    Ok(body)
}

pub(crate) fn encode_atom(
    model: &ResolvedModel,
    atom: &ContextAtom,
) -> Result<Vec<Value>, EncodeError> {
    match atom.value() {
        ContextAtomValue::Collaboration(_) => Err(EncodeError::UnsupportedCollaboration),
        ContextAtomValue::User { text } | ContextAtomValue::CompactionSummary { text } => {
            Ok(vec![json!({"role":"user", "parts":[{"text":text}]})])
        }
        ContextAtomValue::Skill(activation) => Ok(vec![json!({
            "role":"user",
            "parts":[{"text":crate::codec::skill_context(activation)}],
        })]),
        ContextAtomValue::Assistant(output) => Ok(vec![assistant(model, output)?]),
        ContextAtomValue::ToolBatch(batch) => {
            let assistant = assistant(model, batch.assistant())?;
            let parts = assistant["parts"]
                .as_array()
                .ok_or(EncodeError::InvalidReplayItem)?;
            let calls = parts.iter().filter_map(|part| part.get("functionCall"));
            let mut results = Vec::new();
            for (call, result) in calls.zip(batch.results()) {
                let output = tool_output(result.outcome());
                let response = if matches!(result.outcome(), ToolOutcome::Succeeded { .. }) {
                    json!({"output":output})
                } else {
                    json!({"error":output})
                };
                let mut function = json!({"name":call["name"], "response":response});
                if let Some(id) = call.get("id") {
                    function["id"] = id.clone();
                }
                results.push(json!({"functionResponse":function}));
            }
            if results.len() != batch.results().len() {
                return Err(EncodeError::InvalidReplayItem);
            }
            Ok(vec![assistant, json!({"role":"user","parts":results})])
        }
    }
}

fn assistant(model: &ResolvedModel, output: &AssistantOutput) -> Result<Value, EncodeError> {
    let expected = model.replay_compatibility();
    if let Some(replay) = output.replay()
        && replay.compatible_with() != &expected
    {
        return Err(EncodeError::IncompatibleReplay {
            found: Box::new(replay.compatible_with().clone()),
            expected: Box::new(expected),
        });
    }
    let mut parts = Vec::new();
    for (index, block) in output.blocks().iter().enumerate() {
        let replay = output.replay().and_then(|replay| {
            replay
                .attachments()
                .iter()
                .find(|part| usize::from(part.block()) == index)
        });
        let metadata = replay
            .map(|part| {
                serde_json::from_str::<PartReplay>(part.payload())
                    .map_err(|_| EncodeError::InvalidReplayItem)
            })
            .transpose()?;
        let (mut part, signature) = match (block, metadata) {
            (
                AssistantBlock::ToolCall { call, .. },
                Some(PartReplay::FunctionCall {
                    upstream_id,
                    signature,
                }),
            ) => {
                let args: Value = serde_json::from_str(&call.arguments)
                    .map_err(|_| EncodeError::InvalidToolArguments)?;
                if !args.is_object() {
                    return Err(EncodeError::InvalidToolArguments);
                }
                let mut function = json!({"name":call.name,"args":args});
                if let Some(id) = upstream_id {
                    function["id"] = Value::String(id);
                }
                (json!({"functionCall":function}), signature)
            }
            (AssistantBlock::Text { text, .. }, None) => (json!({"text":text}), None),
            (AssistantBlock::Reasoning { text, .. }, None) => {
                (json!({"text":text,"thought":true}), None)
            }
            (
                block,
                Some(PartReplay::Text {
                    thought,
                    text_present,
                    signature,
                }),
            ) => {
                let text = match block {
                    AssistantBlock::Text { text, .. } if !thought => Some(text.as_str()),
                    AssistantBlock::Reasoning { text, .. } if thought => Some(text.as_str()),
                    AssistantBlock::ReplayOnly { .. } => text_present.then_some(""),
                    _ => return Err(EncodeError::InvalidReplayItem),
                };
                let mut part = json!({});
                if let Some(text) = text {
                    part["text"] = json!(text);
                }
                if thought {
                    part["thought"] = Value::Bool(true);
                }
                (part, Some(signature))
            }
            _ => return Err(EncodeError::InvalidReplayItem),
        };
        if let Some(signature) = signature {
            if signature.is_empty() {
                return Err(EncodeError::InvalidReplayItem);
            }
            part["thoughtSignature"] = Value::String(signature);
        }
        parts.push(part);
    }
    Ok(json!({"role":"model","parts":parts}))
}
