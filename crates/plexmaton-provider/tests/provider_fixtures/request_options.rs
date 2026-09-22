use super::*;
use plexmaton_core::ReasoningEffort;
use plexmaton_provider::{DecodeError, ModelRegistry, request_environment};

/// PRV-5: one named accounting field is admitted on the Chat delta, and nothing else is.
///
/// The shape is the one an OpenAI-compatible gateway actually sends: it arrives on the *last*
/// delta, beside `finish_reason`, carrying cost, cache counts and routing attempts. Refusing it
/// threw away a completed answer at its final chunk. The allowance is one name on one surface —
/// the same payload under another name, or on a Gemini part, still fails.
#[test]
fn prv_5_chat_admits_gateway_accounting_and_still_refuses_everything_else() {
    use serde_json::json;
    let scope = plexmaton_agent::RequestAttemptId::new("gateway-accounting").expect("scope");
    let accounting = json!({
        "deepseek": {"choiceIndex":0, "promptCacheHitTokens":0, "promptCacheMissTokens":6},
        "gateway": {
            "cost":"0.0000015", "generationId":"gen_01M2",
            "routing": {"finalProvider":"deepseek", "modelAttemptCount":1,
                        "modelAttempts":[{"providerAttempts":[{"statusCode":200,"success":true}]}]},
        },
    });

    let mut codec = ProviderCodec::new(
        &scope,
        &profile(ModelApi::OpenaiChatCompletions),
        DecodeLimits::production(),
    );
    let answer =
        json!({"choices":[{"index":0,"delta":{"content":"Visible."},"finish_reason":null}]});
    let events = codec
        .push_sse("message", &answer.to_string())
        .expect("the answer streams");
    assert_eq!(visible_text(&events), "Visible.");
    let last = json!({"choices":[{
        "index":0,
        "delta":{"provider_metadata":accounting.clone()},
        "finish_reason":"stop",
    }]});
    let events = codec
        .push_sse("message", &last.to_string())
        .expect("accounting on the final delta must not cost the turn");
    assert!(matches!(
        events.last(),
        Some(ModelEvent::Stopped(StopReason::EndOfTurn))
    ));
    codec.push_sse("message", "[DONE]").expect("trailer");
    codec.finish().expect("complete stream");

    // The same payload nobody has read still fails, on this surface and on the other one.
    let mut codec = ProviderCodec::new(
        &scope,
        &profile(ModelApi::OpenaiChatCompletions),
        DecodeLimits::production(),
    );
    let renamed = json!({"choices":[{"index":0,"delta":{"vendor_metadata":accounting.clone()},"finish_reason":"stop"}]});
    assert!(
        matches!(
            codec.push_sse("message", &renamed.to_string()),
            Err(DecodeError::UnsupportedEvent(_))
        ),
        "an unexamined field is refused whatever it resembles"
    );
    let mut codec = ProviderCodec::new(
        &scope,
        &profile(ModelApi::GoogleGenerateContent),
        DecodeLimits::production(),
    );
    let part = json!({"candidates":[{
        "content":{"parts":[{"text":"Visible.","provider_metadata":accounting}]},
        "finishReason":"STOP",
    }]});
    assert!(
        matches!(
            codec.push_sse("message", &part.to_string()),
            Err(DecodeError::UnsupportedEvent(_))
        ),
        "the allowance belongs to the surface that was examined, not to the name"
    );
}

