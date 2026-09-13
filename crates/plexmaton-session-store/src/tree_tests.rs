use std::io::Write;

use plexmaton_agent::{
    ConversationEntry, JournalEntryPayload, JournalRecord, JournalSequence, UnixMillis,
};
use plexmaton_core::{
    AgentId, AgentStatus, ConversationEntryId, ConversationId, HeadName, JournalRecordId,
};

use super::test_support::TestDir;
use super::{JournalFile, JournalRecovery};

fn id<T>(value: &str, build: impl FnOnce(String) -> Result<T, plexmaton_core::IdError>) -> T {
    build(value.to_owned()).unwrap_or_else(|error| panic!("fixture identity: {error}"))
}

fn agent_created(journal: &plexmaton_agent::ConversationJournal) -> JournalRecord {
    let head = journal.selected_head().clone();
    JournalRecord::AppendEntry {
        sequence: journal.next_sequence(),
        record_id: id("record-1", JournalRecordId::new),
        expected_head_revision: journal
            .head_revision(&head)
            .unwrap_or_else(|error| panic!("head revision: {error:?}")),
        head,
        entry: Box::new(ConversationEntry {
            id: id("entry-1", ConversationEntryId::new),
            parent_id: None,
            payload: JournalEntryPayload::AgentCreated {
                agent_id: id("agent-a", AgentId::new),
                label: "Plexmaton".to_owned(),
                status: AgentStatus::Idle,
            },
        }),
    }
}

fn fork_and_select(
    journal: &plexmaton_agent::ConversationJournal,
    destination: &str,
    at: Option<ConversationEntryId>,
) -> JournalRecord {
    let source = journal.selected_head().clone();
    JournalRecord::ForkAndSelectHead {
        sequence: journal.next_sequence(),
        record_id: id("record-fork", JournalRecordId::new),
        expected_source_revision: journal
            .head_revision(&source)
            .unwrap_or_else(|error| panic!("source revision: {error:?}")),
        source,
        destination: id(destination, HeadName::new),
        at,
    }
}

/// TRE-3: a durable fork/select survives reopen; the original header bytes stay intact.
#[test]
fn tre_3_fork_and_select_survives_reopen_without_rewriting_the_header() {
    let directory = TestDir::new("tree-reopen");
    let path = directory.path().join("session.jsonl");
    let mut store = JournalFile::create(
        &path,
        id("session-a", ConversationId::new),
        UnixMillis::EPOCH,
    )
    .unwrap_or_else(|error| panic!("create store: {error}"));
    let header = std::fs::read(&path).unwrap_or_else(|error| panic!("read header: {error}"));
    store
        .append(agent_created(store.journal()))
        .unwrap_or_else(|failure| panic!("append created: {}", failure.error()));
    let at = store
        .journal()
        .head_target(store.journal().selected_head())
        .unwrap_or_else(|error| panic!("selected target: {error:?}"))
        .cloned();
    store
        .append(fork_and_select(store.journal(), "rewound", at))
        .unwrap_or_else(|failure| panic!("append fork: {}", failure.error()));
    assert_eq!(
        store.journal().selected_head(),
        &id("rewound", HeadName::new)
    );
    drop(store);

    let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("read journal: {error}"));
    assert_eq!(&bytes[..header.len()], header.as_slice());
    let reopened =
        JournalFile::open(&path).unwrap_or_else(|error| panic!("reopen selected head: {error}"));
    assert_eq!(
        reopened.journal().selected_head(),
        &id("rewound", HeadName::new)
    );
    assert_eq!(reopened.recovery(), &JournalRecovery::Clean);
}

