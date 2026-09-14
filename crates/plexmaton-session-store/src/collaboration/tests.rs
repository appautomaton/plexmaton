use std::fs::OpenOptions;
use std::io::Write;

use plexmaton_agent::HeadRevision;
use plexmaton_agent::collaboration::{
    CollaborationEvent, CollaborationLimits, CollaborationSequence, CollaborationText,
    DelegationController, DelegationRevision, MailEndpoint, MailEnvelope, Preparation,
    TurnBoundary,
};
use plexmaton_core::{
    AgentId, CollaborationId, CollaborationItemId, ConversationId, DelegationId, HeadName, MailId,
    TurnId,
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
fn sibling_delegation() -> DelegationId {
    DelegationId::new("sibling-task").expect("fixture identity")
}

fn creation() -> CollaborationEvent {
    CollaborationEvent::DelegationCreated {
        delegation: delegation(),
        delegator: endpoint("a"),
        worker: endpoint("b"),
        task: text("Inspect the parser"),
    }
}

fn sibling_creation() -> CollaborationEvent {
    CollaborationEvent::DelegationCreated {
        delegation: sibling_delegation(),
        delegator: endpoint("a"),
        worker: endpoint("c"),
        task: text("Inspect another boundary"),
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

fn update(expected: u64, task: &str) -> CollaborationEvent {
    CollaborationEvent::TaskUpdated {
        delegation: delegation(),
        expected: DelegationRevision(expected),
        author: endpoint("a"),
        task: text(task),
    }
}

fn handoff(expected: u64) -> CollaborationEvent {
    CollaborationEvent::HandoffCompleted {
        delegation: delegation(),
        expected: DelegationRevision(expected),
        author: endpoint("a"),
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

/// COL-1/COL-3/COL-4: reopen preserves mail, task, controller and exact Handoff retry.
#[test]
fn col_4_file_roundtrip_retains_attribution_and_exact_retry() {
    let directory = crate::test_support::TestDir::new("collaboration-roundtrip");
    let path = directory.path().join("private/log.jsonl");
    let mut file = create(&path);
    let receipt = file.admit(item("mail"), mail()).expect("admit mail");
    file.admit(item("update"), update(0, "Inspect without changing files"))
        .expect("main task update");
    let handoff_receipt = file.admit(item("handoff"), handoff(1)).expect("handoff");
    let expected = file.ledger().clone();
    let bytes = std::fs::read(&path).expect("read file");
    drop(file);
    let mut reopened = CollaborationFile::open(&path).expect("reopen");
    assert_eq!(reopened.ledger(), &expected);
    assert_eq!(
        reopened.admit(item("mail"), mail()).expect("retry"),
        receipt
    );
    assert_eq!(
        reopened
            .admit(item("handoff"), handoff(1))
            .expect("handoff retry"),
        handoff_receipt
    );
    assert_eq!(std::fs::read(&path).expect("read after retry"), bytes);
    assert_eq!(reopened.ledger().mail_for(&endpoint("a")).count(), 1);
    assert_eq!(
        reopened
            .ledger()
            .delegation(&delegation())
            .expect("delegation")
            .controller,
        DelegationController::User
    );
}

/// CHB-2: child control retains the exact canonical creation provenance across reopen.
#[test]
fn chb_2_delegated_control_retains_all_creation_provenance_across_reopen() {
    let directory = crate::test_support::TestDir::new("delegated-provenance");
    let path = directory.path().join("private/log.jsonl");
    let file = create(&path);
    let expected_creation = file
        .ledger()
        .item_reference(&item("create"))
        .expect("creation reference");
    let control = file
        .delegated_control(&delegation())
        .expect("delegated control");
    assert_eq!(control.provenance().collaboration(), file.ledger().id());
    assert_eq!(control.provenance().delegation(), &delegation());
    assert_eq!(control.provenance().creation(), &expected_creation);
    assert_eq!(control.provenance().delegator(), &endpoint("a"));
    assert_eq!(control.provenance().worker(), &endpoint("b"));
    let expected = control.provenance().clone();
    drop(control);
    drop(file);

    let reopened = CollaborationFile::open(&path).expect("reopen collaboration");
    let recovered = reopened
        .delegated_control(&delegation())
        .expect("recover delegated control");
    assert_eq!(recovered.provenance(), &expected);
}

fn admitted(
    file: &mut CollaborationFile,
    name: &str,
) -> std::sync::Arc<plexmaton_agent::collaboration::ResolvedTurnAdmission> {
    admitted_for(file, name, endpoint("b"))
}

fn admitted_for(
    file: &mut CollaborationFile,
    name: &str,
    recipient: MailEndpoint,
) -> std::sync::Arc<plexmaton_agent::collaboration::ResolvedTurnAdmission> {
    let id = item(name);
    let boundary = TurnBoundary {
        recipient,
        head: HeadName::new("main").expect("head"),
        head_revision: HeadRevision::new(0),
        parent: None,
        turn: TurnId::new(name).expect("turn"),
    };
    let Preparation::Append(record) = file
        .ledger()
        .prepare_turn(id.clone(), boundary, None)
        .expect("prepare admission")
    else {
        panic!("new admission")
    };
    file.admit(record.id, record.event)
        .expect("append admission");
    file.ledger()
        .resolve_turn(&file.ledger().item_reference(&id).expect("reference"))
        .expect("resolve admission")
}

/// COL-3: an admission is inspectable; only a current bounded permit can execute it.
#[test]
fn col_3_execution_permit_blocks_handoff_and_old_ticket_dies_after_handoff() {
    let directory = crate::test_support::TestDir::new("collaboration-permit");
    let path = directory.path().join("private/log.jsonl");
    let mut file = create(&path);
    let control = file.control();
    let pending_input = control
        .reserve_execution(&delegation())
        .expect("reserve Main input");
    let refused = file
        .admit(item("handoff"), handoff(0))
        .expect_err("undisposed input blocks release");
    assert!(matches!(
        refused.error(),
        CollaborationStoreError::ControlNotQuiescent
    ));
    drop(pending_input);
    let admission = admitted(&mut file, "turn");
    let ticket = file
        .execution_ticket(&delegation(), &admission)
        .expect("inspectable ticket");
    let second = file
        .execution_ticket(&delegation(), &admission)
        .expect("second inspection");
    let permit = control.issue_execution(second).expect("execution permit");
    assert_eq!(permit.admission(), ticket.admission());
    let before = std::fs::read(&path).expect("before refused release");
    let refused = file
        .admit(item("handoff"), handoff(0))
        .expect_err("live permit blocks release");
    assert!(matches!(
        refused.error(),
        CollaborationStoreError::ControlNotQuiescent
    ));
    assert_eq!(std::fs::read(&path).expect("unchanged"), before);
    drop(permit);
    file.admit(item("handoff"), handoff(0))
        .expect("quiescent release");
    assert!(matches!(
        control.issue_execution(ticket),
        Err(CollaborationStoreError::Rejected(
            plexmaton_agent::collaboration::CollaborationError::HandoffCompleted
        ))
    ));
}

/// CIN-4: one durable turn admission can issue execution authority only once per owner lifetime.
#[test]
fn cin_4_one_admission_cannot_issue_execution_twice_or_after_reopen() {
    let directory = crate::test_support::TestDir::new("collaboration-single-issue");
    let path = directory.path().join("private/log.jsonl");
    let mut file = create(&path);
    let admission = admitted(&mut file, "turn");
    let ticket = file
        .execution_ticket(&delegation(), &admission)
        .expect("first ticket");
    let control = file.control();
    drop(control.issue_execution(ticket).expect("first permit"));
    assert!(matches!(
        file.execution_ticket(&delegation(), &admission),
        Err(CollaborationStoreError::ExecutionAlreadyIssued)
    ));
    drop(control);
    drop(file);
    let reopened = CollaborationFile::open(&path).expect("reopen");
    assert!(matches!(
        reopened.execution_ticket(&delegation(), &admission),
        Err(CollaborationStoreError::ExecutionAlreadyIssued)
    ));
}

/// COL-3: an execution ticket is bound to one physical authority, not matching logical IDs.
#[test]
fn col_3_ticket_from_another_file_cannot_cross_authority() {
    let directory = crate::test_support::TestDir::new("collaboration-ticket-scope");
    let mut first = create(&directory.path().join("first/log.jsonl"));
    let mut second = create(&directory.path().join("second/log.jsonl"));
    let first_admission = admitted(&mut first, "turn");
    let second_admission = admitted(&mut second, "turn");
    let foreign = second
        .execution_ticket(&delegation(), &second_admission)
        .expect("foreign ticket");
    let control = first.control();
    assert!(matches!(
        control.issue_execution(foreign),
        Err(CollaborationStoreError::InvalidExecutionTicket)
    ));
    let local = first
        .execution_ticket(&delegation(), &first_admission)
        .expect("local ticket remains available");
    drop(control.issue_execution(local).expect("local permit"));
}

/// COL-3: each delegation has one execution slot, and a failed bind returns that slot.
#[test]
fn col_3_execution_slot_is_single_and_failed_bind_releases_it() {
    let directory = crate::test_support::TestDir::new("collaboration-execution-slot");
    let mut first = create(&directory.path().join("first/log.jsonl"));
    let mut second = create(&directory.path().join("second/log.jsonl"));
    let first_admission = admitted(&mut first, "turn");
    let second_admission = admitted(&mut second, "turn");
    let control = first.control();
    let reservation = control
        .reserve_execution(&delegation())
        .expect("first reservation");
    assert!(matches!(
        control.reserve_execution(&delegation()),
        Err(CollaborationStoreError::ExecutionBusy)
    ));
    let foreign = second
        .execution_ticket(&delegation(), &second_admission)
        .expect("foreign ticket");
    assert!(matches!(
        reservation.bind(foreign),
        Err(CollaborationStoreError::InvalidExecutionTicket)
    ));

    let next = control
        .reserve_execution(&delegation())
        .expect("failed bind returned the slot");
    let local = first
        .execution_ticket(&delegation(), &first_admission)
        .expect("local ticket");
    drop(next.bind(local).expect("replacement reservation binds"));
}

/// COL-3/COL-4: release closes admission before its append, so a racing ticket cannot run.
#[test]
fn col_3_handoff_serializes_new_execution_admission() {
    let directory = crate::test_support::TestDir::new("collaboration-release-race");
    let path = directory.path().join("private/log.jsonl");
    let mut file = create(&path);
    let admission = admitted(&mut file, "turn");
    let ticket = file
        .execution_ticket(&delegation(), &admission)
        .expect("ticket");
    let control = file.control();
    let attempt = CollaborationAttempt {
        id: item("handoff"),
        event: handoff(0),
    };
    let mut refused_while_releasing = false;
    file.admit_with(attempt, |writer, bytes| {
        refused_while_releasing = matches!(
            control.issue_execution(ticket),
            Err(CollaborationStoreError::ControlNotQuiescent)
        );
        writer.write_all(bytes)
    })
    .expect("release");
    assert!(refused_while_releasing);
}

/// COL-3/COL-4/COL-5: every uncertain release cut freezes authority until canonical reopen.
#[test]
fn col_4_uncertain_handoff_recovers_before_any_execution_or_retry() {
    let directory = crate::test_support::TestDir::new("collaboration-release-cuts");
    let seed_path = directory.path().join("private/seed.jsonl");
    let seed = create(&seed_path);
    let Preparation::Append(record) = seed
        .ledger()
        .prepare(item("handoff"), handoff(0))
        .expect("prepare release")
    else {
        panic!("new release")
    };
    let encoded = crate::codec::encode_line(&record).expect("encode");
    drop(seed);
    for cut in 0..=encoded.len() {
        let path = directory
            .path()
            .join(format!("private/release-cut-{cut}.jsonl"));
        let mut file = create(&path);
        let before = file.ledger().clone();
        let attempt = CollaborationAttempt {
            id: item("handoff"),
            event: handoff(0),
        };
        let failed = file
            .admit_with(attempt.clone(), |writer, bytes| {
                writer.write_all(&bytes[..cut])?;
                Err(std::io::Error::other("injected unknown release"))
            })
            .expect_err("write must be uncertain");
        assert!(matches!(
            failed.error(),
            CollaborationStoreError::WriteUncertain(_)
        ));
        assert_eq!(file.ledger(), &before, "cut {cut}");
        assert!(matches!(
            file.admit(attempt.id.clone(), attempt.event.clone())
                .expect_err("poisoned retry")
                .error(),
            CollaborationStoreError::WriterPoisoned
        ));
        drop(file);
        let mut reopened =
            CollaborationFile::open(&path).unwrap_or_else(|error| panic!("cut {cut}: {error}"));
        let already_complete = cut >= encoded.len() - 1;
        assert_eq!(
            reopened
                .ledger()
                .delegation(&delegation())
                .expect("delegation")
                .controller,
            if already_complete {
                DelegationController::User
            } else {
                DelegationController::Main
            },
            "cut {cut}"
        );
        reopened
            .admit(attempt.id, attempt.event)
            .expect("reconcile exact release");
        assert_eq!(
            reopened
                .ledger()
                .delegation(&delegation())
                .expect("delegation")
                .controller,
            DelegationController::User,
            "cut {cut}"
        );
        assert_eq!(reopened.ledger().records().len(), 2, "cut {cut}");
    }
}

/// COL-4/COL-5: one unknown append freezes every delegation sharing the writer until reopen.
#[test]
fn col_4_uncertain_append_freezes_every_delegation_until_reopen() {
    let directory = crate::test_support::TestDir::new("collaboration-global-freeze");
    let path = directory.path().join("private/log.jsonl");
    let mut file = create(&path);
    file.admit(item("create-sibling"), sibling_creation())
        .expect("create sibling delegation");
    let control = file.control();
    let failed = file
        .admit_with(
            CollaborationAttempt {
                id: item("handoff"),
                event: handoff(0),
            },
            |_writer, _bytes| Err(std::io::Error::other("injected unknown append")),
        )
        .expect_err("append outcome must be unknown");
    assert!(matches!(
        failed.error(),
        CollaborationStoreError::WriteUncertain(_)
    ));
    assert!(matches!(
        file.project_mail(&endpoint("a")),
        Err(CollaborationStoreError::WriterPoisoned)
    ));
    assert!(matches!(
        control.reserve_execution(&sibling_delegation()),
        Err(CollaborationStoreError::WriterPoisoned)
    ));
    drop(control);
    drop(file);

    let reopened = CollaborationFile::open(&path).expect("reopen reconciles authority");
    let reservation = reopened
        .control()
        .reserve_execution(&sibling_delegation())
        .expect("sibling authority restored from canonical log");
    drop(reservation);
}

/// COL-3/COL-5: a live execution permit retains physical writer authority after owner drop.
#[test]
fn col_5_execution_permit_retains_writer_lock_until_disposed() {
    let directory = crate::test_support::TestDir::new("collaboration-permit-lock");
    let path = directory.path().join("private/log.jsonl");
    let mut file = create(&path);
    let admission = admitted(&mut file, "turn");
    let ticket = file
        .execution_ticket(&delegation(), &admission)
        .expect("ticket");
    let control = file.control();
    let permit = control.issue_execution(ticket).expect("permit");
    drop(control);
    drop(file);
    assert!(matches!(
        CollaborationFile::open(&path),
        Err(CollaborationStoreError::Framing(StoreError::WriterLocked))
    ));
    drop(permit);
    drop(CollaborationFile::open(path).expect("last permit releases writer"));
}

/// COL-3/COL-5: accepted but unbound Main input retains the writer until explicitly disposed.
#[test]
fn col_5_execution_reservation_retains_writer_lock_until_disposed() {
    let directory = crate::test_support::TestDir::new("collaboration-reservation-lock");
    let path = directory.path().join("private/log.jsonl");
    let file = create(&path);
    let reservation = file
        .control()
        .reserve_execution(&delegation())
        .expect("reservation");
    drop(file);
    assert!(matches!(
        CollaborationFile::open(&path),
        Err(CollaborationStoreError::Framing(StoreError::WriterLocked))
    ));
    drop(reservation);
    drop(CollaborationFile::open(path).expect("reservation disposal releases writer"));
}

/// COL-5: an idle control handle neither owns execution nor prolongs the physical writer lock.
#[test]
fn col_5_idle_control_does_not_keep_a_closed_writer_locked() {
    let directory = crate::test_support::TestDir::new("collaboration-idle-control");
    let path = directory.path().join("private/log.jsonl");
    let file = create(&path);
    let control = file.control();
    drop(file);
    drop(CollaborationFile::open(&path).expect("idle control has no writer lease"));
    assert!(matches!(
        control.reserve_execution(&delegation()),
        Err(CollaborationStoreError::ControlOwnerClosed)
    ));
}

/// COL-5: a duplicated descriptor cannot prolong an unretained collaboration writer's lock.
#[test]
fn col_5_writer_drop_unlocks_before_a_duplicate_descriptor_survives() {
    let directory = crate::test_support::TestDir::new("collaboration-duplicate-descriptor");
    let path = directory.path().join("private/log.jsonl");
    let file = create(&path);
    let duplicate = file.file.try_clone().expect("duplicate descriptor");
    drop(file);

    drop(CollaborationFile::open(&path).expect("writer drop released flock"));
    drop(duplicate);
}

/// COL-3/COL-5: an older permit retains only itself, not authority for fresh reservations.
#[test]
fn col_5_live_permit_does_not_keep_a_stale_control_open() {
    let directory = crate::test_support::TestDir::new("collaboration-stale-control");
    let path = directory.path().join("private/log.jsonl");
    let mut file = create(&path);
    file.admit(item("create-sibling"), sibling_creation())
        .expect("create sibling delegation");
    let first = admitted(&mut file, "first-turn");
    let sibling = admitted_for(&mut file, "sibling-turn", endpoint("c"));
    let first_ticket = file
        .execution_ticket(&delegation(), &first)
        .expect("first ticket");
    let sibling_ticket = file
        .execution_ticket(&sibling_delegation(), &sibling)
        .expect("sibling ticket");
    let control = file.control();
    let permit = control.issue_execution(first_ticket).expect("first permit");
    drop(file);

    assert!(matches!(
        control.reserve_execution(&sibling_delegation()),
        Err(CollaborationStoreError::ControlOwnerClosed)
    ));
    assert!(matches!(
        control.issue_execution(sibling_ticket),
        Err(CollaborationStoreError::ControlOwnerClosed)
    ));
    assert!(matches!(
        CollaborationFile::open(&path),
        Err(CollaborationStoreError::Framing(StoreError::WriterLocked))
    ));
    drop(permit);
    drop(CollaborationFile::open(path).expect("permit disposal releases writer"));
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
    for case in [
        "prior-schema",
        "foreign-schema",
        "limits",
        "text",
        "unknown-kind",
    ] {
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
            "prior-schema" => lines[0]["schema"] = "2026-09-07".into(),
            "foreign-schema" => lines[0]["schema"] = "unimplemented-epoch".into(),
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