/// PRV-5: empty additive fields preserve visible output; populated unknown content fails explicitly.
#[test]
fn prv_5_empty_additive_fields_preserve_text_and_populated_fields_fail() {
    use serde_json::json;
    let scope = plexmaton_agent::RequestAttemptId::new("additive-fields").expect("scope");
    for (api, fields) in [
        (
            ModelApi::OpenaiChatCompletions,
            [
                "annotations",
                "audio",
                "function_call",
                "provider_specific_fields",
            ],
        ),
        (
            ModelApi::GoogleGenerateContent,
            ["partMetadata", "videoMetadata", "inlineData", "vendorField"],
        ),
    ] {
        let model = profile(api);
        for field in fields {
            for (value, empty) in [
                (Value::Null, true),
                (json!([]), true),
                (json!({}), true),
                (json!([{"text":"unsupported"}]), false),
                (json!({"data":"private"}), false),
                (json!(false), false),
                (json!("unknown"), false),
            ] {
                let mut codec = ProviderCodec::new(&scope, &model, DecodeLimits::production());
                let mut content = if api == ModelApi::OpenaiChatCompletions {
                    json!({"content":"Visible."})
                } else {
                    json!({"text":"Visible."})
                };
                content[field] = value;
                let chunk = if api == ModelApi::OpenaiChatCompletions {
                    json!({"choices":[{"index":0,"delta":content,"finish_reason":"stop"}]})
                } else {
                    json!({"candidates":[{"content":{"parts":[content]},"finishReason":"STOP"}]})
                };
                let result = codec.push_sse("message", &chunk.to_string());
                if empty {
                    let events = result.unwrap_or_else(|error| panic!("empty {field}: {error}"));
                    assert_eq!(visible_text(&events), "Visible.");
                    assert!(matches!(
                        events.last(),
                        Some(ModelEvent::Stopped(StopReason::EndOfTurn))
                    ));
                    if api == ModelApi::OpenaiChatCompletions {
                        codec.push_sse("message", "[DONE]").expect("trailer");
                    }
                    codec.finish().expect("complete stream");
                } else {
                    assert!(
                        matches!(result, Err(DecodeError::UnsupportedEvent(_))),
                        "populated {field} must not be discarded"
                    );
                }
            }
        }
    }
}

/// PRV-3: an unsigned summary in a tool turn is the signature-requiring dialect's problem alone.
///
/// Messages must refuse it: a `thinking` block beside `tool_use` has to arrive complete and
/// signed, so a turn that lost its signature cannot be replayed at all. Responses has no
/// signature — a reasoning item is identified by the provider's own opaque id — so the summary is
/// omitted there exactly as it is in a turn that made no calls, and the call itself still goes.
///
/// The two were one rule until an interrupted tool turn made every later request in its
/// conversation unencodable, which is a session that can be read and never continued.
#[test]
fn prv_3_unsigned_reasoning_in_a_tool_turn_is_refused_by_messages_and_omitted_by_responses() {
    let call = tool_call("call", "read_file");
    let output = AssistantOutput::new(
        vec![
            AssistantBlock::Reasoning {
                item_id: transcript_item("thinking"),
                text: "Unsigned.".into(),
            },
            AssistantBlock::ToolCall {
                item_id: transcript_item("call"),
                call: call.clone(),
            },
        ],
        None,
    )
    .expect("canonical blocks");
    let batch = ToolBatch::new(
        output,
        vec![ToolBatchResult::new(
            call.call_id,
            ToolOutcome::Succeeded {
                output: "file".into(),
            },
        )],
    )
    .expect("paired batch");
    let mut request = open_agent("Read.").1;
    request
        .atoms
        .push(ContextAtom::tool_batch(vec![session_entry("batch")], batch).expect("atom"));
    assert!(
        encode_request(&profile(ModelApi::AnthropicMessages), &request, &[], None).is_err(),
        "a thinking block beside a tool call must arrive signed"
    );

    let encoded = encode_request(&profile(ModelApi::OpenaiResponses), &request, &[], None)
        .expect("an unsigned summary is left out rather than refused");
    let items = encoded["input"]
        .as_array()
        .unwrap_or_else(|| panic!("the Responses request carries an input array: {encoded}"));
    assert!(
        items
            .iter()
            .all(|item| item["type"] != "reasoning" && item["summary"].is_null()),
        "the summary with no replay item is not on the wire: {encoded}"
    );
    // The call and its result are what the turn actually did, and they survive.
    assert!(
        items
            .iter()
            .any(|item| item["type"] == "function_call" && item["name"] == "read_file"),
        "the call the summary preceded still goes: {encoded}"
    );
    assert!(
        items
            .iter()
            .any(|item| item["type"] == "function_call_output"),
        "and so does its result: {encoded}"
    );
}

