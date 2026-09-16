use super::{pending::PendingRootProjection, *};
use crate::{
    test_support::empty_session_for,
    tests::{FixtureServer, FixtureWorkspace, fixture_http_server},
};
use plexmaton_agent::collaboration::{
    AttentionReference, CollaborationEvent, CollaborationItemRef, CollaborationText,
    DelegationRevision,
};
use plexmaton_agent::{
    Agent, ApprovalPolicy, Effect, Input, ModelEvent, ModelOutputPosition, Reaction, StopReason,
    ToolCall, ToolDefinitionRevision, TurnBudget, UnixMillis,
};
use plexmaton_core::{
    ApprovalDecision, ApprovalId, AttentionId, AttentionRequest, DelegationId, ToolCallId,
    ToolCallStatus, ToolCapability, ToolDefinitionId,
};
use plexmaton_runtime::{
    DelegatedChildFactory, DispatchReport, NativeToolCatalog, OwnedStopReport, RunnerGeneration,
};
use plexmaton_session_store::collaboration::{CollaborationAttempt, CollaborationFile};
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

fn chat_fixture_model(base_url: &str) -> plexmaton_provider::ResolvedModel {
    plexmaton_provider::ModelRegistry::parse(&format!(
        r#"
active_model = {{ provider = "fixture", model = "test" }}
[providers.fixture]
base_url = "{base_url}"
api_key_env = "PLEXMATON_TEST_UNUSED_KEY"
[providers.fixture.models.test]
api = "openai_chat_completions"
id = "fixture"
reasoning_effort = "none"
allowed_reasoning_efforts = ["none", "low", "medium", "high", "xhigh", "max"]
context_window_tokens = 100000
max_output_tokens = 1000
output_reserve_tokens = 1000
"#,
    ))
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
    bound_root_with_model(fixture, id, fixture_model()).await
}

async fn bound_root_with_model(
    fixture: &FixtureWorkspace,
    id: ConversationId,
    model: plexmaton_provider::ResolvedModel,
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

async fn reopened_bound_root(
    fixture: &FixtureWorkspace,
    id: &ConversationId,
    model: plexmaton_provider::ResolvedModel,
    base_tools: NativeToolCatalog,
) -> (LiveRuntime, Collaboration) {
    let journal = ConversationDirectory::under(fixture.path())
        .expect("session directory")
        .resume(id)
        .expect("resume root journal");
    let (mut collaboration, ingress) = open(fixture.path(), id).expect("reopen collaboration");
    let root_tools = base_tools
        .clone()
        .with_main_collaboration(ingress)
        .expect("restore Main collaboration tools");
    let root_key = plexmaton_provider::resolve_api_key(&model, Some("fixture-only".into()))
        .expect("fixture root key");
    let (mut runtime, _recovery) = LiveRuntime::provider_with_resumed_journal(
        agent("root"),
        model.clone(),
        root_key,
        root_tools,
        journal,
    )
    .await
    .expect("resume root runtime");
    let child_key = plexmaton_provider::resolve_api_key(&model, Some("fixture-only".into()))
        .expect("fixture child key");
    collaboration
        .bind(
            &runtime,
            DelegatedChildFactory::new(
                DelegatedConversationDirectory::under(fixture.path()).expect("children"),
                model,
                child_key,
                base_tools,
            ),
        )
        .expect("bind reopened collaboration");
    collaboration
        .restore(&mut runtime)
        .await
        .expect("restore collaboration");
    (runtime, collaboration)
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

fn seed_pending_approval(
    file: &mut DelegatedJournalFile,
    child: &AgentId,
) -> (ApprovalId, AttentionId) {
    let mut owner = Agent::for_conversation(
        child.clone(),
        file.journal().metadata().clone(),
        TurnBudget::default(),
        ApprovalPolicy::default(),
    );
    let submitted = owner.handle_at(
        Input::Submitted {
            text: "change the file".to_owned(),
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
                        call_id: ToolCallId::new("pending-write").expect("call"),
                        name: "edit_file".to_owned(),
                        arguments: r#"{"path":"README.md"}"#.to_owned(),
                    },
                },
            },
            UnixMillis::EPOCH,
        ),
    );
    let mut stopped = owner.handle_at(
        Input::Streamed {
            step_id: step,
            event: ModelEvent::Stopped(StopReason::ToolCalls),
        },
        UnixMillis::EPOCH,
    );
    let request = stopped
        .effects
        .pop()
        .and_then(|effect| match effect {
            Effect::AdmitTool(request) => Some(request),
            _ => None,
        })
        .expect("tool call requests trusted admission");
    append_reaction(file, stopped);
    let admitted = request
        .admit(
            ToolDefinitionId::new("fixture-edit").expect("definition"),
            ToolDefinitionRevision::new(1).expect("revision"),
            [ToolCapability::FileWrite],
            r#"{"path":"README.md"}"#.to_owned(),
            "edit README.md".to_owned(),
            None,
        )
        .expect("admit protected fixture");
    let waiting = owner.handle_at(Input::ToolAdmissionResolved(admitted), UnixMillis::EPOCH);
    append_reaction(file, waiting);
    let pending = owner
        .pending_approvals()
        .next()
        .expect("protected call is waiting");
    (
        pending.approval_id().clone(),
        pending.attention_id().clone(),
    )
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

