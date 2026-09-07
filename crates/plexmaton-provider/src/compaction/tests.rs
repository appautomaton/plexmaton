use super::*;
use crate::{
    FunctionTool, ModelRegistry, budgeted_context, encode_request, estimate_request,
    request_environment,
};
use plexmaton_agent::{
    Agent, AssistantBlock, AssistantOutput, ContextAtomValue, Input, ModelEvent,
    ModelOutputPosition, RequestAttemptTerminal, RequestAttemptTerminalState, RequestCost,
    RequestDispatchedOutcome, StopReason, UnixMillis,
};
use plexmaton_core::{AgentId, HeadName, TokenCounts, TokenUsage, TranscriptItemId};
use serde_json::json;

mod instructions;
mod skills;

fn model(api: &str) -> ResolvedModel {
    model_with_limits(api, 12_000, 2_000, 1_000)
}

fn model_with_limits(
    api: &str,
    context_window_tokens: u32,
    max_output_tokens: u32,
    output_reserve_tokens: u32,
) -> ResolvedModel {
    model_with_tail(
        api,
        context_window_tokens,
        max_output_tokens,
        output_reserve_tokens,
        20_000,
    )
}

fn model_with_tail(
    api: &str,
    context_window_tokens: u32,
    max_output_tokens: u32,
    output_reserve_tokens: u32,
    keep_recent_tokens: u32,
) -> ResolvedModel {
    ModelRegistry::parse(&format!(
        r#"
active_model = {{ provider = "fixture", model = "test" }}
[providers.fixture]
base_url = "http://127.0.0.1:1/v1"
api_key_env = "UNUSED_FIXTURE_KEY"
api = "{api}"
[providers.fixture.models.test]
id = "fixture-model"
context_window_tokens = {context_window_tokens}
max_output_tokens = {max_output_tokens}
output_reserve_tokens = {output_reserve_tokens}
compaction_keep_recent_tokens = {keep_recent_tokens}
"#
    ))
    .expect("fixture config")
    .active_model()
    .clone()
}

fn tools() -> [FunctionTool; 1] {
    [FunctionTool::new(
        "read_file",
        "Read a workspace file.",
        json!({"type":"object","properties":{"path":{"type":"string"}}}),
    )
    .expect("tool")]
}

fn history() -> Agent {
    let mut agent = Agent::new(AgentId::new("agent").expect("id"));
    agent.announce("Agent");
    for (index, text) in ["first request", "second request", "third request"]
        .into_iter()
        .enumerate()
    {
        agent.handle_at(
            Input::Submitted {
                text: text.to_owned(),
            },
            UnixMillis::new(index as u64 + 1),
        );
        let step = agent.active_model_step().expect("step");
        agent.handle_at(
            Input::Streamed {
                step_id: step.clone(),
                event: ModelEvent::TextDelta {
                    position: ModelOutputPosition::new(0, 0),
                    delta: format!("answer {index}"),
                },
            },
            UnixMillis::new(index as u64 + 10),
        );
        agent.handle_at(
            Input::Streamed {
                step_id: step,
                event: ModelEvent::Stopped(StopReason::EndOfTurn),
            },
            UnixMillis::new(index as u64 + 20),
        );
    }
    agent
}

fn head() -> HeadName {
    HeadName::new("main").expect("head")
}

fn history_with_native_output(model: &ResolvedModel, api: &str) -> Agent {
    let wire = match api {
        "openai_responses" => include_str!("../../tests/fixtures/responses_final_answer.sse"),
        "openai_chat_completions" => include_str!("../../tests/fixtures/chat_final_answer.sse"),
        "anthropic_messages" => include_str!("../../tests/fixtures/messages_final_answer.sse"),
        "google_generate_content" => include_str!("../../tests/fixtures/gemini_final_answer.sse"),
        _ => unreachable!("fixture api"),
    };
    let mut agent = history();
    agent.handle_at(
        Input::Submitted {
            text: "Read the project name.".into(),
        },
        UnixMillis::EPOCH,
    );
    let mut codec = crate::ProviderCodec::new(
        &plexmaton_agent::RequestAttemptId::new("native-history").expect("attempt"),
        model,
        crate::DecodeLimits::production(),
    );
    for frame in wire.split("\n\n") {
        let Some(data) = frame.lines().find_map(|line| line.strip_prefix("data: ")) else {
            continue;
        };
        let event_name = frame
            .lines()
            .find_map(|line| line.strip_prefix("event: "))
            .unwrap_or("message");
        for event in codec
            .push_sse(event_name, data)
            .expect("native fixture event")
        {
            if matches!(event, ModelEvent::Usage(_)) {
                continue;
            }
            let step_id = agent.active_model_step().expect("fixture step");
            let reaction = agent.handle_at(Input::Streamed { step_id, event }, UnixMillis::EPOCH);
            assert!(reaction.undelivered_model.is_empty());
        }
    }
    codec.finish().expect("complete fixture");
    assert!(!agent.is_running());
    agent
}

