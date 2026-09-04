use plexmaton_core::{
    AgentId, AgentStatus, HeadName, JournalRecordId, SessionEntryId, SessionId, TokenUsage,
    TranscriptItemId, TurnId,
};

use super::{
    HeadRevision, JournalEntryPayload, JournalError, JournalRecord, JournalSequence, SessionEntry,
    SessionJournal,
};
use crate::test_support::replay_compatibility;
use crate::{
    CompactionId, DispatchedRequestTiming, ElapsedMillis, ModelStepId, RequestAttemptAuthorized,
    RequestAttemptId, RequestAttemptOwner, RequestAttemptTerminal, RequestAttemptTerminalState,
    RequestDispatchedOutcome, RequestEnvironment, RequestEnvironmentFingerprint,
    RequestNotDispatchedOutcome, TurnFinished, TurnFinishedAt, TurnOutcome, UnixMillis,
};

fn id<T>(value: &str, build: impl FnOnce(String) -> Result<T, plexmaton_core::IdError>) -> T {
    build(value.to_owned()).unwrap_or_else(|error| panic!("fixture identity: {error}"))
}

fn head(value: &str) -> HeadName {
    id(value, HeadName::new)
}

fn record(value: &str) -> JournalRecordId {
    id(value, JournalRecordId::new)
}

fn entry(value: &str) -> SessionEntryId {
    id(value, SessionEntryId::new)
}

fn attempt(value: &str) -> RequestAttemptId {
    RequestAttemptId::new(value).unwrap_or_else(|error| panic!("attempt fixture: {error}"))
}

fn step(turn_id: &TurnId, index: u16) -> ModelStepId {
    ModelStepId::new(turn_id.clone(), index)
}

fn environment(seed: u8) -> RequestEnvironment {
    RequestEnvironment::new(
        replay_compatibility(),
        RequestEnvironmentFingerprint::new([seed; 32]),
    )
}

fn append(sequence: u64, record_id: &str, revision: u64, entry: SessionEntry) -> JournalRecord {
    JournalRecord::AppendEntry {
        sequence: JournalSequence::new(sequence),
        record_id: record(record_id),
        head: head("main"),
        expected_head_revision: HeadRevision::new(revision),
        entry: Box::new(entry),
    }
}

fn authorization_record(
    sequence: u64,
    record_id: &str,
    revision: u64,
    fact: RequestAttemptAuthorized,
) -> JournalRecord {
    JournalRecord::RequestAttemptAuthorized {
        sequence: JournalSequence::new(sequence),
        record_id: record(record_id),
        head: head("main"),
        expected_head_revision: HeadRevision::new(revision),
        fact,
    }
}

fn terminal_record(sequence: u64, record_id: &str, fact: RequestAttemptTerminal) -> JournalRecord {
    JournalRecord::RequestAttemptFinished {
        sequence: JournalSequence::new(sequence),
        record_id: record(record_id),
        fact,
    }
}

fn not_dispatched(attempt_id: &str) -> RequestAttemptTerminal {
    RequestAttemptTerminal::new(
        attempt(attempt_id),
        RequestAttemptTerminalState::NotDispatched {
            outcome: RequestNotDispatchedOutcome::Cancelled,
        },
    )
    .unwrap_or_else(|error| panic!("not-dispatched terminal: {error}"))
}

struct OpenTurnFixture {
    journal: SessionJournal,
    agent_id: AgentId,
    turn_id: TurnId,
    root: SessionEntryId,
    boundary: SessionEntryId,
}

fn open_turn() -> OpenTurnFixture {
    let agent_id = id("agent-a", AgentId::new);
    let turn_id = id("turn-a", TurnId::new);
    let root = entry("entry-agent");
    let boundary = entry("entry-turn");
    let mut journal = SessionJournal::new(id("session-a", SessionId::new));
    journal
        .apply(append(
            1,
            "record-agent",
            0,
            SessionEntry {
                id: root.clone(),
                parent_id: None,
                payload: JournalEntryPayload::AgentCreated {
                    agent_id: agent_id.clone(),
                    label: "Agent A".to_owned(),
                    status: AgentStatus::Idle,
                },
            },
        ))
        .unwrap_or_else(|error| panic!("append agent: {error:?}"));
    journal
        .apply(append(
            2,
            "record-turn",
            1,
            SessionEntry {
                id: boundary.clone(),
                parent_id: Some(root.clone()),
                payload: JournalEntryPayload::TurnStarted {
                    agent_id: agent_id.clone(),
                    item_id: id("item-user", TranscriptItemId::new),
                    turn_id: turn_id.clone(),
                    text: "hello".to_owned(),
                    accepted_at: UnixMillis::new(10),
                    opened_at: UnixMillis::new(11),
                },
            },
        ))
        .unwrap_or_else(|error| panic!("append turn: {error:?}"));
    OpenTurnFixture {
        journal,
        agent_id,
        turn_id,
        root,
        boundary,
    }
}

