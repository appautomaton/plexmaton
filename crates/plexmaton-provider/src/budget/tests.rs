use super::*;
use crate::ModelRegistry;
use plexmaton_agent::{
    Agent, AssistantBlock, AssistantOutput, AssistantReplay, DispatchedRequestTiming,
    ElapsedMillis, Input, ModelEvent, ModelOutputPosition, ProviderReplay, RequestAttemptTerminal,
    RequestAttemptTerminalState, RequestCost, RequestDispatchedOutcome, SkillActivation,
    SkillSource, StopReason, ToolBatch, ToolBatchResult, ToolCall, ToolOutcome, UnixMillis,
};
use plexmaton_core::{
    AgentId, ConversationEntryId, TokenCounts, TokenUsage, ToolCallId, TranscriptItemId,
};
use serde_json::json;

mod instructions;

fn model(api: &str) -> ResolvedModel {
    ModelRegistry::parse(&format!(
        r#"
active_model = {{ provider = "fixture", model = "test" }}
[providers.fixture]
base_url = "http://127.0.0.1:1/v1"
api_key_env = "UNUSED_FIXTURE_KEY"
api = "{api}"
[providers.fixture.models.test]
id = "fixture-model"
reasoning_effort = "high"
context_window_tokens = 10000
max_output_tokens = 2000
output_reserve_tokens = 1000
"#
    ))
    .expect("fixture config")
    .active_model()
    .clone()
}
fn head() -> HeadName {
    HeadName::new("main").expect("head")
}
fn open() -> Agent {
    let mut agent = Agent::new(AgentId::new("agent").expect("id"));
    agent.announce("Agent");
    agent.handle_at(
        Input::Submitted {
            text: "private user text".to_owned(),
        },
        UnixMillis::new(1),
    );
    agent
}
fn tool(description: &str) -> FunctionTool {
    FunctionTool::new(
        "read_file",
        description,
        json!({"type":"object","properties":{"path":{"type":"string"}}}),
    )
    .expect("tool")
}

fn skill_atom(instructions: &str) -> ContextAtom {
    let activation = SkillActivation::new(
        "review".to_owned(),
        SkillSource::ProjectNative,
        "/workspace/.plexmaton/skills/review/SKILL.md".to_owned(),
        "b".repeat(64),
        instructions.to_owned(),
    )
    .expect("skill activation");
    ContextAtom::skill(
        ConversationEntryId::new("entry-skill").expect("entry id"),
        activation,
    )
}

/// BUD-1/BUD-2/BUD-3: real codec inputs are budgeted, but only identities/counts reach diagnostics.
#[test]
fn bud_1_both_codecs_produce_redacted_deterministic_ledgers_without_writes() {
    for api in ["openai_responses", "openai_chat_completions"] {
        let model = model(api);
        let agent = open();
        let before = agent.journal().clone();
        let ledger = budget_ledger(
            agent.journal(),
            &head(),
            &model,
            &[tool("private schema prose")],
        )
        .expect("ledger");
        assert!(ledger.anchor.is_none());
        assert!(ledger.environment_estimate.tokens > 0);
        assert!(ledger.input_tokens > ledger.environment_estimate.tokens);
        assert_eq!(ledger.limits.input_capacity(), 9000);
        assert_eq!(ledger.limits.soft_input_limit(), 7200);
        assert_eq!(
            ledger,
            budget_ledger(
                agent.journal(),
                &head(),
                &model,
                &[tool("private schema prose")]
            )
            .expect("again")
        );
        assert_eq!(agent.journal(), &before);
        for rendered in [
            format!("{ledger:?}"),
            serde_json::to_string(&ledger).expect("snapshot"),
        ] {
            assert!(!rendered.contains("private user text"));
            assert!(!rendered.contains("private schema prose"));
        }
        let bigger = budget_ledger(
            agent.journal(),
            &head(),
            &model,
            &[tool(&"long schema".repeat(100))],
        )
        .expect("ledger");
        assert!(bigger.environment_estimate.tokens > ledger.environment_estimate.tokens);
        assert_ne!(bigger.environment, ledger.environment);
    }
}