/// PRV-6: Gemini 3.8 uses levels and omits deprecated candidate/budget controls.
/// https://ai.google.dev/gemini-api/docs/generate-content/latest-model
#[test]
fn prv_6_gemini_38_requests_use_only_frontier_thinking_controls() {
    use serde_json::json;
    let (_, request) = open_agent("Inspect the project.");
    for effort in ["default", "low", "medium", "high"] {
        let source = format!(
            r#"
active_model = {{ provider = "google", model = "flash" }}
[providers.google]
base_url = "http://127.0.0.1:1/v1beta"
api_key_env = "FIXTURE_KEY"
api = "google_generate_content"
[providers.google.models.flash]
id = "gemini-3.8-flash"
reasoning_effort = "{effort}"
context_window_tokens = 32768
max_output_tokens = 4096
output_reserve_tokens = 4096
"#
        );
        let registry = ModelRegistry::parse(&source).expect("frontier model");
        let body = encode_request(registry.active_model(), &request, &[], None).expect("request");
        let mut expected =
            json!({"maxOutputTokens":4096,"thinkingConfig":{"includeThoughts":true}});
        if effort != "default" {
            expected["thinkingConfig"] =
                json!({"thinkingLevel":effort.to_ascii_uppercase(),"includeThoughts":true});
        }
        assert_eq!(body["generationConfig"], expected);
    }
}

/// PRV-6: native Messages controls follow the documented request contract with no model-name guess.
/// https://platform.claude.com/docs/en/api/messages/create (cache_control, thinking, output_config).
/// Automatic caching covers the last cacheable block even when there is no system block.
#[test]
fn prv_6_messages_cache_and_thinking_match_native_request_contract() {
    use serde_json::json;
    let (_, request) = open_agent("Hello.");
    for (options, thinking, effort, cache) in [
        (
            "",
            Some(json!({"type":"adaptive","display":"summarized"})),
            None,
            true,
        ),
        (
            "reasoning_effort = 'none'",
            Some(json!({"type":"disabled"})),
            None,
            true,
        ),
        (
            "reasoning_effort = 'high'",
            Some(json!({"type":"adaptive","display":"summarized"})),
            Some("high"),
            true,
        ),
        (
            "prompt_cache = 'disabled'",
            Some(json!({"type":"adaptive","display":"summarized"})),
            None,
            false,
        ),
    ] {
        let source = format!(
            r#"
active_model = {{ provider = "fixture", model = "model" }}
[providers.fixture]
base_url = "http://127.0.0.1:1/v1"
api_key_env = "FIXTURE_KEY"
api = "anthropic_messages"
[providers.fixture.models.model]
id = "opaque-user-selected-model"
context_window_tokens = 8192
max_output_tokens = 2048
output_reserve_tokens = 512
{options}
"#
        );
        let registry = ModelRegistry::parse(&source).expect("fixture options");
        let body = encode_request(registry.active_model(), &request, &[read_tool()], None)
            .expect("native request");
        assert_eq!(body.get("thinking"), thinking.as_ref());
        assert_eq!(
            body.get("output_config"),
            effort.map(|value| json!({"effort":value})).as_ref()
        );
        assert_eq!(
            body.get("cache_control"),
            cache.then(|| json!({"type":"ephemeral"})).as_ref()
        );
        assert!(body.get("system").is_none());
        assert_eq!(
            body["messages"][0]["content"],
            json!([{"type":"text","text":"Hello."}])
        );
        assert!(body["tools"][0].get("cache_control").is_none());
    }
}

/// PRV-3: Chat aliases retain their wire identity after tool execution and re-encoding.
#[tokio::test]
async fn prv_3_chat_reasoning_aliases_round_trip_without_renaming() {
    let model = profile(ModelApi::OpenaiChatCompletions);
    for field in ["reasoning_content", "reasoning", "reasoning_text"] {
        let fixture = CHAT_TOOL_CALL.replace("reasoning_content", field);
        let events = decode_fixture(&model, &fixture, &[1, 7, 19]).await;
        let (mut agent, _) = open_agent("Read the file.");
        let request = complete_tool_step(&mut agent, &events, "Plexmaton");
        let encoded = encode_request(&model, &request, &[], None)
            .unwrap_or_else(|error| panic!("fixture: {error}"));
        assert_eq!(encoded["messages"][1][field], "Need README.");
        assert_eq!(encoded["messages"][2]["role"], "tool");
        if field != "reasoning_content" {
            assert!(encoded["messages"][1].get("reasoning_content").is_none());
        }
    }
}

