use super::*;
use crate::test_support::TestDir;
use plexmaton_agent::{Agent, ApprovalPolicy, ConversationMetadata, Input, TurnBudget, UnixMillis};
use plexmaton_core::AgentId;

#[test]
fn test_directory_ownership_cleans_up_on_unwind() {
    let mut path = PathBuf::new();
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let directory = TestDir::new("unwind");
        path = directory.path().to_path_buf();
        std::fs::write(path.join("fixture"), b"owned").expect("write fixture");
        panic!("fixture assertion failed");
    }));
    assert!(failure.is_err());
    assert!(!path.exists());
}

fn fixture() -> (TestDir, AutomaticJournal, Agent) {
    let root = TestDir::new("lazy");
    let journal = AutomaticJournal::new(root.path().join("home"), UnixMillis::new(1234));
    let agent = Agent::for_conversation(
        AgentId::new("primary").expect("id"),
        ConversationMetadata::new(
            journal.metadata().conversation_id().clone(),
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
    assert!(
        !root.path().join("home").exists(),
        "no directory creation on announcement"
    );
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
    assert!(!root.path().join("home").exists());
}
