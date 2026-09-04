use plexmaton_core::{ToolCallId, TranscriptItemId};

use super::{
    AssistantBlock, AssistantOutput, AssistantReplay, ContextError, MAX_ASSISTANT_TEXT_BYTES,
    MAX_ASSISTANT_TOOL_ARGUMENT_BYTES, MAX_TOOL_IDENTITY_BYTES, ToolBatch, ToolBatchResult,
};
use crate::{
    ProviderCodecId, ProviderCodecRevision, ProviderModelFamilyId, ProviderReplay,
    ProviderReplayOwnerId, ReplayCompatibility, ToolCall, ToolOutcome,
};

fn id<T, E>(value: &str, constructor: impl FnOnce(&str) -> Result<T, E>) -> T
where
    E: std::fmt::Display,
{
    constructor(value).unwrap_or_else(|error| panic!("fixture identity: {error}"))
}

fn item(value: &str) -> TranscriptItemId {
    id(value, |value| TranscriptItemId::new(value))
}

fn call(value: &str, arguments: String) -> ToolCall {
    ToolCall {
        call_id: id(value, |value| ToolCallId::new(value)),
        name: "read_file".to_owned(),
        arguments,
    }
}

fn compatibility() -> ReplayCompatibility {
    ReplayCompatibility::new(
        ProviderReplayOwnerId::new("profile|route|credential-realm")
            .unwrap_or_else(|error| panic!("fixture owner: {error:?}")),
        ProviderCodecId::new("openai_responses")
            .unwrap_or_else(|error| panic!("fixture codec: {error:?}")),
        ProviderCodecRevision::new(1).unwrap_or_else(|error| panic!("fixture revision: {error:?}")),
        ProviderModelFamilyId::new("gpt-5.6")
            .unwrap_or_else(|error| panic!("fixture family: {error:?}")),
    )
}

/// JRN-5 and PRV-3: ordered semantics and opaque replay survive one typed JSON boundary.
#[test]
fn assistant_output_round_trips_order_and_redacts_replay() {
    let replay = ProviderReplay::new(compatibility(), "encrypted-secret".to_owned())
        .unwrap_or_else(|error| panic!("fixture replay: {error:?}"));
    let output = AssistantOutput::new(
        vec![
            AssistantBlock::Text {
                item_id: item("text-a"),
                text: "first".to_owned(),
            },
            AssistantBlock::Reasoning {
                item_id: item("reasoning-a"),
                text: "considering".to_owned(),
            },
            AssistantBlock::ToolCall {
                item_id: item("tool-a"),
                call: call("call-a", "{}".to_owned()),
            },
        ],
        AssistantReplay::from_positioned([(1, replay)])
            .unwrap_or_else(|error| panic!("fixture attachment: {error}")),
    )
    .unwrap_or_else(|error| panic!("fixture output: {error}"));

    let encoded =
        serde_json::to_string(&output).unwrap_or_else(|error| panic!("serialize output: {error}"));
    let decoded: AssistantOutput = serde_json::from_str(&encoded)
        .unwrap_or_else(|error| panic!("deserialize output: {error}"));

    assert_eq!(decoded, output);
    assert!(matches!(decoded.blocks()[0], AssistantBlock::Text { .. }));
    assert!(matches!(
        decoded.blocks()[1],
        AssistantBlock::Reasoning { .. }
    ));
    assert!(matches!(
        decoded.blocks()[2],
        AssistantBlock::ToolCall { .. }
    ));
    assert!(!format!("{decoded:?}").contains("encrypted-secret"));

    let mut invalid: serde_json::Value = serde_json::from_str(&encoded)
        .unwrap_or_else(|error| panic!("decode fixture JSON: {error}"));
    invalid["replay"]["attachments"][0]["payload"] = serde_json::Value::String(String::new());
    assert!(serde_json::from_value::<AssistantOutput>(invalid).is_err());
}

