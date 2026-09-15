use super::{pending::PendingRootProjection, *};
use crate::{test_support::empty_session_for, tests::FixtureWorkspace};
use plexmaton_agent::UnixMillis;
use plexmaton_runtime::{DispatchReport, OwnedStopReport, RunnerGeneration};
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers},
};
use std::{fs, os::unix::fs::PermissionsExt as _};

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

fn one_history_warning(runtime: &mut LiveRuntime) -> (AgentId, String) {
    let warnings: Vec<_> = std::iter::from_fn(|| runtime.try_next_event())
        .filter_map(|envelope| match envelope.event {
            ConversationEvent::RuntimeWarning {
                agent_id, message, ..
            } => Some((agent_id, message)),
            _ => None,
        })
        .collect();
    assert_eq!(warnings.len(), 1, "one explicit history state");
    warnings.into_iter().next().expect("history warning")
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

/// CHB-3/INS-6: a canonical child whose journal is absent stays selectable and explains the gap.
#[tokio::test]
async fn missing_resumed_child_history_projects_one_explicit_unavailable_state() {
    let fixture = FixtureWorkspace::new();
    let (mut runtime, _, mut workspace, _) = empty_session_for(fixture.path(), agent("root"));
    let (mut collaboration, _ingress) =
        open(fixture.path(), runtime.conversation_id()).expect("open root collaboration");
    let conversation = conversation("missing-child");
    let child = agent("delegated-1");
    collaboration
        .announce(&mut runtime, conversation, AgentStatus::Idle)
        .expect("announce resumed child");

    collaboration
        .replay_children(&mut runtime)
        .expect("missing history is a visible child state");
    let events: Vec<_> = std::iter::from_fn(|| runtime.try_next_event()).collect();
    let warnings: Vec<_> = events
        .iter()
        .filter_map(|envelope| match &envelope.event {
            ConversationEvent::RuntimeWarning {
                agent_id, message, ..
            } => Some((agent_id.clone(), message.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        warnings,
        vec![(
            child,
            "History unavailable: this delegated conversation journal is missing.".to_owned()
        )]
    );
    assert!(!runtime.has_active_work(), "passive history starts no work");
    workspace.emit(events);
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).expect("terminal");
    workspace
        .draw(&mut terminal)
        .expect("restored roster frame");
    assert!(
        workspace
            .state()
            .agent(&agent("delegated-1"))
            .expect("restored child")
            .transcript()
            .any(|item| {
                item.kind == plexmaton_tui::TranscriptTextKind::Warning
                    && item.source
                        == "History unavailable: this delegated conversation journal is missing."
            }),
        "the child retains one explicit unavailable-history entry"
    );
    workspace.handle(&Event::Key(KeyEvent::new(
        KeyCode::Down,
        KeyModifiers::NONE,
    )));
    assert_eq!(
        workspace
            .state()
            .selected_agent()
            .map(|agent| agent.id.as_str()),
        Some("delegated-1"),
        "the unavailable child stays selectable"
    );

    collaboration.shutdown().await.expect("shutdown owner");
    runtime.shutdown().await.expect("shutdown root");
}

/// CHB-3/INS-6: a journal held by another owner is a readable locked state, never an empty child.
#[tokio::test]
async fn locked_resumed_child_history_projects_one_explicit_unavailable_state() {
    let fixture = FixtureWorkspace::new();
    let (mut runtime, _, _, _) = empty_session_for(fixture.path(), agent("root"));
    let (mut collaboration, _ingress) =
        open(fixture.path(), runtime.conversation_id()).expect("open root collaboration");
    let conversation = conversation("locked-child");
    let child = agent("delegated-1");
    let held = collaboration
        .children
        .create(conversation.clone(), UnixMillis::EPOCH)
        .expect("hold child journal");
    collaboration.announced.insert(conversation, child.clone());

    collaboration
        .replay_children(&mut runtime)
        .expect("locked history is a visible child state");
    assert_eq!(
        one_history_warning(&mut runtime),
        (
            child,
            "History unavailable: this delegated conversation journal is open in another session."
                .to_owned()
        )
    );
    assert!(!runtime.has_active_work(), "passive history starts no work");

    drop(held);
    collaboration.shutdown().await.expect("shutdown owner");
    runtime.shutdown().await.expect("shutdown root");
}

/// CHB-3/INS-6: malformed child evidence remains on disk and becomes an explicit invalid state.
#[tokio::test]
async fn corrupt_resumed_child_history_projects_one_explicit_unavailable_state() {
    let fixture = FixtureWorkspace::new();
    let (mut runtime, _, _, _) = empty_session_for(fixture.path(), agent("root"));
    let (mut collaboration, _ingress) =
        open(fixture.path(), runtime.conversation_id()).expect("open root collaboration");
    let conversation = conversation("corrupt-child");
    let child = agent("delegated-1");
    let path = collaboration
        .children
        .path_for(&conversation)
        .expect("child path");
    fs::write(&path, b"not a journal header\n").expect("write corrupt evidence");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("secure corrupt evidence");
    collaboration.announced.insert(conversation, child.clone());

    collaboration
        .replay_children(&mut runtime)
        .expect("invalid history is a visible child state");
    assert_eq!(
        one_history_warning(&mut runtime),
        (
            child,
            "History unavailable: this delegated conversation journal could not be validated."
                .to_owned()
        )
    );
    assert!(!runtime.has_active_work(), "passive history starts no work");
    assert_eq!(
        fs::read(&path).expect("corrupt evidence remains"),
        b"not a journal header\n"
    );

    collaboration.shutdown().await.expect("shutdown owner");
    runtime.shutdown().await.expect("shutdown root");
}