/// ATT-1/APV-6/JRN-7: shutdown drains recovery resolution before a full passive reopen.
#[tokio::test]
async fn graceful_shutdown_resolution_does_not_reopen_a_child_request() {
    let fixture = FixtureWorkspace::new();
    let root_id = conversation("shutdown-attention-root");
    let (mut runtime, mut collaboration, model, base_tools) =
        bound_root(&fixture, root_id.clone()).await;
    let root = collaboration.root.clone().expect("root endpoint");
    let worker = endpoint("shutdown-attention-child", "child-runtime");
    collaboration
        .owner
        .admit(CollaborationAttempt {
            id: CollaborationItemId::new("shutdown-attention-created").expect("item"),
            event: CollaborationEvent::DelegationCreated {
                delegation: DelegationId::new("shutdown-attention-task").expect("delegation"),
                delegator: root,
                worker: worker.clone(),
                task: CollaborationText::new("retain shutdown request").expect("task"),
            },
        })
        .await
        .expect("create child");
    let mut child_file = collaboration
        .children
        .create(worker.conversation.clone(), UnixMillis::EPOCH)
        .expect("create child journal");
    let (approval_id, attention_id) = seed_pending_approval(&mut child_file, &worker.agent);
    drop(child_file);
    collaboration
        .admit_attention(
            &worker.conversation,
            RunnerGeneration::new(1).expect("generation"),
            &ConversationEvent::AttentionRequested {
                agent_id: worker.agent.clone(),
                attention_id: attention_id.clone(),
                request: AttentionRequest::Approval {
                    approval_id,
                    call_id: ToolCallId::new("pending-write").expect("call"),
                    tool: "edit_file".to_owned(),
                    capabilities: vec![ToolCapability::FileWrite],
                    detail: "edit README.md".to_owned(),
                    reason: plexmaton_core::ApprovalReason::NativeFileChange,
                    remember: None,
                },
            },
        )
        .await
        .expect("admit pending request reference");
    let child_key = plexmaton_provider::resolve_api_key(&model, Some("fixture-only".into()))
        .expect("fixture child key");
    collaboration
        .owner
        .bind_child_factory(DelegatedChildFactory::new(
            DelegatedConversationDirectory::under(fixture.path()).expect("children"),
            model.clone(),
            child_key,
            base_tools.clone(),
        ))
        .expect("bind child factory");
    let selector = collaboration
        .owner
        .register_collaboration_targets()
        .await
        .expect("register child")
        .pop()
        .expect("one child")
        .selector()
        .clone();
    collaboration
        .owner
        .resume_collaboration_target(&selector)
        .await
        .expect("activate child recovery");
    collaboration
        .shutdown()
        .await
        .expect("shutdown admits the recovery resolution");
    runtime.shutdown().await.expect("shutdown root");
    let file = CollaborationFile::open(
        fixture
            .path()
            .join("collaborations")
            .join(format!("{}.jsonl", root_id.as_str())),
    )
    .expect("reopen collaboration");
    for resolved in [false, true] {
        assert!(
            file.ledger()
                .records()
                .iter()
                .any(|record| match &record.event {
                    CollaborationEvent::AttentionRequested { attention } if !resolved => {
                        attention.producer == worker && attention.attention_id == attention_id
                    }
                    CollaborationEvent::AttentionResolved { attention } if resolved => {
                        attention.producer == worker && attention.attention_id == attention_id
                    }
                    _ => false,
                })
        );
    }
    drop(file);

    let (mut reopened_runtime, mut reopened) =
        reopened_bound_root(&fixture, &root_id, model, base_tools).await;
    let mut workspace = plexmaton_tui::Workspace::default();
    workspace.emit(std::iter::from_fn(|| reopened_runtime.try_next_event()).collect());
    assert_eq!(workspace.state().attention_count(), 0);
    assert_eq!(workspace.state().attention_pending(), 0);
    reopened.shutdown().await.expect("shutdown reopened owner");
    reopened_runtime
        .shutdown()
        .await
        .expect("shutdown reopened root");
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

/// CCV-1–CCV-4: canonical Main/User snapshots and Handoff rows reach the real workspace.
#[tokio::test]
async fn control_snapshots_apply_to_the_announced_child_and_unlock_after_handoff() {
    let fixture = FixtureWorkspace::new();
    let (mut runtime, mut collaboration, _, _) =
        bound_root(&fixture, conversation("control-root")).await;
    let root = collaboration.root.clone().expect("root endpoint");
    let worker = endpoint("control-child", "child-runtime");
    let delegation = DelegationId::new("control-task").expect("delegation");
    let created = collaboration
        .owner
        .admit(CollaborationAttempt {
            id: CollaborationItemId::new("control-created").expect("item"),
            event: CollaborationEvent::DelegationCreated {
                delegation: delegation.clone(),
                delegator: root.clone(),
                worker: worker.clone(),
                task: CollaborationText::new("show controller").expect("task"),
            },
        })
        .await
        .expect("create delegation");
    drop(
        collaboration
            .children
            .create(worker.conversation.clone(), UnixMillis::EPOCH)
            .expect("create passive child history"),
    );
    runtime
        .link_collaboration_item(CollaborationItemRef {
            collaboration: collaboration.collaboration.clone(),
            item: created.id,
            sequence: created.sequence,
        })
        .await
        .expect("link delegation to Main history");
    collaboration
        .sync_roster(&mut runtime, AgentStatus::Idle)
        .await
        .expect("sync Main controller");
    let records = collaboration
        .owner
        .records()
        .await
        .expect("initial delegation records");
    collaboration
        .replay_children(&mut runtime, &records)
        .await
        .expect("project the passive child before Handoff");
    let mut workspace = plexmaton_tui::Workspace::default();
    workspace.emit(std::iter::from_fn(|| runtime.try_next_event()).collect());
    collaboration
        .apply_child_controls(&mut workspace)
        .expect("apply Main controller");
    let child = agent("delegated-1");
    assert_eq!(
        workspace
            .state()
            .agent(&child)
            .expect("announced child")
            .control(),
        Some(plexmaton_tui::ChildControlSnapshot {
            revision: 0,
            control: plexmaton_tui::ChildControl::Main,
        })
    );
    let handoff = collaboration
        .owner
        .handoff(CollaborationAttempt {
            id: CollaborationItemId::new("control-handoff").expect("item"),
            event: CollaborationEvent::HandoffCompleted {
                delegation,
                expected: DelegationRevision(0),
                author: root,
            },
        })
        .await
        .expect("durable Handoff");
    runtime
        .link_collaboration_item(CollaborationItemRef {
            collaboration: collaboration.collaboration.clone(),
            item: handoff.receipt.id,
            sequence: handoff.receipt.sequence,
        })
        .await
        .expect("link Handoff to Main history");
    collaboration
        .sync_roster(&mut runtime, AgentStatus::Idle)
        .await
        .expect("sync User controller");
    collaboration
        .show(&mut runtime)
        .await
        .expect("show Handoff");
    collaboration
        .refresh_pending_children(&mut runtime)
        .await
        .expect("show Handoff immediately in passive child history");
    workspace.emit(std::iter::from_fn(|| runtime.try_next_event()).collect());
    collaboration
        .apply_child_controls(&mut workspace)
        .expect("apply User controller");
    assert_eq!(
        workspace
            .state()
            .agent(&child)
            .expect("announced child")
            .control(),
        Some(plexmaton_tui::ChildControlSnapshot {
            revision: 1,
            control: plexmaton_tui::ChildControl::User,
        })
    );
    let root_handoff = workspace
        .state()
        .agent(&agent("root"))
        .expect("root")
        .entries()
        .find_map(|entry| match entry {
            plexmaton_tui::TranscriptEntryView::Handoff(view) => Some(view.entry_id.clone()),
            _ => None,
        })
        .expect("root Handoff entry");
    let child_handoff = workspace
        .state()
        .agent(&child)
        .expect("child")
        .entries()
        .find_map(|entry| match entry {
            plexmaton_tui::TranscriptEntryView::Handoff(view) => Some(view.entry_id.clone()),
            _ => None,
        })
        .expect("child Handoff entry");
    assert_ne!(root_handoff, child_handoff);
    collaboration.shutdown().await.expect("shutdown owner");
    runtime.shutdown().await.expect("shutdown root");
}

fn settle_workspace_frame(
    workspace: &mut plexmaton_tui::Workspace,
    terminal: &mut Terminal<TestBackend>,
) {
    loop {
        workspace.draw(terminal).expect("draw User child input");
        let Some(work) = workspace.take_preparation() else {
            break;
        };
        let prepared = plexmaton_tui::preparation::prepare_batch(&work.requests)
            .expect("prepare bounded child frame");
        assert!(workspace.complete_preparation(work.token, prepared));
    }
}

fn focused_child_submission(workspace: &mut plexmaton_tui::Workspace) -> plexmaton_tui::Submission {
    let mut terminal = Terminal::new(TestBackend::new(95, 36)).expect("terminal");
    workspace.draw(&mut terminal).expect("draw roster");
    workspace.handle(&Event::Key(KeyEvent::new(
        KeyCode::Down,
        KeyModifiers::NONE,
    )));
    workspace.draw(&mut terminal).expect("draw child window");
    workspace.handle(&Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )));
    for width in [120, 95, 60] {
        terminal.backend_mut().resize(width, 36);
        workspace.handle(&Event::Resize(width, 36));
        settle_workspace_frame(workspace, &mut terminal);
        let frame: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        for marker in [
            "Controller: User",
            "Read-only files | No shell",
            "handoff · Controller: User",
            "Message Plexmaton",
            "to return",
        ] {
            assert!(frame.contains(marker), "{width}: missing {marker}: {frame}");
        }
        assert!(
            terminal.backend().cursor_visible(),
            "{width}: one child cursor"
        );
    }
    workspace.handle(&Event::Paste("answer only this child".into()));
    workspace
        .handle(&Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )))
        .submitted
        .expect("focused child submission")
}

