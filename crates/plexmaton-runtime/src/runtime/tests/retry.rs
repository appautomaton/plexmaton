use super::*;
use crate::{RuntimeError, RuntimeUpdate};
use plexmaton_agent::{Agent, ContextAtomValue, JournalRecord};

pub(super) async fn settle(runtime: &mut LiveRuntime) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while runtime.has_active_work() {
            match runtime.next_update().await.expect("fixture update") {
                RuntimeUpdate::Report(report) => assert!(report.is_empty(), "{report:?}"),
                RuntimeUpdate::Event(_) | RuntimeUpdate::Finished => {}
            }
        }
        while runtime.try_next_event().is_some() {}
    })
    .await
    .expect("fixture settles");
}

fn limited() -> Script {
    Script::Fail(ModelError::RateLimited { retry_after: None })
}

/// JRN-5: a late rate limit never offers whole-question retry after retained model output.
#[tokio::test]
async fn partial_text_and_reasoning_disqualify_unanswered_retry() {
    for event in [
        text_delta("partial answer"),
        ModelEvent::ReasoningDelta {
            position: ModelOutputPosition::new(0, 0),
            delta: "partial reasoning".into(),
        },
    ] {
        let mut runtime = runtime(FakeDriver::new([Script::OutputThenFail(
            event,
            ModelError::RateLimited { retry_after: None },
        )]));
        runtime
            .submit(
                agent_id(),
                Input::Submitted {
                    text: "question".into(),
                },
            )
            .await
            .expect("submit");
        settle(&mut runtime).await;
        assert!(runtime.retry_candidate().is_none());
        assert!(runtime.agent.journal().records().iter().any(|record| matches!(record,
            JournalRecord::AppendEntry { entry, .. } if matches!(entry.payload, plexmaton_agent::JournalEntryPayload::AssistantOutput { .. })
        )));
    }
}

/// JRN-1/JRN-2: a user-defined archive name collision is a no-mutation refusal.
#[tokio::test]
async fn edit_retry_archive_collision_is_typed_and_keeps_both_heads_unchanged() {
    let mut runtime = runtime(FakeDriver::new([limited()]));
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "original".into(),
            },
        )
        .await
        .expect("submit");
    settle(&mut runtime).await;
    let mut journal = runtime.agent.journal().clone();
    let sequence = journal.next_sequence();
    journal
        .apply(JournalRecord::CreateHead {
            sequence,
            record_id: plexmaton_core::JournalRecordId::new("user-named-head").expect("id"),
            head: plexmaton_core::HeadName::new(format!("before-edit-{}", sequence.get() + 1))
                .expect("head"),
            at: None,
        })
        .expect("valid user head");
    let mut agent = Agent::from_journal(
        agent_id(),
        journal.clone(),
        Default::default(),
        Default::default(),
    )
    .expect("rebuild");
    let target = agent.retry_candidate().expect("eligible").target;
    assert!(
        agent
            .retry_at(&target, Some("changed".into()), UnixMillis::new(999))
            .is_err()
    );
    assert_eq!(agent.journal(), &journal);
    assert!(!agent.is_running());
}

/// JRN-5/JRN-6, session-interaction slice 2: repeated retries retain one user atom and
/// separate attempt identities; replay restores eligibility without executing anything.
#[tokio::test]
async fn repeated_retry_preserves_context_and_reopens_without_duplicate_user_input() {
    let driver = FakeDriver::new([
        limited(),
        limited(),
        Script::Events(vec![
            text_delta("answer"),
            ModelEvent::Stopped(StopReason::EndOfTurn),
        ]),
    ]);
    let mut runtime = runtime(driver.clone());
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "original".into(),
            },
        )
        .await
        .expect("submit");
    settle(&mut runtime).await;
    let first = runtime.retry_candidate().expect("eligible").target;
    runtime.retry(first.clone(), None).await.expect("retry");
    assert!(matches!(
        runtime.retry(first.clone(), None).await,
        Err(RuntimeError::RetryUnavailable)
    ));
    settle(&mut runtime).await;
    let second = runtime.retry_candidate().expect("retry again").target;
    assert_ne!(first.turn_id, second.turn_id);
    let journal = runtime.agent.journal().clone();
    let restored = Agent::from_journal(
        agent_id(),
        journal.clone(),
        Default::default(),
        Default::default(),
    )
    .expect("rebuild");
    assert_eq!(
        restored.retry_candidate().expect("restored retry").target,
        second
    );
    assert_eq!(restored.journal().records(), journal.records());
    assert!(matches!(
        runtime.retry(first, None).await,
        Err(RuntimeError::RetryUnavailable)
    ));
    runtime.retry(second, None).await.expect("second retry");
    settle(&mut runtime).await;
    let calls = driver.calls().await;
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0].request, calls[1].request);
    assert_eq!(calls[1].request, calls[2].request);
    assert_eq!(calls[0].request.atoms.len(), 1);
    assert!(runtime.retry_candidate().is_none());
    assert_eq!(runtime.agent.journal().request_attempts().count(), 3);
    assert!(!runtime.agent.journal().records().iter().any(|r| matches!(
        r,
        JournalRecord::CreateHead { .. } | JournalRecord::MoveHead { .. }
    )));
}

/// JRN-1/JRN-5: editing preserves the old path; sending normally instead appends on the same path.
#[tokio::test]
async fn edit_retry_branches_but_normal_submission_keeps_both_questions() {
    for edited in [false, true] {
        let driver = FakeDriver::new([limited(), limited()]);
        let mut runtime = runtime(driver.clone());
        runtime
            .submit(
                agent_id(),
                Input::Submitted {
                    text: "original".into(),
                },
            )
            .await
            .expect("submit");
        settle(&mut runtime).await;
        let target = runtime.retry_candidate().expect("eligible").target;
        let old_records = runtime.agent.journal().records().to_vec();
        if edited {
            let report = runtime
                .retry(target.clone(), Some("changed".into()))
                .await
                .expect("edit retry");
            assert!(report.projection_reset.is_some());
        } else {
            runtime
                .submit(
                    agent_id(),
                    Input::Submitted {
                        text: "changed".into(),
                    },
                )
                .await
                .expect("continue");
        }
        settle(&mut runtime).await;
        assert!(matches!(
            runtime.retry(target, None).await,
            Err(RuntimeError::RetryUnavailable)
        ));
        let calls = driver.calls().await;
        let questions: Vec<_> = calls[1]
            .request
            .atoms
            .iter()
            .filter_map(|a| match a.value() {
                ContextAtomValue::User { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            questions,
            if edited {
                vec!["changed"]
            } else {
                vec!["original", "changed"]
            }
        );
        let journal = runtime.agent.journal();
        assert!(journal.records().starts_with(&old_records));
        let archive = journal.records().iter().find_map(|r| match r {
            JournalRecord::CreateHead { head, .. } => Some(head),
            _ => None,
        });
        assert_eq!(archive.is_some(), edited);
        if let Some(head) = archive {
            assert_eq!(
                journal.project(head).expect("archive").request(),
                &calls[0].request
            );
        }
    }
}
