use plexmaton_core::{
    AgentStatus, HeadName, JournalRecordId, SessionEvent, SessionId, TokenCounts, TokenUsage,
    TranscriptItemId,
};

use super::super::{HeadRevision, JournalEntryPayload, JournalRecord, SessionJournal};
use super::tests::{agent, append, head, id, message};
use crate::test_support::{
    output_with_replay, reasoning_block, replay, replay_compatibility, step,
};
use crate::{
    AssistantBlock, ContextAtomValue, DispatchedRequestTiming, ElapsedMillis,
    RequestAttemptAuthorized, RequestAttemptId, RequestAttemptOwner, RequestAttemptTerminal,
    RequestAttemptTerminalState, RequestCost, RequestDispatchedOutcome, RequestEnvironment,
    RequestEnvironmentFingerprint, UsdCostTicks,
};

/// TIM-3/TIM-4/JRN-5: immutable attempt usage projects at its terminal journal position while
/// replay, usage and notices keep their separate consumer visibility.
#[test]
fn jrn_5_hidden_replay_and_visible_diagnostics_project_to_their_exact_consumers() {
    let agent_id = agent();
    let replay = replay("encrypted-secret");
    let usage = TokenUsage::Complete(TokenCounts {
        input: 10,
        cached_input: Some(6),
        cache_write_input: Some(0),
        output: 3,
        reasoning_output: Some(1),
        total: 13,
    });
    let mut journal = SessionJournal::new(id("session-a", SessionId::new));
    append(
        &mut journal,
        1,
        JournalEntryPayload::AgentCreated {
            agent_id: agent_id.clone(),
            label: "Plexmaton".to_owned(),
            status: AgentStatus::Idle,
        },
    );
    append(
        &mut journal,
        2,
        message(plexmaton_core::TranscriptRole::User, 1, "inspect"),
    );
    let main = id("main", HeadName::new);
    let boundary = journal
        .head_target(&main)
        .unwrap_or_else(|error| panic!("head target: {error:?}"))
        .cloned()
        .unwrap_or_else(|| panic!("turn boundary"));
    journal
        .apply(JournalRecord::RequestAttemptAuthorized {
            sequence: journal.next_sequence(),
            record_id: id("record-attempt-authorized", JournalRecordId::new),
            head: main,
            expected_head_revision: HeadRevision::new(2),
            fact: RequestAttemptAuthorized::new(
                RequestAttemptId::new("attempt-1")
                    .unwrap_or_else(|error| panic!("attempt id: {error}")),
                RequestAttemptOwner::AgentStep {
                    step_id: step("turn-1", 1),
                },
                boundary,
                RequestEnvironment::new(
                    replay_compatibility(),
                    RequestEnvironmentFingerprint::new([7; 32]),
                ),
                crate::UnixMillis::new(10),
            ),
        })
        .unwrap_or_else(|error| panic!("authorize attempt: {error:?}"));
    journal
        .apply(JournalRecord::RequestAttemptFinished {
            sequence: journal.next_sequence(),
            record_id: id("record-attempt-finished", JournalRecordId::new),
            fact: RequestAttemptTerminal::new(
                RequestAttemptId::new("attempt-1")
                    .unwrap_or_else(|error| panic!("attempt id: {error}")),
                RequestAttemptTerminalState::Dispatched {
                    timing: DispatchedRequestTiming::new(
                        crate::UnixMillis::new(11),
                        Some(ElapsedMillis::new(1)),
                        None,
                        ElapsedMillis::new(2),
                    )
                    .unwrap_or_else(|error| panic!("timing: {error}")),
                    outcome: RequestDispatchedOutcome::Completed {
                        stop_reason: crate::StopReason::EndOfTurn,
                    },
                    usage: usage.clone(),
                    cost: RequestCost::Known {
                        usd_ticks: UsdCostTicks::new(17),
                    },
                },
            )
            .unwrap_or_else(|error| panic!("terminal: {error}")),
        })
        .unwrap_or_else(|error| panic!("finish attempt: {error:?}"));
    append(
        &mut journal,
        5,
        JournalEntryPayload::AssistantOutput {
            agent_id: agent_id.clone(),
            step_id: step("turn-1", 1),
            output: output_with_replay(
                vec![reasoning_block("reasoning", "bounded reasoning")],
                [(0, replay.clone())],
            ),
        },
    );
    append(
        &mut journal,
        6,
        JournalEntryPayload::RuntimeWarning {
            agent_id,
            item_id: id("warning", TranscriptItemId::new),
            message: "recovered valid prefix".to_owned(),
        },
    );

    let projection = journal
        .project(&head("main"))
        .unwrap_or_else(|error| panic!("project journal: {error:?}"));
    assert_eq!(projection.request().atoms.len(), 2);
    let ContextAtomValue::Assistant(output) = projection.request().atoms[1].value() else {
        panic!("reasoning output projects as one assistant atom")
    };
    assert!(matches!(
        output.blocks(),
        [AssistantBlock::Reasoning { text, .. }] if text == "bounded reasoning"
    ));
    assert_eq!(
        output
            .replay()
            .and_then(|replay| replay.attachments().first())
            .map(|attachment| attachment.payload()),
        Some(replay.payload())
    );
    assert!(projection.events().iter().any(|event| matches!(
        &event.event,
        SessionEvent::TurnUsageUpdated {
            usage: actual, ..
        } if actual == &usage
    )));
    let usage_position = projection
        .events()
        .iter()
        .position(|event| matches!(event.event, SessionEvent::TurnUsageUpdated { .. }))
        .unwrap_or_else(|| panic!("usage event"));
    let reasoning_position = projection
        .events()
        .iter()
        .position(|event| matches!(
            &event.event,
            SessionEvent::TranscriptItemStarted { item_id, .. } if item_id.as_str() == "reasoning"
        ))
        .unwrap_or_else(|| panic!("reasoning event"));
    assert!(
        usage_position < reasoning_position,
        "terminal sequence orders usage"
    );
    assert!(projection.events().iter().any(|event| matches!(
        &event.event,
        SessionEvent::RuntimeWarning { message, .. } if message == "recovered valid prefix"
    )));
    assert!(!format!("{projection:?}").contains("encrypted-secret"));
}