async fn collect_child_request(
    server: FixtureServer,
    collaboration: &mut Collaboration,
    runtime: &mut LiveRuntime,
) -> Vec<Vec<u8>> {
    let fixture_join = tokio::task::spawn_blocking(move || server.join());
    tokio::pin!(fixture_join);
    loop {
        tokio::select! {
            result = &mut fixture_join => {
                return result
                    .expect("fixture join task")
                    .expect("fixture server thread")
                    .expect("fixture request");
            }
            activity = collaboration.next() => {
                let activity = activity.expect("child activity before fixture request");
                let staged = collaboration.stage(activity).expect("stage child activity");
                if let Some((_to, outcome)) = collaboration.take_user_input_settlement() {
                    assert_eq!(
                        outcome,
                        plexmaton_runtime::DispatchReport::default(),
                        "accepted child input settles successfully"
                    );
                    continue;
                }
                if staged {
                    collaboration
                        .drive_pending(runtime)
                        .await
                        .expect("project child activity");
                }
            }
        }
    }
}

async fn finish_child_response(
    collaboration: &mut Collaboration,
    runtime: &mut LiveRuntime,
    child: &AgentId,
) {
    for _ in 0..64 {
        let idle = std::iter::from_fn(|| runtime.try_next_event()).any(|envelope| {
            matches!(
                envelope.event,
                ConversationEvent::AgentStatusChanged {
                    ref agent_id,
                    status: AgentStatus::Idle,
                } if agent_id == child
            )
        });
        if idle {
            return;
        }
        let activity =
            tokio::time::timeout(std::time::Duration::from_secs(5), collaboration.next())
                .await
                .expect("child completion activity")
                .expect("owned child completion");
        if collaboration
            .stage(activity)
            .expect("stage child completion")
        {
            collaboration
                .drive_pending(runtime)
                .await
                .expect("project child completion");
        }
    }
    panic!("child response did not reach terminal Idle");
}