#[test]
fn assistant_output_rejects_duplicate_semantic_identities() {
    let repeated_item = AssistantOutput::new(
        vec![
            AssistantBlock::Text {
                item_id: item("same-item"),
                text: "one".to_owned(),
            },
            AssistantBlock::Reasoning {
                item_id: item("same-item"),
                text: "two".to_owned(),
            },
        ],
        None,
    );
    assert_eq!(repeated_item, Err(ContextError::DuplicateAssistantItemId));

    let repeated_call = AssistantOutput::new(
        vec![
            AssistantBlock::ToolCall {
                item_id: item("tool-a"),
                call: call("same-call", "{}".to_owned()),
            },
            AssistantBlock::ToolCall {
                item_id: item("tool-b"),
                call: call("same-call", "{}".to_owned()),
            },
        ],
        None,
    );
    assert_eq!(repeated_call, Err(ContextError::DuplicateToolCallId));
}

#[test]
fn parallel_calls_share_one_aggregate_argument_bound() {
    let per_call = crate::MAX_REQUESTED_TOOL_ARGUMENT_BYTES;
    assert_eq!(MAX_ASSISTANT_TOOL_ARGUMENT_BYTES, per_call * 8);
    let blocks = (0..9)
        .map(|index| AssistantBlock::ToolCall {
            item_id: item(&format!("tool-{index}")),
            call: call(
                &format!("call-{index}"),
                "x".repeat(if index == 8 { 1 } else { per_call }),
            ),
        })
        .collect();
    let output = AssistantOutput::new(blocks, None);

    assert_eq!(output, Err(ContextError::ToolArgumentsTooLarge));
}

#[test]
fn assistant_output_rechecks_aggregate_text_and_tool_identity_bounds() {
    let oversized_text = AssistantOutput::new(
        vec![AssistantBlock::Text {
            item_id: item("text-a"),
            text: "x".repeat(MAX_ASSISTANT_TEXT_BYTES + 1),
        }],
        None,
    );
    assert_eq!(oversized_text, Err(ContextError::AssistantTextTooLarge));

    let oversized_name = AssistantOutput::new(
        vec![AssistantBlock::ToolCall {
            item_id: item("tool-a"),
            call: ToolCall {
                call_id: id("call-a", |value| ToolCallId::new(value)),
                name: "x".repeat(MAX_TOOL_IDENTITY_BYTES + 1),
                arguments: "{}".to_owned(),
            },
        }],
        None,
    );
    assert_eq!(oversized_name, Err(ContextError::ToolIdentityTooLarge));
}

/// JRN-5: completion order cannot reorder the result vector a provider receives.
#[test]
fn tool_batch_accepts_only_model_call_order() {
    let output = AssistantOutput::new(
        vec![
            AssistantBlock::ToolCall {
                item_id: item("tool-a"),
                call: call("call-a", "{}".to_owned()),
            },
            AssistantBlock::ToolCall {
                item_id: item("tool-b"),
                call: call("call-b", "{}".to_owned()),
            },
        ],
        None,
    )
    .unwrap_or_else(|error| panic!("fixture output: {error}"));
    let result_a = ToolBatchResult::new(
        id("call-a", |value| ToolCallId::new(value)),
        ToolOutcome::Succeeded {
            output: "a".to_owned(),
        },
    );
    let result_b = ToolBatchResult::new(
        id("call-b", |value| ToolCallId::new(value)),
        ToolOutcome::Succeeded {
            output: "b".to_owned(),
        },
    );

    assert_eq!(
        ToolBatch::new(output.clone(), vec![result_b.clone(), result_a.clone()]),
        Err(ContextError::ToolResultOrderMismatch)
    );
    let batch = ToolBatch::new(output, vec![result_a, result_b])
        .unwrap_or_else(|error| panic!("ordered batch: {error}"));
    assert_eq!(batch.results()[0].call_id().as_str(), "call-a");
    assert_eq!(batch.results()[1].call_id().as_str(), "call-b");
}

#[test]
fn tool_batch_rechecks_persisted_result_bounds() {
    let output = AssistantOutput::new(
        vec![AssistantBlock::ToolCall {
            item_id: item("tool-a"),
            call: call("call-a", "{}".to_owned()),
        }],
        None,
    )
    .unwrap_or_else(|error| panic!("fixture output: {error}"));
    let result = ToolBatchResult::new(
        id("call-a", |value| ToolCallId::new(value)),
        ToolOutcome::Succeeded {
            output: "x".repeat(crate::MAX_TOOL_PRESENTATION_TEXT_BYTES + 1),
        },
    );

    assert_eq!(
        ToolBatch::new(output, vec![result]),
        Err(ContextError::ToolOutcomeTooLarge)
    );
}