fn agent_authorization(fixture: &OpenTurnFixture, attempt_id: &str) -> RequestAttemptAuthorized {
    RequestAttemptAuthorized::new(
        attempt(attempt_id),
        RequestAttemptOwner::AgentStep {
            step_id: step(&fixture.turn_id, 1),
        },
        fixture.boundary.clone(),
        environment(1),
        UnixMillis::new(12),
    )
}

/// TIM-2/TIM-4: authorization and terminal records round-trip but advance neither semantic head
/// target nor revision.
#[test]
fn tim_2_attempt_records_are_lossless_and_non_advancing() {
    let mut fixture = open_turn();
    let target = fixture
        .journal
        .head_target(&head("main"))
        .unwrap_or_else(|error| panic!("head target: {error:?}"))
        .cloned();
    let revision = fixture
        .journal
        .head_revision(&head("main"))
        .unwrap_or_else(|error| panic!("head revision: {error:?}"));
    let authorization = authorization_record(
        3,
        "record-authorized",
        revision.get(),
        agent_authorization(&fixture, "attempt-a"),
    );
    let encoded = serde_json::to_string(&authorization)
        .unwrap_or_else(|error| panic!("encode authorization record: {error}"));
    assert_eq!(
        serde_json::from_str::<JournalRecord>(&encoded)
            .unwrap_or_else(|error| panic!("decode authorization record: {error}")),
        authorization
    );
    fixture
        .journal
        .apply(authorization)
        .unwrap_or_else(|error| panic!("authorize request: {error:?}"));

    let terminal = terminal_record(4, "record-terminal", not_dispatched("attempt-a"));
    let encoded = serde_json::to_string(&terminal)
        .unwrap_or_else(|error| panic!("encode terminal record: {error}"));
    assert_eq!(
        serde_json::from_str::<JournalRecord>(&encoded)
            .unwrap_or_else(|error| panic!("decode terminal record: {error}")),
        terminal
    );
    fixture
        .journal
        .apply(terminal)
        .unwrap_or_else(|error| panic!("finish request: {error:?}"));

    assert_eq!(
        fixture
            .journal
            .head_target(&head("main"))
            .unwrap_or_else(|error| panic!("head target after audit: {error:?}")),
        target.as_ref()
    );
    assert_eq!(
        fixture
            .journal
            .head_revision(&head("main"))
            .unwrap_or_else(|error| panic!("head revision after audit: {error:?}")),
        revision
    );
}

/// TIM-2/TIM-3: a compaction owner uses the same checked boundary and wire without pretending to
/// belong to an agent turn.
#[test]
fn tim_3_compaction_authorization_uses_the_shared_attempt_spine() {
    let mut fixture = open_turn();
    let fact = RequestAttemptAuthorized::new(
        attempt("attempt-compaction"),
        RequestAttemptOwner::Compaction {
            compaction_id: CompactionId::new("compaction-a")
                .unwrap_or_else(|error| panic!("compaction fixture: {error}")),
        },
        fixture.boundary.clone(),
        environment(2),
        UnixMillis::new(20),
    );
    let record = authorization_record(3, "record-compaction", 2, fact);
    let encoded = serde_json::to_string(&record)
        .unwrap_or_else(|error| panic!("encode compaction record: {error}"));
    assert_eq!(
        serde_json::from_str::<JournalRecord>(&encoded)
            .unwrap_or_else(|error| panic!("decode compaction record: {error}")),
        record
    );

    fixture
        .journal
        .apply(record)
        .unwrap_or_else(|error| panic!("authorize compaction: {error:?}"));

    let retained = fixture
        .journal
        .request_attempt(&attempt("attempt-compaction"))
        .unwrap_or_else(|| panic!("missing compaction attempt"));
    assert!(matches!(
        retained.authorization().owner(),
        RequestAttemptOwner::Compaction { .. }
    ));
}

