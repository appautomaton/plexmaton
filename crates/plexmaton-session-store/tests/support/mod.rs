use std::path::{Path, PathBuf};

use plexmaton_agent::{JournalEntryPayload, JournalRecord, SessionEntry, SessionJournal};
use plexmaton_core::{AgentId, AgentStatus, HeadName, JournalRecordId, SessionEntryId, SessionId};

pub struct TestDir(PathBuf);

impl TestDir {
    pub fn new(label: &str) -> Self {
        for ordinal in 0..32_u8 {
            let path = std::env::temp_dir().join(format!(
                "plexmaton-session-{label}-{}-{ordinal}",
                std::process::id()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("create test directory: {error}"),
            }
        }
        panic!("could not reserve test directory")
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ignored = std::fs::remove_dir_all(&self.0);
    }
}

pub fn id<T>(value: &str, build: impl FnOnce(String) -> Result<T, plexmaton_core::IdError>) -> T {
    build(value.to_owned()).unwrap_or_else(|error| panic!("fixture identity: {error}"))
}

pub fn session(value: &str) -> SessionId {
    id(value, SessionId::new)
}

pub fn agent_created(journal: &SessionJournal, ordinal: u64) -> JournalRecord {
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
    journal: &SessionJournal,
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
        entry: Box::new(SessionEntry {
            id: id(&format!("entry-{ordinal}"), SessionEntryId::new),
            parent_id: journal
                .head_target(&head)
                .unwrap_or_else(|error| panic!("main target: {error:?}"))
                .cloned(),
            payload,
        }),
    }
}
