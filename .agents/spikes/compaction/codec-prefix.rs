//! Offline probe for the cache-preserving-compaction question.
//!
//! Run `./.agents/spikes/compaction/run-codec-spike.sh` from the worktree.
//! It intentionally uses public journal projections and `encode_request`, not copied codecs.

use plexmaton_agent::{
    AssistantBlock, AssistantOutput, AssistantReplay, ContextAtom, JournalEntryPayload,
    JournalRecord, ModelRequest, ProviderReplay, RequestEnvironment, ConversationEntry, ConversationJournal,
    ToolCall, ToolOutcome,
};
use plexmaton_core::{
    AgentId, AgentStatus, HeadName, JournalRecordId, ConversationEntryId, ConversationId, ToolCallId,
    ToolCallStatus, ToolPresentation, TranscriptItemId, TurnId,
};
use plexmaton_provider::{FunctionTool, ModelRegistry, encode_request, request_environment};
use serde_json::{Value, json};

const SUMMARY: &str = "Write a compact factual checkpoint of the earlier conversation.";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Dialect {
    Responses,
    Chat,
    Messages,
    Gemini,
}

impl Dialect {
    const ALL: [Self; 4] = [Self::Responses, Self::Chat, Self::Messages, Self::Gemini];

    const fn api(self) -> &'static str {
        match self {
            Self::Responses => "openai_responses",
            Self::Chat => "openai_chat_completions",
            Self::Messages => "anthropic_messages",
            Self::Gemini => "google_generate_content",
        }
    }

    const fn input_key(self) -> &'static str {
        match self {
            Self::Responses => "input",
            Self::Chat | Self::Messages => "messages",
            Self::Gemini => "contents",
        }
    }
}

/// PRV-4/JRN-5: a normal journal-derived final answer is an exact wire-element prefix after
/// appending one summary instruction. Detects a codec that rewrites earlier semantic elements.
#[test]
fn ordinary_final_assistant_extension_keeps_the_wire_input_prefix() {
    for dialect in Dialect::ALL {
        let old = journal_request(assistant_text("final-item", "The repository is ready."));
        let tools = tools();
        assert_appended_summary_preserves_prefix(dialect, old, &tools);
    }
}

/// PRV-1/JRN-5: encoded history after a complete call/result batch remains a prefix.
/// Detects an encoder that rewrites the completed batch when an instruction is appended.
#[test]
fn complete_tool_batch_extension_keeps_the_wire_input_prefix() {
    for dialect in Dialect::ALL {
        let old = journal_request(tool_output(dialect));
        let tools = tools();
        assert_appended_summary_preserves_prefix(dialect, old, &tools);
    }
}

/// PRV-3/PRV-4: exact compatible replay survives before the appended instruction. Detects a
/// reconstruction path that drops, translates, or relocates a retained replay sidecar.
#[test]
fn compatible_replay_extension_keeps_the_wire_input_prefix() {
    for dialect in Dialect::ALL {
        let old = journal_request(replayed_output(dialect));
        let tools = tools();
        let baseline = encode(dialect, &old, "Stable environment.", &tools);
        assert_replay_shape(&baseline, dialect);
        assert_appended_summary_preserves_prefix(dialect, old, &tools);
    }
}

/// A new instruction stays separate from an existing user/tool-result tail. This detects a
/// normalizer that folds the instruction into that tail and invalidates the old wire prefix.
#[test]
fn user_and_tool_result_tails_do_not_coalesce_with_the_appended_instruction() {
    for dialect in Dialect::ALL {
        let tools = tools();
        let user_tail = ModelRequest {
            session_id: session_id(),
            atoms: vec![ContextAtom::user(
                entry_id("old-user"),
                "Earlier user input.".into(),
            )],
        };
        assert_appended_summary_preserves_prefix(dialect, user_tail, &tools);

        let tool_tail = journal_request(tool_output(dialect));
        assert_appended_summary_preserves_prefix(dialect, tool_tail, &tools);
    }
}