/// BUD-2: identical measured input replaces environment/prefix estimates; changed tools invalidate it.
#[test]
fn bud_2_codec_environment_controls_measurement_reuse() {
    let model = model("openai_responses");
    let tools = [tool("read")];
    let mut agent = open();
    let step = agent.active_model_step().expect("step");
    let (id, _) = agent
        .authorize_request_attempt(
            step.clone(),
            request_environment(&model, &tools, Some(model.max_output_tokens())),
            UnixMillis::new(1),
        )
        .expect("authorize");
    let terminal = RequestAttemptTerminal::new(
        id,
        RequestAttemptTerminalState::Dispatched {
            timing: DispatchedRequestTiming::new(
                UnixMillis::new(2),
                None,
                None,
                ElapsedMillis::new(3),
            )
            .expect("timing"),
            outcome: RequestDispatchedOutcome::Completed {
                stop_reason: StopReason::EndOfTurn,
            },
            usage: TokenUsage::Complete(TokenCounts {
                input: 100,
                output: 30,
                total: 130,
                cached_input: Some(90),
                cache_write_input: Some(0),
                reasoning_output: Some(0),
            }),
            cost: RequestCost::Unavailable,
        },
    )
    .expect("terminal");
    agent
        .finish_request_attempt(&terminal)
        .expect("record terminal");
    let measured = budget_ledger(agent.journal(), &head(), &model, &tools).expect("ledger");
    assert_eq!(measured.input_tokens, 100);
    assert_eq!(measured.estimated_remainder.tokens, 0);
    agent.handle_at(
        Input::Streamed {
            step_id: step.clone(),
            event: ModelEvent::TextDelta {
                position: ModelOutputPosition::new(0, 0),
                delta: "answer".to_owned(),
            },
        },
        UnixMillis::new(3),
    );
    agent.handle_at(
        Input::Streamed {
            step_id: step,
            event: ModelEvent::Stopped(StopReason::EndOfTurn),
        },
        UnixMillis::new(4),
    );
    let extended = budget_ledger(agent.journal(), &head(), &model, &tools).expect("ledger");
    assert_eq!(
        extended.input_tokens,
        100 + extended.atoms[1].estimate.tokens
    );
    let changed = budget_ledger(
        agent.journal(),
        &head(),
        &model,
        &[tool("different instructions")],
    )
    .expect("ledger");
    assert!(changed.anchor.is_none());
}

fn replay_atom(model: &ResolvedModel) -> ContextAtom {
    let replay = ProviderReplay::new(
        model.replay_compatibility(),
        json!({"type":"reasoning","encrypted_content":"secret-ciphertext","summary":[]})
            .to_string(),
    )
    .expect("replay");
    let output = AssistantOutput::new(
        vec![AssistantBlock::Reasoning {
            item_id: TranscriptItemId::new("reason").expect("id"),
            text: String::new(),
        }],
        AssistantReplay::from_positioned([(0, replay)]).expect("attachments"),
    )
    .expect("output");
    ContextAtom::assistant(
        ConversationEntryId::new("entry-replay").expect("id"),
        output,
    )
    .expect("atom")
}

/// BUD-3/PRV-3: opaque bytes have explicit heuristic provenance; incompatible replay is still refused.
#[test]
fn bud_3_opaque_replay_is_flagged_and_incompatibility_never_becomes_a_zero_estimate() {
    let responses = model("openai_responses");
    let atom = replay_atom(&responses);
    let before = atom.clone();
    let estimated = estimate_atom(&responses, &atom).expect("estimate");
    assert!(estimated.tokens > 0);
    assert!(estimated.opaque_replay_bytes > 0);
    assert!(!format!("{estimated:?}").contains("secret-ciphertext"));
    assert_eq!(atom, before);
    assert!(matches!(
        estimate_atom(&model("openai_chat_completions"), &atom),
        Err(ContextBudgetError::Encoding(
            EncodeError::IncompatibleReplay { .. }
        ))
    ));
}

