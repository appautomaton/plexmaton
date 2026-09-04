use super::*;
use crate::{
    Agent, CompactionId, DispatchedRequestTiming, ElapsedMillis, Input, JournalRecord, ModelEvent,
    ModelOutputPosition, RequestAttemptAuthorized, RequestAttemptId, RequestAttemptTerminal,
    RequestCost, RequestDispatchedOutcome, RequestEnvironmentFingerprint, StopReason, UnixMillis,
    test_support::replay_compatibility,
};
use plexmaton_core::{AgentId, JournalRecordId, TokenCounts};

fn environment(seed: u8) -> RequestEnvironment {
    RequestEnvironment::new(
        replay_compatibility(),
        RequestEnvironmentFingerprint::new([seed; 32]),
    )
}
fn main_head() -> HeadName {
    HeadName::new("main").expect("head")
}
fn counts() -> TokenCounts {
    TokenCounts {
        input: 100,
        cached_input: Some(80),
        cache_write_input: Some(0),
        output: 20,
        reasoning_output: Some(10),
        total: 120,
    }
}
fn terminal(id: RequestAttemptId, usage: TokenUsage) -> RequestAttemptTerminal {
    RequestAttemptTerminal::new(
        id,
        RequestAttemptTerminalState::Dispatched {
            timing: DispatchedRequestTiming::new(
                UnixMillis::new(1),
                None,
                None,
                ElapsedMillis::new(2),
            )
            .expect("timing"),
            outcome: RequestDispatchedOutcome::Completed {
                stop_reason: StopReason::EndOfTurn,
            },
            usage,
            cost: RequestCost::Unavailable,
        },
    )
    .expect("terminal")
}
fn open() -> Agent {
    let mut agent = Agent::new(AgentId::new("agent").expect("id"));
    agent.announce("Agent");
    agent.handle(Input::Submitted {
        text: "first user".to_owned(),
    });
    agent
}
fn complete(agent: &mut Agent, usage: Option<TokenUsage>) {
    let step_id = agent.active_model_step().expect("step");
    let (id, _) = agent
        .authorize_request_attempt(step_id.clone(), environment(1), UnixMillis::new(1))
        .expect("authorize");
    if let Some(usage) = usage {
        agent
            .finish_request_attempt(&terminal(id, usage))
            .expect("terminal");
    }
    agent.handle(Input::Streamed {
        step_id: step_id.clone(),
        event: ModelEvent::TextDelta {
            position: ModelOutputPosition::new(0, 0),
            delta: "answer".to_owned(),
        },
    });
    agent.handle(Input::Streamed {
        step_id,
        event: ModelEvent::Stopped(StopReason::EndOfTurn),
    });
}

/// BUD-1/BUD-2: reload rebuilds the same anchor; billing totals and cache hits do not change occupancy.
#[test]
fn bud_2_anchor_uses_exact_input_and_survives_record_reload() {
    let mut agent = open();
    complete(&mut agent, Some(TokenUsage::Complete(counts())));
    agent.handle(Input::Submitted {
        text: "next user".to_owned(),
    });
    let journal = agent.journal();
    let before = journal.clone();
    let basis = journal
        .budget_basis(&main_head(), &environment(1))
        .expect("basis");
    let anchor = basis.anchor.as_ref().expect("measured prefix");
    assert_eq!(anchor.atom_count(), 1);
    assert_eq!(anchor.input_tokens(), 100);
    assert_eq!(basis.request.atoms.len(), 3);
    assert_eq!(journal, &before);
    let mut loaded = SessionJournal::with_metadata(journal.metadata().clone());
    for record in journal.records() {
        let encoded = serde_json::to_vec(record).expect("encode");
        loaded
            .apply(serde_json::from_slice(&encoded).expect("decode"))
            .expect("apply");
    }
    let replayed = loaded
        .budget_basis(&main_head(), &environment(1))
        .expect("basis");
    assert_eq!(basis.anchor, replayed.anchor);
    assert_eq!(basis.request, replayed.request);
}

/// BUD-2: missing, partial and different-environment reports cannot silently become measurements.
#[test]
fn bud_2_missing_partial_and_changed_environment_have_no_anchor() {
    for usage in [
        None,
        Some(TokenUsage::Unavailable),
        Some(TokenUsage::Partial(counts())),
    ] {
        let mut agent = open();
        complete(&mut agent, usage);
        assert!(
            agent
                .journal()
                .budget_basis(&main_head(), &environment(1))
                .expect("basis")
                .anchor
                .is_none()
        );
    }
    let mut agent = open();
    complete(&mut agent, Some(TokenUsage::Complete(counts())));
    assert!(
        agent
            .journal()
            .budget_basis(&main_head(), &environment(2))
            .expect("basis")
            .anchor
            .is_none()
    );
}

/// BUD-2: longer measured prefixes win; a rewind cannot borrow a later sibling's measurement.
#[test]
fn bud_2_longest_prefix_wins_and_other_branches_are_excluded() {
    let mut agent = open();
    complete(&mut agent, Some(TokenUsage::Complete(counts())));
    let first_boundary = agent
        .journal()
        .head_target(&main_head())
        .expect("head")
        .cloned();
    agent.handle(Input::Submitted {
        text: "next user".to_owned(),
    });
    complete(&mut agent, Some(TokenUsage::Complete(counts())));
    let mut journal = agent.journal().clone();
    assert_eq!(
        journal
            .budget_basis(&main_head(), &environment(1))
            .expect("basis")
            .anchor
            .expect("anchor")
            .atom_count(),
        3
    );
    let branch = HeadName::new("earlier").expect("id");
    journal
        .apply(JournalRecord::CreateHead {
            sequence: journal.next_sequence(),
            record_id: JournalRecordId::new("branch").expect("id"),
            head: branch.clone(),
            at: first_boundary,
        })
        .expect("branch");
    assert_eq!(
        journal
            .budget_basis(&branch, &environment(1))
            .expect("basis")
            .anchor
            .expect("anchor")
            .atom_count(),
        1
    );
}