/// A changed instruction slot and a flattened transcript are both concrete prefix breaks.
/// Detects the tempting implementation that asks the summary under altered environment data or
/// substitutes rendered history for the authoritative semantic request.
#[test]
fn changed_environment_and_flattened_transcript_are_not_prefix_preserving() {
    for dialect in Dialect::ALL {
        let tools = tools();
        let old = journal_request(assistant_text("final-item", "The repository is ready."));
        let before = encode(dialect, &old, "Stable environment.", &tools);
        let changed_environment = encode(
            dialect,
            &old,
            "Stable environment. Also summarize the preceding conversation.",
            &tools,
        );
        assert_ne!(
            semantic_environment(dialect, "Stable environment.", &tools),
            semantic_environment(
                dialect,
                "Stable environment. Also summarize the preceding conversation.",
                &tools,
            ),
            "{dialect:?}: the actual request-environment fingerprint must reject changed instructions"
        );
        if dialect == Dialect::Chat {
            assert!(
                !has_wire_prefix(&before, &changed_environment, dialect),
                "Chat: its system instruction is a leading messages element and must change"
            );
        } else {
            assert_ne!(
                wire_environment(&before, dialect),
                wire_environment(&changed_environment, dialect),
                "{dialect:?}: changing the system/instructions slot is not a stable environment"
            );
        }

        let flattened = ModelRequest {
            session_id: old.session_id.clone(),
            atoms: vec![ContextAtom::user(
                entry_id("flattened"),
                "user: Inspect the repository.\nassistant: The repository is ready.\n\nSUMMARY: \
                 Write a compact factual checkpoint."
                    .into(),
            )],
        };
        let flattened_wire = encode(dialect, &flattened, "Stable environment.", &tools);
        assert!(!has_wire_prefix(&before, &flattened_wire, dialect));

        assert_ne!(
            semantic_environment(dialect, "Stable environment.", &tools),
            semantic_environment(dialect, "Stable environment.", &[]),
            "{dialect:?}: omitting read_file must change the actual request environment"
        );
        let changed_tools = changed_tools();
        assert_ne!(
            semantic_environment(dialect, "Stable environment.", &tools),
            semantic_environment(dialect, "Stable environment.", &changed_tools),
            "{dialect:?}: changing read_file's schema must change the actual request environment"
        );
        let missing_tools_wire = encode(dialect, &old, "Stable environment.", &[]);
        let changed_tools_wire = encode(dialect, &old, "Stable environment.", &changed_tools);
        assert_ne!(
            wire_environment(&before, dialect),
            wire_environment(&missing_tools_wire, dialect),
            "{dialect:?}: removing read_file must change emitted non-context request fields"
        );
        assert_ne!(
            wire_environment(&before, dialect),
            wire_environment(&changed_tools_wire, dialect),
            "{dialect:?}: changing read_file must change emitted non-context request fields"
        );
    }
}

fn assert_appended_summary_preserves_prefix(
    dialect: Dialect,
    old: ModelRequest,
    tools: &[FunctionTool],
) {
    let before = encode(dialect, &old, "Stable environment.", tools);
    let mut extended = old;
    extended
        .atoms
        .push(ContextAtom::user(entry_id("summary"), SUMMARY.into()));
    let after = encode(dialect, &extended, "Stable environment.", tools);

    assert_eq!(
        wire_environment(&before, dialect),
        wire_environment(&after, dialect),
        "{dialect:?}: appending a summary must not change request environment"
    );
    assert_summary_appended(&before, &after, dialect);
}

fn encode(
    dialect: Dialect,
    request: &ModelRequest,
    instructions: &str,
    tools: &[FunctionTool],
) -> Value {
    let registry = registry(dialect, instructions);
    encode_request(registry.active_model(), request, tools, Some(128))
        .expect("the synthetic semantic request is representable")
}

fn semantic_environment(
    dialect: Dialect,
    instructions: &str,
    tools: &[FunctionTool],
) -> RequestEnvironment {
    let registry = registry(dialect, instructions);
    request_environment(registry.active_model(), tools, Some(128))
}

fn registry(dialect: Dialect, instructions: &str) -> ModelRegistry {
    ModelRegistry::parse(&format!(
        r#"
active_model = {{ provider = "fixture", model = "fixture" }}
[providers.fixture]
base_url = "http://127.0.0.1:1/v1"
api_key_env = "UNUSED_FIXTURE_KEY"
api = "{}"
[providers.fixture.models.fixture]
id = "fixture-model"
instructions = "{}"
prompt_cache = "automatic"
context_window_tokens = 8192
max_output_tokens = 1024
output_reserve_tokens = 512
"#,
        dialect.api(),
        instructions
    ))
    .expect("synthetic model registry is valid")
}

fn tools() -> Vec<FunctionTool> {
    vec![
        FunctionTool::new(
            "read_file",
            "Read a bounded UTF-8 file from the workspace.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["path"],
                "properties": {"path": {"type": "string"}},
            }),
        )
        .expect("fixture read_file definition"),
    ]
}

fn changed_tools() -> Vec<FunctionTool> {
    vec![
        FunctionTool::new(
            "read_file",
            "Read a bounded UTF-8 file from the workspace.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["path", "offset"],
                "properties": {
                    "path": {"type": "string"},
                    "offset": {"type": "integer"},
                },
            }),
        )
        .expect("changed fixture read_file definition"),
    ]
}