/// PRV-5: recognized but unsupported structured reasoning cannot disappear through Serde.
#[test]
fn prv_5_chat_rejects_structured_or_conflicting_reasoning() {
    let model = profile(ModelApi::OpenaiChatCompletions);
    for delta in [
        serde_json::json!({"reasoning_details":[{"type":"reasoning.encrypted","data":"private"}]}),
        serde_json::json!({"reasoning":"one","reasoning_text":"two"}),
        // A signature travels with the text it signs; keeping only the text would strip it.
        serde_json::json!({
            "reasoning":"weigh",
            "reasoning_details":[{"type":"reasoning.text","text":"weigh","signature":"sig"}]
        }),
        // Details that say more than the plain field carry reasoning nothing else retains.
        serde_json::json!({
            "reasoning":"weigh",
            "reasoning_details":[{"type":"reasoning.text","text":"weigh more"}]
        }),
        // Details with no plain sibling are the only copy, in a shape the codec cannot replay.
        serde_json::json!({"reasoning_details":[{"type":"reasoning.text","text":"weigh"}]}),
    ] {
        let mut codec = ProviderCodec::new(
            &plexmaton_agent::RequestAttemptId::new("fixture-attempt")
                .unwrap_or_else(|error| panic!("attempt: {error}")),
            &model,
            DecodeLimits::production(),
        );
        let chunk = serde_json::json!({"choices":[{"index":0,"delta":delta,"finish_reason":null}]});
        assert!(
            matches!(
                codec.push_sse("message", &chunk.to_string()),
                Err(DecodeError::UnsupportedEvent(_))
            ),
            "{delta}"
        );
    }
}

/// PRV-3: gateways that restate one reasoning delta as `reasoning.text` alongside it add no
/// content, so the plain field is decoded once and the restatement is not a second event.
#[test]
fn prv_3_chat_accepts_reasoning_details_that_only_restate_the_plain_delta() {
    let model = profile(ModelApi::OpenaiChatCompletions);
    let mut codec = ProviderCodec::new(
        &plexmaton_agent::RequestAttemptId::new("fixture-attempt")
            .unwrap_or_else(|error| panic!("attempt: {error}")),
        &model,
        DecodeLimits::production(),
    );
    // The observed gateway shape: one entry, a `format` label, an `index`, and identical text.
    let chunk = serde_json::json!({"choices":[{"index":0,"delta":{
        "reasoning":"17*23 = ",
        "reasoning_details":[
            {"type":"reasoning.text","text":"17*23 = ","format":"unknown","index":0}
        ]
    },"finish_reason":null}]});
    let events = codec
        .push_sse("message", &chunk.to_string())
        .unwrap_or_else(|error| panic!("restated reasoning: {error}"));
    let reasoning = events
        .iter()
        .filter(|event| matches!(event, ModelEvent::ReasoningDelta { delta, .. } if delta == "17*23 = "))
        .count();
    assert_eq!(reasoning, 1, "{events:?}");
}

/// PRV-6/TIM-3: unspecified effort is omitted, and stable instructions participate in identity.
#[test]
fn prv_6_standard_requests_omit_unspecified_reasoning_and_encode_instructions() {
    let base = r#"
active_model = { provider = "fixture", model = "plain" }
[providers.fixture]
base_url = "http://127.0.0.1:1/v1"
api_key_env = "FIXTURE_KEY"
api = "openai_chat_completions"
[providers.fixture.models.plain]
id = "fixture"
instructions = "Keep exact tool results."
prompt_cache = "disabled"
context_window_tokens = 8192
max_output_tokens = 1024
output_reserve_tokens = 512
"#;
    let (_, request) = open_agent("Hello.");
    for api in ["openai_chat_completions", "openai_responses"] {
        let source = base.replace("openai_chat_completions", api);
        let registry =
            ModelRegistry::parse(&source).unwrap_or_else(|error| panic!("fixture: {error}"));
        let model = registry.active_model();
        assert_eq!(model.reasoning_effort(), ReasoningEffort::Default);
        let body = encode_request(model, &request, &[], None)
            .unwrap_or_else(|error| panic!("fixture: {error}"));
        assert!(body.get("reasoning").is_none());
        assert!(body.get("reasoning_effort").is_none());
        assert!(body.get("prompt_cache_key").is_none());
        if api == "openai_responses" {
            assert_eq!(body["instructions"], "Keep exact tool results.");
        } else {
            assert_eq!(body["messages"][0]["role"], "system");
            assert_eq!(body["messages"][0]["content"], "Keep exact tool results.");
        }
        let changed = ModelRegistry::parse(
            &source.replace("Keep exact tool results.", "Summarize findings."),
        )
        .unwrap_or_else(|error| panic!("fixture: {error}"));
        assert_ne!(
            request_environment(model, &[], None),
            request_environment(changed.active_model(), &[], None)
        );
    }
}