fn long_history(turns: usize, answer_bytes: usize) -> Agent {
    let mut agent = Agent::new(AgentId::new("long-history").expect("agent"));
    agent.announce("Agent");
    for index in 0..turns {
        agent.handle_at(
            Input::Submitted {
                text: format!("Question {index}"),
            },
            UnixMillis::EPOCH,
        );
        let step_id = agent.active_model_step().expect("active step");
        agent.handle_at(
            Input::Streamed {
                step_id: step_id.clone(),
                event: ModelEvent::TextDelta {
                    position: ModelOutputPosition::new(0, 0),
                    delta: "x".repeat(answer_bytes),
                },
            },
            UnixMillis::EPOCH,
        );
        agent.handle_at(
            Input::Streamed {
                step_id,
                event: ModelEvent::Stopped(StopReason::EndOfTurn),
            },
            UnixMillis::EPOCH,
        );
    }
    agent.handle_at(
        Input::Submitted {
            text: "Keep the current request exactly.".into(),
        },
        UnixMillis::EPOCH,
    );
    agent
}

fn assert_tail_budget(
    agent: &Agent,
    model: &ResolvedModel,
    expected_limit: u64,
) -> PreparedCompaction {
    let tools = tools();
    let prepared = plan_compaction(
        agent.journal(),
        &head(),
        model,
        &tools,
        CompactionId::new("retained-tail-plan").expect("id"),
    )
    .expect("plan");
    let basis = budgeted_context(agent.journal(), &head(), model, &tools).expect("basis");
    let first = prepared
        .plan()
        .cut()
        .first_retained()
        .expect("current user retained");
    let index = basis
        .request
        .atoms
        .iter()
        .position(|atom| atom.source_entries().first() == Some(first))
        .expect("whole retained atom");
    let retained_tokens: u64 = basis.ledger.atoms[index..]
        .iter()
        .map(|atom| atom.estimate.tokens)
        .sum();
    assert!(
        retained_tokens <= expected_limit,
        "retained {retained_tokens}, limit {expected_limit}"
    );
    assert!(index > 0);
    assert!(
        retained_tokens + basis.ledger.atoms[index - 1].estimate.tokens > expected_limit,
        "the next whole atom would cross the tail target"
    );
    let output = AssistantOutput::new(
        vec![AssistantBlock::Text {
            item_id: TranscriptItemId::new("tail-summary").expect("item"),
            text: "Earlier decisions and completed work.".into(),
        }],
        None,
    )
    .expect("output");
    let replacement =
        validate_compaction_output(&prepared, model, &tools, &output).expect("replacement fits");
    assert_eq!(
        &replacement.atoms[1..],
        &basis.request.atoms[index..],
        "retained context is exact"
    );
    prepared
}

/// CPL-2/CPL-3: the configured tail target changes the replacement cut, never summarizer history.
#[test]
fn cpl_3_configured_recent_tail_changes_only_the_checkpoint_cut() {
    let agent = long_history(16, 8_000);
    let before = agent.journal().clone();
    let default = model_with_tail("openai_responses", 128_000, 8_000, 4_000, 20_000);
    let smaller = model_with_tail("openai_responses", 128_000, 8_000, 4_000, 4_000);
    let default_plan = assert_tail_budget(&agent, &default, 20_000);
    let smaller_plan = assert_tail_budget(&agent, &smaller, 4_000);
    assert_ne!(default_plan.plan().cut(), smaller_plan.plan().cut());
    assert_eq!(default_plan.input(), smaller_plan.input());
    assert_eq!(
        default_plan.plan().environment(),
        smaller_plan.plan().environment()
    );
    assert_eq!(agent.journal(), &before);
}

/// CPL-3: a nominal 20k target stays within the available share of a smaller context window.
#[test]
fn cpl_3_recent_tail_target_is_capped_by_available_input() {
    let agent = long_history(4, 4_000);
    let model = model("openai_responses");
    let basis = budgeted_context(agent.journal(), &head(), &model, &tools()).expect("basis");
    let available = input_capacity(&model) - basis.ledger.environment_estimate.tokens;
    assert!(available / 4 < u64::from(model.compaction_keep_recent_tokens()));
    let _prepared = assert_tail_budget(&agent, &model, available / 4);
}