/// TRE-3: a torn ForkAndSelectHead line cannot report a half-created selection.
#[test]
fn tre_3_partial_fork_and_select_write_does_not_select_or_create_the_destination() {
    let directory = TestDir::new("tree-partial");
    let path = directory.path().join("session.jsonl");
    let mut store = JournalFile::create(
        &path,
        id("session-a", ConversationId::new),
        UnixMillis::EPOCH,
    )
    .unwrap_or_else(|error| panic!("create store: {error}"));
    store
        .append(agent_created(store.journal()))
        .unwrap_or_else(|failure| panic!("append created: {}", failure.error()));
    let at = store
        .journal()
        .head_target(store.journal().selected_head())
        .unwrap_or_else(|error| panic!("selected target: {error:?}"))
        .cloned();
    let attempted = fork_and_select(store.journal(), "rewound", at);
    let failure = store
        .append_with(attempted.clone(), |file, encoded| {
            file.write_all(&encoded[..encoded.len() / 2])?;
            Err(std::io::Error::other("injected short write"))
        })
        .err()
        .unwrap_or_else(|| panic!("partial fork unexpectedly succeeded"));
    assert_eq!(failure.record(), &attempted);
    assert_eq!(store.journal().selected_head(), &id("main", HeadName::new));
    drop(store);

    let reopened =
        JournalFile::open(&path).unwrap_or_else(|error| panic!("recover partial fork: {error}"));
    let tail_path = match reopened.recovery() {
        JournalRecovery::IsolatedFinalTail { path, .. } => path.clone(),
        other => panic!("expected isolated tail, got {other:?}"),
    };
    assert_eq!(
        reopened.journal().selected_head(),
        &id("main", HeadName::new)
    );
    assert!(
        reopened
            .journal()
            .head_revision(&id("rewound", HeadName::new))
            .is_err(),
        "torn fork must not create the destination head"
    );
    drop(reopened);
    std::fs::remove_file(tail_path).unwrap_or_else(|error| panic!("remove isolated tail: {error}"));
}

/// TRE-3: CreateHead then SelectHead as two appends is not the rewind mutation; a rejected
/// fork leaves the writer usable and selection unchanged.
#[test]
fn tre_3_rejected_fork_and_select_writes_nothing() {
    let directory = TestDir::new("tree-rejected");
    let path = directory.path().join("session.jsonl");
    let mut store = JournalFile::create(
        &path,
        id("session-a", ConversationId::new),
        UnixMillis::EPOCH,
    )
    .unwrap_or_else(|error| panic!("create store: {error}"));
    store
        .append(agent_created(store.journal()))
        .unwrap_or_else(|failure| panic!("append created: {}", failure.error()));
    let before = std::fs::read(&path).unwrap_or_else(|error| panic!("read before: {error}"));
    let invalid = JournalRecord::ForkAndSelectHead {
        sequence: JournalSequence::new(99),
        record_id: id("invalid-fork", JournalRecordId::new),
        source: id("main", HeadName::new),
        expected_source_revision: plexmaton_agent::HeadRevision::new(0),
        destination: id("rewound", HeadName::new),
        at: None,
    };
    let failure = store
        .append(invalid.clone())
        .err()
        .unwrap_or_else(|| panic!("invalid fork unexpectedly appended"));
    assert_eq!(failure.record(), &invalid);
    assert_eq!(
        std::fs::read(&path).unwrap_or_else(|error| panic!("reread: {error}")),
        before
    );
    assert_eq!(store.journal().selected_head(), &id("main", HeadName::new));
}

