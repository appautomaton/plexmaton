use super::*;
use plexmaton_agent::RequestAttemptId;
use plexmaton_core::HeadName;
use serde_json::json;

const TOOL: &str = include_str!("../fixtures/gemini_tool_call.sse");
const ANSWER: &str = include_str!("../fixtures/gemini_final_answer.sse");

/// PRV-1/PRV-3/TIM-3: native parts, signatures and generated reasoning tokens survive a tool loop.
#[tokio::test]
async fn prv_1_gemini_tool_round_trip_preserves_signatures_and_counts_thoughts() {
    let model = profile(ModelApi::GoogleGenerateContent);
    let (mut agent, request) = open_agent("Read the file.");
    let body = encode_request(&model, &request, &[read_tool()], None)
        .unwrap_or_else(|error| panic!("request: {error}"));
    assert_eq!(
        body["generationConfig"]["thinkingConfig"]["thinkingLevel"],
        "HIGH"
    );
    assert_eq!(
        body["tools"][0]["functionDeclarations"][0]["parametersJsonSchema"]["type"],
        "object"
    );
    let first = decode_fixture(&model, TOOL, &[1, 7, 23]).await;
    let usage = first
        .iter()
        .filter_map(|event| match event {
            ModelEvent::Usage(usage) => Some(usage),
            _ => None,
        })
        .next_back();
    assert!(
        matches!(usage,Some(TokenUsage::Complete(counts)) if counts.input == 100 && counts.cached_input == Some(64) && counts.output == 9 && counts.reasoning_output == Some(5) && counts.total == 109)
    );
    assert_ne!(called(&first).call_id.as_str(), "0");
    let request = complete_tool_step(&mut agent, &first, "Plexmaton");
    let body = encode_request(&model, &request, &[], None)
        .unwrap_or_else(|error| panic!("replay: {error}"));
    assert_eq!(
        body["contents"][1]["parts"][0]["thoughtSignature"],
        "thought-fixture"
    );
    assert_eq!(
        body["contents"][1]["parts"][1]["thoughtSignature"],
        "call-fixture"
    );
    assert_eq!(body["contents"][1]["parts"][1]["functionCall"]["id"], "0");
    assert_eq!(
        body["contents"][2]["parts"][0]["functionResponse"],
        json!({"id":"0","name":"read_file","response":{"output":"Plexmaton"}})
    );
    let second = decode_fixture(&model, ANSWER, &[1, 5, 17]).await;
    complete_answer(&mut agent, &second);
    assert_eq!(visible_text(&second), "Plexmaton.");
    let head = HeadName::new("main").unwrap_or_else(|error| panic!("head: {error}"));
    let projected = agent
        .journal()
        .project(&head)
        .unwrap_or_else(|error| panic!("projection: {error:?}"));
    let body = encode_request(&model, projected.request(), &[], None)
        .unwrap_or_else(|error| panic!("final replay: {error}"));
    assert_eq!(
        body["contents"][3]["parts"][0],
        json!({"text":"Plexmaton.","thoughtSignature":"answer-fixture"})
    );
}

fn decode_parts(scope: &str, parts: Value) -> Vec<ModelEvent> {
    let model = profile(ModelApi::GoogleGenerateContent);
    let scope = RequestAttemptId::new(scope).unwrap_or_else(|error| panic!("scope: {error}"));
    let mut codec = ProviderCodec::new(&scope, &model, DecodeLimits::production());
    let chunk = json!({"candidates":[{"content":{"parts":parts},"finishReason":"STOP"}]});
    let events = codec
        .push_sse("message", &chunk.to_string())
        .unwrap_or_else(|error| panic!("chunk: {error}"));
    codec
        .finish()
        .unwrap_or_else(|error| panic!("finality: {error}"));
    events
}

/// PRV-2/PRV-3: missing or reused upstream IDs cannot collide across local request attempts.
#[test]
fn prv_2_gemini_call_ids_are_scoped_and_parallel_signatures_keep_their_part() {
    let parts = json!([
        {"functionCall":{"id":"0","name":"first","args":{}},"thoughtSignature":"first-signature"},
        {"functionCall":{"name":"second","args":{}}}
    ]);
    let first = decode_parts("attempt-one", parts.clone());
    let second = decode_parts("attempt-two", parts);
    assert_ne!(called(&first).call_id, called(&second).call_id);
    let calls: Vec<_> = first
        .iter()
        .filter_map(|event| match event {
            ModelEvent::Called { position, call } => Some((*position, call)),
            _ => None,
        })
        .collect();
    assert_eq!(calls.len(), 2);
    assert_ne!(calls[0].1.call_id, calls[1].1.call_id);
    let replay: Vec<_> = first
        .iter()
        .filter_map(|event| match event {
            ModelEvent::Replay { position, replay } => Some((*position, replay)),
            _ => None,
        })
        .collect();
    assert_eq!(replay[0].0, calls[0].0);
    assert_eq!(replay[1].0, calls[1].0);
    assert!(replay[0].1.payload().contains("first-signature"));
    assert!(!replay[1].1.payload().contains("first-signature"));
}

