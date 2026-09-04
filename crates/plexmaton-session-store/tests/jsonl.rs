mod support;

use std::fs::OpenOptions;
use std::io::Write;

use plexmaton_agent::{JournalRecord, JournalSequence};
use plexmaton_core::{HeadName, JournalRecordId};
use plexmaton_session_store::{JournalFile, JournalRecovery, StoreError};

use support::{TestDir, agent_created, id, session};

/// JRN-4: append returns only after another handle can read the complete record.
#[test]
fn jrn_4_create_append_reopen_and_immediate_visibility() {
    let directory = TestDir::new("round-trip");
    let path = directory.path().join("session.jsonl");
    let session_id = session("session-a");
    let mut store = JournalFile::create(&path, session_id.clone())
        .unwrap_or_else(|error| panic!("create store: {error}"));
    let record = agent_created(store.journal(), 1);
    store
        .append(record.clone())
        .unwrap_or_else(|failure| panic!("append record: {}", failure.error()));

    let visible = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read from second handle: {error}"));
    assert_eq!(visible.lines().count(), 2);
    assert!(visible.contains("agent_created"));
    drop(store);

    let reopened = JournalFile::open(&path).unwrap_or_else(|error| panic!("reopen store: {error}"));
    assert_eq!(reopened.journal().session_id(), &session_id);
    assert_eq!(reopened.journal().records(), [record]);
    assert_eq!(reopened.recovery(), &JournalRecovery::Clean);
}

/// JRN-4: one process cannot acquire two writers for the same session file.
#[test]
fn jrn_4_a_second_writer_is_refused_until_the_owner_closes() {
    let directory = TestDir::new("lock");
    let path = directory.path().join("session.jsonl");
    let first = JournalFile::create(&path, session("session-a"))
        .unwrap_or_else(|error| panic!("create store: {error}"));
    assert!(matches!(
        JournalFile::open(&path),
        Err(StoreError::WriterLocked)
    ));
    drop(first);
    let _second =
        JournalFile::open(&path).unwrap_or_else(|error| panic!("lock after close: {error}"));
}

/// JRN-4: a complete final value missing only its newline is repaired in place.
#[test]
fn jrn_4_valid_final_record_without_newline_is_repaired() {
    let directory = TestDir::new("newline");
    let path = directory.path().join("session.jsonl");
    let mut store = JournalFile::create(&path, session("session-a"))
        .unwrap_or_else(|error| panic!("create store: {error}"));
    store
        .append(agent_created(store.journal(), 1))
        .unwrap_or_else(|failure| panic!("append record: {}", failure.error()));
    drop(store);
    let length = std::fs::metadata(&path)
        .unwrap_or_else(|error| panic!("file metadata: {error}"))
        .len();
    OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap_or_else(|error| panic!("open for truncation: {error}"))
        .set_len(length.saturating_sub(1))
        .unwrap_or_else(|error| panic!("remove newline: {error}"));

    let repaired =
        JournalFile::open(&path).unwrap_or_else(|error| panic!("open repaired store: {error}"));
    assert_eq!(repaired.recovery(), &JournalRecovery::AddedFinalNewline);
    let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("read repaired file: {error}"));
    assert_eq!(bytes.last(), Some(&b'\n'));
    assert_eq!(repaired.journal().records().len(), 1);
}

