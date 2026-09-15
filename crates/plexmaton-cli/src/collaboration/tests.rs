use super::{pending::PendingRootProjection, *};
use crate::{test_support::empty_session_for, tests::FixtureWorkspace};
use plexmaton_agent::collaboration::{CollaborationEvent, CollaborationItemRef, CollaborationText};
use plexmaton_agent::{
    Agent, ApprovalPolicy, Effect, Input, ModelEvent, ModelOutputPosition, Reaction, StopReason,
    ToolCall, TurnBudget, UnixMillis,
};
use plexmaton_core::{DelegationId, ToolCallId, ToolCallStatus};
use plexmaton_runtime::{
    DelegatedChildFactory, DispatchReport, NativeToolCatalog, OwnedStopReport, RunnerGeneration,
};
use plexmaton_session_store::collaboration::CollaborationAttempt;
use plexmaton_session_store::{
    ConversationDirectory, DelegatedConversationDirectory, DelegatedJournalFile,
};
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers},
};
use std::{collections::VecDeque, fs, os::unix::fs::PermissionsExt as _, path::Path};

fn agent(value: &str) -> AgentId {
    AgentId::new(value).expect("agent")
}

fn conversation(value: &str) -> ConversationId {
    ConversationId::new(value).expect("conversation")
}

fn endpoint(conversation_id: &str, agent_id: &str) -> MailEndpoint {
    MailEndpoint {
        conversation: conversation(conversation_id),
        agent: agent(agent_id),
    }
}

fn fixture_model() -> plexmaton_provider::ResolvedModel {
    plexmaton_provider::ModelRegistry::parse(
        r#"
active_model = { provider = "fixture", model = "test" }
[providers.fixture]
base_url = "http://127.0.0.1:9/v1"
api_key_env = "PLEXMATON_TEST_UNUSED_KEY"
api = "openai_responses"
[providers.fixture.models.test]
id = "fixture"
allowed_reasoning_efforts = ["none", "low", "medium", "high", "xhigh", "max"]
context_window_tokens = 100000
max_output_tokens = 1000
output_reserve_tokens = 1000
"#,
    )
    .expect("fixture model")
    .active_model()
    .clone()
}

fn native_tools(root: &Path, model: &plexmaton_provider::ResolvedModel) -> NativeToolCatalog {
    NativeToolCatalog::open(
        root,
        model.api_key_env(),
        "/bin/false",
        "/bin/false",
        Vec::new(),
    )
    .expect("native tools")
}

async fn bound_root(
    fixture: &FixtureWorkspace,
    id: ConversationId,
) -> (
    LiveRuntime,
    Collaboration,
    plexmaton_provider::ResolvedModel,
    NativeToolCatalog,
) {
    let sessions = ConversationDirectory::under(fixture.path()).expect("session directory");
    let journal = sessions
        .create(id.clone(), UnixMillis::EPOCH)
        .expect("root journal");
    let (mut collaboration, ingress) = open(fixture.path(), &id).expect("root collaboration");
    let model = fixture_model();
    let base_tools = native_tools(fixture.path(), &model);
    let tools = base_tools
        .clone()
        .with_main_collaboration(ingress)
        .expect("Main collaboration tools");
    let key = plexmaton_provider::resolve_api_key(&model, Some("fixture-only".into()))
        .expect("fixture key");
    let runtime = LiveRuntime::provider_with_fresh_journal(
        agent("root"),
        "Plexmaton",
        model.clone(),
        key,
        tools,
        journal,
    )
    .await
    .expect("root runtime");
    let identity = runtime
        .main_collaboration_identity()
        .expect("Main runtime identity");
    collaboration.root = Some(identity.endpoint().clone());
    collaboration
        .owner
        .bind_main_runtime(identity)
        .expect("bind Main runtime");
    collaboration.bound_runtime = Some(runtime.collaboration_runtime_stamp());
    (runtime, collaboration, model, base_tools)
}

fn append_reaction(file: &mut DelegatedJournalFile, reaction: Reaction) {
    for record in reaction.records {
        file.append(record).expect("append delegated record");
    }
}