/// TIM-3: retrying one semantic step creates another attempt rather than rewriting the first
/// attempt's immutable accounting.
#[test]
fn tim_3_retry_attempts_keep_distinct_identities() {
    let mut fixture = open_turn();
    fixture
        .journal
        .apply(authorization_record(
            3,
            "authorize-first",
            2,
            agent_authorization(&fixture, "attempt-first"),
        ))
        .unwrap_or_else(|error| panic!("authorize first: {error:?}"));
    fixture
        .journal
        .apply(terminal_record(
            4,
            "finish-first",
            not_dispatched("attempt-first"),
        ))
        .unwrap_or_else(|error| panic!("finish first: {error:?}"));
    fixture
        .journal
        .apply(authorization_record(
            5,
            "authorize-retry",
            2,
            agent_authorization(&fixture, "attempt-retry"),
        ))
        .unwrap_or_else(|error| panic!("authorize retry: {error:?}"));

    let attempts = fixture.journal.request_attempts().collect::<Vec<_>>();
    assert_eq!(attempts.len(), 2);
    assert_eq!(
        attempts
            .iter()
            .map(|attempt| attempt.authorization().attempt_id().as_str())
            .collect::<Vec<_>>(),
        ["attempt-first", "attempt-retry"]
    );
    assert!(attempts[0].terminal().is_some());
    assert!(attempts[1].terminal().is_none());
}

/// TIM-3: one owner cannot have two requests in flight, while a terminal first attempt makes a
/// later attempt an ordinary retry.
#[test]
fn tim_3_one_owner_has_at_most_one_unfinished_attempt() {
    let mut fixture = open_turn();
    fixture
        .journal
        .apply(authorization_record(
            3,
            "authorize-first",
            2,
            agent_authorization(&fixture, "attempt-first"),
        ))
        .unwrap_or_else(|error| panic!("authorize first: {error:?}"));
    let before_second = fixture.journal.clone();

    assert_eq!(
        fixture.journal.apply(authorization_record(
            4,
            "authorize-concurrent",
            2,
            agent_authorization(&fixture, "attempt-concurrent"),
        )),
        Err(JournalError::RequestAttemptOwnerActive(attempt(
            "attempt-first"
        )))
    );
    assert_eq!(fixture.journal, before_second);
}

/// TIM-2/JRN-2: missing, duplicate and boundary-invalid attempt facts leave the journal exactly
/// unchanged.
#[test]
fn tim_2_invalid_attempt_records_change_nothing() {
    let fixture = open_turn();

    let invalid = [
        terminal_record(3, "orphan-terminal", not_dispatched("missing")),
        authorization_record(
            3,
            "stale-head",
            1,
            agent_authorization(&fixture, "stale-head"),
        ),
        authorization_record(
            3,
            "wrong-boundary",
            2,
            RequestAttemptAuthorized::new(
                attempt("wrong-boundary"),
                RequestAttemptOwner::AgentStep {
                    step_id: step(&fixture.turn_id, 1),
                },
                fixture.root.clone(),
                environment(3),
                UnixMillis::new(12),
            ),
        ),
        authorization_record(
            3,
            "missing-turn",
            2,
            RequestAttemptAuthorized::new(
                attempt("missing-turn"),
                RequestAttemptOwner::AgentStep {
                    step_id: step(&id("turn-missing", TurnId::new), 1),
                },
                fixture.boundary.clone(),
                environment(4),
                UnixMillis::new(12),
            ),
        ),
        authorization_record(
            3,
            "skipped-step",
            2,
            RequestAttemptAuthorized::new(
                attempt("skipped-step"),
                RequestAttemptOwner::AgentStep {
                    step_id: step(&fixture.turn_id, 2),
                },
                fixture.boundary.clone(),
                environment(5),
                UnixMillis::new(12),
            ),
        ),
    ];

    for record in invalid {
        let mut candidate = fixture.journal.clone();
        let unchanged = candidate.clone();
        assert!(candidate.apply(record).is_err());
        assert_eq!(candidate, unchanged);
    }

    let mut journal = fixture.journal.clone();
    journal
        .apply(authorization_record(
            3,
            "authorized",
            2,
            agent_authorization(&fixture, "attempt-a"),
        ))
        .unwrap_or_else(|error| panic!("authorize fixture: {error:?}"));
    let authorized = journal.clone();
    assert_eq!(
        journal.apply(authorization_record(
            4,
            "duplicate-authorization",
            2,
            agent_authorization(&fixture, "attempt-a"),
        )),
        Err(JournalError::DuplicateRequestAttempt(attempt("attempt-a")))
    );
    assert_eq!(journal, authorized);

    journal
        .apply(terminal_record(4, "terminal", not_dispatched("attempt-a")))
        .unwrap_or_else(|error| panic!("terminal fixture: {error:?}"));
    let terminal = journal.clone();
    assert_eq!(
        journal.apply(terminal_record(
            5,
            "duplicate-terminal",
            not_dispatched("attempt-a"),
        )),
        Err(JournalError::DuplicateRequestAttemptTerminal(attempt(
            "attempt-a"
        )))
    );
    assert_eq!(journal, terminal);
}

