mod support;
#[path = "jsonl/timing.rs"]
mod timing;

use std::fs::OpenOptions;
use std::io::Write;

use plexmaton_agent::{
    JournalEntryPayload, JournalError, JournalRecord, JournalSequence, UnixMillis,
};
use plexmaton_core::{
    AgentId, AgentStatus, HeadName, JournalRecordId, TranscriptItemId, TranscriptRole,
};
use plexmaton_session_store::{JournalFile, JournalRecovery, StoreError};

use support::{TestDir, agent_created, append, id, session};

fn append_raw(path: &std::path::Path, record: &JournalRecord) {
    let value = serde_json::to_value(record)
        .unwrap_or_else(|error| panic!("encode raw record value: {error}"));
    append_raw_value(path, &value);
}

fn append_raw_value(path: &std::path::Path, value: &serde_json::Value) {
    let mut file = OpenOptions::new()
        .append(true)
        .open(path)
        .unwrap_or_else(|error| panic!("open raw journal: {error}"));
    serde_json::to_writer(&mut file, value)
        .unwrap_or_else(|error| panic!("encode raw record: {error}"));
    file.write_all(b"\n")
        .unwrap_or_else(|error| panic!("terminate raw record: {error}"));
}

/// TIM-1/JRN-4: file loading rejects wire shapes that bypass typed turn chronology.
#[test]
fn tim_1_jsonl_rejects_timeless_user_and_unscoped_lifecycle_records() {
    let directory = TestDir::new("unscoped-turn-wire");
    let agent_id = id("agent-a", AgentId::new);

    let running_path = directory.path().join("running-agent.jsonl");
    let running = JournalFile::create(&running_path, session("running-agent"), UnixMillis::EPOCH)
        .unwrap_or_else(|error| panic!("create running fixture: {error}"));
    let invalid = append(
        running.journal(),
        1,
        JournalEntryPayload::AgentCreated {
            agent_id: agent_id.clone(),
            label: "Agent A".to_owned(),
            status: AgentStatus::Running,
        },
    );
    drop(running);
    append_raw(&running_path, &invalid);
    assert!(matches!(
        JournalFile::open(&running_path),
        Err(StoreError::RejectedRecord {
            reason: JournalError::InvalidInitialAgentStatus(_),
            ..
        })
    ));

    let status_path = directory.path().join("unscoped-status.jsonl");
    let mut status =
        JournalFile::create(&status_path, session("unscoped-status"), UnixMillis::EPOCH)
            .unwrap_or_else(|error| panic!("create status fixture: {error}"));
    status
        .append(agent_created(status.journal(), 1))
        .unwrap_or_else(|failure| panic!("announce status fixture: {failure:?}"));
    let template = append(
        status.journal(),
        2,
        JournalEntryPayload::RuntimeWarning {
            agent_id: agent_id.clone(),
            item_id: id("status-placeholder", TranscriptItemId::new),
            message: "placeholder".to_owned(),
        },
    );
    let mut invalid_status = serde_json::to_value(template)
        .unwrap_or_else(|error| panic!("encode status template: {error}"));
    invalid_status["entry"]["payload"] = serde_json::json!({
        "type": "agent_status_changed",
        "agent_id": "agent-a",
        "status": "waiting"
    });
    drop(status);
    append_raw_value(&status_path, &invalid_status);
    assert!(matches!(
        JournalFile::open(&status_path),
        Err(StoreError::MalformedLine { line: 3, .. })
    ));

    let user_path = directory.path().join("timeless-user.jsonl");
    let mut user = JournalFile::create(&user_path, session("timeless-user"), UnixMillis::EPOCH)
        .unwrap_or_else(|error| panic!("create user fixture: {error}"));
    user.append(agent_created(user.journal(), 1))
        .unwrap_or_else(|failure| panic!("announce user fixture: {failure:?}"));
    let invalid_user = append(
        user.journal(),
        2,
        JournalEntryPayload::Message {
            agent_id,
            item_id: id("timeless-user-item", TranscriptItemId::new),
            role: TranscriptRole::User,
            text: "missing chronology".to_owned(),
        },
    );
    drop(user);
    append_raw(&user_path, &invalid_user);
    assert!(matches!(
        JournalFile::open(&user_path),
        Err(StoreError::RejectedRecord {
            reason: JournalError::TimelessUserMessage(_),
            ..
        })
    ));
}