/// COM-4/CCV-2: focused child input fails closed before Handoff and reaches only the child after.
#[tokio::test]
async fn production_handoff_routes_child_input_and_retains_the_locked_draft() {
    let (base_url, server) = fixture_http_server([include_str!(
        "../../../plexmaton-provider/tests/fixtures/chat_final_answer.sse"
    )]);
    let fixture = FixtureWorkspace::new();
    let model = chat_fixture_model(&base_url);
    let (mut runtime, mut collaboration, model, tools) =
        bound_root_with_model(&fixture, conversation("input-root"), model).await;
    let key = plexmaton_provider::resolve_api_key(&model, Some("fixture-only".into()))
        .expect("fixture key");
    collaboration
        .owner
        .bind_child_factory(DelegatedChildFactory::new(
            DelegatedConversationDirectory::under(fixture.path()).expect("children"),
            model,
            key,
            tools,
        ))
        .expect("bind child factory");
    let root = collaboration.root.clone().expect("root endpoint");
    let worker = endpoint("input-child", "child-runtime");
    let delegation = DelegationId::new("input-task").expect("delegation");
    collaboration
        .owner
        .admit(CollaborationAttempt {
            id: CollaborationItemId::new("input-created").expect("item"),
            event: CollaborationEvent::DelegationCreated {
                delegation: delegation.clone(),
                delegator: root.clone(),
                worker: worker.clone(),
                task: CollaborationText::new("take direct input").expect("task"),
            },
        })
        .await
        .expect("create delegation");
    drop(
        collaboration
            .children
            .create(worker.conversation.clone(), UnixMillis::EPOCH)
            .expect("create child history"),
    );
    collaboration
        .sync_roster(&mut runtime, AgentStatus::Idle)
        .await
        .expect("sync Main controller");
    let child = agent("delegated-1");
    let locked = collaboration.dispatch_child_input(crate::input::AddressedInput {
        to: child.clone(),
        input: Input::Submitted {
            text: "keep this exact draft".into(),
        },
        skill: None,
    });
    assert!(locked.undelivered.iter().any(|input| {
        input.text == "keep this exact draft"
            && input.reason == plexmaton_agent::UndeliveredReason::ControlledByMain
    }));
    assert!(
        !runtime.has_active_work(),
        "root runtime receives no child input"
    );

    let handoff = collaboration
        .owner
        .handoff(CollaborationAttempt {
            id: CollaborationItemId::new("input-handoff").expect("item"),
            event: CollaborationEvent::HandoffCompleted {
                delegation,
                expected: DelegationRevision(0),
                author: root,
            },
        })
        .await
        .expect("durable Handoff");
    let handoff_reference = CollaborationItemRef {
        collaboration: collaboration.collaboration.clone(),
        item: handoff.receipt.id,
        sequence: handoff.receipt.sequence,
    };
    collaboration
        .sync_roster(&mut runtime, AgentStatus::Idle)
        .await
        .expect("sync User controller");
    let records = collaboration
        .owner
        .records()
        .await
        .expect("Handoff records");
    collaboration
        .replay_children(&mut runtime, &records)
        .await
        .expect("project passive Handoff history");
    let mut workspace = plexmaton_tui::Workspace::default();
    workspace.emit(std::iter::from_fn(|| runtime.try_next_event()).collect());
    collaboration
        .apply_child_controls(&mut workspace)
        .expect("apply User controller");
    let submitted = focused_child_submission(&mut workspace);
    assert_eq!(submitted.to, child);
    assert_eq!(submitted.text, "answer only this child");
    let report = collaboration.dispatch_child_input(crate::input::route_submission(submitted));
    assert!(report.undelivered.is_empty());
    assert!(!runtime.has_active_work(), "root remains idle");
    assert!(
        collaboration
            .owner
            .child_session_source(&worker.conversation)
            .await
            .expect("inspect queued child input")
            .is_none(),
        "terminal-path admission does not wait for cold activation"
    );
    let requests = collect_child_request(server, &mut collaboration, &mut runtime).await;
    assert_eq!(requests.len(), 1);
    let body: serde_json::Value = serde_json::from_slice(&requests[0]).expect("request JSON");
    assert!(body.to_string().contains("answer only this child"));
    let child_source = collaboration
        .owner
        .child_session_source(&worker.conversation)
        .await
        .expect("inspect child after input")
        .expect("active User child");
    assert!(
        child_source
            .journal()
            .collaboration_links(child_source.selected_head())
            .expect("child Handoff links")
            .iter()
            .any(|origin| origin.reference() == &handoff_reference),
        "User activation anchors Handoff before the child turn"
    );
    finish_child_response(&mut collaboration, &mut runtime, &child).await;

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

fn workspace_with_root_draft(
    runtime: &mut LiveRuntime,
) -> (plexmaton_tui::Workspace, Terminal<TestBackend>) {
    let mut workspace = plexmaton_tui::Workspace::default();
    workspace.emit(std::iter::from_fn(|| runtime.try_next_event()).collect());
    let mut terminal = Terminal::new(TestBackend::new(120, 36)).expect("terminal");
    settle_workspace_frame(&mut workspace, &mut terminal);
    for _ in 0..workspace.surfaces().len() {
        if workspace.state().focused(workspace.surfaces())
            == Some(plexmaton_tui::SurfaceId::Composer)
        {
            break;
        }
        workspace.handle(&Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
        workspace.draw(&mut terminal).expect("draw composer focus");
    }
    workspace.handle(&Event::Paste("keep the root draft".into()));
    (workspace, terminal)
}

fn assert_live_attention_frames(
    workspace: &mut plexmaton_tui::Workspace,
    terminal: &mut Terminal<TestBackend>,
    approval_id: &ApprovalId,
) {
    assert_eq!(workspace.state().attention_count(), 1);
    assert_eq!(workspace.state().attention_pending(), 1);
    assert_eq!(workspace.state().composer().text(), "keep the root draft");
    for width in [120, 95, 60] {
        terminal.backend_mut().resize(width, 36);
        workspace.handle(&Event::Resize(width, 36));
        settle_workspace_frame(workspace, terminal);
        let frame: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        if width < 72 {
            assert!(frame.contains("Agents ^B"), "{width}: {frame}");
            assert!(!frame.contains("Agents !1"), "{width}: {frame}");
            assert!(
                !frame.contains("approval"),
                "{width}: the collapsed navigator exposed a background row: {frame}"
            );
        } else {
            assert!(frame.contains("approval"), "{width}: {frame}");
        }
        assert!(frame.contains("( !1 )"), "{width}: {frame}");
        assert!(
            !frame.contains("Allow once"),
            "{width}: request stole focus"
        );
    }
    workspace.handle(&Event::Key(KeyEvent::new(
        KeyCode::Char('b'),
        KeyModifiers::CONTROL,
    )));
    settle_workspace_frame(workspace, terminal);
    assert_eq!(
        workspace.state().focused(workspace.surfaces()),
        Some(plexmaton_tui::SurfaceId::Agents)
    );
    workspace.handle(&Event::Key(KeyEvent::new(
        KeyCode::Down,
        KeyModifiers::NONE,
    )));
    workspace.handle(&Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )));
    assert_eq!(
        workspace
            .state()
            .approval()
            .expect("user navigation opens the request")
            .approval_id,
        approval_id
    );
    assert_eq!(workspace.state().attention_pending(), 0);
    assert_eq!(workspace.state().attention_count(), 1);
    for width in [120, 95, 60] {
        terminal.backend_mut().resize(width, 36);
        workspace.handle(&Event::Resize(width, 36));
        settle_workspace_frame(workspace, terminal);
        let frame: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        for marker in [
            "exec_command",
            "run a bounded command",
            "Allow once",
            "Deny",
        ] {
            assert!(frame.contains(marker), "{width}: missing {marker}: {frame}");
        }
    }
}

