use super::*;
use plexmaton_agent::*;
use plexmaton_core::*;

fn record_id(journal: &ConversationJournal) -> JournalRecordId {
    JournalRecordId::new(format!("fixture-{}", journal.next_sequence().get())).expect("record id")
}

fn fixture() -> (ConversationJournal, HeadName, ResolvedModel) {
    let model = super::super::tests::model();
    let mut agent = Agent::new(AgentId::new("fixture").expect("agent id"));
    let reaction = agent.handle_at(
        Input::Submitted {
            text: "secret-prompt-marker".into(),
        },
        UnixMillis::new(10),
    );
    let step_id = reaction
        .effects
        .into_iter()
        .find_map(|effect| match effect {
            Effect::CallModel(call) => Some(call.step_id),
            _ => None,
        })
        .expect("model step");
    let mut journal = agent.journal().clone();
    let head = HeadName::new("main").expect("head");
    let id = RequestAttemptId::new("attempt").expect("attempt id");
    journal
        .apply(JournalRecord::RequestAttemptAuthorized {
            sequence: journal.next_sequence(),
            record_id: record_id(&journal),
            head: head.clone(),
            expected_head_revision: journal.head_revision(&head).expect("revision"),
            fact: RequestAttemptAuthorized::new(
                id.clone(),
                RequestAttemptOwner::AgentStep { step_id },
                journal
                    .head_target(&head)
                    .expect("head")
                    .expect("target")
                    .clone(),
                RequestEnvironment::new(
                    model.replay_compatibility(),
                    RequestEnvironmentFingerprint::new([1; 32]),
                ),
                UnixMillis::new(11),
            ),
        })
        .expect("authorize");
    journal
        .apply(JournalRecord::RequestAttemptFinished {
            sequence: journal.next_sequence(),
            record_id: record_id(&journal),
            fact: RequestAttemptTerminal::new(
                id,
                RequestAttemptTerminalState::Dispatched {
                    timing: DispatchedRequestTiming::new(
                        UnixMillis::new(12),
                        Some(ElapsedMillis::new(2)),
                        Some(ElapsedMillis::new(4)),
                        ElapsedMillis::new(30),
                    )
                    .expect("timing"),
                    outcome: RequestDispatchedOutcome::Completed {
                        stop_reason: StopReason::EndOfTurn,
                    },
                    usage: TokenUsage::Complete(TokenCounts {
                        input: 1000,
                        cached_input: Some(800),
                        cache_write_input: Some(50),
                        output: 50,
                        reasoning_output: Some(10),
                        total: 1050,
                    }),
                    cost: RequestCost::Known {
                        usd_ticks: UsdCostTicks::new(100_000_000),
                    },
                },
            )
            .expect("terminal"),
        })
        .expect("finish request");
    (journal, head, model)
}

fn snapshot<'a>(model: &'a ResolvedModel) -> Snapshot<'a> {
    Snapshot::base(
        model,
        "/fixture/project",
        Dimensions {
            columns: 95,
            rows: 30,
        },
        Context::Available {
            input_tokens: 1100,
            output_reserve_tokens: 8192,
            measured_prefix_tokens: Some(1000),
            estimated_tokens: 100,
            opaque_replay_bytes: 0,
            estimator: "utf8_heuristic_v1",
        },
    )
}

/// STL-3: omitted effort is unknown, never serialized as a claim that thinking is enabled.
#[test]
fn status_snapshot_default_thinking_is_explicitly_null() {
    let config = super::super::tests::CONFIG.replace("reasoning_effort = \"high\"", "");
    let registry = plexmaton_provider::ModelRegistry::parse(&config).expect("default effort");
    let json = serde_json::to_value(snapshot(registry.active_model())).expect("snapshot JSON");
    assert!(
        json["thinking"]
            .as_object()
            .expect("thinking object")
            .contains_key("enabled")
    );
    assert!(json["thinking"]["enabled"].is_null());

    // PRV-6/STL-3: Messages explicitly enables adaptive thinking to expose summaries, even at default effort.
    let config = config.replace("openai_responses", "anthropic_messages");
    let registry =
        plexmaton_provider::ModelRegistry::parse(&config).expect("Messages default effort");
    let json = serde_json::to_value(snapshot(registry.active_model())).expect("Messages snapshot");
    assert_eq!(json["thinking"]["enabled"], true);
    assert_eq!(json["effort"]["level"], "default");
}