fn has_wire_prefix(before: &Value, after: &Value, dialect: Dialect) -> bool {
    let before = before[dialect.input_key()]
        .as_array()
        .expect("codec supplies its input array");
    let after = after[dialect.input_key()]
        .as_array()
        .expect("codec supplies its input array");
    after.starts_with(before)
}

/// The emitted request body with exactly its conversation array removed. This is a wire-shape
/// comparison, distinct from the provider's request-environment fingerprint and cache behavior.
fn wire_environment(body: &Value, dialect: Dialect) -> Value {
    let mut environment = body.clone();
    environment
        .as_object_mut()
        .expect("codec emits an object body")
        .remove(dialect.input_key())
        .expect("codec emits its conversation array");
    environment
}

fn assert_summary_appended(before: &Value, after: &Value, dialect: Dialect) {
    let old = before[dialect.input_key()]
        .as_array()
        .expect("codec supplies its input array");
    let next = after[dialect.input_key()]
        .as_array()
        .expect("codec supplies its input array");
    assert_eq!(
        next.len(),
        old.len() + 1,
        "{dialect:?}: summary must add exactly one wire input element"
    );
    assert_eq!(
        &next[..old.len()],
        old,
        "{dialect:?}: prior wire input must remain an element prefix"
    );
    let summary = &next[old.len()];
    let text = match dialect {
        Dialect::Responses | Dialect::Chat => summary["content"].as_str(),
        Dialect::Messages => summary["content"]
            .as_array()
            .and_then(|content| content.first())
            .and_then(|part| part["text"].as_str()),
        Dialect::Gemini => summary["parts"]
            .as_array()
            .and_then(|parts| parts.first())
            .and_then(|part| part["text"].as_str()),
    };
    assert_eq!(
        text,
        Some(SUMMARY),
        "{dialect:?}: the one appended element must carry the stable summary instruction"
    );
}

fn assert_replay_shape(body: &Value, dialect: Dialect) {
    let input = body[dialect.input_key()]
        .as_array()
        .expect("codec supplies its input array");
    let retained = match dialect {
        Dialect::Responses => input.iter().any(|item| {
            item["type"] == "reasoning" && item["encrypted_content"] == "opaque-capsule"
        }),
        Dialect::Chat => input
            .iter()
            .any(|item| item["reasoning"] == "recognized reasoning"),
        Dialect::Messages => input.iter().any(|item| {
            item["content"].as_array().is_some_and(|content| {
                content.iter().any(|part| {
                    part["type"] == "thinking"
                        && part["thinking"] == "signed thinking"
                        && part["signature"] == "signature"
                })
            })
        }),
        Dialect::Gemini => input.iter().any(|item| {
            item["parts"].as_array().is_some_and(|parts| {
                parts.iter().any(|part| {
                    part["text"] == "signed thought" && part["thoughtSignature"] == "signature"
                })
            })
        }),
    };
    assert!(
        retained,
        "{dialect:?}: dialect-specific replay must reach baseline wire input"
    );
}