/// BUD-3/BUD-4: a maximal two-result batch remains one item, with both outputs included in its cost.
#[test]
fn bud_3_maximal_tool_results_are_estimated_as_one_indivisible_atom() {
    let calls: Vec<_> = (0..2)
        .map(|index| ToolCall {
            call_id: ToolCallId::new(format!("call-{index}")).expect("id"),
            name: "read_file".to_owned(),
            arguments: "{}".to_owned(),
        })
        .collect();
    let output = AssistantOutput::new(
        calls
            .iter()
            .enumerate()
            .map(|(index, call)| AssistantBlock::ToolCall {
                item_id: TranscriptItemId::new(format!("item-{index}")).expect("id"),
                call: call.clone(),
            })
            .collect(),
        None,
    )
    .expect("output");
    let batch = ToolBatch::new(
        output,
        calls
            .iter()
            .map(|call| {
                ToolBatchResult::new(
                    call.call_id.clone(),
                    ToolOutcome::Succeeded {
                        output: "x".repeat(plexmaton_agent::MAX_TOOL_PRESENTATION_TEXT_BYTES),
                    },
                )
            })
            .collect(),
    )
    .expect("batch");
    let atom = ContextAtom::tool_batch(vec![ConversationEntryId::new("batch").expect("id")], batch)
        .expect("atom");
    for api in ["openai_responses", "openai_chat_completions"] {
        let estimate = estimate_atom(&model(api), &atom).expect("estimate");
        assert!(
            estimate.tokens >= (2 * plexmaton_agent::MAX_TOOL_PRESENTATION_TEXT_BYTES / 4) as u64
        );
        assert_eq!(estimate.opaque_replay_bytes, 0);
    }
}

/// BUD-3: byte rounding includes Unicode, escaped text and framing deterministically.
#[test]
fn bud_3_estimator_counts_utf8_wire_bytes_without_allocating_another_request_string() {
    for text in ["", "hello", "你好🦀", "\"\\\n"] {
        let encoded = json!({"role":"user","content":text});
        assert_eq!(
            estimate(TokenEstimator::default(), &encoded).expect("estimate"),
            (serde_json::to_vec(&encoded).expect("encode").len() as u64).div_ceil(4)
        );
    }
}

/// SKL-6/BUD-3: each dialect estimates the complete wire representation of one skill atom.
#[test]
fn skl_6_every_codec_budgets_exact_skill_wire_content() {
    let atom = skill_atom("Read exactly: 🦀 \\\"quoted\\\"\nsecond line");
    for api in [
        "openai_responses",
        "openai_chat_completions",
        "anthropic_messages",
        "google_generate_content",
    ] {
        let model = model(api);
        let encoded = match model.api() {
            ModelApi::OpenaiResponses => crate::responses::encode_atom(&model, &atom),
            ModelApi::OpenaiChatCompletions => crate::chat::encode_atom(&model, &atom),
            ModelApi::AnthropicMessages => crate::messages::encode_atom(&model, &atom),
            ModelApi::GoogleGenerateContent => crate::gemini::encode_atom(&model, &atom),
        }
        .unwrap_or_else(|error| panic!("encode {api}: {error}"));
        let actual =
            estimate_atom(&model, &atom).unwrap_or_else(|error| panic!("estimate {api}: {error}"));
        assert_eq!(
            actual.tokens,
            estimate(model.token_estimator(), &encoded).expect("wire estimate"),
            "{api}"
        );
        assert_eq!(actual.opaque_replay_bytes, 0, "{api}");
    }
}

/// BUD-1/JRN-5: an unfinished tool batch is unavailable, not a deceptively smaller fitted context.
#[test]
fn bud_1_incomplete_tool_batch_cannot_produce_a_fit_snapshot() {
    let mut agent = open();
    let step = agent.active_model_step().expect("step");
    agent.handle_at(
        Input::Streamed {
            step_id: step.clone(),
            event: ModelEvent::Called {
                position: ModelOutputPosition::new(0, 0),
                call: ToolCall {
                    call_id: ToolCallId::new("pending-call").expect("id"),
                    name: "read_file".to_owned(),
                    arguments: "{}".to_owned(),
                },
            },
        },
        UnixMillis::new(2),
    );
    agent.handle_at(
        Input::Streamed {
            step_id: step,
            event: ModelEvent::Stopped(StopReason::ToolCalls),
        },
        UnixMillis::new(3),
    );
    assert!(matches!(
        budget_ledger(agent.journal(), &head(), &model("openai_responses"), &[]),
        Err(ContextBudgetError::IncompleteToolBatch)
    ));
}