#[test]
fn status_snapshot_projects_accounting_without_prompt_or_config_and_reloads_identically() {
    // STL-3: real journal mutations, not a status-line copy of usage or source text.
    let (journal, head, model) = fixture();
    let mut value = snapshot(&model);
    value.enrich(&journal, &head);
    let json = serde_json::to_value(&value).expect("JSON");
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["context_window"]["total_input_tokens"], 1000);
    assert_eq!(json["plexmaton"]["context"]["input_tokens"], 1100);
    assert_eq!(json["context_window"]["current_usage"]["input_tokens"], 150);
    assert_eq!(
        json["context_window"]["current_usage"]["cache_read_input_tokens"],
        800
    );
    assert_eq!(json["cost"]["total_cost_usd"], 0.01);
    assert_eq!(json["plexmaton"]["turn"]["api_duration_ms"], 30);
    assert_eq!(json["plexmaton"]["turn"]["usage"]["counts"]["input"], 1000);
    let encoded = serde_json::to_string(&value).expect("JSON");
    for excluded in [
        "secret-prompt-marker",
        "PLEXMATON_TEST_UNUSED_KEY",
        "127.0.0.1",
        "encrypted_content",
    ] {
        assert!(!encoded.contains(excluded));
    }
    let mut restored = ConversationJournal::with_metadata(journal.metadata().clone());
    for record in journal.records() {
        let decoded = serde_json::from_slice(&serde_json::to_vec(record).expect("encode record"))
            .expect("decode record");
        restored.apply(decoded).expect("replay record");
    }
    let mut reopened = snapshot(&model);
    reopened.enrich(&restored, &head);
    assert_eq!(encoded, serde_json::to_string(&reopened).expect("JSON"));
}

#[test]
fn status_snapshot_keeps_unknown_cache_subsets_and_measurements_null() {
    // STL-3: an omitted provider breakdown is not a known zero for the Claude-shaped fields.
    assert!(claude_usage(&TokenUsage::Unavailable).is_none());
    let partial = TokenUsage::Partial(TokenCounts {
        input: 100,
        cached_input: Some(80),
        cache_write_input: None,
        output: 1,
        reasoning_output: None,
        total: 101,
    });
    let usage = serde_json::to_value(claude_usage(&partial)).expect("JSON");
    assert!(usage["input_tokens"].is_null());
    assert!(usage["cache_creation_input_tokens"].is_null());
    assert_eq!(usage["cache_read_input_tokens"], 80);
    let model = super::super::tests::model();
    let absent = Snapshot::base(
        &model,
        "/fixture",
        Dimensions {
            columns: 60,
            rows: 20,
        },
        Context::Unavailable {
            reason: context::Reason::PendingCommit,
        },
    );
    let absent = serde_json::to_value(absent).expect("JSON");
    assert!(absent["session_id"].is_null());
    assert!(absent["context_window"]["used_percentage"].is_null());
    assert!(absent["cost"]["total_cost_usd"].is_null());
}