/// BUD-2: a summarizer's input extends context with extra instructions, so it is never an agent anchor.
#[test]
fn bud_2_compaction_measurements_do_not_anchor_agent_context() {
    let mut agent = open();
    complete(&mut agent, None);
    let mut journal = agent.journal().clone();
    let id = RequestAttemptId::new("compaction-attempt").expect("id");
    journal
        .apply(JournalRecord::RequestAttemptAuthorized {
            sequence: journal.next_sequence(),
            record_id: JournalRecordId::new("compact-auth").expect("id"),
            head: main_head(),
            expected_head_revision: journal.head_revision(&main_head()).expect("revision"),
            fact: RequestAttemptAuthorized::new(
                id.clone(),
                RequestAttemptOwner::Compaction {
                    compaction_id: CompactionId::new("compact").expect("id"),
                },
                journal
                    .head_target(&main_head())
                    .expect("head")
                    .cloned()
                    .expect("target"),
                environment(1),
                UnixMillis::new(1),
            ),
        })
        .expect("authorize compaction");
    journal
        .apply(JournalRecord::RequestAttemptFinished {
            sequence: journal.next_sequence(),
            record_id: JournalRecordId::new("compact-end").expect("id"),
            fact: terminal(id, TokenUsage::Complete(counts())),
        })
        .expect("terminal");
    assert!(
        journal
            .budget_basis(&main_head(), &environment(1))
            .expect("basis")
            .anchor
            .is_none()
    );
}

/// BUD-2/JRN-5: reverse completion preserves a whole batch; a measured cut inside it is unusable.
#[test]
fn bud_2_parallel_batch_anchors_require_every_result_in_model_order() {
    use crate::{AdmissionOutcome, AdmissionRefusal, ContextAtomValue, ToolCall};
    use plexmaton_core::ToolCallId;

    let mut agent = open();
    let step = agent.active_model_step().expect("step");
    let calls: Vec<_> = (0..2)
        .map(|index| ToolCall {
            call_id: ToolCallId::new(format!("call-{index}")).expect("id"),
            name: "unknown_tool".to_owned(),
            arguments: "{}".to_owned(),
        })
        .collect();
    for (index, call) in calls.iter().enumerate() {
        agent.handle(Input::Streamed {
            step_id: step.clone(),
            event: ModelEvent::Called {
                position: ModelOutputPosition::new(index as u16, 0),
                call: call.clone(),
            },
        });
    }
    agent.handle(Input::Streamed {
        step_id: step,
        event: ModelEvent::Stopped(StopReason::ToolCalls),
    });
    agent.handle(Input::ToolAdmissionResolved(AdmissionOutcome::Refused {
        call_id: calls[1].call_id.clone(),
        reason: AdmissionRefusal::UnknownTool,
    }));
    let mut split = agent.journal().clone();
    let prior_records = split.records().len();
    agent.handle(Input::ToolAdmissionResolved(AdmissionOutcome::Refused {
        call_id: calls[0].call_id.clone(),
        reason: AdmissionRefusal::UnknownTool,
    }));
    let next_step = agent.active_model_step().expect("next step");
    // Construct an otherwise valid audit at the adversarial, partial-batch boundary.
    let id = RequestAttemptId::new("partial-batch-attempt").expect("id");
    split
        .apply(JournalRecord::RequestAttemptAuthorized {
            sequence: split.next_sequence(),
            record_id: JournalRecordId::new("bad-prefix-auth").expect("id"),
            head: main_head(),
            expected_head_revision: split.head_revision(&main_head()).expect("revision"),
            fact: RequestAttemptAuthorized::new(
                id.clone(),
                RequestAttemptOwner::AgentStep {
                    step_id: next_step.clone(),
                },
                split
                    .head_target(&main_head())
                    .expect("head")
                    .cloned()
                    .expect("target"),
                environment(1),
                UnixMillis::new(1),
            ),
        })
        .expect("partial-boundary audit");
    split
        .apply(JournalRecord::RequestAttemptFinished {
            sequence: split.next_sequence(),
            record_id: JournalRecordId::new("bad-prefix-end").expect("id"),
            fact: terminal(id, TokenUsage::Complete(counts())),
        })
        .expect("terminal");
    for original in &agent.journal().records()[prior_records..] {
        let mut record = original.clone();
        let JournalRecord::AppendEntry { sequence, .. } = &mut record else {
            panic!("result is a semantic append");
        };
        *sequence = split.next_sequence();
        split.apply(record).expect("remaining result");
    }
    assert!(
        split
            .budget_basis(&main_head(), &environment(1))
            .expect("basis")
            .anchor
            .is_none()
    );

    let (id, _) = agent
        .authorize_request_attempt(next_step, environment(1), UnixMillis::new(1))
        .expect("authorize complete batch");
    agent
        .finish_request_attempt(&terminal(id, TokenUsage::Complete(counts())))
        .expect("terminal");
    let basis = agent
        .journal()
        .budget_basis(&main_head(), &environment(1))
        .expect("basis");
    assert_eq!(basis.anchor.expect("whole prefix").atom_count(), 2);
    let ContextAtomValue::ToolBatch(batch) = basis.request.atoms[1].value() else {
        panic!("one whole batch");
    };
    assert_eq!(
        batch
            .results()
            .iter()
            .map(|result| result.call_id())
            .collect::<Vec<_>>(),
        calls.iter().map(|call| &call.call_id).collect::<Vec<_>>()
    );
}
