use super::*;
use plexmaton_agent::*;
use plexmaton_core::*;

fn record_id(journal: &SessionJournal) -> JournalRecordId {
    JournalRecordId::new(format!("fixture-{}", journal.next_sequence().get())).expect("record id")
}

fn fixture() -> (SessionJournal, HeadName, ResolvedModel) {
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
    value.enrich(&journal, &head).expect("snapshot");
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
    let mut restored = SessionJournal::with_metadata(journal.metadata().clone());
    for record in journal.records() {
        let decoded = serde_json::from_slice(&serde_json::to_vec(record).expect("encode record"))
            .expect("decode record");
        restored.apply(decoded).expect("replay record");
    }
    let mut reopened = snapshot(&model);
    reopened.enrich(&restored, &head).expect("snapshot");
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
            reason: "pending_commit",
        },
    );
    let absent = serde_json::to_value(absent).expect("JSON");
    assert!(absent["session_id"].is_null());
    assert!(absent["context_window"]["used_percentage"].is_null());
    assert!(absent["cost"]["total_cost_usd"].is_null());
}
