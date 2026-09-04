use plexmaton_core::{
    AgentId, AgentStatus, HeadName, JournalRecordId, SessionEntryId, SessionId, TokenCounts,
    TokenUsage, TranscriptItemId, TurnId,
};

use super::{
    JournalEntryPayload, JournalRecord, RequestAccounting, RequestAccountingError, SessionEntry,
    SessionJournal,
};
use crate::test_support::replay_compatibility;
use crate::{
    CompactionId, DispatchedRequestTiming, ElapsedMillis, ModelStepId, RequestAttemptAuthorized,
    RequestAttemptId, RequestAttemptOwner, RequestAttemptTerminal, RequestAttemptTerminalState,
    RequestCost, RequestDispatchedOutcome, RequestEnvironment, RequestEnvironmentFingerprint,
    RequestNotDispatchedOutcome, TurnFinished, TurnFinishedAt, TurnOutcome, UnixMillis,
    UsdCostTicks,
};

fn head(name: &str) -> HeadName {
    HeadName::new(name).expect("fixture head")
}

fn attempt_id(name: &str) -> RequestAttemptId {
    RequestAttemptId::new(name).expect("fixture attempt")
}

fn record_id(journal: &SessionJournal) -> JournalRecordId {
    JournalRecordId::new(format!("record-{}", journal.next_sequence().get()))
        .expect("fixture record")
}

fn append(journal: &mut SessionJournal, head_name: &str, payload: JournalEntryPayload) {
    let head = head(head_name);
    journal
        .apply(JournalRecord::AppendEntry {
            sequence: journal.next_sequence(),
            record_id: record_id(journal),
            expected_head_revision: journal.head_revision(&head).expect("fixture revision"),
            entry: Box::new(SessionEntry {
                id: SessionEntryId::new(format!("entry-{}", journal.next_sequence().get()))
                    .expect("fixture entry"),
                parent_id: journal.head_target(&head).expect("fixture target").cloned(),
                payload,
            }),
            head,
        })
        .expect("append fixture entry");
}

fn journal() -> SessionJournal {
    let mut journal = SessionJournal::new(SessionId::new("session").expect("fixture session"));
    append(
        &mut journal,
        "main",
        JournalEntryPayload::AgentCreated {
            agent_id: AgentId::new("agent").expect("fixture agent"),
            label: "Agent".to_owned(),
            status: AgentStatus::Idle,
        },
    );
    journal
}

fn start_turn(journal: &mut SessionJournal, head_name: &str, name: &str) -> RequestAttemptOwner {
    let turn_id = TurnId::new(name).expect("fixture turn");
    append(
        journal,
        head_name,
        JournalEntryPayload::TurnStarted {
            agent_id: AgentId::new("agent").expect("fixture agent"),
            item_id: TranscriptItemId::new(format!("user-{name}")).expect("fixture user"),
            turn_id: turn_id.clone(),
            text: "request".to_owned(),
            accepted_at: UnixMillis::EPOCH,
            opened_at: UnixMillis::EPOCH,
        },
    );
    RequestAttemptOwner::AgentStep {
        step_id: ModelStepId::new(turn_id, 1),
    }
}

fn finish_turn(journal: &mut SessionJournal, head_name: &str, name: &str) {
    let head = head(head_name);
    journal
        .apply(JournalRecord::TurnFinished {
            sequence: journal.next_sequence(),
            record_id: record_id(journal),
            expected_head_revision: journal.head_revision(&head).expect("fixture revision"),
            fact: TurnFinished {
                agent_id: AgentId::new("agent").expect("fixture agent"),
                turn_id: TurnId::new(name).expect("fixture turn"),
                semantic_boundary: journal
                    .head_target(&head)
                    .expect("fixture head")
                    .expect("fixture boundary")
                    .clone(),
                outcome: TurnOutcome::Completed,
                at: TurnFinishedAt::Observed {
                    completed_at: UnixMillis::EPOCH,
                },
            },
            head,
        })
        .expect("finish fixture turn");
}

fn create_head(journal: &mut SessionJournal, name: &str, from: &str) {
    journal
        .apply(JournalRecord::CreateHead {
            sequence: journal.next_sequence(),
            record_id: record_id(journal),
            head: head(name),
            at: journal
                .head_target(&head(from))
                .expect("fixture target")
                .cloned(),
        })
        .expect("create fixture head");
}

fn authorize(
    journal: &mut SessionJournal,
    head_name: &str,
    owner: RequestAttemptOwner,
    name: &str,
) {
    let head = head(head_name);
    journal
        .apply(JournalRecord::RequestAttemptAuthorized {
            sequence: journal.next_sequence(),
            record_id: record_id(journal),
            expected_head_revision: journal.head_revision(&head).expect("fixture revision"),
            fact: RequestAttemptAuthorized::new(
                attempt_id(name),
                owner,
                journal
                    .head_target(&head)
                    .expect("fixture head")
                    .expect("fixture boundary")
                    .clone(),
                RequestEnvironment::new(
                    replay_compatibility(),
                    RequestEnvironmentFingerprint::new([1; 32]),
                ),
                UnixMillis::EPOCH,
            ),
            head,
        })
        .expect("authorize fixture attempt");
}

