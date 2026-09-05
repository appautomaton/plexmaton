use std::time::Duration;

use plexmaton_agent::{Input, UndeliveredReason};

use super::{FakeDriver, agent_id, runtime};
use crate::{LiveRuntime, RuntimeUpdate};

async fn finish(runtime: &mut LiveRuntime) -> Vec<plexmaton_agent::UndeliveredInput> {
    let mut returned = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        while runtime.has_active_work() {
            match runtime.next_update().await.expect("owned runtime update") {
                RuntimeUpdate::Report(report) => returned.extend(report.undelivered),
                RuntimeUpdate::Event(_) => {}
                RuntimeUpdate::Finished => break,
            }
        }
        returned.extend(runtime.take_report().undelivered);
    })
    .await
    .expect("skill input control settles");
    returned
}

/// SKL-6: explicit preparation must not make the interrupt it is holding behind input rejectable.
#[tokio::test]
async fn skill_preparation_cannot_starve_interrupt_at_input_capacity() {
    let driver = FakeDriver::new([]);
    let mut runtime = runtime(driver.clone());
    runtime
        .submit_skill(
            agent_id(),
            Input::Submitted {
                text: "$missing original".to_owned(),
            },
            "missing".to_owned(),
        )
        .await
        .expect("begin retained preparation");
    for index in 0..super::super::PENDING_INPUT_CAPACITY {
        assert!(
            runtime
                .submit(
                    agent_id(),
                    Input::Submitted {
                        text: format!("queued {index}"),
                    }
                )
                .await
                .expect("bounded pending input")
                .undelivered
                .is_empty()
        );
    }
    let report = runtime
        .submit(agent_id(), Input::Interrupted)
        .await
        .expect("interrupt retains control even when the queue is full");
    assert_eq!(
        report.undelivered.len(),
        super::super::PENDING_INPUT_CAPACITY
    );
    assert!(
        report
            .undelivered
            .iter()
            .all(|input| input.reason == UndeliveredReason::Interrupted)
    );
    let returned = finish(&mut runtime).await;
    assert_eq!(returned.len(), 1);
    assert_eq!(returned[0].text, "$missing original");
    assert_eq!(returned[0].skill.as_deref(), Some("missing"));
    assert_eq!(returned[0].reason, UndeliveredReason::Interrupted);
    assert!(
        driver.calls().await.is_empty(),
        "no abandoned input dispatched a model"
    );
    runtime.shutdown().await.expect("shutdown");
}

/// SKL-6: cancellation records its actual ownership cause, independently of file-worker timing.
#[tokio::test]
async fn skill_preparation_preserves_shutdown_and_persistence_failure_causes() {
    for reason in [
        UndeliveredReason::Shutdown,
        UndeliveredReason::PersistenceFailed,
    ] {
        let driver = FakeDriver::new([]);
        let mut runtime = runtime(driver.clone());
        runtime
            .submit_skill(
                agent_id(),
                Input::Submitted {
                    text: "$missing keep me".to_owned(),
                },
                "missing".to_owned(),
            )
            .await
            .expect("begin retained preparation");
        let report = if reason == UndeliveredReason::Shutdown {
            runtime
                .shutdown()
                .await
                .expect("shutdown joins preparation")
        } else {
            runtime.journal_failed = true;
            runtime.finish_failed_owners().await;
            runtime.take_report()
        };
        assert_eq!(report.undelivered.len(), 1);
        assert_eq!(report.undelivered[0].text, "$missing keep me");
        assert_eq!(report.undelivered[0].skill.as_deref(), Some("missing"));
        assert_eq!(report.undelivered[0].reason, reason);
        assert!(runtime.preparing_input.is_none());
        assert!(driver.calls().await.is_empty());
    }
}

/// SKL-5/JRN-8: editing into an explicit skill uses the same owned preparation and branch commit.
#[tokio::test]
async fn edited_retry_prepares_skill_before_replacing_the_failed_branch() {
    use super::{Script, tools::TestWorkspace};
    use plexmaton_agent::{ContextAtomValue, ModelError, ModelEvent, StopReason};
    let files = TestWorkspace::new("skill-edit-retry");
    let directory = files.0.join(".agents/skills/review");
    std::fs::create_dir_all(&directory).expect("skill directory");
    std::fs::write(
        directory.join("SKILL.md"),
        "---\nname: review\ndescription: Review code\n---\nEDIT_RETRY_SKILL",
    )
    .expect("skill body");
    let tools = files
        .catalog()
        .with_skill_roots(
            &files.0,
            &files.0,
            &plexmaton_file_tools::FileCancellation::new(),
        )
        .expect("skill catalog");
    let driver = FakeDriver::new([
        Script::Fail(ModelError::RateLimited { retry_after: None }),
        Script::Events(vec![ModelEvent::Stopped(StopReason::EndOfTurn)]),
    ]);
    let mut runtime =
        LiveRuntime::with_driver(agent_id(), "Plexmaton".to_owned(), driver.clone(), tools)
            .expect("runtime");
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "original".to_owned(),
            },
        )
        .await
        .expect("submit");
    super::retry::settle(&mut runtime).await;
    let target = runtime
        .retry_candidate()
        .expect("rate-limited target")
        .target;
    let original_journal = runtime.agent.journal().clone();
    let report = runtime
        .retry_skill(
            target.clone(),
            "$review edited".to_owned(),
            "review".to_owned(),
        )
        .await
        .expect("prepare edit");
    assert_eq!(report.accepted_retry_edit, Some(target));
    assert_eq!(
        runtime.agent.journal(),
        &original_journal,
        "loading cannot mutate a branch"
    );
    let mut reset = false;
    tokio::time::timeout(Duration::from_secs(5), async {
        while runtime.has_active_work() {
            if let RuntimeUpdate::Report(report) = runtime.next_update().await.expect("update") {
                reset |= report.projection_reset.is_some();
                assert!(report.undelivered.is_empty());
                assert!(report.skill_errors.is_empty());
            }
        }
    })
    .await
    .expect("edited retry settles");
    assert!(reset, "new branch publishes a replacement projection");
    let calls = driver.calls().await;
    assert_eq!(calls.len(), 2);
    assert!(matches!(calls[1].request.atoms.as_slice(), [text,skill]
        if matches!(text.value(),ContextAtomValue::User {text} if text == "$review edited")
        && matches!(skill.value(),ContextAtomValue::Skill(skill) if skill.instructions() == "EDIT_RETRY_SKILL")));
    runtime.shutdown().await.expect("shutdown");
}
