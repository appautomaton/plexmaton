//! Assertions for native tool facts carried through the runtime boundary (ENT-4).

use plexmaton_core::{ConversationEvent, ConversationEventEnvelope, ToolCallStatus, ToolDetail};

pub(super) fn assert_edit(events: &[ConversationEventEnvelope]) {
    let presentation = events.iter().find_map(|envelope| match &envelope.event {
        ConversationEvent::ToolCallChanged {
            call_id,
            status: ToolCallStatus::Succeeded,
            presentation,
            ..
        } if call_id.as_str() == "edit-1" => Some(presentation),
        _ => None,
    });
    assert!(matches!(
        presentation,
        Some(presentation)
            if presentation.invocation.is_some()
                && matches!(
                    &presentation.outcome,
                    Some(ToolDetail::Diff { patch })
                        if patch.contains(
                            "-old value\n\\ No newline at end of edit\n+new value\n"
                        )
                )
    ));
}

pub(super) fn assert_command(events: &[ConversationEventEnvelope], expected_model_result: &str) {
    assert!(events.iter().any(|envelope| matches!(
        &envelope.event,
        ConversationEvent::ToolCallChanged {
            call_id,
            status: ToolCallStatus::Succeeded,
            presentation,
            ..
        } if call_id.as_str() == "command-1"
            && presentation.invocation.is_some()
            && matches!(
                &presentation.outcome,
                Some(ToolDetail::Text { source, omitted_bytes })
                    if source == expected_model_result && *omitted_bytes > 0
            )
    )));
}