fn finish(journal: &mut SessionJournal, name: &str, terminal: RequestAttemptTerminalState) {
    journal
        .apply(JournalRecord::RequestAttemptFinished {
            sequence: journal.next_sequence(),
            record_id: record_id(journal),
            fact: RequestAttemptTerminal::new(attempt_id(name), terminal)
                .expect("valid fixture terminal"),
        })
        .expect("finish fixture attempt");
}

fn compaction(name: &str) -> RequestAttemptOwner {
    RequestAttemptOwner::Compaction {
        compaction_id: CompactionId::new(name).expect("fixture compaction"),
    }
}

fn counts(tokens: u64) -> TokenCounts {
    TokenCounts {
        input: tokens,
        cached_input: Some(0),
        cache_write_input: Some(0),
        output: 0,
        reasoning_output: Some(0),
        total: tokens,
    }
}

fn cost(ticks: u64) -> RequestCost {
    RequestCost::Known {
        usd_ticks: UsdCostTicks::new(ticks),
    }
}

fn dispatched(usage: TokenUsage, cost: RequestCost) -> RequestAttemptTerminalState {
    RequestAttemptTerminalState::Dispatched {
        timing: DispatchedRequestTiming::new(UnixMillis::EPOCH, None, None, ElapsedMillis::new(2))
            .expect("fixture timing"),
        outcome: RequestDispatchedOutcome::Cancelled,
        usage,
        cost,
    }
}

/// TIM-3: one journal attempt reachable through several heads is incurred only once.
#[test]
fn tim_3_session_accounting_counts_shared_attempts_once() {
    let mut journal = journal();
    let owner = start_turn(&mut journal, "main", "turn");
    authorize(&mut journal, "main", owner, "attempt");
    let usage = TokenUsage::Complete(TokenCounts {
        input: 10,
        cached_input: Some(3),
        cache_write_input: Some(2),
        output: 4,
        reasoning_output: Some(1),
        total: 14,
    });
    finish(&mut journal, "attempt", dispatched(usage.clone(), cost(23)));
    finish_turn(&mut journal, "main", "turn");
    create_head(&mut journal, "other", "main");
    create_head(&mut journal, "third", "other");
    let before = journal.clone();

    assert_eq!(
        journal.incurred_accounting(),
        Ok(RequestAccounting {
            usage,
            cost: cost(23)
        })
    );
    assert_eq!(journal, before, "accounting is an on-demand projection");
}

/// TIM-3: abandoning a head cannot erase incurred usage; compaction is a separate subset.
#[test]
fn tim_3_session_accounting_includes_abandoned_branches_and_compaction() {
    let mut journal = journal();
    create_head(&mut journal, "abandoned", "main");
    for (head_name, turn_name, tokens, ticks) in [
        ("main", "main-turn", 3, 11),
        ("abandoned", "other-turn", 5, 13),
    ] {
        let owner = start_turn(&mut journal, head_name, turn_name);
        authorize(&mut journal, head_name, owner, turn_name);
        finish(
            &mut journal,
            turn_name,
            dispatched(TokenUsage::Complete(counts(tokens)), cost(ticks)),
        );
        finish_turn(&mut journal, head_name, turn_name);
    }
    for (name, tokens, ticks) in [("compact-first", 7, 17), ("compact-retry", 11, 19)] {
        authorize(&mut journal, "abandoned", compaction("compact"), name);
        finish(
            &mut journal,
            name,
            dispatched(TokenUsage::Complete(counts(tokens)), cost(ticks)),
        );
    }
    journal
        .apply(JournalRecord::AbandonHead {
            sequence: journal.next_sequence(),
            record_id: record_id(&journal),
            head: head("abandoned"),
            expected_head_revision: journal
                .head_revision(&head("abandoned"))
                .expect("fixture revision"),
        })
        .expect("abandon completed fixture head");

    assert_eq!(
        journal.incurred_accounting(),
        Ok(RequestAccounting {
            usage: TokenUsage::Complete(counts(26)),
            cost: cost(60),
        })
    );
    assert_eq!(
        journal.compaction_accounting(),
        Ok(RequestAccounting {
            usage: TokenUsage::Complete(counts(18)),
            cost: cost(36),
        })
    );

    let mut restored = SessionJournal::with_metadata(journal.metadata().clone());
    for record in journal.records() {
        restored
            .apply(record.clone())
            .expect("replay valid journal");
    }
    assert_eq!(
        restored.incurred_accounting(),
        journal.incurred_accounting()
    );
    assert_eq!(
        restored.compaction_accounting(),
        journal.compaction_accounting()
    );
}

