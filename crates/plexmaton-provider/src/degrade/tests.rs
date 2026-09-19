use plexmaton_core::ToolCallId;

use super::{MAX_CALL_ID_LEN, wire_call_id};

fn call_id(value: &str) -> ToolCallId {
    ToolCallId::new(value).unwrap_or_else(|error| panic!("fixture call id: {error:?}"))
}

#[test]
fn prv_3_an_id_every_dialect_accepts_is_carried_through_unchanged() {
    for original in [
        "call_abc123",
        "toolu_01A09q90qw90lq917835lq9",
        "gemini-main-0",
        "a",
        &"x".repeat(MAX_CALL_ID_LEN),
    ] {
        let id = call_id(original);
        assert_eq!(
            wire_call_id(&id),
            original,
            "an acceptable id must reach the wire as the model wrote it"
        );
    }
}

#[test]
fn prv_3_an_id_the_destination_would_reject_becomes_one_it_accepts() {
    for original in [
        // The pipe form OpenAI Responses issues, which Anthropic's charset refuses.
        "call_9wq1|fc_0198ab",
        &"y".repeat(MAX_CALL_ID_LEN + 1),
        "call with spaces",
        "café",
    ] {
        let id = call_id(original);
        let wire = wire_call_id(&id);
        assert_ne!(wire, original, "an unacceptable id must not reach the wire");
        assert!(
            wire.len() <= MAX_CALL_ID_LEN
                && wire
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'),
            "{wire} must satisfy the bound it was rewritten for"
        );
        let again = call_id(original);
        assert_eq!(
            wire_call_id(&again),
            wire,
            "the call side and the result side evaluate this independently and must agree"
        );
    }
}

#[test]
fn prv_3_distinct_rewritten_ids_stay_distinct() {
    let (left, right) = (
        call_id("call_9wq1|fc_0198ab"),
        call_id("call_9wq1|fc_0198ac"),
    );
    let (first, second) = (wire_call_id(&left), wire_call_id(&right));
    assert_ne!(
        first, second,
        "two calls in one turn must not collapse onto one id, which would orphan a result"
    );
}
