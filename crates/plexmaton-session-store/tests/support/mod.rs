#[path = "../../src/test_support.rs"]
mod directory;
pub use directory::TestDir;

use plexmaton_agent::{ConversationEntry, ConversationJournal, JournalEntryPayload, JournalRecord};
use plexmaton_core::{
    AgentId, AgentStatus, ConversationEntryId, ConversationId, HeadName, JournalRecordId,
};

pub fn id<T>(value: &str, build: impl FnOnce(String) -> Result<T, plexmaton_core::IdError>) -> T {
    build(value.to_owned()).unwrap_or_else(|error| panic!("fixture identity: {error}"))
}

pub fn session(value: &str) -> ConversationId {
    id(value, ConversationId::new)
}

pub fn agent_created(journal: &ConversationJournal, ordinal: u64) -> JournalRecord {
    append(
        journal,
        ordinal,
        JournalEntryPayload::AgentCreated {
            agent_id: id("agent-a", AgentId::new),
            label: "Plexmaton".to_owned(),
            status: AgentStatus::Idle,
        },
    )
}

pub fn append(
    journal: &ConversationJournal,
    ordinal: u64,
    payload: JournalEntryPayload,
) -> JournalRecord {
    let head = id("main", HeadName::new);
    JournalRecord::AppendEntry {
        sequence: journal.next_sequence(),
        record_id: id(&format!("record-{ordinal}"), JournalRecordId::new),
        head: head.clone(),
        expected_head_revision: journal
            .head_revision(&head)
            .unwrap_or_else(|error| panic!("main revision: {error:?}")),
        entry: Box::new(ConversationEntry {
            id: id(&format!("entry-{ordinal}"), ConversationEntryId::new),
            parent_id: journal
                .head_target(&head)
                .unwrap_or_else(|error| panic!("main target: {error:?}"))
                .cloned(),
            payload,
        }),
    }
}