/// JRN-4: append returns only after another handle can read the complete record.
#[test]
fn jrn_4_create_append_reopen_and_immediate_visibility() {
    let directory = TestDir::new("round-trip");
    let path = directory.path().join("session.jsonl");
    let session_id = session("session-a");
    let created_at = UnixMillis::new(1_788_537_600_123);
    let mut store = JournalFile::create(&path, session_id.clone(), created_at)
        .unwrap_or_else(|error| panic!("create store: {error}"));
    let record = agent_created(store.journal(), 1);
    store
        .append(record.clone())
        .unwrap_or_else(|failure| panic!("append record: {}", failure.error()));

    let visible = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read from second handle: {error}"));
    assert_eq!(visible.lines().count(), 2);
    assert!(visible.contains("agent_created"));
    let header: serde_json::Value = serde_json::from_str(
        visible
            .lines()
            .next()
            .unwrap_or_else(|| panic!("journal header is missing")),
    )
    .unwrap_or_else(|error| panic!("decode journal header: {error}"));
    assert_eq!(
        header,
        serde_json::json!({
            "format": "plexmaton.session",
            "schema": "2026-09-04",
            "session_id": "session-a",
            "created_at_unix_ms": 1_788_537_600_123_u64,
        })
    );
    drop(store);

    let reopened = JournalFile::open(&path).unwrap_or_else(|error| panic!("reopen store: {error}"));
    assert_eq!(reopened.journal().session_id(), &session_id);
    assert_eq!(reopened.journal().created_at_unix_ms(), created_at);
    assert_eq!(reopened.journal().records(), [record]);
    assert_eq!(reopened.recovery(), &JournalRecovery::Clean);
}

/// JRN-4: one process cannot acquire two writers for the same session file.
#[test]
fn jrn_4_a_second_writer_is_refused_until_the_owner_closes() {
    let directory = TestDir::new("lock");
    let path = directory.path().join("session.jsonl");
    let first = JournalFile::create(&path, session("session-a"), UnixMillis::EPOCH)
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
    let mut store = JournalFile::create(&path, session("session-a"), UnixMillis::EPOCH)
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
    let mut store = JournalFile::create(&path, session("session-a"), UnixMillis::EPOCH)
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
    let mut store = JournalFile::create(&path, session("session-a"), UnixMillis::EPOCH)
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
    let mut source = JournalFile::create(&source_path, session("source"), UnixMillis::EPOCH)
        .unwrap_or_else(|error| panic!("create source: {error}"));
    source
        .append(agent_created(source.journal(), 1))
        .unwrap_or_else(|failure| panic!("append source: {}", failure.error()));

    let forked = source
        .fork(&destination, session("forked"), UnixMillis::new(20))
        .unwrap_or_else(|error| panic!("fork session: {error}"));
    assert_eq!(forked.journal().session_id(), &session("forked"));
    assert_eq!(forked.journal().created_at_unix_ms(), UnixMillis::new(20));
    assert_eq!(forked.journal().records(), source.journal().records());
    let published =
        std::fs::read(&destination).unwrap_or_else(|error| panic!("read published fork: {error}"));
    assert!(matches!(
        JournalFile::open(&destination),
        Err(StoreError::WriterLocked)
    ));
    assert!(matches!(
        source.fork(&destination, session("other"), UnixMillis::new(30)),
        Err(StoreError::ForkDestinationExists)
    ));
    assert_eq!(
        std::fs::read(&destination)
            .unwrap_or_else(|error| panic!("reread published fork: {error}")),
        published
    );
    drop(forked);
    let reopened = JournalFile::open(&destination)
        .unwrap_or_else(|error| panic!("reopen fork after owner closes: {error}"));
    assert_eq!(reopened.journal().created_at_unix_ms(), UnixMillis::new(20));
}

/// JRN-3: a foreign schema epoch is refused before any record is decoded.
#[test]
fn jrn_3_a_foreign_schema_epoch_is_refused_before_records() {
    let directory = TestDir::new("schema-epoch");
    let path = directory.path().join("session.jsonl");
    let store = JournalFile::create(&path, session("session-a"), UnixMillis::new(10))
        .unwrap_or_else(|error| panic!("create store: {error}"));
    drop(store);
    let source =
        std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read header: {error}"));
    let mut header: serde_json::Value = serde_json::from_str(source.trim_end())
        .unwrap_or_else(|error| panic!("decode header: {error}"));
    header["schema"] = serde_json::Value::from("2099-01-01");
    let changed = format!(
        "{}\n",
        serde_json::to_string(&header)
            .unwrap_or_else(|error| panic!("encode changed header: {error}"))
    );
    std::fs::write(&path, format!("{changed}not-json\n"))
        .unwrap_or_else(|error| panic!("write changed header: {error}"));

    assert!(matches!(
        JournalFile::open(&path),
        Err(StoreError::UnsupportedSchema(schema)) if schema == "2099-01-01"
    ));
}

/// JRN-4: a syntactically valid but semantically invalid final record is not tail recovery.
#[test]
fn jrn_4_invalid_sequence_is_refused_even_on_the_final_line() {
    let directory = TestDir::new("sequence");
    let path = directory.path().join("session.jsonl");
    let store = JournalFile::create(&path, session("session-a"), UnixMillis::EPOCH)
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
    let store = JournalFile::create(&path, session("session-a"), UnixMillis::EPOCH)
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
    let store = JournalFile::create(&path, session("session-a"), UnixMillis::EPOCH)
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
    let store = JournalFile::create(&path, session("session-a"), UnixMillis::EPOCH)
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
