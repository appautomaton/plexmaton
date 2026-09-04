use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use plexmaton_agent::{JournalEntryPayload, JournalRecord, JournalSequence, SessionEntry};
use plexmaton_core::{AgentId, AgentStatus, HeadName, JournalRecordId, SessionEntryId, SessionId};

use super::{JournalFile, JournalRecovery, MAX_JOURNAL_LINE_BYTES, StoreError, WriterState};

fn id<T>(value: &str, build: impl FnOnce(String) -> Result<T, plexmaton_core::IdError>) -> T {
    build(value.to_owned()).unwrap_or_else(|error| panic!("fixture identity: {error}"))
}

fn record(journal: &plexmaton_agent::SessionJournal, label: String) -> JournalRecord {
    let head = id("main", HeadName::new);
    JournalRecord::AppendEntry {
        sequence: journal.next_sequence(),
        record_id: id("record-1", JournalRecordId::new),
        head: head.clone(),
        expected_head_revision: journal
            .head_revision(&head)
            .unwrap_or_else(|error| panic!("head revision: {error:?}")),
        entry: Box::new(SessionEntry {
            id: id("entry-1", SessionEntryId::new),
            parent_id: None,
            payload: JournalEntryPayload::AgentCreated {
                agent_id: id("agent-a", AgentId::new),
                label,
                status: AgentStatus::Idle,
            },
        }),
    }
}

fn path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "plexmaton-session-unit-{label}-{}",
        std::process::id()
    ))
}

/// JRN-4: a filesystem write error returns the record and poisons the partial writer.
#[test]
fn jrn_4_write_failure_changes_no_memory_and_requires_reopen() {
    let path = path("write-failure");
    let _stale = std::fs::remove_file(&path);
    std::fs::write(&path, b"fixture").unwrap_or_else(|error| panic!("write fixture file: {error}"));
    let session_id = id("session-a", SessionId::new);
    let journal = plexmaton_agent::SessionJournal::new(session_id);
    let attempted = record(&journal, "Plexmaton".to_owned());
    let mut store = JournalFile {
        path: path.clone(),
        file: File::open(&path).unwrap_or_else(|error| panic!("open read-only file: {error}")),
        journal,
        recovery: JournalRecovery::Clean,
        state: WriterState::Ready,
    };

    let failure = store
        .append(attempted.clone())
        .err()
        .unwrap_or_else(|| panic!("read-only append unexpectedly succeeded"));
    assert!(matches!(failure.error(), StoreError::Io { .. }));
    assert_eq!(failure.record(), &attempted);
    assert!(store.journal().records().is_empty());
    let retry = store
        .append(attempted)
        .err()
        .unwrap_or_else(|| panic!("poisoned append unexpectedly succeeded"));
    assert!(matches!(retry.error(), StoreError::WriterPoisoned));
    drop(store);
    std::fs::remove_file(path).unwrap_or_else(|error| panic!("remove fixture: {error}"));
}

/// JRN-4: line bounds fail before writing and leave a healthy writer usable.
#[test]
fn jrn_4_oversized_record_is_returned_without_poisoning_the_writer() {
    let path = path("oversized");
    let _stale = std::fs::remove_file(&path);
    let session_id = id("session-a", SessionId::new);
    let mut store = JournalFile::create(&path, session_id)
        .unwrap_or_else(|error| panic!("create store: {error}"));
    let oversized = record(&store.journal, "x".repeat(MAX_JOURNAL_LINE_BYTES));
    let failure = store
        .append(oversized)
        .err()
        .unwrap_or_else(|| panic!("oversized append unexpectedly succeeded"));
    assert!(matches!(failure.error(), StoreError::LineTooLarge { .. }));
    assert!(store.journal().records().is_empty());
    store
        .append(record(&store.journal, "Plexmaton".to_owned()))
        .unwrap_or_else(|failure| panic!("valid append after refusal: {}", failure.error()));
    drop(store);
    std::fs::remove_file(path).unwrap_or_else(|error| panic!("remove fixture: {error}"));
}

/// JRN-4: a short write leaves only an isolatable tail and returns exact ownership.
#[test]
fn jrn_4_partial_write_reopens_at_the_last_complete_record() {
    let path = path("partial-write");
    let _stale = std::fs::remove_file(&path);
    let mut store = JournalFile::create(&path, id("session-a", SessionId::new))
        .unwrap_or_else(|error| panic!("create store: {error}"));
    let attempted = record(&store.journal, "Plexmaton".to_owned());
    let failure = store
        .append_with(attempted.clone(), |file, encoded| {
            file.write_all(&encoded[..encoded.len() / 2])?;
            Err(std::io::Error::other("injected short write"))
        })
        .err()
        .unwrap_or_else(|| panic!("partial append unexpectedly succeeded"));
    assert_eq!(failure.record(), &attempted);
    assert!(store.journal().records().is_empty());
    drop(store);

    let reopened =
        JournalFile::open(&path).unwrap_or_else(|error| panic!("recover partial append: {error}"));
    let tail_path = match reopened.recovery() {
        JournalRecovery::IsolatedFinalTail { path, .. } => path.clone(),
        other => panic!("expected isolated tail, got {other:?}"),
    };
    assert!(reopened.journal().records().is_empty());
    drop(reopened);
    std::fs::remove_file(path).unwrap_or_else(|error| panic!("remove fixture: {error}"));
    std::fs::remove_file(tail_path).unwrap_or_else(|error| panic!("remove isolated tail: {error}"));
}