/// ATT-1/APV-4: a live child request is admitted before projection and issues one exact route.
#[tokio::test]
async fn live_child_attention_is_canonical_before_root_projection() {
    let fixture = FixtureWorkspace::new();
    let (mut runtime, mut collaboration, _, _) =
        bound_root(&fixture, conversation("attention-root")).await;
    let root = collaboration.root.clone().expect("root endpoint");
    let worker = endpoint("attention-child", "child-runtime");
    collaboration
        .owner
        .admit(CollaborationAttempt {
            id: CollaborationItemId::new("attention-created").expect("item"),
            event: CollaborationEvent::DelegationCreated {
                delegation: DelegationId::new("attention-task").expect("delegation"),
                delegator: root,
                worker: worker.clone(),
                task: CollaborationText::new("request a decision").expect("task"),
            },
        })
        .await
        .expect("create child");
    collaboration
        .sync_roster(&mut runtime, AgentStatus::Running)
        .await
        .expect("announce child");
    let (mut workspace, mut terminal) = workspace_with_root_draft(&mut runtime);
    let child = agent("delegated-1");
    let approval_id = ApprovalId::new("live-child-approval").expect("approval");
    let attention_id = AttentionId::new("live-child-attention").expect("Attention");
    let generation = RunnerGeneration::new(7).expect("generation");
    collaboration.pending_projection = Some(PendingRootProjection::RunnerEvent {
        child: worker.conversation.clone(),
        generation,
        event: Box::new(ConversationEvent::AttentionRequested {
            agent_id: child.clone(),
            attention_id: attention_id.clone(),
            request: AttentionRequest::Approval {
                approval_id: approval_id.clone(),
                call_id: ToolCallId::new("live-child-call").expect("call"),
                tool: "exec_command".to_owned(),
                capabilities: vec![ToolCapability::ProcessSpawn],
                detail: "run a bounded command".to_owned(),
                reason: plexmaton_core::ApprovalReason::CommandExecution,
                remember: None,
            },
        }),
        recovery: pending::RecoverySource::ChildJournal,
    });
    assert_eq!(
        collaboration
            .drive_pending(&mut runtime)
            .await
            .expect("admit and project Attention"),
        RootProjectionProgress::Applied
    );
    let records = collaboration
        .owner
        .records()
        .await
        .expect("canonical records");
    assert!(records.iter().any(|record| matches!(
        &record.event,
        CollaborationEvent::AttentionRequested { attention }
            if attention.producer == worker && attention.attention_id == attention_id
    )));
    workspace.emit(std::iter::from_fn(|| runtime.try_next_event()).collect());
    assert_live_attention_frames(&mut workspace, &mut terminal, &approval_id);
    assert_eq!(
        collaboration
            .live_approvals
            .get(&(child.clone(), approval_id.clone()))
            .map(|route| route.generation),
        Some(generation)
    );
    let (_, refusal) = collaboration
        .dispatch_child_approval(plexmaton_tui::ApprovalSubmission {
            to: child,
            approval_id,
            decision: ApprovalDecision::Deny,
        })
        .expect("known child route");
    assert!(matches!(
        refusal.unresolved_approvals.as_slice(),
        [plexmaton_agent::UnresolvedApprovalDecision {
            reason: plexmaton_agent::ApprovalDecisionRefusal::NotPending,
            ..
        }]
    ));
    assert!(
        collaboration
            .owner
            .child_session_source(&worker.conversation)
            .await
            .expect("inspect passive child")
            .is_none(),
        "a stale decision never cold-activates the child"
    );
    collaboration.pending_projection = Some(PendingRootProjection::RunnerEvent {
        child: worker.conversation.clone(),
        generation,
        event: Box::new(ConversationEvent::AttentionResolved {
            agent_id: agent("delegated-1"),
            attention_id: attention_id.clone(),
        }),
        recovery: pending::RecoverySource::ChildJournal,
    });
    collaboration
        .drive_pending(&mut runtime)
        .await
        .expect("admit and project resolution");
    assert!(collaboration.live_approvals.is_empty());
    assert!(
        collaboration
            .owner
            .records()
            .await
            .expect("records after resolution")
            .iter()
            .any(|record| matches!(
                &record.event,
                CollaborationEvent::AttentionResolved { attention }
                    if attention.producer == worker && attention.attention_id == attention_id
            ))
    );
    collaboration.shutdown().await.expect("shutdown owner");
    runtime.shutdown().await.expect("shutdown root");
}