/// TRE-3/TRE-8/JRN-3: additive node labels and head edits survive reopen on both supported headers
/// without relabeling the epoch, rewriting old bytes or changing the selected path's context.
#[test]
fn tre_3_8_labels_and_head_edits_reopen_without_rewriting_supported_headers() {
    for epoch in ["2026-09-04", "2026-09-05"] {
        let directory = TestDir::new("tree-metadata-reopen");
        let path = directory.path().join("session.jsonl");
        let store = JournalFile::create(
            &path,
            id("session-meta", ConversationId::new),
            UnixMillis::EPOCH,
        )
        .expect("create");
        drop(store);
        // Fixture setup chooses an old supported header; production opening never rewrites it.
        let header = std::fs::read_to_string(&path)
            .expect("header")
            .replace("2026-09-05", epoch);
        std::fs::write(&path, &header).expect("fixture header");
        let mut store = JournalFile::open(&path).expect("supported header");
        store
            .append(agent_created(store.journal()))
            .expect("announce");
        let source = id("entry-1", ConversationEntryId::new);
        let label = plexmaton_core::TreeLabel::new("decision 中文  ".to_owned()).expect("label");
        let prefix = std::fs::read(&path).expect("existing bytes");
        let original = store
            .journal()
            .project(store.journal().selected_head())
            .expect("projection");
        store
            .append(JournalRecord::SetEntryLabel {
                sequence: store.journal().next_sequence(),
                record_id: id("label-set", JournalRecordId::new),
                entry_id: source.clone(),
                label: Some(label.clone()),
            })
            .expect("label");
        store
            .append(fork_and_select(
                store.journal(),
                "branch",
                Some(source.clone()),
            ))
            .expect("fork");
        let branch = id("branch", HeadName::new);
        let renamed = id("reviewed", HeadName::new);
        store
            .append(JournalRecord::RenameHead {
                sequence: store.journal().next_sequence(),
                record_id: id("rename", JournalRecordId::new),
                head: branch.clone(),
                expected_head_revision: store.journal().head_revision(&branch).expect("head"),
                renamed: renamed.clone(),
            })
            .expect("rename selected");
        let main = id("main", HeadName::new);
        store
            .append(JournalRecord::AbandonHead {
                sequence: store.journal().next_sequence(),
                record_id: id("abandon", JournalRecordId::new),
                head: main.clone(),
                expected_head_revision: store.journal().head_revision(&main).expect("main"),
            })
            .expect("abandon inactive");
        drop(store);
        assert!(std::fs::read(&path).expect("bytes").starts_with(&prefix));
        let mut reopened = JournalFile::open(&path).expect("reopen metadata");
        assert_eq!(reopened.journal().selected_head(), &renamed);
        assert_eq!(reopened.journal().tree_label(&source), Some(&label));
        assert_eq!(
            reopened
                .journal()
                .project(&renamed)
                .expect("same projection"),
            original
        );
        assert!(reopened.journal().head_revision(&main).is_err());
        reopened
            .append(JournalRecord::SetEntryLabel {
                sequence: reopened.journal().next_sequence(),
                record_id: id("label-clear", JournalRecordId::new),
                entry_id: source.clone(),
                label: None,
            })
            .expect("clear label");
        drop(reopened);
        let reopened = JournalFile::open(&path).expect("reopen cleared label");
        assert_eq!(reopened.journal().tree_label(&source), None);
        assert_eq!(reopened.journal().selected_head(), &renamed);
        assert!(
            std::fs::read(&path)
                .expect("bytes")
                .starts_with(header.as_bytes())
        );
    }
}

/// TRE-8/JRN-4: an incomplete label append cannot change the in-memory or recovered annotation.
#[test]
fn tre_8_partial_label_write_keeps_the_prior_annotation() {
    let directory = TestDir::new("tree-label-partial");
    let path = directory.path().join("session.jsonl");
    let mut store = JournalFile::create(
        &path,
        id("session-meta", ConversationId::new),
        UnixMillis::EPOCH,
    )
    .expect("create");
    store
        .append(agent_created(store.journal()))
        .expect("announce");
    let source = id("entry-1", ConversationEntryId::new);
    let original = plexmaton_core::TreeLabel::new("original".to_owned()).expect("label");
    store
        .append(JournalRecord::SetEntryLabel {
            sequence: store.journal().next_sequence(),
            record_id: id("label-set", JournalRecordId::new),
            entry_id: source.clone(),
            label: Some(original.clone()),
        })
        .expect("label");
    let attempted = JournalRecord::SetEntryLabel {
        sequence: store.journal().next_sequence(),
        record_id: id("label-replace", JournalRecordId::new),
        entry_id: source.clone(),
        label: Some(plexmaton_core::TreeLabel::new("replacement".to_owned()).expect("label")),
    };
    let failure = store
        .append_with(attempted.clone(), |file, encoded| {
            file.write_all(&encoded[..encoded.len() / 2])?;
            Err(std::io::Error::other("injected partial label"))
        })
        .expect_err("partial write");
    assert_eq!(failure.record(), &attempted);
    assert_eq!(store.journal().tree_label(&source), Some(&original));
    drop(store);
    let reopened = JournalFile::open(&path).expect("recover");
    assert_eq!(reopened.journal().tree_label(&source), Some(&original));
    assert!(matches!(
        reopened.recovery(),
        JournalRecovery::IsolatedFinalTail { .. }
    ));
}