/// JRN-4: a syntactically broken final fragment is retained beside the valid-prefix file.
#[test]
fn jrn_4_incomplete_final_tail_is_isolated() {
    let directory = TestDir::new("tail");
    let path = directory.path().join("session.jsonl");
    let mut store = JournalFile::create(&path, session("session-a"))
        .unwrap_or_else(|error| panic!("create store: {error}"));
    store
        .append(agent_created(store.journal(), 1))
        .unwrap_or_else(|failure| panic!("append record: {}", failure.error()));
    drop(store);
    let mut file = OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap_or_else(|error| panic!("open tail writer: {error}"));
    file.write_all(br#"{"kind":"append_entry""#)
        .unwrap_or_else(|error| panic!("write incomplete tail: {error}"));
    drop(file);

    let recovered =
        JournalFile::open(&path).unwrap_or_else(|error| panic!("recover store: {error}"));
    let JournalRecovery::IsolatedFinalTail {
        path: tail_path,
        bytes,
    } = recovered.recovery()
    else {
        panic!("expected isolated-tail recovery")
    };
    assert_eq!(*bytes, 22);
    assert_eq!(
        std::fs::read(tail_path).unwrap_or_else(|error| panic!("read isolated tail: {error}")),
        br#"{"kind":"append_entry""#
    );
    assert_eq!(recovered.journal().records().len(), 1);
    assert_eq!(
        std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read recovered journal: {error}"))
            .lines()
            .count(),
        2
    );
}

/// JRN-4: corruption before a later record is typed and the original file is untouched.
#[test]
fn jrn_4_middle_corruption_is_not_guessed_around() {
    let directory = TestDir::new("middle");
    let path = directory.path().join("session.jsonl");
    let mut store = JournalFile::create(&path, session("session-a"))
        .unwrap_or_else(|error| panic!("create store: {error}"));
    store
        .append(agent_created(store.journal(), 1))
        .unwrap_or_else(|failure| panic!("append first: {}", failure.error()));
    let second = agent_created(store.journal(), 2);
    let second_json = serde_json::to_string(&second)
        .unwrap_or_else(|error| panic!("encode second record: {error}"));
    drop(store);
    let before = std::fs::read(&path).unwrap_or_else(|error| panic!("read journal: {error}"));
    let mut corrupted = before.clone();
    corrupted.extend_from_slice(b"not-json\n");
    corrupted.extend_from_slice(second_json.as_bytes());
    corrupted.push(b'\n');
    std::fs::write(&path, &corrupted)
        .unwrap_or_else(|error| panic!("write corrupt journal: {error}"));

    assert!(matches!(
        JournalFile::open(&path),
        Err(StoreError::MalformedLine { line: 3, .. })
    ));
    assert_eq!(
        std::fs::read(&path).unwrap_or_else(|error| panic!("reread journal: {error}")),
        corrupted
    );
}

/// JRN-4: a fork is fully staged and publishes a separate session identity.
#[test]
fn jrn_4_fork_publishes_a_complete_sibling() {
    let directory = TestDir::new("fork");
    let source_path = directory.path().join("source.jsonl");
    let destination = directory.path().join("fork.jsonl");
    let mut source = JournalFile::create(&source_path, session("source"))
        .unwrap_or_else(|error| panic!("create source: {error}"));
    source
        .append(agent_created(source.journal(), 1))
        .unwrap_or_else(|failure| panic!("append source: {}", failure.error()));

    let forked = source
        .fork(&destination, session("forked"))
        .unwrap_or_else(|error| panic!("fork session: {error}"));
    assert_eq!(forked.journal().session_id(), &session("forked"));
    assert_eq!(forked.journal().records(), source.journal().records());
    let published =
        std::fs::read(&destination).unwrap_or_else(|error| panic!("read published fork: {error}"));
    assert!(matches!(
        JournalFile::open(&destination),
        Err(StoreError::WriterLocked)
    ));
    assert!(matches!(
        source.fork(&destination, session("other")),
        Err(StoreError::ForkDestinationExists)
    ));
    assert_eq!(
        std::fs::read(&destination)
            .unwrap_or_else(|error| panic!("reread published fork: {error}")),
        published
    );
    drop(forked);
    let _reopened = JournalFile::open(&destination)
        .unwrap_or_else(|error| panic!("reopen fork after owner closes: {error}"));
}

/// JRN-4: a header selects one explicit decoder and unknown versions stay typed.
#[test]
fn jrn_4_unknown_format_version_is_refused() {
    let directory = TestDir::new("version");
    let path = directory.path().join("session.jsonl");
    let store = JournalFile::create(&path, session("session-a"))
        .unwrap_or_else(|error| panic!("create store: {error}"));
    drop(store);
    let source =
        std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read header: {error}"));
    let mut header: serde_json::Value = serde_json::from_str(source.trim_end())
        .unwrap_or_else(|error| panic!("decode header: {error}"));
    header["version"] = serde_json::Value::from(99);
    let changed = format!(
        "{}\n",
        serde_json::to_string(&header)
            .unwrap_or_else(|error| panic!("encode changed header: {error}"))
    );
    std::fs::write(&path, changed).unwrap_or_else(|error| panic!("write changed header: {error}"));

    assert!(matches!(
        JournalFile::open(&path),
        Err(StoreError::UnsupportedVersion(99))
    ));
}

/// JRN-4: a syntactically valid but semantically invalid final record is not tail recovery.
#[test]
fn jrn_4_invalid_sequence_is_refused_even_on_the_final_line() {
    let directory = TestDir::new("sequence");
    let path = directory.path().join("session.jsonl");
    let store = JournalFile::create(&path, session("session-a"))
        .unwrap_or_else(|error| panic!("create store: {error}"));
    drop(store);
    let invalid = JournalRecord::CreateHead {
        sequence: JournalSequence::new(99),
        record_id: id("record-invalid", JournalRecordId::new),
        head: id("branch", HeadName::new),
        at: None,
    };
    let mut file = OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap_or_else(|error| panic!("open raw writer: {error}"));
    serde_json::to_writer(&mut file, &invalid)
        .unwrap_or_else(|error| panic!("write invalid record: {error}"));
    file.write_all(b"\n")
        .unwrap_or_else(|error| panic!("terminate invalid record: {error}"));
    drop(file);

    assert!(matches!(
        JournalFile::open(&path),
        Err(StoreError::RejectedRecord { line: 2, .. })
    ));
}

/// JRN-4: complete JSON with an unknown record kind is not mistaken for a torn write.
#[test]
fn jrn_4_unknown_final_record_kind_is_a_schema_failure() {
    let directory = TestDir::new("record-kind");
    let path = directory.path().join("session.jsonl");
    let store = JournalFile::create(&path, session("session-a"))
        .unwrap_or_else(|error| panic!("create store: {error}"));
    drop(store);
    let mut file = OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap_or_else(|error| panic!("open raw writer: {error}"));
    file.write_all(br#"{"kind":"future_record","sequence":1}"#)
        .unwrap_or_else(|error| panic!("write unknown record: {error}"));
    drop(file);
    let before = std::fs::read(&path).unwrap_or_else(|error| panic!("read journal: {error}"));

    assert!(matches!(
        JournalFile::open(&path),
        Err(StoreError::MalformedLine { line: 2, .. })
    ));
    assert_eq!(
        std::fs::read(&path).unwrap_or_else(|error| panic!("reread journal: {error}")),
        before
    );
}

/// JRN-4: duplicate JSON fields cannot be normalized into a different canonical mutation.
#[test]
fn jrn_4_duplicate_record_field_is_refused_without_tail_recovery() {
    let directory = TestDir::new("duplicate-field");
    let path = directory.path().join("session.jsonl");
    let store = JournalFile::create(&path, session("session-a"))
        .unwrap_or_else(|error| panic!("create store: {error}"));
    let record = agent_created(store.journal(), 1);
    drop(store);
    let encoded =
        serde_json::to_string(&record).unwrap_or_else(|error| panic!("encode record: {error}"));
    let duplicated = encoded.replacen("\"sequence\":1", "\"sequence\":1,\"sequence\":1", 1);
    let mut file = OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap_or_else(|error| panic!("open raw writer: {error}"));
    file.write_all(duplicated.as_bytes())
        .unwrap_or_else(|error| panic!("write duplicate field: {error}"));
    drop(file);

    assert!(matches!(
        JournalFile::open(&path),
        Err(StoreError::MalformedLine { line: 2, .. })
    ));
}

/// JRN-4: a newline-terminated syntax error is corruption even when it is last.
#[test]
fn jrn_4_terminated_invalid_final_line_is_not_tail_recovery() {
    let directory = TestDir::new("terminated-invalid");
    let path = directory.path().join("session.jsonl");
    let store = JournalFile::create(&path, session("session-a"))
        .unwrap_or_else(|error| panic!("create store: {error}"));
    drop(store);
    let mut file = OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap_or_else(|error| panic!("open raw writer: {error}"));
    file.write_all(b"not-json\n")
        .unwrap_or_else(|error| panic!("write invalid record: {error}"));
    drop(file);
    let before = std::fs::read(&path).unwrap_or_else(|error| panic!("read journal: {error}"));

    assert!(matches!(
        JournalFile::open(&path),
        Err(StoreError::MalformedLine { line: 2, .. })
    ));
    assert_eq!(
        std::fs::read(&path).unwrap_or_else(|error| panic!("reread journal: {error}")),
        before
    );
}
