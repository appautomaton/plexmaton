use super::{pending::PendingRootProjection, *};
use crate::{test_support::empty_session_for, tests::FixtureWorkspace};
use plexmaton_runtime::{DispatchReport, OwnedStopReport, RunnerGeneration};

fn agent(value: &str) -> AgentId {
    AgentId::new(value).expect("agent")
}

fn conversation(value: &str) -> ConversationId {
    ConversationId::new(value).expect("conversation")
}

fn failed_projection() -> PendingRootProjection {
    PendingRootProjection::RunnerEvent {
        child: conversation("child-a"),
        generation: RunnerGeneration::new(7).expect("generation"),
        event: Box::new(ConversationEvent::AgentStatusChanged {
            agent_id: agent("delegated-1"),
            status: AgentStatus::Failed,
        }),
        recovery: pending::RecoverySource::None,
    }
}

impl Collaboration {
    /// Makes one persisted child addressable without creating a process-local runner.
    pub(crate) fn announce_resumed_child_for_test(
        &mut self,
        conversation: ConversationId,
        agent: AgentId,
    ) {
        self.announced.insert(conversation, agent);
    }
}

/// JRN-7/ENT-1: a selected root fact cannot cross the session picker's runtime replacement.
#[tokio::test]
async fn retained_projection_stays_with_its_root_and_applies_once() {
    let fixture = FixtureWorkspace::new();
    let (mut root_runtime, _, _, _) = empty_session_for(fixture.path(), agent("root"));
    let (mut replacement, _, _, _) = empty_session_for(fixture.path(), agent("replacement"));
    while root_runtime.try_next_event().is_some() {}
    while replacement.try_next_event().is_some() {}
    let (mut collaboration, _ingress) =
        open(fixture.path(), root_runtime.conversation_id()).expect("open root collaboration");
    collaboration.bound_runtime = Some(root_runtime.collaboration_runtime_stamp());
    collaboration.pending_projection = Some(failed_projection());

    assert_eq!(
        collaboration
            .drive_pending(&mut replacement)
            .await
            .expect("mismatch is typed"),
        RootProjectionProgress::ConversationMismatch
    );
    assert!(collaboration.has_pending_projection());
    assert!(replacement.try_next_event().is_none());
    assert_eq!(replacement.delegated_projection_refusal(), None);

    assert_eq!(
        collaboration
            .drive_pending(&mut root_runtime)
            .await
            .expect("apply to root"),
        RootProjectionProgress::Applied
    );
    assert!(!collaboration.has_pending_projection());
    let projected = root_runtime.try_next_event().expect("one root event");
    assert!(matches!(
        projected.event,
        ConversationEvent::AgentStatusChanged {
            ref agent_id,
            status: AgentStatus::Failed,
        } if agent_id == &agent("delegated-1")
    ));
    assert!(root_runtime.try_next_event().is_none(), "exactly once");

    collaboration.shutdown().await.expect("shutdown owner");
    root_runtime.shutdown().await.expect("shutdown root");
    replacement.shutdown().await.expect("shutdown replacement");
}

/// JRN-7/ENT-1: reopening the same durable conversation does not inherit the old runtime's
/// process-local projection authority.
#[tokio::test]
async fn retained_projection_rejects_a_replacement_for_the_same_conversation() {
    let fixture = FixtureWorkspace::new();
    let (mut bound_runtime, _, _, _) = empty_session_for(fixture.path(), agent("bound"));
    let (mut replacement, _, _, _) = empty_session_for(fixture.path(), agent("replacement"));
    while bound_runtime.try_next_event().is_some() {}
    while replacement.try_next_event().is_some() {}

    // Pair the replacement's durable conversation with the old process-local stamp to isolate the
    // instance check. This is the state left when a picker reopens the same conversation without
    // rebinding the collaboration composition.
    let (mut collaboration, _ingress) =
        open(fixture.path(), replacement.conversation_id()).expect("open root collaboration");
    collaboration.bound_runtime = Some(bound_runtime.collaboration_runtime_stamp());
    collaboration.pending_projection = Some(failed_projection());

    assert_eq!(replacement.conversation_id(), &collaboration.conversation);
    assert_eq!(
        collaboration
            .drive_pending(&mut replacement)
            .await
            .expect("replacement refusal is typed"),
        RootProjectionProgress::ConversationMismatch
    );
    assert!(collaboration.has_pending_projection());
    assert!(replacement.try_next_event().is_none());

    collaboration.shutdown().await.expect_err(
        "the transient projection stays owned and must be reported when the bound runtime is gone",
    );
    bound_runtime
        .shutdown()
        .await
        .expect("shutdown bound runtime");
    replacement.shutdown().await.expect("shutdown replacement");
}

