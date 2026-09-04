use plexmaton_core::{
    AgentStatus, SessionEvent, SessionId, TokenCounts, TokenUsage, TranscriptItemId,
    TranscriptRole, TurnId,
};

use super::super::{JournalEntryPayload, SessionJournal};
use super::tests::{agent, append, head, id};
use crate::{ProviderCodecId, ProviderReplay, RequestItem};

/// JRN-5: reasoning, usage and notices keep their separate consumer visibility.
#[test]
fn jrn_5_hidden_replay_and_visible_diagnostics_project_to_their_exact_consumers() {
    let agent_id = agent();
    let replay = ProviderReplay::new(
        ProviderCodecId::new("openai_responses")
            .unwrap_or_else(|error| panic!("fixture codec: {error:?}")),
        "encrypted-secret".to_owned(),
    )
    .unwrap_or_else(|error| panic!("fixture replay: {error:?}"));
    let usage = TokenUsage::Complete(TokenCounts {
        input: 10,
        cached_input: Some(6),
        cache_write_input: None,
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
        JournalEntryPayload::Message {
            agent_id: agent_id.clone(),
            item_id: id("reasoning", TranscriptItemId::new),
            role: TranscriptRole::Reasoning,
            text: "bounded reasoning".to_owned(),
        },
    );
    append(
        &mut journal,
        3,
        JournalEntryPayload::ProviderReplay(replay.clone()),
    );
    append(
        &mut journal,
        4,
        JournalEntryPayload::TurnUsageUpdated {
            agent_id: agent_id.clone(),
            turn_id: id("turn-1", TurnId::new),
            usage: usage.clone(),
        },
    );
    append(
        &mut journal,
        5,
        JournalEntryPayload::RuntimeWarning {
            agent_id,
            item_id: id("warning", TranscriptItemId::new),
            message: "recovered valid prefix".to_owned(),
        },
    );

    let projection = journal
        .project(&head("main"))
        .unwrap_or_else(|error| panic!("project journal: {error:?}"));
    assert_eq!(
        projection.request().items,
        [
            RequestItem::Reasoning {
                text: "bounded reasoning".to_owned(),
            },
            RequestItem::ProviderReplay(replay),
        ]
    );
    assert!(projection.events().iter().any(|event| matches!(
        &event.event,
        SessionEvent::TurnUsageUpdated {
            usage: actual, ..
        } if actual == &usage
    )));
    assert!(projection.events().iter().any(|event| matches!(
        &event.event,
        SessionEvent::RuntimeWarning { message, .. } if message == "recovered valid prefix"
    )));
    assert!(!format!("{projection:?}").contains("encrypted-secret"));
}
