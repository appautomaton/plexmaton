use std::fs::OpenOptions;
use std::io::Write;

use plexmaton_agent::collaboration::{
    CollaborationEvent, CollaborationLimits, CollaborationSequence, CollaborationText,
    DelegationAuthor, DelegationRevision, MailEndpoint, MailEnvelope, Preparation,
};
use plexmaton_core::{
    AgentId, CollaborationId, CollaborationItemId, ConversationId, DelegationId, MailId,
};

use super::{CollaborationAttempt, CollaborationFile, CollaborationStoreError};
use crate::{JournalRecovery, StoreError};

fn endpoint(name: &str) -> MailEndpoint {
    MailEndpoint {
        conversation: ConversationId::new(format!("session-{name}")).expect("fixture identity"),
        agent: AgentId::new(name).expect("fixture identity"),
    }
}

fn item(name: &str) -> CollaborationItemId {
    CollaborationItemId::new(name).expect("fixture identity")
}
fn text(value: &str) -> CollaborationText {
    CollaborationText::new(value).expect("fixture text")
}
fn delegation() -> DelegationId {
    DelegationId::new("task").expect("fixture identity")
}

fn creation() -> CollaborationEvent {
    CollaborationEvent::DelegationCreated {
        delegation: delegation(),
        delegator: endpoint("a"),
        worker: endpoint("b"),
        task: text("Inspect the parser"),
    }
}

fn mail() -> CollaborationEvent {
    CollaborationEvent::MailAccepted {
        mail: MailEnvelope {
            id: MailId::new("finding").expect("fixture identity"),
            from: endpoint("b"),
            to: endpoint("a"),
            summary: text("Found a parser boundary: 中😀\n\"quoted\"\\path"),
            artifacts: Vec::new(),
        },
    }
}

fn create(path: &std::path::Path) -> CollaborationFile {
    let mut file = CollaborationFile::create(
        path,
        CollaborationId::new("collaboration").expect("fixture identity"),
        CollaborationLimits::default(),
    )
    .expect("create");
    file.admit(item("create"), creation())
        .expect("create delegation");
    file
}

/// COL-1/COL-3/COL-4: the file reconstructs identical mail and task authority, and retry writes nothing.
#[test]
fn col_4_file_roundtrip_retains_attribution_and_exact_retry() {
    let directory = crate::test_support::TestDir::new("collaboration-roundtrip");
    let path = directory.path().join("private/log.jsonl");
    let mut file = create(&path);
    let receipt = file.admit(item("mail"), mail()).expect("admit mail");
    file.admit(
        item("user"),
        CollaborationEvent::TaskAmended {
            delegation: delegation(),
            expected: DelegationRevision(0),
            author: DelegationAuthor::User,
            task: text("Inspect without changing files"),
        },
    )
    .expect("user amendment");
    let expected = file.ledger().clone();
    let bytes = std::fs::read(&path).expect("read file");
    drop(file);
    let mut reopened = CollaborationFile::open(&path).expect("reopen");
    assert_eq!(reopened.ledger(), &expected);
    assert_eq!(
        reopened.admit(item("mail"), mail()).expect("retry"),
        receipt
    );
    assert_eq!(std::fs::read(&path).expect("read after retry"), bytes);
    assert_eq!(reopened.ledger().mail_for(&endpoint("a")).count(), 1);
}