/// TIM-3/TIM-5: partial usage, missing usage, and unresolved authorization never produce a full bill.
#[test]
fn tim_3_accounting_keeps_partial_missing_and_unpriced_attempts_honest() {
    for (name, terminal, expected_usage) in [
        (
            "partial",
            Some(dispatched(
                TokenUsage::Partial(counts(5)),
                RequestCost::Unavailable,
            )),
            TokenUsage::Partial(counts(8)),
        ),
        (
            "missing",
            Some(dispatched(
                TokenUsage::Unavailable,
                RequestCost::Unavailable,
            )),
            TokenUsage::Partial(counts(3)),
        ),
        ("orphan", None, TokenUsage::Partial(counts(3))),
        (
            "unpriced",
            Some(dispatched(
                TokenUsage::Complete(counts(5)),
                RequestCost::Unavailable,
            )),
            TokenUsage::Complete(counts(8)),
        ),
    ] {
        let mut journal = journal();
        authorize(&mut journal, "main", compaction("known"), "known");
        finish(
            &mut journal,
            "known",
            dispatched(TokenUsage::Complete(counts(3)), cost(11)),
        );
        authorize(&mut journal, "main", compaction(name), name);
        if let Some(terminal) = terminal {
            finish(&mut journal, name, terminal);
        }
        let expected = Ok(RequestAccounting {
            usage: expected_usage,
            cost: RequestCost::Unavailable,
        });
        assert_eq!(journal.incurred_accounting(), expected, "{name}");
        assert_eq!(journal.compaction_accounting(), expected, "{name}");
    }
}

/// TIM-3/TIM-5: a prefix containing only authorization has no invented zero usage or cost.
#[test]
fn tim_5_accounting_tracks_unresolved_authorization_without_fabricated_usage() {
    let mut journal = journal();
    authorize(&mut journal, "main", compaction("orphan"), "orphan");
    assert_eq!(
        journal.incurred_accounting(),
        Ok(RequestAccounting {
            usage: TokenUsage::Unavailable,
            cost: RequestCost::Unavailable,
        })
    );
    finish(
        &mut journal,
        "orphan",
        dispatched(TokenUsage::Complete(counts(5)), cost(7)),
    );
    assert_eq!(
        journal.incurred_accounting(),
        Ok(RequestAccounting {
            usage: TokenUsage::Complete(counts(5)),
            cost: cost(7),
        })
    );
}

/// TIM-3: no dispatch is known zero incurred cost, but never a fabricated provider usage report.
#[test]
fn tim_3_accounting_skips_not_dispatched_usage_and_retains_zero_cost() {
    let mut journal = journal();
    let empty = Ok(RequestAccounting {
        usage: TokenUsage::Unavailable,
        cost: cost(0),
    });
    assert_eq!(journal.incurred_accounting(), empty);
    authorize(&mut journal, "main", compaction("cancelled"), "cancelled");
    finish(
        &mut journal,
        "cancelled",
        RequestAttemptTerminalState::NotDispatched {
            outcome: RequestNotDispatchedOutcome::Cancelled,
        },
    );
    assert_eq!(journal.incurred_accounting(), empty);
    authorize(&mut journal, "main", compaction("known"), "known");
    finish(
        &mut journal,
        "known",
        dispatched(TokenUsage::Complete(counts(5)), cost(7)),
    );
    assert_eq!(
        journal.incurred_accounting(),
        Ok(RequestAccounting {
            usage: TokenUsage::Complete(counts(5)),
            cost: cost(7),
        })
    );
}

/// TIM-3: usage and cost overflow are typed errors, never saturation or a partial bill.
#[test]
fn tim_3_accounting_rejects_usage_and_cost_overflow() {
    for (first_tokens, first_ticks, expected) in [
        (
            u64::MAX,
            0,
            RequestAccountingError::UsageOverflow {
                attempt_id: attempt_id("second"),
            },
        ),
        (
            0,
            u64::MAX,
            RequestAccountingError::CostOverflow {
                attempt_id: attempt_id("second"),
            },
        ),
    ] {
        let mut journal = journal();
        for (name, tokens, ticks) in [("first", first_tokens, first_ticks), ("second", 1, 1)] {
            authorize(&mut journal, "main", compaction(name), name);
            finish(
                &mut journal,
                name,
                dispatched(TokenUsage::Complete(counts(tokens)), cost(ticks)),
            );
        }
        assert_eq!(journal.incurred_accounting(), Err(expected.clone()));
        assert_eq!(journal.compaction_accounting(), Err(expected));
    }
}