/// STL-3/TIM-3: canonical accounting refusal cannot erase metadata, requests or duration.
#[test]
fn status_snapshot_isolates_accounting_overflow() {
    for (usage_overflow, expected) in [(true, "usage_overflow"), (false, "cost_overflow")] {
        let (mut journal, head, model) = fixture();
        let prior = journal
            .request_attempts()
            .next()
            .expect("fixture attempt")
            .authorization()
            .clone();
        let id = RequestAttemptId::new("overflow-attempt").expect("id");
        journal
            .apply(JournalRecord::RequestAttemptAuthorized {
                sequence: journal.next_sequence(),
                record_id: record_id(&journal),
                head: head.clone(),
                expected_head_revision: journal.head_revision(&head).expect("revision"),
                fact: RequestAttemptAuthorized::new(
                    id.clone(),
                    prior.owner().clone(),
                    prior.semantic_boundary().clone(),
                    prior.environment().clone(),
                    UnixMillis::new(50),
                ),
            })
            .expect("second authorization");
        journal
            .apply(JournalRecord::RequestAttemptFinished {
                sequence: journal.next_sequence(),
                record_id: record_id(&journal),
                fact: RequestAttemptTerminal::new(
                    id,
                    RequestAttemptTerminalState::Dispatched {
                        timing: DispatchedRequestTiming::new(
                            UnixMillis::new(51),
                            None,
                            None,
                            ElapsedMillis::new(5),
                        )
                        .expect("timing"),
                        outcome: RequestDispatchedOutcome::Completed {
                            stop_reason: StopReason::EndOfTurn,
                        },
                        usage: TokenUsage::Complete(TokenCounts {
                            input: if usage_overflow { u64::MAX } else { 1 },
                            output: 0,
                            total: if usage_overflow { u64::MAX } else { 1 },
                            cached_input: Some(0),
                            cache_write_input: Some(0),
                            reasoning_output: Some(0),
                        }),
                        cost: RequestCost::Known {
                            usd_ticks: UsdCostTicks::new(if usage_overflow { 0 } else { u64::MAX }),
                        },
                    },
                )
                .expect("terminal"),
            })
            .expect("second terminal");
        assert!(journal.incurred_accounting().is_err());
        let mut value = snapshot(&model);
        value.enrich(&journal, &head);
        let json = serde_json::to_value(value).expect("JSON");
        assert_eq!(json["session_id"], journal.conversation_id().as_str());
        assert_eq!(json["plexmaton"]["context"]["availability"], "available");
        assert_eq!(json["plexmaton"]["issues"]["session_accounting"], expected);
        assert_eq!(json["plexmaton"]["issues"]["turn_accounting"], expected);
        assert!(json["cost"]["total_cost_usd"].is_null());
        assert!(json["context_window"]["total_input_tokens"].is_null());
        assert_eq!(json["plexmaton"]["usage"]["coverage"], "unavailable");
        assert_eq!(
            json["plexmaton"]["turn"]["usage"]["coverage"],
            "unavailable"
        );
        assert_eq!(json["plexmaton"]["turn"]["api_duration_ms"], 35);
        assert_eq!(
            json["plexmaton"]["latest_request"]["id"],
            "overflow-attempt"
        );
        assert!(!json["context_window"]["current_usage"].is_null());
    }
}

/// STL-3: a selected-path failure does not invalidate whole-journal accounting.
#[test]
fn status_snapshot_isolates_invalid_selected_path() {
    let (journal, _, model) = fixture();
    let mut value = snapshot(&model);
    value.enrich(&journal, &HeadName::new("missing-head").expect("head"));
    let json = serde_json::to_value(value).expect("JSON");
    assert_eq!(
        json["plexmaton"]["issues"]["selected_path"],
        "invalid_selected_path"
    );
    assert_eq!(json["session_id"], journal.conversation_id().as_str());
    assert_eq!(json["context_window"]["total_input_tokens"], 1000);
    assert_eq!(json["cost"]["total_cost_usd"], 0.01);
    assert!(json["plexmaton"]["latest_request"].is_null());
    assert!(json["plexmaton"]["turn"].is_null());
    assert_eq!(json["plexmaton"]["context"]["availability"], "available");
}

/// STL-3/BUD-3: diagnostic categories never serialize provider error contents.
#[test]
fn status_context_refusals_are_typed_and_content_free() {
    use plexmaton_provider::{ContextBudgetError, EncodeError};
    for (error, reason) in [
        (
            EncodeError::PlainReasoningInResponses,
            "history_incompatible",
        ),
        (EncodeError::OpaqueReplayInChat, "history_incompatible"),
        (
            EncodeError::MissingThinkingSignature,
            "history_incompatible",
        ),
        (
            EncodeError::UnrepresentableChatOrder,
            "history_incompatible",
        ),
        (
            EncodeError::OrphanToolResult("secret-tool-marker".into()),
            "encoding_failed",
        ),
    ] {
        let context = Context::capture(Err(ContextBudgetError::Encoding(error)));
        assert_eq!(
            serde_json::to_value(context).expect("JSON"),
            serde_json::json!({"availability":"unavailable","reason":reason})
        );
    }
    for (error, reason) in [
        (BudgetError::Overflow, "arithmetic_overflow"),
        (BudgetError::InvalidLimits, "invalid_budget"),
        (BudgetError::InvalidAnchor, "invalid_budget"),
    ] {
        let context = Context::capture(Err(ContextBudgetError::Arithmetic(error)));
        assert_eq!(
            serde_json::to_value(context).expect("JSON"),
            serde_json::json!({"availability":"unavailable","reason":reason})
        );
    }
}