/// JRN-4: a writer with an uncertain tail cannot publish a fork from stale memory.
#[test]
fn jrn_4_poisoned_writer_cannot_fork() {
    let source_path = path("poisoned-fork-source");
    let destination = path("poisoned-fork-destination");
    let _stale_source = std::fs::remove_file(&source_path);
    let _stale_destination = std::fs::remove_file(&destination);
    let mut store = JournalFile::create(&source_path, id("session-a", SessionId::new))
        .unwrap_or_else(|error| panic!("create store: {error}"));
    let attempted = record(&store.journal, "Plexmaton".to_owned());
    let failure = store
        .append_with(attempted, |file, encoded| {
            file.write_all(&encoded[..encoded.len() / 2])?;
            Err(std::io::Error::other("injected short write"))
        })
        .err()
        .unwrap_or_else(|| panic!("partial append unexpectedly succeeded"));
    assert!(matches!(failure.error(), StoreError::Io { .. }));

    assert!(matches!(
        store.fork(&destination, id("session-b", SessionId::new)),
        Err(StoreError::WriterPoisoned)
    ));
    assert!(!destination.exists());
    drop(store);
    std::fs::remove_file(source_path)
        .unwrap_or_else(|error| panic!("remove source fixture: {error}"));
}

/// JRN-4: an error after complete JSON requires reopen reconciliation before any retry.
#[test]
fn jrn_4_newline_write_failure_recovers_the_record_as_committed() {
    let path = path("newline-write");
    let _stale = std::fs::remove_file(&path);
    let mut store = JournalFile::create(&path, id("session-a", SessionId::new))
        .unwrap_or_else(|error| panic!("create store: {error}"));
    let attempted = record(&store.journal, "Plexmaton".to_owned());
    let failure = store
        .append_with(attempted.clone(), |file, encoded| {
            file.write_all(&encoded[..encoded.len().saturating_sub(1)])?;
            Err(std::io::Error::other("injected newline failure"))
        })
        .err()
        .unwrap_or_else(|| panic!("newline failure unexpectedly succeeded"));
    assert_eq!(failure.record(), &attempted);
    assert!(store.journal().records().is_empty());
    drop(store);

    let reopened = JournalFile::open(&path)
        .unwrap_or_else(|error| panic!("reconcile newline failure: {error}"));
    assert_eq!(reopened.recovery(), &JournalRecovery::AddedFinalNewline);
    assert_eq!(reopened.journal().records(), [attempted]);
    drop(reopened);
    std::fs::remove_file(path).unwrap_or_else(|error| panic!("remove fixture: {error}"));
}

/// JRN-4: header encoding failure never reserves an unusable destination.
#[test]
fn jrn_4_failed_header_encoding_leaves_no_file() {
    let path = path("header-bound");
    let _stale = std::fs::remove_file(&path);
    let oversized = id(&"x".repeat(MAX_JOURNAL_LINE_BYTES), SessionId::new);

    assert!(matches!(
        JournalFile::create(&path, oversized),
        Err(StoreError::LineTooLarge { line: 0, .. })
    ));
    assert!(!path.exists());
}

/// JRN-4: semantic validation precedes bytes and does not poison a healthy writer.
#[test]
fn jrn_4_rejected_append_writes_nothing_and_returns_exact_ownership() {
    let path = path("rejected-record");
    let _stale = std::fs::remove_file(&path);
    let mut store = JournalFile::create(&path, id("session-a", SessionId::new))
        .unwrap_or_else(|error| panic!("create store: {error}"));
    let before = std::fs::read(&path).unwrap_or_else(|error| panic!("read header: {error}"));
    let invalid = JournalRecord::CreateHead {
        sequence: JournalSequence::new(99),
        record_id: id("invalid-record", JournalRecordId::new),
        head: id("branch", HeadName::new),
        at: None,
    };
    let failure = store
        .append(invalid.clone())
        .err()
        .unwrap_or_else(|| panic!("invalid record unexpectedly appended"));

    assert!(matches!(failure.error(), StoreError::RejectedRecord { .. }));
    assert_eq!(failure.record(), &invalid);
    assert_eq!(
        std::fs::read(&path).unwrap_or_else(|error| panic!("reread header: {error}")),
        before
    );
    assert!(store.journal().records().is_empty());
    store
        .append(record(&store.journal, "Plexmaton".to_owned()))
        .unwrap_or_else(|failure| panic!("valid append after rejection: {}", failure.error()));
    drop(store);
    std::fs::remove_file(path).unwrap_or_else(|error| panic!("remove fixture: {error}"));
}