fn seed_interrupted_tool(file: &mut DelegatedJournalFile, child: &AgentId) {
    let mut owner = Agent::for_conversation(
        child.clone(),
        file.journal().metadata().clone(),
        TurnBudget::default(),
        ApprovalPolicy::default(),
    );
    let submitted = owner.handle_at(
        Input::Submitted {
            text: "inspect the file".to_owned(),
        },
        UnixMillis::EPOCH,
    );
    let step = submitted
        .effects
        .iter()
        .find_map(|effect| match effect {
            Effect::CallModel(call) => Some(call.step_id.clone()),
            _ => None,
        })
        .expect("submitted turn calls the model");
    append_reaction(file, submitted);
    append_reaction(
        file,
        owner.handle_at(
            Input::Streamed {
                step_id: step.clone(),
                event: ModelEvent::Called {
                    position: ModelOutputPosition::new(0, 0),
                    call: ToolCall {
                        call_id: ToolCallId::new("interrupted-read").expect("call"),
                        name: "read_file".to_owned(),
                        arguments: r#"{"path":"Cargo.toml"}"#.to_owned(),
                    },
                },
            },
            UnixMillis::EPOCH,
        ),
    );
    append_reaction(
        file,
        owner.handle_at(
            Input::Streamed {
                step_id: step,
                event: ModelEvent::Stopped(StopReason::ToolCalls),
            },
            UnixMillis::EPOCH,
        ),
    );
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
        orphaned_link: None,
    });

    collaboration
        .shutdown()
        .await
        .expect("durable projection rebuilds after reopen");
    runtime.shutdown().await.expect("shutdown root");
}

/// ENT-1/JRN-7: when an accepted ingress outlives its tool wait, the composition persists the
/// canonical reference through the caller's real journal owner before drawing the row.
#[tokio::test]
async fn orphaned_root_ingress_persists_one_link_before_live_projection() {
    let fixture = FixtureWorkspace::new();
    let root_id = conversation("root-session");
    let (mut runtime, mut collaboration, model, _) = bound_root(&fixture, root_id.clone()).await;
    while runtime.try_next_event().is_some() {}
    let root = endpoint("root-session", "root");
    let child = endpoint("child-session", "child-runtime");
    collaboration
        .announced
        .insert(child.conversation.clone(), child.agent.clone());
    let receipt = collaboration
        .owner
        .admit(CollaborationAttempt {
            id: CollaborationItemId::new("orphaned-delegation-item").expect("item"),
            event: CollaborationEvent::DelegationCreated {
                delegation: DelegationId::new("orphaned-delegation").expect("delegation"),
                delegator: root.clone(),
                worker: child,
                task: CollaborationText::new("accepted after the wait ended").expect("task"),
            },
        })
        .await
        .expect("admit orphaned mail");
    let reference = CollaborationItemRef {
        collaboration: collaboration.collaboration.clone(),
        item: receipt.id,
        sequence: receipt.sequence,
    };
    let records = collaboration.owner.records().await.expect("records");
    collaboration.observe_records(&records);
    let link = pending::OrphanedIngressLink {
        caller: root,
        reference,
    };
    assert_eq!(
        collaboration
            .persist_orphaned_link(&mut runtime, link.clone())
            .await
            .expect("persist orphaned link"),
        None
    );
    collaboration
        .persist_orphaned_link(&mut runtime, link)
        .await
        .expect("idempotent retry");
    collaboration
        .show(&mut runtime)
        .await
        .expect("project linked row");

    let delivered: Vec<_> = std::iter::from_fn(|| runtime.try_next_event())
        .filter_map(|envelope| match envelope.event {
            ConversationEvent::TaskAssigned { task, .. } => Some(task),
            _ => None,
        })
        .collect();
    assert_eq!(delivered, ["accepted after the wait ended"]);
    let source = runtime
        .collaboration_session_source()
        .expect("root session source");
    assert_eq!(
        source
            .journal()
            .collaboration_links(source.selected_head())
            .expect("session links")
            .len(),
        1,
        "a cancelled outer wait cannot duplicate its durable placement"
    );

    collaboration.shutdown().await.expect("shutdown owner");
    runtime.shutdown().await.expect("shutdown root");

    let sessions = ConversationDirectory::under(fixture.path()).expect("session directory");
    let session_path = sessions.path_for(&root_id).expect("session path");
    let collaboration_path = fixture
        .path()
        .join("collaborations")
        .join("root-session.jsonl");
    let session_bytes = fs::read(&session_path).expect("session bytes");
    let collaboration_bytes = fs::read(&collaboration_path).expect("collaboration bytes");
    let journal = sessions.resume(&root_id).expect("reopen root journal");
    let (mut restored_collaboration, ingress) =
        open(fixture.path(), &root_id).expect("reopen collaboration");
    let tools = native_tools(fixture.path(), &model)
        .with_main_collaboration(ingress)
        .expect("restored Main tools");
    let key = plexmaton_provider::resolve_api_key(&model, Some("fixture-only".into()))
        .expect("fixture key");
    let (mut restored, _) =
        LiveRuntime::provider_with_resumed_journal(agent("root"), model, key, tools, journal)
            .await
            .expect("reopen root runtime");
    let identity = restored
        .main_collaboration_identity()
        .expect("restored Main identity");
    restored_collaboration.root = Some(identity.endpoint().clone());
    restored_collaboration
        .owner
        .bind_main_runtime(identity)
        .expect("bind restored Main");
    restored_collaboration.bound_runtime = Some(restored.collaboration_runtime_stamp());
    restored_collaboration
        .restore(&mut restored)
        .await
        .expect("restore placed collaboration rows");
    let reopened: Vec<_> = std::iter::from_fn(|| restored.try_next_event())
        .filter_map(|envelope| match envelope.event {
            ConversationEvent::TaskAssigned { task, .. } => Some(task),
            _ => None,
        })
        .collect();
    assert_eq!(reopened, delivered);
    assert_eq!(
        fs::read(&session_path).expect("session bytes"),
        session_bytes
    );
    assert_eq!(
        fs::read(&collaboration_path).expect("collaboration bytes"),
        collaboration_bytes
    );
    restored_collaboration
        .shutdown()
        .await
        .expect("shutdown restored owner");
    restored.shutdown().await.expect("shutdown restored root");
}