/// COL-4/COL-5: every real-file byte cut is reconciled before any new receipt can be issued.
#[test]
fn col_4_uncertain_append_recovers_every_byte_cut_and_retries_once() {
    let directory = crate::test_support::TestDir::new("collaboration-cuts");
    let seed_path = directory.path().join("private/seed.jsonl");
    let seed = create(&seed_path);
    let Preparation::Append(record) = seed
        .ledger()
        .prepare(item("mail"), mail())
        .expect("prepare")
    else {
        panic!("new mail")
    };
    let encoded = crate::codec::encode_line(&record).expect("encode");
    drop(seed);
    for cut in 0..=encoded.len() {
        let path = directory.path().join(format!("private/cut-{cut}.jsonl"));
        let mut file = create(&path);
        let before = file.ledger().clone();
        let attempt = CollaborationAttempt {
            id: item("mail"),
            event: mail(),
        };
        let failed = file
            .admit_with(attempt.clone(), |file, bytes| {
                file.write_all(&bytes[..cut])?;
                Err(std::io::Error::other("injected unknown outcome"))
            })
            .expect_err("write must fail");
        assert!(matches!(
            failed.error(),
            CollaborationStoreError::WriteUncertain(_)
        ));
        assert_eq!(failed.attempt(), &attempt);
        assert_eq!(
            file.ledger(),
            &before,
            "cut {cut} published before acknowledgement"
        );
        let poisoned = file
            .admit(item("create"), creation())
            .expect_err("even old retries require reopen");
        assert!(matches!(
            poisoned.error(),
            CollaborationStoreError::WriterPoisoned
        ));
        drop(file);
        let mut reopened =
            CollaborationFile::open(&path).unwrap_or_else(|error| panic!("cut {cut}: {error}"));
        let already_complete = cut >= encoded.len() - 1;
        assert_eq!(
            reopened.ledger().records().len(),
            if already_complete { 2 } else { 1 },
            "cut {cut}"
        );
        if cut > 0 && !already_complete {
            let JournalRecovery::IsolatedFinalTail { path, bytes } = reopened.recovery() else {
                panic!("cut {cut} lost tail evidence")
            };
            assert_eq!(*bytes, cut as u64);
            assert_eq!(std::fs::read(path).expect("isolated tail"), encoded[..cut]);
        }
        if cut == encoded.len() - 1 {
            assert_eq!(reopened.recovery(), &JournalRecovery::AddedFinalNewline);
        }
        let receipt = reopened
            .admit(attempt.id, attempt.event)
            .expect("reconcile retry");
        assert_eq!(receipt, record.receipt());
        assert_eq!(
            reopened.ledger().records().len(),
            2,
            "cut {cut} duplicated mail"
        );
        assert_eq!(reopened.ledger().mail_for(&endpoint("a")).count(), 1);
        drop(reopened);
        assert_eq!(
            CollaborationFile::open(&path)
                .expect("second reopen")
                .ledger()
                .records()
                .len(),
            2
        );
    }
}

/// COL-5: semantic corruption is never treated as a recoverable syntactic tail.
#[test]
fn col_5_corruption_fails_closed_without_rewriting_evidence() {
    let directory = crate::test_support::TestDir::new("collaboration-corruption");
    for (case, terminated) in [("complete", true), ("missing-newline", false)] {
        let path = directory.path().join(format!("private/{case}.jsonl"));
        let file = create(&path);
        let Preparation::Append(mut record) = file
            .ledger()
            .prepare(item("mail"), mail())
            .expect("prepare")
        else {
            panic!("new mail")
        };
        record.sequence = CollaborationSequence(99);
        let mut bytes = crate::codec::encode_line(&record).expect("encode");
        if !terminated {
            bytes.pop();
        }
        drop(file);
        OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open corrupt fixture")
            .write_all(&bytes)
            .expect("append");
        let before = std::fs::read(&path).expect("read");
        assert!(matches!(
            CollaborationFile::open(&path),
            Err(CollaborationStoreError::InvalidRecord { .. })
        ));
        assert_eq!(std::fs::read(&path).expect("read after refusal"), before);
    }
    for (name, corrupt) in [
        ("middle", b"{invalid\n{}\n".as_slice()),
        ("final", b"{invalid".as_slice()),
        ("broken-utf8", b"{\"id\":\"bad\xff".as_slice()),
    ] {
        let path = directory.path().join(format!("private/{name}.jsonl"));
        drop(create(&path));
        OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open fixture")
            .write_all(corrupt)
            .expect("append");
        let before = std::fs::read(&path).expect("read");
        let error = CollaborationFile::open(&path).err();
        assert!(
            matches!(error, Some(CollaborationStoreError::Malformed { .. })),
            "{name}: {error:?}"
        );
        assert_eq!(std::fs::read(&path).expect("read after refusal"), before);
    }
}