/// PRV-3/JRN-5: signature-only text is invisible and can later receive text at the same position.
#[test]
fn prv_3_gemini_signature_only_and_early_signature_parts_are_replayable() {
    let model = profile(ModelApi::GoogleGenerateContent);
    let scope =
        RequestAttemptId::new("signature-only").unwrap_or_else(|error| panic!("scope: {error}"));
    for initial_text in [None, Some("")] {
        for later_text in [None, Some("Visible later.")] {
            let (mut agent, _) = open_agent("Hello.");
            let mut codec = ProviderCodec::new(&scope, &model, DecodeLimits::production());
            let mut part = json!({"thoughtSignature":"early-signature"});
            if let Some(text) = initial_text {
                part["text"] = json!(text);
            }
            let mut events = codec
                .push_sse(
                    "message",
                    &json!({"candidates":[{"content":{"parts":[part]}}]}).to_string(),
                )
                .expect("signature");
            if let Some(text) = later_text {
                events.extend(
                    codec
                        .push_sse(
                            "message",
                            &json!({"candidates":[{"content":{"parts":[{"text":text}]}}]})
                                .to_string(),
                        )
                        .unwrap_or_else(|error| panic!("text: {error}")),
                );
            }
            events.extend(
                codec
                    .push_sse("message", r#"{"candidates":[{"finishReason":"STOP"}]}"#)
                    .unwrap_or_else(|error| panic!("stop: {error}")),
            );
            codec
                .finish()
                .unwrap_or_else(|error| panic!("finish: {error}"));
            complete_answer(&mut agent, &events);
            let head = HeadName::new("main").unwrap_or_else(|error| panic!("head: {error}"));
            let projection = agent
                .journal()
                .project(&head)
                .unwrap_or_else(|error| panic!("project: {error:?}"));
            let body = encode_request(&model, projection.request(), &[], None)
                .unwrap_or_else(|error| panic!("encode: {error}"));
            let mut expected = json!({"thoughtSignature":"early-signature"});
            if let Some(text) = later_text.or(initial_text) {
                expected["text"] = json!(text);
            }
            assert_eq!(body["contents"][1]["parts"][0], expected);
            assert!(!format!("{:?}", projection.events()).contains("early-signature"));
        }
    }
}

/// PRV-2/PRV-5: Gemini never invents finality or drops an unsupported content-bearing part.
#[test]
fn prv_2_gemini_rejects_incomplete_duplicate_and_oversized_content() {
    use plexmaton_provider::DecodeError;
    let model = profile(ModelApi::GoogleGenerateContent);
    let scope = RequestAttemptId::new("invalid-part").expect("scope");
    let mut codec = ProviderCodec::new(&scope, &model, DecodeLimits::production());
    codec
        .push_sse(
            "message",
            r#"{"candidates":[{"content":{"parts":[{"text":"partial"}]}}]}"#,
        )
        .expect("partial text");
    assert!(matches!(codec.finish(), Err(DecodeError::IncompleteStream)));
    for parts in [
        json!([{"inlineData":{"mimeType":"image/png","data":"private"}}]),
        json!([{"functionCall":{"id":"same","name":"read","args":{}}},{"functionCall":{"id":"same","name":"read","args":{}}}]),
    ] {
        let mut codec = ProviderCodec::new(&scope, &model, DecodeLimits::production());
        let chunk = json!({"candidates":[{"content":{"parts":parts},"finishReason":"STOP"}]});
        assert!(codec.push_sse("message", &chunk.to_string()).is_err());
    }
    let mut limits = DecodeLimits::production();
    limits.max_replay_bytes = 8;
    let mut codec = ProviderCodec::new(&scope, &model, limits);
    assert!(matches!(codec.push_sse("message",r#"{"candidates":[{"content":{"parts":[{"text":"","thoughtSignature":"long-private-signature"}]}}]}"#),Err(DecodeError::RetainedReplayTooLarge { limit:8 })));
    assert!(!format!("{codec:?}").contains("long-private-signature"));
    let mut codec = ProviderCodec::new(&scope, &model, DecodeLimits::production());
    let blocked = codec
        .push_sse("message", r#"{"promptFeedback":{"blockReason":"SAFETY"}}"#)
        .expect("explicit prompt block");
    assert!(matches!(
        blocked.as_slice(),
        [ModelEvent::Stopped(StopReason::Refused)]
    ));
    codec.finish().expect("blocked request is terminal");
}

/// PRV-5: native policy finishes are refusals; image-only finishes stay outside this grammar.
#[test]
fn prv_5_gemini_policy_and_unsupported_image_finishes_are_distinct() {
    use plexmaton_provider::DecodeError;
    let model = profile(ModelApi::GoogleGenerateContent);
    let scope = RequestAttemptId::new("finish-reasons").expect("scope");
    for reason in ["ESCALATION", "PUP_LIMITED_DISABLED"] {
        let mut codec = ProviderCodec::new(&scope, &model, DecodeLimits::production());
        let chunk = json!({"candidates":[{"finishReason":reason}]});
        let events = codec
            .push_sse("message", &chunk.to_string())
            .expect("declared policy stop");
        assert!(matches!(
            events.as_slice(),
            [ModelEvent::Stopped(StopReason::Refused)]
        ));
        codec.finish().expect("final refusal");
    }
    for reason in [
        "IMAGE_SAFETY",
        "IMAGE_PROHIBITED_CONTENT",
        "IMAGE_OTHER",
        "NO_IMAGE",
        "IMAGE_RECITATION",
    ] {
        let mut codec = ProviderCodec::new(&scope, &model, DecodeLimits::production());
        let chunk = json!({"candidates":[{"finishReason":reason}]});
        assert!(
            matches!(codec.push_sse("message", &chunk.to_string()), Err(DecodeError::UnknownStopReason(actual)) if actual == reason)
        );
    }
}
