//! Stateless Responses request reconstruction.

use plexmaton_agent::{
    AssistantBlock, AssistantOutput, BlockReplay, ContextAtomValue, ModelRequest,
};
use serde_json::{Value, json};

use crate::{
    FunctionTool, ProviderProfile,
    codec::{EncodeError, RESPONSES_CODEC_ID, tool_output},
};

pub(crate) fn encode(
    profile: &ProviderProfile,
    request: &ModelRequest,
    tools: &[FunctionTool],
    max_output_tokens: Option<u32>,
) -> Result<Value, EncodeError> {
    let input = encode_input(profile, request)?;
    let tools: Vec<_> = tools
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
    let mut body = json!({
        "model": profile.model(),
        "input": input,
        "stream": true,
        "store": false,
        "include": ["reasoning.encrypted_content"],
        "reasoning": { "effort": profile.reasoning_effort().as_str() },
        "parallel_tool_calls": true,
    });
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
        body["tool_choice"] = Value::String("auto".to_owned());
    }
    if let Some(limit) = max_output_tokens {
        body["max_output_tokens"] = Value::from(limit);
    }
    Ok(body)
}

fn encode_input(
    profile: &ProviderProfile,
    request: &ModelRequest,
) -> Result<Vec<Value>, EncodeError> {
    let mut input = Vec::new();
    for atom in &request.atoms {
        match atom.value() {
            ContextAtomValue::User { text } => {
                input.push(json!({ "role": "user", "content": text }));
            }
            ContextAtomValue::Assistant(output) => {
                encode_assistant(profile, output, &mut input)?;
            }
            ContextAtomValue::ToolBatch(batch) => {
                encode_assistant(profile, batch.assistant(), &mut input)?;
                input.extend(batch.results().iter().map(|result| {
                    json!({
                        "type": "function_call_output",
                        "call_id": result.call_id().as_str(),
                        "output": tool_output(result.outcome()),
                    })
                }));
            }
        }
    }
    Ok(input)
}

fn encode_assistant(
    profile: &ProviderProfile,
    output: &AssistantOutput,
    input: &mut Vec<Value>,
) -> Result<(), EncodeError> {
    let mut attachments = output
        .replay()
        .map(|replay| {
            let expected = profile.replay_compatibility();
            debug_assert_eq!(expected.codec().as_str(), RESPONSES_CODEC_ID);
            if replay.compatible_with() != &expected {
                return Err(EncodeError::IncompatibleReplay {
                    found: Box::new(replay.compatible_with().clone()),
                    expected: Box::new(expected),
                });
            }
            Ok(replay.attachments().iter().peekable())
        })
        .transpose()?;

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
            AssistantBlock::Text { text, .. } => input.push(json!({
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": text }],
            })),
            AssistantBlock::Reasoning { .. } => {
                let replay = replay.ok_or(EncodeError::PlainReasoningInResponses)?;
                input.push(decode_replay(replay)?);
            }
            AssistantBlock::ToolCall { call, .. } => {
                debug_assert!(replay.is_none(), "replay anchors are reasoning-only");
                input.push(json!({
                    "type": "function_call",
                    "call_id": call.call_id.as_str(),
                    "name": call.name,
                    "arguments": call.arguments,
                }));
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