/// CPL-1/CPL-2/AGI-4: one instruction extends exact history and leaves the workspace prefix intact.
#[test]
fn cpl_2_compaction_appends_only_the_instruction_across_all_dialects() {
    for api in [
        "openai_responses",
        "openai_chat_completions",
        "anthropic_messages",
        "google_generate_content",
    ] {
        let model = model(api)
            .with_workspace_instructions("AGENTS.md fixture rules".into())
            .expect("workspace snapshot");
        let tools = tools();
        let agent = history_with_native_output(&model, api);
        let before = agent.journal().clone();
        let prepared = plan_compaction(
            agent.journal(),
            &head(),
            &model,
            &tools,
            CompactionId::new(format!("compact-{api}")).expect("id"),
        )
        .expect("plan");
        assert_eq!(agent.journal(), &before);
        let base = before
            .project(&head())
            .expect("source projection")
            .into_request();
        let input = prepared.input().request();
        assert_eq!(input.session_id, base.session_id);
        assert_eq!(input.atoms.len(), base.atoms.len() + 1);
        assert_eq!(&input.atoms[..base.atoms.len()], base.atoms);
        let mut source_body =
            encode_request(&model, &base, &tools, Some(model.max_output_tokens()))
                .expect("source encoding");
        let mut summary_body =
            encode_request(&model, input, &tools, Some(model.max_output_tokens()))
                .expect("summary encoding");
        assert_eq!(last_text(&summary_body, api), Some(SUMMARY_INSTRUCTION));
        let key = match api {
            "openai_responses" => "input",
            "openai_chat_completions" | "anthropic_messages" => "messages",
            "google_generate_content" => "contents",
            _ => unreachable!("fixture api"),
        };
        let old = source_body
            .as_object_mut()
            .expect("request object")
            .remove(key)
            .expect("source input");
        let extended = summary_body
            .as_object_mut()
            .expect("request object")
            .remove(key)
            .expect("summary input");
        let old = old.as_array().expect("source array");
        let extended = extended.as_array().expect("summary array");
        assert_eq!(extended.len(), old.len() + 1);
        assert_eq!(
            serde_json::to_vec(&extended[..old.len()]).expect("prefix bytes"),
            serde_json::to_vec(old).expect("original bytes")
        );
        assert_eq!(
            summary_body, source_body,
            "{api}: tools, instructions, model and output settings stay unchanged"
        );
    }
}

/// CPL-3: replacement preview refuses an over-budget summary before any checkpoint can publish.
#[test]
fn cpl_3_replacement_preview_rejects_oversized_summary_without_mutating_source() {
    let model = model("openai_responses");
    let tools = tools();
    let agent = history();
    let before = agent.journal().clone();
    let prepared = plan_compaction(
        agent.journal(),
        &head(),
        &model,
        &tools,
        CompactionId::new("compact-preview").expect("id"),
    )
    .expect("plan");
    let output = AssistantOutput::new(
        vec![AssistantBlock::Text {
            item_id: TranscriptItemId::new("summary-output").expect("id"),
            text: "x".repeat(prepared.plan().max_summary_bytes() + 1),
        }],
        None,
    )
    .expect("bounded output");
    assert!(matches!(
        validate_compaction_output(&prepared, &model, &tools, &output),
        Err(CompactionPreparationError::OutputTooLarge)
    ));
    assert_eq!(agent.journal(), &before);
}

/// CPL-2/BUD-2/BUD-3: measured overflow refuses compaction; a cheap heuristic cannot justify rewriting history.
#[test]
fn cpl_2_measured_overflow_refuses_compaction_without_rewriting_history() {
    let model = model("openai_responses");
    let tools = tools();
    let mut agent = history();
    agent.handle_at(
        Input::Submitted {
            text: "current request".to_owned(),
        },
        UnixMillis::new(100),
    );
    let step = agent.active_model_step().expect("active step");
    let (attempt, _) = agent
        .authorize_request_attempt(
            step,
            request_environment(&model, &tools, Some(model.max_output_tokens())),
            UnixMillis::new(101),
        )
        .expect("authorization");
    agent
        .finish_request_attempt(
            &RequestAttemptTerminal::new(
                attempt,
                RequestAttemptTerminalState::Dispatched {
                    timing: plexmaton_agent::DispatchedRequestTiming::new(
                        UnixMillis::new(102),
                        None,
                        None,
                        plexmaton_agent::ElapsedMillis::new(1),
                    )
                    .expect("timing"),
                    outcome: RequestDispatchedOutcome::Completed {
                        stop_reason: StopReason::EndOfTurn,
                    },
                    usage: TokenUsage::Complete(TokenCounts {
                        input: 50_000,
                        output: 0,
                        total: 50_000,
                        cached_input: Some(0),
                        cache_write_input: Some(0),
                        reasoning_output: Some(0),
                    }),
                    cost: RequestCost::Unavailable,
                },
            )
            .expect("terminal"),
        )
        .expect("finish measured request");
    let before = agent.journal().clone();
    assert!(matches!(
        plan_compaction(
            agent.journal(),
            &head(),
            &model,
            &tools,
            CompactionId::new("anchored-compaction").expect("id")
        ),
        Err(CompactionPreparationError::NoFittingInput)
    ));
    assert_eq!(agent.journal(), &before);
    let basis = budgeted_context(agent.journal(), &head(), &model, &tools).expect("basis");
    let estimated = estimate_request(&model, &basis.request, &tools).expect("estimate");
    let units = basis
        .ledger
        .atoms
        .iter()
        .fold(basis.ledger.environment_estimate, |total, atom| {
            total.checked_add(atom.estimate).expect("bounded fixture")
        });
    assert_eq!(estimated, units);
}