fn journal_request(output: AssistantOutput) -> ModelRequest {
    let mut journal = ConversationJournal::new(session_id());
    append(
        &mut journal,
        1,
        JournalEntryPayload::AgentCreated {
            agent_id: agent_id(),
            label: "Fixture".into(),
            status: AgentStatus::Idle,
        },
    );
    append(
        &mut journal,
        2,
        JournalEntryPayload::TurnStarted {
            agent_id: agent_id(),
            item_id: transcript_id("user-item"),
            turn_id: turn_id(),
            text: "Inspect the repository.".into(),
            accepted_at: plexmaton_agent::UnixMillis::EPOCH,
            opened_at: plexmaton_agent::UnixMillis::EPOCH,
        },
    );
    append(
        &mut journal,
        3,
        JournalEntryPayload::AssistantOutput {
            agent_id: agent_id(),
            step_id: serde_json::from_str(r#"{"turn_id":"turn-1","index":1}"#)
                .expect("validated public model-step wire form"),
            output: output.clone(),
        },
    );
    if output.tool_calls().next().is_some() {
        for call in output.tool_calls() {
            let ordinal = 10 + journal.next_sequence().get();
            append(
                &mut journal,
                ordinal,
                JournalEntryPayload::ToolCallRequested {
                    agent_id: agent_id(),
                    call_id: call.call_id.clone(),
                    presentation: ToolPresentation::default(),
                },
            );
            let ordinal = 10 + journal.next_sequence().get();
            append(
                &mut journal,
                ordinal,
                JournalEntryPayload::ToolCallChanged {
                    agent_id: agent_id(),
                    call_id: call.call_id.clone(),
                    item_revision: 1,
                    status: ToolCallStatus::Running,
                    presentation: ToolPresentation::default(),
                    outcome: None,
                },
            );
            let ordinal = 10 + journal.next_sequence().get();
            append(
                &mut journal,
                ordinal,
                JournalEntryPayload::ToolCallChanged {
                    agent_id: agent_id(),
                    call_id: call.call_id.clone(),
                    item_revision: 2,
                    status: ToolCallStatus::Succeeded,
                    presentation: ToolPresentation::default(),
                    outcome: Some(ToolOutcome::Succeeded {
                        output: "tool result".into(),
                    }),
                },
            );
        }
    }
    journal
        .project(&head())
        .expect("synthetic journal projects through the public JRN-5 path")
        .into_request()
}

fn append(journal: &mut ConversationJournal, ordinal: u64, payload: JournalEntryPayload) {
    let head = head();
    let parent_id = journal.head_target(&head).expect("main exists").cloned();
    let revision = journal.head_revision(&head).expect("main revision exists");
    journal
        .apply(JournalRecord::AppendEntry {
            sequence: journal.next_sequence(),
            record_id: JournalRecordId::new(format!("record-{ordinal}")).expect("fixture id"),
            head,
            expected_head_revision: revision,
            entry: Box::new(ConversationEntry {
                id: entry_id(&format!("entry-{ordinal}")),
                parent_id,
                payload,
            }),
        })
        .expect("fixture append follows journal preconditions");
}

fn assistant_text(item: &str, text: &str) -> AssistantOutput {
    AssistantOutput::new(
        vec![AssistantBlock::Text {
            item_id: transcript_id(item),
            text: text.into(),
        }],
        None,
    )
    .expect("fixture output")
}

fn tool_output(dialect: Dialect) -> AssistantOutput {
    let replay = (dialect == Dialect::Gemini).then(|| {
        AssistantReplay::from_positioned([(
            0,
            provider_replay(
                dialect,
                r#"{"type":"function_call","upstream_id":null,"signature":null}"#,
            ),
        )])
        .expect("Gemini call replay validates")
        .expect("one attachment produces replay")
    });
    AssistantOutput::new(
        vec![AssistantBlock::ToolCall {
            item_id: transcript_id("call-item"),
            call: ToolCall {
                call_id: ToolCallId::new("call-1").expect("fixture id"),
                name: "read_file".into(),
                arguments: r#"{"path":"README.md"}"#.into(),
            },
        }],
        replay,
    )
    .expect("fixture tool output")
}

fn replayed_output(dialect: Dialect) -> AssistantOutput {
    let (text, payload) = match dialect {
        Dialect::Responses => (
            "signed reasoning",
            r#"{"type":"reasoning","encrypted_content":"opaque-capsule"}"#,
        ),
        Dialect::Chat => (
            "recognized reasoning",
            r#"{"type":"reasoning","field":"reasoning"}"#,
        ),
        Dialect::Messages => (
            "signed thinking",
            r#"{"type":"thinking","thinking":"signed thinking","signature":"signature"}"#,
        ),
        Dialect::Gemini => (
            "signed thought",
            r#"{"type":"text","thought":true,"text_present":true,"signature":"signature"}"#,
        ),
    };
    let replay = provider_replay(dialect, payload);
    let replay = AssistantReplay::from_positioned([(0, replay)])
        .expect("replay attaches to its assistant block");
    AssistantOutput::new(
        vec![AssistantBlock::Reasoning {
            item_id: transcript_id("replay-item"),
            text: text.into(),
        }],
        replay,
    )
    .expect("fixture replay output")
}

fn provider_replay(dialect: Dialect, payload: &str) -> ProviderReplay {
    let registry = ModelRegistry::parse(&format!(
        r#"
active_model = {{ provider = "fixture", model = "fixture" }}
[providers.fixture]
base_url = "http://127.0.0.1:1/v1"
api_key_env = "UNUSED_FIXTURE_KEY"
api = "{}"
[providers.fixture.models.fixture]
id = "fixture-model"
context_window_tokens = 8192
max_output_tokens = 1024
output_reserve_tokens = 512
"#,
        dialect.api()
    ))
    .expect("synthetic model registry is valid");
    ProviderReplay::new(
        registry.active_model().replay_compatibility(),
        payload.to_owned(),
    )
    .expect("bounded compatible replay")
}

fn session_id() -> ConversationId {
    ConversationId::new("codec-spike").expect("fixture id")
}

fn agent_id() -> AgentId {
    AgentId::new("fixture-agent").expect("fixture id")
}

fn turn_id() -> TurnId {
    TurnId::new("turn-1").expect("fixture id")
}

fn head() -> HeadName {
    HeadName::new("main").expect("fixture id")
}

fn entry_id(value: &str) -> ConversationEntryId {
    ConversationEntryId::new(value).expect("fixture id")
}

fn transcript_id(value: &str) -> TranscriptItemId {
    TranscriptItemId::new(value).expect("fixture id")
}
