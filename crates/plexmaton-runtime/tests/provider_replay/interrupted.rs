use super::*;
use plexmaton_agent::{
    AssistantBlock, ContextAtomValue, ModelError, ModelOutputPosition, StopReason,
};
use plexmaton_core::ConversationEvent;
use serde_json::Value;

/// PRV-3/JRN-5: incomplete reasoning remains visible without stranding live or reopened history.
#[test]
fn prv_3_interrupted_unsigned_reasoning_allows_durable_continuation() {
    for api in ["anthropic_messages", "openai_responses"] {
        let registry = ModelRegistry::parse(&format!(
            r#"
active_model = {{ provider = "fixture", model = "model" }}
[providers.fixture]
base_url = "http://127.0.0.1:1/v1"
api_key_env = "FIXTURE_KEY"
api = "{api}"
[providers.fixture.models.model]
id = "fixture-model"
context_window_tokens = 8192
max_output_tokens = 1024
output_reserve_tokens = 512
"#
        ))
        .expect("model");
        let model = registry.active_model();
        for failed in [false, true] {
            for prefix in [false, true] {
                for signature_started in [false, true] {
                    let id = AgentId::new("agent").expect("agent id");
                    let mut agent = Agent::new(id.clone());
                    agent.handle_at(
                        Input::Submitted {
                            text: "Inspect.".into(),
                        },
                        UnixMillis::EPOCH,
                    );
                    let scope =
                        plexmaton_agent::RequestAttemptId::new("interrupted").expect("scope");
                    let mut codec = ProviderCodec::new(&scope, model, DecodeLimits::production());
                    for event in interrupted_frames(api, prefix, signature_started) {
                        let name = event["type"].as_str().expect("event name");
                        for event in codec
                            .push_sse(name, &event.to_string())
                            .expect("wire frame")
                        {
                            deliver(&mut agent, event);
                        }
                    }
                    assert!(
                        codec.finish().is_err(),
                        "the thinking block never completed"
                    );
                    let input = if failed {
                        Input::Failed {
                            step_id: agent.active_model_step().expect("step"),
                            error: ModelError::Transport {
                                message: "connection ended".into(),
                            },
                        }
                    } else {
                        Input::Interrupted
                    };
                    agent.handle_at(input, UnixMillis::EPOCH);
                    let head = HeadName::new("main").expect("head");
                    let before = agent.journal().project(&head).expect("project");
                    let ContextAtomValue::Assistant(output) = before.request().atoms[1].value()
                    else {
                        panic!("partial assistant output remains canonical");
                    };
                    assert!(output.replay().is_none());
                    assert!(output.blocks().iter().any(|block| matches!(
                        block, AssistantBlock::Reasoning { text, .. } if text == "Unfinished reasoning."
                    )));
                    assert!(before.events().iter().any(|event| matches!(
                        &event.event, ConversationEvent::TranscriptDelta { text, .. } if text == "Unfinished reasoning."
                    )));
                    let scratch = Scratch::new("interrupted-thinking");
                    let path = scratch.0.join("session.jsonl");
                    let mut file = JournalFile::create(
                        &path,
                        agent.journal().conversation_id().clone(),
                        UnixMillis::EPOCH,
                    )
                    .expect("create");
                    for record in agent.journal().records() {
                        file.append(record.clone()).expect("append");
                    }
                    drop(file);
                    let reopened = JournalFile::open(&path).expect("reopen");
                    let after = reopened.journal().project(&head).expect("reproject");
                    assert_eq!(before.events(), after.events());
                    assert_eq!(before.request(), after.request());
                    let resumed = Agent::from_journal(
                        id,
                        reopened.journal().clone(),
                        Default::default(),
                        Default::default(),
                    )
                    .expect("resume");
                    let live_requests = continue_three_turns(agent, model, prefix);
                    let resumed_requests = continue_three_turns(resumed, model, prefix);
                    assert_eq!(
                        live_requests, resumed_requests,
                        "the same journal and configuration produce identical wire histories"
                    );
                }
            }
        }
    }
}

fn continue_three_turns(
    mut agent: Agent,
    model: &plexmaton_provider::ResolvedModel,
    prefix: bool,
) -> Vec<Value> {
    let original_records = agent.journal().records().to_vec();
    let mut requests = Vec::new();
    for turn in 0..3 {
        let reaction = agent.handle_at(
            Input::Submitted {
                text: format!("Continue {turn}."),
            },
            UnixMillis::EPOCH,
        );
        let [Effect::CallModel(call)] = reaction.effects.as_slice() else {
            panic!("continuation must request the model");
        };
        let body = encode_request(model, &call.request, &[], None)
            .expect("interrupted history must encode");
        assert!(!body.to_string().contains("Unfinished reasoning."));
        assert_eq!(body.to_string().contains("Visible prefix."), prefix);
        if model.api() == plexmaton_provider::ModelApi::AnthropicMessages {
            assert!(
                body["messages"]
                    .as_array()
                    .expect("messages")
                    .iter()
                    .all(|message| !message["content"].as_array().expect("content").is_empty())
            );
        }
        requests.push(body);
        deliver(
            &mut agent,
            ModelEvent::TextDelta {
                position: ModelOutputPosition::new(0, 0),
                delta: "Recovered.".into(),
            },
        );
        deliver(&mut agent, ModelEvent::Stopped(StopReason::EndOfTurn));
        assert!(
            agent.journal().records().starts_with(&original_records),
            "continuation only appends; failed/interrupted records remain unchanged"
        );
    }
    requests
}

fn interrupted_frames(api: &str, prefix: bool, signature_started: bool) -> Vec<Value> {
    let index = usize::from(prefix);
    if api == "openai_responses" {
        let mut events = Vec::new();
        if prefix {
            events.push(json!({"type":"response.output_text.delta", "output_index":0,"content_index":0,"delta":"Visible prefix."}));
        }
        events.push(json!({"type":"response.reasoning_summary_text.delta","output_index":index,"summary_index":0,"delta":"Unfinished reasoning."}));
        return events;
    }
    let mut events = vec![
        json!({"type":"message_start","message":{"role":"assistant","content":[],"usage":{"input_tokens":12,"output_tokens":0}}}),
    ];
    if prefix {
        events.extend([
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Visible prefix."}}),
            json!({"type":"content_block_stop","index":0}),
        ]);
    }
    events.extend([
        json!({"type":"content_block_start","index":index,"content_block":{"type":"thinking","thinking":"","signature":""}}),
        json!({"type":"content_block_delta","index":index,"delta":{"type":"thinking_delta","thinking":"Unfinished reasoning."}}),
    ]);
    if signature_started {
        events.push(json!({"type":"content_block_delta","index":index,"delta":{"type":"signature_delta","signature":"not-yet-final"}}));
    }
    events
}
