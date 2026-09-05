use super::*;
use plexmaton_agent::{Agent, ApprovalPolicy, Input, SessionMetadata, TurnBudget, UnixMillis};
use plexmaton_core::AgentId;

fn fixture() -> (PathBuf, AutomaticJournal, Agent) {
    let root = std::env::temp_dir().join(format!("plexmaton-lazy-{}", uuid::Uuid::now_v7()));
    let journal = AutomaticJournal::new(&root, UnixMillis::new(1234));
    let agent = Agent::for_session(
        AgentId::new("primary").expect("id"),
        SessionMetadata::new(
            journal.metadata().session_id().clone(),
            UnixMillis::new(1234),
        ),
        TurnBudget::default(),
        ApprovalPolicy::default(),
    );
    (root, journal, agent)
}

/// JRN-4/JRN-7: the only buffered record is bootstrap; successful first input flushes the exact prefix.
#[test]
fn automatic_journal_materializes_exact_bootstrap_and_first_turn_only_on_input() {
    let (root, mut journal, mut agent) = fixture();
    let bootstrap = agent.announce("Plexmaton").records;
    assert_eq!(bootstrap.len(), 1);
    journal.append(bootstrap[0].clone()).expect("bootstrap");
    assert!(!root.exists(), "no directory creation on announcement");
    let records = agent
        .handle_at(
            Input::Submitted {
                text: "first input".into(),
            },
            UnixMillis::new(2345),
        )
        .records;
    let path = journal.path().to_path_buf();
    for record in records {
        journal.append(record).expect("first input");
    }
    assert!(path.is_file());
    drop(journal);
    let reopened = JournalFile::open(&path).expect("reopen");
    assert_eq!(reopened.journal(), agent.journal());
    assert_eq!(
        reopened.journal().created_at_unix_ms(),
        UnixMillis::new(1234)
    );
    drop(reopened);
    std::fs::remove_dir_all(root).expect("remove unique fixture");
}

/// JRN-4: an unsupported pre-input mutation cannot silently accumulate or create an empty file.
#[test]
fn automatic_journal_rejects_extra_bootstrap_without_creating_storage() {
    let (root, mut journal, mut agent) = fixture();
    let bootstrap = agent.announce("Plexmaton").records.remove(0);
    journal.append(bootstrap.clone()).expect("bootstrap");
    assert!(matches!(
        journal.append(bootstrap.clone()),
        Err(StoreError::InvalidAutomaticBootstrap)
    ));
    assert!(matches!(
        journal.append(bootstrap),
        Err(StoreError::WriterPoisoned)
    ));
    assert!(!root.exists());
}