/// PRV-6: a declared hosted tool is spelled by the dialect, after the function tools, and a model
/// that declared none carries none. The three spellings are the wire's, asserted as JSON.
#[test]
fn prv_6_declared_hosted_tools_are_spelled_by_the_dialect_and_absent_otherwise() {
    use serde_json::{Value, json};
    let base = r#"
active_model = { provider = "fixture", model = "searching" }
[providers.fixture]
base_url = "http://127.0.0.1:1/v1"
api_key_env = "FIXTURE_KEY"
api = "openai_responses"
[providers.fixture.models.searching]
id = "fixture"
server_tools = ["web_search"]
context_window_tokens = 8192
max_output_tokens = 1024
output_reserve_tokens = 512
[providers.fixture.models.plain]
id = "fixture"
context_window_tokens = 8192
max_output_tokens = 1024
output_reserve_tokens = 512
"#;
    let (_, request) = open_agent("Hello.");
    let hosted = |body: &Value| -> Vec<Value> {
        body.get("tools")
            .and_then(Value::as_array)
            .map(|tools| {
                tools
                    .iter()
                    .filter(|tool| {
                        tool.get("type").and_then(Value::as_str) != Some("function")
                            && tool.get("input_schema").is_none()
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    };
    for (api, spelling) in [
        ("openai_responses", json!({"type": "web_search"})),
        ("openai_chat_completions", json!({})),
        (
            "anthropic_messages",
            json!({"type": "web_search_20250305", "name": "web_search"}),
        ),
    ] {
        let registry = ModelRegistry::parse(&base.replace("openai_responses", api))
            .unwrap_or_else(|error| panic!("{api}: {error}"));
        let searching = registry.active_model();
        let plain = registry.model("fixture", "plain").expect("plain");
        let with_function = encode_request(searching, &request, &[read_tool()], None)
            .unwrap_or_else(|error| panic!("{api}: {error}"));
        let alone = encode_request(searching, &request, &[], None)
            .unwrap_or_else(|error| panic!("{api}: {error}"));
        let undeclared = encode_request(plain, &request, &[read_tool()], None)
            .unwrap_or_else(|error| panic!("{api}: {error}"));
        if api == "openai_chat_completions" {
            assert_eq!(with_function["web_search_options"], spelling, "{api}");
            assert_eq!(alone["web_search_options"], spelling, "{api}");
            assert!(undeclared.get("web_search_options").is_none(), "{api}");
            assert!(
                hosted(&with_function).is_empty(),
                "{api}: not a tool type here"
            );
        } else {
            assert_eq!(hosted(&with_function), vec![spelling.clone()], "{api}");
            assert_eq!(hosted(&alone), vec![spelling.clone()], "{api}");
            assert!(hosted(&undeclared).is_empty(), "{api}");
            let tools = with_function["tools"].as_array().expect("tools");
            assert_eq!(tools.len(), 2, "{api}: one function tool, one hosted");
            assert_eq!(
                tools[1], spelling,
                "{api}: hosted follows the function tools"
            );
            assert!(with_function.get("tool_choice").is_some(), "{api}");
            assert!(
                alone.get("tool_choice").is_some(),
                "{api}: a hosted tool alone still names a choice"
            );
        }
    }
}