/// COL-5: only the owning writer can admit, and a duplicate descriptor cannot prolong its lock.
#[cfg(unix)]
#[test]
fn col_5_exclusive_writer_and_owner_only_files() {
    use std::os::unix::fs::PermissionsExt;
    let directory = crate::test_support::TestDir::new("collaboration-lock");
    let path = directory.path().join("private/log.jsonl");
    let file = create(&path);
    let duplicate = file.file.try_clone().expect("duplicate descriptor");
    assert!(matches!(
        CollaborationFile::open(&path),
        Err(CollaborationStoreError::Framing(StoreError::WriterLocked))
    ));
    assert_eq!(
        std::fs::metadata(&path)
            .expect("file mode")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(path.parent().expect("parent"))
            .expect("directory mode")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    drop(file);
    drop(CollaborationFile::open(&path).expect("writer explicitly unlocked"));
    assert!(duplicate.metadata().is_ok());
    let alias = directory.path().join("private/alias.jsonl");
    std::os::unix::fs::symlink(&path, &alias).expect("symlink fixture");
    assert!(matches!(
        CollaborationFile::open(alias),
        Err(CollaborationStoreError::Framing(StoreError::SymlinkPath))
    ));
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
        .expect("insecure fixture");
    assert!(matches!(
        CollaborationFile::open(path),
        Err(CollaborationStoreError::Framing(
            StoreError::InsecurePermissions(_)
        ))
    ));
}

/// COL-4: process exit bypasses Drop; accepted records and writer authority still recover.
#[test]
fn col_4_accepted_mail_survives_process_exit_without_drop() {
    let directory = crate::test_support::TestDir::new("collaboration-process");
    let path = directory.path().join("private/log.jsonl");
    let status = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "collaboration::tests::collaboration_child_exit_fixture",
        ])
        .env("PLEXMATON_TEST_COLLABORATION_EXIT_PATH", &path)
        .output()
        .expect("child process");
    assert!(
        status.status.success(),
        "child failed: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    let mut reopened = CollaborationFile::open(&path).expect("reopen after process exit");
    assert_eq!(reopened.ledger().records().len(), 2);
    reopened
        .admit(item("mail"), mail())
        .expect("idempotent recovered mail");
    assert_eq!(reopened.ledger().mail_for(&endpoint("a")).count(), 1);
}

#[test]
fn collaboration_child_exit_fixture() {
    let Some(path) = std::env::var_os("PLEXMATON_TEST_COLLABORATION_EXIT_PATH") else {
        return;
    };
    let mut file = create(std::path::Path::new(&path));
    file.admit(item("mail"), mail()).expect("acknowledged mail");
    std::process::exit(0);
}

/// COL-4/COL-5: schema and semantic decoding failures do not repair away complete evidence.
#[test]
fn col_5_schema_and_decoded_bounds_fail_without_tail_repair() {
    let directory = crate::test_support::TestDir::new("collaboration-schema");
    for case in ["schema", "limits", "text", "unknown-kind"] {
        let path = directory.path().join(format!("private/{case}.jsonl"));
        let file = create(&path);
        let Preparation::Append(record) = file
            .ledger()
            .prepare(item("mail"), mail())
            .expect("prepare")
        else {
            panic!("new mail")
        };
        drop(file);
        let mut lines: Vec<serde_json::Value> = std::fs::read_to_string(&path)
            .expect("read")
            .lines()
            .map(|line| serde_json::from_str(line).expect("decode fixture"))
            .collect();
        match case {
            "schema" => lines[0]["schema"] = "unimplemented-epoch".into(),
            "limits" => lines[0]["limits"]["items"] = 0.into(),
            "text" => {
                let mut record = serde_json::to_value(&record).expect("fixture");
                record["event"]["mail"]["summary"] = "".into();
                lines.push(record);
            }
            _ => lines.push(
                serde_json::json!({"id":"unknown", "sequence":2, "event":{"kind":"unknown"}}),
            ),
        }
        // No final newline: a complete but invalid typed value must still fail closed.
        let bytes = lines
            .iter()
            .map(|line| serde_json::to_string(line).expect("encode fixture"))
            .collect::<Vec<_>>()
            .join("\n")
            .into_bytes();
        std::fs::write(&path, &bytes).expect("replace fixture");
        assert!(CollaborationFile::open(&path).is_err(), "accepted {case}");
        assert_eq!(std::fs::read(&path).expect("unchanged evidence"), bytes);
    }
}

/// COL-1/COL-4: an invalid attempt is known unwritten and leaves the writer usable.
#[test]
fn col_4_rejected_attempt_writes_nothing_and_does_not_poison() {
    let directory = crate::test_support::TestDir::new("collaboration-refusal");
    let path = directory.path().join("private/log.jsonl");
    let mut file = create(&path);
    let before = std::fs::read(&path).expect("read");
    let attempt = CollaborationAttempt {
        id: item("create"),
        event: mail(),
    };
    let failure = file
        .admit_with(attempt.clone(), |_, _| panic!("refusal reached writer"))
        .expect_err("identity conflict");
    assert!(matches!(
        failure.error(),
        CollaborationStoreError::Rejected(_)
    ));
    assert_eq!(failure.attempt(), &attempt);
    assert_eq!(std::fs::read(&path).expect("unchanged"), before);
    file.admit(item("mail"), mail())
        .expect("writer remains usable");
}