/// JRN-7: a transient runner outcome retained behind a failed/unfinished root cannot vanish at exit.
#[tokio::test]
async fn shutdown_surfaces_an_unresolved_transient_projection() {
    let fixture = FixtureWorkspace::new();
    let (mut runtime, _, _, _) = empty_session_for(fixture.path(), agent("root"));
    let (mut collaboration, _ingress) =
        open(fixture.path(), runtime.conversation_id()).expect("open root collaboration");
    collaboration.pending_projection = Some(failed_projection());

    let error = collaboration
        .shutdown()
        .await
        .expect_err("transient projection must be observable")
        .to_string();
    assert!(error.contains("unresolved root projection"));
    assert!(error.contains("child-a"));
    assert!(error.contains("generation 7"));
    assert!(error.contains("AgentStatusChanged"));
    runtime.shutdown().await.expect("shutdown root");
}

/// JRN-7: a durable log refresh may be rebuilt after reopen and needs no transient shutdown error.
#[tokio::test]
async fn shutdown_accepts_a_pending_durable_refresh() {
    let fixture = FixtureWorkspace::new();
    let (mut runtime, _, _, _) = empty_session_for(fixture.path(), agent("root"));
    let (mut collaboration, _ingress) =
        open(fixture.path(), runtime.conversation_id()).expect("open root collaboration");
    collaboration.pending_projection = Some(PendingRootProjection::RefreshLog {
        sync_running_roster: true,
    });

    collaboration
        .shutdown()
        .await
        .expect("durable projection rebuilds after reopen");
    runtime.shutdown().await.expect("shutdown root");
}

/// SCH-2/SCH-4: a retained Stop settlement is consumed once and never becomes a root event.
#[tokio::test]
async fn stop_settlement_is_consumed_exactly_once() {
    let fixture = FixtureWorkspace::new();
    let (mut runtime, _, _, _) = empty_session_for(fixture.path(), agent("root"));
    let (mut collaboration, _ingress) =
        open(fixture.path(), runtime.conversation_id()).expect("open root collaboration");
    let child = agent("delegated-1");
    collaboration.pending_projection = Some(PendingRootProjection::StopSettled {
        agent: child.clone(),
        outcome: Box::new(Ok(OwnedStopReport {
            scheduled: None,
            stopped: DispatchReport::default(),
        })),
    });

    let (target, result) = collaboration
        .take_stop_settlement()
        .expect("settlement retained");
    assert_eq!(target, child);
    assert!(result.is_ok());
    assert!(!collaboration.has_pending_projection());
    assert!(
        collaboration.take_stop_settlement().is_none(),
        "one settlement cannot be applied twice"
    );
    assert!(
        runtime.try_next_event().is_none(),
        "Stop is not a transcript event"
    );

    collaboration.shutdown().await.expect("shutdown owner");
    runtime.shutdown().await.expect("shutdown root");
}

/// SCH-2/INV-7: a known resumed child without a process-local runner refuses Stop twice without
/// touching the root runtime.
#[tokio::test]
async fn resumed_child_stop_refusal_is_repeatable_and_root_is_untouched() {
    let fixture = FixtureWorkspace::new();
    let (mut runtime, _, _, _) = empty_session_for(fixture.path(), agent("root"));
    while runtime.try_next_event().is_some() {}
    let (mut collaboration, _ingress) =
        open(fixture.path(), runtime.conversation_id()).expect("open root collaboration");
    let child = conversation("resumed-child");
    let child_agent = agent("delegated-1");
    collaboration.announced.insert(child, child_agent.clone());

    for _ in 0..2 {
        assert!(matches!(
            collaboration.begin_child_stop(&child_agent),
            Err(OwnedSchedulingError::UnknownRunner)
        ));
        assert!(runtime.try_next_event().is_none(), "root stayed untouched");
    }

    collaboration.shutdown().await.expect("shutdown owner");
    runtime.shutdown().await.expect("shutdown root");
}