/// TIM-5/TIM-4: process recovery leaves authorization observable without fabricating a terminal,
/// and audit facts never enter or perturb model context.
#[test]
fn tim_5_process_death_keeps_authorization_without_inventing_measurements() {
    let mut fixture = open_turn();
    let before = fixture
        .journal
        .project(&head("main"))
        .unwrap_or_else(|error| panic!("project before audit: {error:?}"))
        .request()
        .clone();
    fixture
        .journal
        .apply(authorization_record(
            3,
            "authorized",
            2,
            agent_authorization(&fixture, "attempt-a"),
        ))
        .unwrap_or_else(|error| panic!("authorize fixture: {error:?}"));
    fixture
        .journal
        .apply(JournalRecord::TurnFinished {
            sequence: JournalSequence::new(4),
            record_id: record("turn-died"),
            head: head("main"),
            expected_head_revision: HeadRevision::new(2),
            fact: TurnFinished {
                agent_id: fixture.agent_id.clone(),
                turn_id: fixture.turn_id.clone(),
                semantic_boundary: fixture.boundary.clone(),
                outcome: TurnOutcome::ProcessDied,
                at: TurnFinishedAt::Recovered {
                    recovery_observed_at: UnixMillis::new(30),
                },
            },
        })
        .unwrap_or_else(|error| panic!("recover turn: {error:?}"));

    let projection = fixture
        .journal
        .project(&head("main"))
        .unwrap_or_else(|error| panic!("project recovered session: {error:?}"));
    assert_eq!(projection.request(), &before);
    assert!(matches!(
        projection.request_attempts(),
        [attempt] if attempt.authorization().attempt_id() == &self::attempt("attempt-a")
            && attempt.terminal().is_none()
    ));
    assert!(
        fixture
            .journal
            .request_attempt(&attempt("attempt-a"))
            .is_some_and(|attempt| attempt.terminal().is_none())
    );
}

/// TIM-3: path projection admits audit facts by semantic ancestry while the session accessor still
/// retains every unique attempt once.
#[test]
fn tim_3_selected_head_projects_only_attempts_on_its_path() {
    let mut fixture = open_turn();
    fixture
        .journal
        .apply(authorization_record(
            3,
            "authorized",
            2,
            agent_authorization(&fixture, "attempt-a"),
        ))
        .unwrap_or_else(|error| panic!("authorize fixture: {error:?}"));
    fixture
        .journal
        .apply(terminal_record(
            4,
            "terminal",
            RequestAttemptTerminal::new(
                attempt("attempt-a"),
                RequestAttemptTerminalState::Dispatched {
                    timing: DispatchedRequestTiming::new(
                        UnixMillis::new(13),
                        Some(ElapsedMillis::new(1)),
                        None,
                        ElapsedMillis::new(2),
                    )
                    .unwrap_or_else(|error| panic!("timing fixture: {error}")),
                    outcome: RequestDispatchedOutcome::TransportFailed,
                    usage: TokenUsage::Unavailable,
                },
            )
            .unwrap_or_else(|error| panic!("dispatched terminal: {error}")),
        ))
        .unwrap_or_else(|error| panic!("finish attempt: {error:?}"));
    fixture
        .journal
        .apply(JournalRecord::TurnFinished {
            sequence: JournalSequence::new(5),
            record_id: record("turn-finished"),
            head: head("main"),
            expected_head_revision: HeadRevision::new(2),
            fact: TurnFinished {
                agent_id: fixture.agent_id,
                turn_id: fixture.turn_id,
                semantic_boundary: fixture.boundary,
                outcome: TurnOutcome::Failed,
                at: TurnFinishedAt::Observed {
                    completed_at: UnixMillis::new(15),
                },
            },
        })
        .unwrap_or_else(|error| panic!("finish turn: {error:?}"));
    fixture
        .journal
        .apply(JournalRecord::CreateHead {
            sequence: JournalSequence::new(6),
            record_id: record("create-sibling"),
            head: head("sibling"),
            at: Some(fixture.root),
        })
        .unwrap_or_else(|error| panic!("create sibling: {error:?}"));

    assert_eq!(
        fixture
            .journal
            .project(&head("main"))
            .unwrap_or_else(|error| panic!("project main: {error:?}"))
            .request_attempts()
            .len(),
        1
    );
    assert!(
        fixture
            .journal
            .project(&head("sibling"))
            .unwrap_or_else(|error| panic!("project sibling: {error:?}"))
            .request_attempts()
            .is_empty()
    );
    assert_eq!(fixture.journal.request_attempts().len(), 1);
}
