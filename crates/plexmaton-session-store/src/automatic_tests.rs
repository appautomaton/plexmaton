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

/// CIN-2/JRN-4: the first collaboration input materializes the same exact bootstrap boundary.
#[test]
fn cin_2_automatic_journal_materializes_first_collaboration_turn() {
    use plexmaton_agent::collaboration::{
        CollaborationEvent, CollaborationLedger, CollaborationLimits, CollaborationText,
        MailEndpoint, Preparation,
    };
    use plexmaton_core::{
        CollaborationId, CollaborationItemId, ConversationId, DelegationId, TurnId,
    };
    let (root, mut journal, mut agent) = fixture();
    for record in agent.announce("Plexmaton").records {
        journal.append(record).expect("bootstrap");
    }
    assert!(!root.path().join("home").exists());
    let mut ledger = CollaborationLedger::new(
        CollaborationId::new("collaboration").expect("id"),
        CollaborationLimits::default(),
    )
    .expect("ledger");
    let Preparation::Append(record) = ledger
        .prepare(
            CollaborationItemId::new("create").expect("id"),
            CollaborationEvent::DelegationCreated {
                delegation: DelegationId::new("task").expect("id"),
                delegator: MailEndpoint {
                    conversation: agent.journal().conversation_id().clone(),
                    agent: AgentId::new("primary").expect("id"),
                },
                worker: MailEndpoint {
                    conversation: ConversationId::new("worker-session").expect("id"),
                    agent: AgentId::new("worker").expect("id"),
                },
                task: CollaborationText::new("Inspect files").expect("task"),
            },
        )
        .expect("create delegation")
    else {
        panic!("new record")
    };
    ledger.apply(*record).expect("apply creation");
    let id = CollaborationItemId::new("admitted").expect("id");
    let boundary = agent
        .collaboration_boundary(TurnId::new("collaboration-turn").expect("id"))
        .expect("boundary");
    let Preparation::Append(record) = ledger
        .prepare_turn(id.clone(), boundary, None)
        .expect("prepare")
    else {
        panic!("new turn")
    };
    ledger.apply(*record).expect("admit");
    let source = ledger
        .resolve_turn(&ledger.item_reference(&id).expect("ref"))
        .expect("source");
    let reaction = agent
        .start_collaboration_turn(&source, UnixMillis::new(2345))
        .expect("start");
    for record in reaction.records {
        journal.append(record).expect("persist inclusion");
    }
    let path = journal.path().to_path_buf();
    drop(journal);
    let reopened = JournalFile::open(&path).expect("reopen");
    assert_eq!(reopened.journal(), agent.journal());
    assert!(matches!(
        reopened
            .journal()
            .project(agent.selected_head())
            .expect("projection")
            .request()
            .atoms[0]
            .value(),
        plexmaton_agent::ContextAtomValue::Collaboration(_)
    ));
    assert!(
        !std::fs::read_to_string(&path)
            .expect("session bytes")
            .contains("Inspect files")
    );
}