/// ATT-1/APV-6: passive replay needs the matching collaboration reference and issues no route.
#[tokio::test]
async fn passive_attention_reopens_from_the_validated_prefix_without_waking() {
    let fixture = FixtureWorkspace::new();
    let (mut runtime, mut collaboration, _, _) =
        bound_root(&fixture, conversation("passive-attention-root")).await;
    let root = collaboration.root.clone().expect("root endpoint");
    let worker = endpoint("passive-attention-child", "child-runtime");
    collaboration
        .owner
        .admit(CollaborationAttempt {
            id: CollaborationItemId::new("passive-attention-created").expect("item"),
            event: CollaborationEvent::DelegationCreated {
                delegation: DelegationId::new("passive-attention-task").expect("delegation"),
                delegator: root,
                worker: worker.clone(),
                task: CollaborationText::new("retain the request").expect("task"),
            },
        })
        .await
        .expect("create child");
    let mut child_file = collaboration
        .children
        .create(worker.conversation.clone(), UnixMillis::EPOCH)
        .expect("create child journal");
    let (approval_id, attention_id) = seed_pending_approval(&mut child_file, &worker.agent);
    drop(child_file);
    collaboration
        .owner
        .admit(CollaborationAttempt {
            id: CollaborationItemId::new("passive-attention-request").expect("item"),
            event: CollaborationEvent::AttentionRequested {
                attention: AttentionReference {
                    producer: worker.clone(),
                    attention_id: attention_id.clone(),
                },
            },
        })
        .await
        .expect("publish child request reference");
    collaboration
        .sync_roster(&mut runtime, AgentStatus::Idle)
        .await
        .expect("announce passive child");
    let child_path = collaboration
        .children
        .path_for(&worker.conversation)
        .expect("child path");
    let collaboration_path = fixture
        .path()
        .join("collaborations")
        .join("passive-attention-root.jsonl");
    let child_before = fs::read(&child_path).expect("child bytes");
    let collaboration_before = fs::read(&collaboration_path).expect("collaboration bytes");
    let records = collaboration.owner.records().await.expect("records");
    collaboration
        .replay_children(&mut runtime, &records)
        .await
        .expect("passively replay child request");
    let mut workspace = plexmaton_tui::Workspace::default();
    workspace.emit(std::iter::from_fn(|| runtime.try_next_event()).collect());
    assert_eq!(workspace.state().attention_count(), 1);
    assert!(collaboration.live_approvals.is_empty());
    let child = agent("delegated-1");
    let (_, refusal) = collaboration
        .dispatch_child_approval(plexmaton_tui::ApprovalSubmission {
            to: child,
            approval_id,
            decision: ApprovalDecision::Deny,
        })
        .expect("passive child is selectable");
    assert!(matches!(
        refusal.unresolved_approvals.as_slice(),
        [plexmaton_agent::UnresolvedApprovalDecision {
            reason: plexmaton_agent::ApprovalDecisionRefusal::NotPending,
            ..
        }]
    ));
    assert!(
        collaboration
            .owner
            .child_session_source(&worker.conversation)
            .await
            .expect("inspect passive child")
            .is_none()
    );
    assert_eq!(fs::read(&child_path).expect("child bytes"), child_before);
    assert_eq!(
        fs::read(&collaboration_path).expect("collaboration bytes"),
        collaboration_before
    );
    collaboration.shutdown().await.expect("shutdown owner");
    runtime.shutdown().await.expect("shutdown root");
}

