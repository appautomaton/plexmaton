use super::*;
use plexmaton_core::ReasoningEffort;
use plexmaton_provider::{DecodeError, ModelRegistry, request_environment};

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

/// PRV-3: omitting interrupted summaries must never permit an unsigned thinking/tool batch.
#[test]
fn prv_3_unsigned_reasoning_in_tool_batches_is_still_refused() {
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
    for api in [ModelApi::AnthropicMessages, ModelApi::OpenaiResponses] {
        assert!(
            encode_request(&profile(api), &request, &[], None).is_err(),
            "tool-turn signatures remain required"
        );
    }
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
    ] {
        let mut codec = ProviderCodec::new(
            &plexmaton_agent::RequestAttemptId::new("fixture-attempt")
                .unwrap_or_else(|error| panic!("attempt: {error}")),
            &model,
            DecodeLimits::production(),
        );
        let chunk = serde_json::json!({"choices":[{"index":0,"delta":delta,"finish_reason":null}]});
        assert!(matches!(
            codec.push_sse("message", &chunk.to_string()),
            Err(DecodeError::UnsupportedEvent(_))
        ));
    }
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