/// ENT-1/JRN-5: real passive child history consumes only the exact resumed prefix; process
/// recovery may advance the same tool item once to its next cancelled revision.
#[tokio::test]
async fn passive_child_activation_applies_a_real_same_item_recovery_revision_once() {
    let fixture = FixtureWorkspace::new();
    let (mut runtime, mut collaboration, model, base_tools) =
        bound_root(&fixture, conversation("root-session")).await;
    while runtime.try_next_event().is_some() {}
    let root = endpoint("root-session", "root");
    let child = endpoint("child-session", "child-runtime");
    let delegation = DelegationId::new("delegation-1").expect("delegation");
    let creation = collaboration
        .owner
        .admit(CollaborationAttempt {
            id: CollaborationItemId::new("delegation-item").expect("item"),
            event: CollaborationEvent::DelegationCreated {
                delegation,
                delegator: root,
                worker: child.clone(),
                task: CollaborationText::new("inspect one file").expect("task"),
            },
        })
        .await
        .expect("admit delegation");
    let creation_reference = CollaborationItemRef {
        collaboration: collaboration.collaboration.clone(),
        item: creation.id,
        sequence: creation.sequence,
    };
    let mut child_file = collaboration
        .children
        .create(child.conversation.clone(), UnixMillis::EPOCH)
        .expect("child journal");
    seed_interrupted_tool(&mut child_file, &child.agent);
    drop(child_file);
    let key = plexmaton_provider::resolve_api_key(&model, Some("fixture-only".into()))
        .expect("fixture key");
    collaboration
        .owner
        .bind_child_factory(DelegatedChildFactory::new(
            DelegatedConversationDirectory::under(fixture.path()).expect("children"),
            model,
            key,
            base_tools,
        ))
        .expect("bind child factory");
    let selector = collaboration
        .owner
        .register_collaboration_targets()
        .await
        .expect("register target")
        .pop()
        .expect("one target")
        .selector()
        .clone();
    collaboration
        .sync_roster(&mut runtime, AgentStatus::Idle)
        .await
        .expect("restore roster");
    let records = collaboration.owner.records().await.expect("records");
    collaboration
        .replay_children(&mut runtime, &records)
        .await
        .expect("passive replay");
    let queued = std::iter::from_fn(|| runtime.try_next_event())
        .find_map(|envelope| match envelope.event {
            ConversationEvent::ToolCallChanged {
                item_id,
                item_revision: 0,
                status: ToolCallStatus::Queued,
                ..
            } => Some(item_id),
            _ => None,
        })
        .expect("passive queued tool");

    collaboration
        .owner
        .resume_collaboration_target(&selector)
        .await
        .expect("activate resumed child");
    assert_eq!(
        collaboration
            .persist_orphaned_link(
                &mut runtime,
                pending::OrphanedIngressLink {
                    caller: child.clone(),
                    reference: creation_reference.clone(),
                },
            )
            .await
            .expect("persist through active child owner"),
        Some(child.conversation.clone())
    );
    let source = collaboration
        .owner
        .child_session_source(&child.conversation)
        .await
        .expect("inspect child placement")
        .expect("active child source");
    assert_eq!(
        source
            .journal()
            .collaboration_links(source.selected_head())
            .expect("child links")
            .iter()
            .filter(|origin| origin.reference() == &creation_reference)
            .count(),
        1
    );
    let mut cancelled = Vec::new();
    for _ in 0..32 {
        let activity =
            tokio::time::timeout(std::time::Duration::from_secs(5), collaboration.next())
                .await
                .expect("child recovery activity")
                .expect("owned activity");
        if collaboration.stage(activity).expect("stage child event") {
            assert_eq!(
                collaboration
                    .drive_pending(&mut runtime)
                    .await
                    .expect("drive child event"),
                RootProjectionProgress::Applied
            );
        }
        cancelled.extend(
            std::iter::from_fn(|| runtime.try_next_event()).filter_map(|envelope| {
                match envelope.event {
                    ConversationEvent::ToolCallChanged {
                        item_id,
                        item_revision,
                        status: ToolCallStatus::Cancelled,
                        ..
                    } if item_id == queued => Some(item_revision),
                    _ => None,
                }
            }),
        );
        if !cancelled.is_empty() {
            break;
        }
    }
    assert_eq!(cancelled, [1], "recovery advances the restored tool once");

    collaboration.shutdown().await.expect("shutdown owner");
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
            user_input: None,
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

/// ENT-1/JRN-5: activating a passively restored child suppresses its replayed prefix while the
/// first genuinely new child item still crosses the runner boundary exactly once.
#[tokio::test]
async fn later_child_activation_skips_replayed_entries_and_accepts_new_work() {
    let fixture = FixtureWorkspace::new();
    let (mut runtime, _, _, _) = empty_session_for(fixture.path(), agent("root"));
    let (mut collaboration, _ingress) =
        open(fixture.path(), runtime.conversation_id()).expect("open root collaboration");
    let child = agent("delegated-1");
    let child_conversation = conversation("child-a");
    let replayed = TranscriptItemId::new("child-replayed").expect("item");
    collaboration.replayed_prefix.insert(
        child_conversation.clone(),
        VecDeque::from([
            ConversationEvent::AgentStatusChanged {
                agent_id: agent("child-runtime"),
                status: AgentStatus::Running,
            },
            ConversationEvent::TranscriptDelta {
                agent_id: agent("child-runtime"),
                item_id: replayed.clone(),
                item_revision: 1,
                text: "old work".to_owned(),
            },
        ]),
    );

    assert!(
        collaboration
            .prepare_forwarded_event(
                &child_conversation,
                &child,
                ConversationEvent::AgentStatusChanged {
                    agent_id: agent("child-runtime"),
                    status: AgentStatus::Running,
                },
            )
            .is_none(),
        "the activated runner cannot repeat its replayed status prefix"
    );
    assert!(
        collaboration
            .prepare_forwarded_event(
                &child_conversation,
                &child,
                ConversationEvent::TranscriptDelta {
                    agent_id: agent("child-runtime"),
                    item_id: replayed.clone(),
                    item_revision: 1,
                    text: "old work".to_owned(),
                },
            )
            .is_none(),
        "the activated runner cannot duplicate its restored prefix"
    );
    assert!(matches!(
        collaboration.prepare_forwarded_event(
            &child_conversation,
            &child,
            ConversationEvent::TranscriptDelta {
                agent_id: agent("child-runtime"),
                item_id: replayed.clone(),
                item_revision: 2,
                text: "recovered work".to_owned(),
            },
        ),
        Some(ConversationEvent::TranscriptDelta {
            agent_id,
            item_id,
            item_revision: 2,
            ..
        }) if agent_id == child && item_id == replayed
    ));
    let fresh = TranscriptItemId::new("child-fresh").expect("item");
    assert!(matches!(
        collaboration.prepare_forwarded_event(
            &child_conversation,
            &child,
            ConversationEvent::TranscriptDelta {
                agent_id: agent("child-runtime"),
                item_id: fresh.clone(),
                item_revision: 1,
                text: "new work".to_owned(),
            },
        ),
        Some(ConversationEvent::TranscriptDelta {
            agent_id,
            item_id,
            ..
        }) if agent_id == child && item_id == fresh
    ));
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
        .replay_children(&mut runtime, &[])
        .await
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
        .replay_children(&mut runtime, &[])
        .await
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
        .replay_children(&mut runtime, &[])
        .await
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