/// APV-6: a producer-journal request without its collaboration reference stays inert on reopen.
#[tokio::test]
async fn passive_orphan_attention_is_not_projected_or_activated() {
    let fixture = FixtureWorkspace::new();
    let (mut runtime, mut collaboration, _, _) =
        bound_root(&fixture, conversation("orphan-attention-root")).await;
    let root = collaboration.root.clone().expect("root endpoint");
    let worker = endpoint("orphan-attention-child", "child-runtime");
    collaboration
        .owner
        .admit(CollaborationAttempt {
            id: CollaborationItemId::new("orphan-attention-created").expect("item"),
            event: CollaborationEvent::DelegationCreated {
                delegation: DelegationId::new("orphan-attention-task").expect("delegation"),
                delegator: root,
                worker: worker.clone(),
                task: CollaborationText::new("leave an orphan request").expect("task"),
            },
        })
        .await
        .expect("create child");
    let mut child_file = collaboration
        .children
        .create(worker.conversation.clone(), UnixMillis::EPOCH)
        .expect("create child journal");
    let (_, attention_id) = seed_pending_approval(&mut child_file, &worker.agent);
    drop(child_file);
    collaboration
        .sync_roster(&mut runtime, AgentStatus::Idle)
        .await
        .expect("announce passive child");
    let records = collaboration.owner.records().await.expect("records");
    collaboration
        .replay_children(&mut runtime, &records)
        .await
        .expect("read orphaned child journal");
    let mut workspace = plexmaton_tui::Workspace::default();
    workspace.emit(std::iter::from_fn(|| runtime.try_next_event()).collect());
    assert_eq!(workspace.state().attention_count(), 0);
    assert!(collaboration.live_approvals.is_empty());
    assert!(
        collaboration
            .replayed_prefix
            .get(&worker.conversation)
            .is_some_and(|prefix| prefix.iter().any(|event| matches!(
                event,
                ConversationEvent::AttentionRequested {
                    attention_id: found,
                    ..
                } if found == &attention_id
            ))),
        "activation must consume the orphan event without publishing it"
    );
    assert!(
        collaboration
            .owner
            .child_session_source(&worker.conversation)
            .await
            .expect("inspect passive child")
            .is_none()
    );
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