/// CPL-3: deterministic planning refusals preserve source facts and name the limiting boundary.
#[test]
fn cpl_3_planning_refusals_are_typed_and_leave_source_unchanged() {
    let agent = history();
    let tiny = model_with_limits("openai_responses", 1_000, 500, 500);
    let huge_tools = [FunctionTool::new(
        "read_file",
        "schema".repeat(4_000),
        json!({"type":"object"}),
    )
    .expect("tool")];
    let before = agent.journal().clone();
    assert!(matches!(
        plan_compaction(
            agent.journal(),
            &head(),
            &tiny,
            &huge_tools,
            CompactionId::new("environment-refusal").expect("id"),
        ),
        Err(CompactionPreparationError::UnfittableEnvironment)
    ));
    assert_eq!(agent.journal(), &before);

    let mut oversized_user = history();
    oversized_user.handle_at(
        Input::Submitted {
            text: "x".repeat(8_000),
        },
        UnixMillis::new(200),
    );
    let before = oversized_user.journal().clone();
    assert!(matches!(
        plan_compaction(
            oversized_user.journal(),
            &head(),
            &tiny,
            &tools(),
            CompactionId::new("user-refusal").expect("id"),
        ),
        Err(CompactionPreparationError::OversizedRequiredUser)
    ));
    assert_eq!(oversized_user.journal(), &before);

    let mut one_atom = Agent::new(AgentId::new("single").expect("id"));
    one_atom.announce("Agent");
    one_atom.handle_at(
        Input::Submitted {
            text: "only request".to_owned(),
        },
        UnixMillis::new(1),
    );
    let before = one_atom.journal().clone();
    assert!(matches!(
        plan_compaction(
            one_atom.journal(),
            &head(),
            &model("openai_responses"),
            &tools(),
            CompactionId::new("single-refusal").expect("id"),
        ),
        Err(CompactionPreparationError::NoUsefulReduction)
    ));
    assert_eq!(one_atom.journal(), &before);
}

/// CPL-3: a bounded but token-expanding summary is a no-progress publication refusal.
#[test]
fn cpl_3_replacement_preview_rejects_token_non_progress() {
    let model = model("openai_responses");
    let tools = tools();
    let agent = history();
    let prepared = plan_compaction(
        agent.journal(),
        &head(),
        &model,
        &tools,
        CompactionId::new("non-progress").expect("id"),
    )
    .expect("plan");
    let output = AssistantOutput::new(
        vec![AssistantBlock::Text {
            item_id: TranscriptItemId::new("non-progress-output").expect("id"),
            text: "x".repeat(prepared.plan().max_summary_bytes()),
        }],
        None,
    )
    .expect("bounded output");
    assert!(matches!(
        validate_compaction_output(&prepared, &model, &tools, &output),
        Err(CompactionPreparationError::ReplacementMakesNoProgress)
    ));
}

fn last_text<'a>(body: &'a serde_json::Value, api: &str) -> Option<&'a str> {
    let key = match api {
        "openai_responses" => "input",
        "openai_chat_completions" | "anthropic_messages" => "messages",
        "google_generate_content" => "contents",
        _ => unreachable!("fixture api"),
    };
    let last = body[key].as_array()?.last()?;
    match api {
        "openai_responses" | "openai_chat_completions" => last["content"].as_str(),
        "anthropic_messages" => last["content"].as_array()?.first()?["text"].as_str(),
        "google_generate_content" => last["parts"].as_array()?.first()?["text"].as_str(),
        _ => unreachable!("fixture api"),
    }
}
