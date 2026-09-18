use std::{sync::Arc, time::Duration};

use plexmaton_agent::collaboration::{
    CollaborationEvent, CollaborationLimits, CollaborationText, DelegationController,
    DelegationRevision, MailEndpoint,
};
use plexmaton_agent::{
    AdmissionOutcome, AdmissionRequest, Agent, ConversationEntry, ConversationJournal, Effect,
    HeadRevision, Input, JournalEntryPayload, JournalRecord, JournalSequence, ModelEvent,
    ModelOutputPosition, StopReason, ToolCall, ToolOutcome, UnixMillis,
};
use plexmaton_core::{
    AgentId, AgentStatus, ArtifactId, CollaborationId, CollaborationItemId, ConversationEntryId,
    ConversationId, DelegationId, HeadName, JournalRecordId, ToolCallId, TranscriptItemId,
};
use plexmaton_provider::{ModelRegistry, resolve_api_key};
use plexmaton_session_store::collaboration::CollaborationFile;
use plexmaton_session_store::{ConversationDirectory, DelegatedConversationDirectory};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::*;
use crate::native::NativeCancellation;
use crate::runtime::FixedWallClock;
use crate::runtime::tests::{FakeDriver, Script};
use crate::{
    CollaborationWriter, DELEGATE_TOOL_NAME, DelegatedChildFactory, HANDOFF_TOOL_NAME, LiveRuntime,
    NativeToolCatalog, OwnedShutdownSettlement, SEND_MAIL_TOOL_NAME, SchedulerLimits,
    UPDATE_TASK_TOOL_NAME,
};

mod process_death;

const UNKNOWN_ARTIFACT: &str =
    "artifact-v1-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

struct Directory(std::path::PathBuf);

impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "plexmaton-collaboration-ingress-{}",
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir(&path).expect("create test directory");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                .expect("protect test directory");
        }
        Self(path)
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove test directory");
    }
}

fn endpoint(name: &str) -> MailEndpoint {
    MailEndpoint {
        agent: AgentId::new(name).expect("agent"),
        conversation: ConversationId::new(format!("conversation-{name}")).expect("conversation"),
    }
}

fn delegation() -> DelegationId {
    DelegationId::new("delegation").expect("delegation")
}

fn settled_outcome<E>(
    result: &Result<CollaborationIngressResult, E>,
) -> Option<&CollaborationIngressOutcome> {
    result
        .as_ref()
        .ok()
        .map(CollaborationIngressResult::outcome)
}

fn owner(directory: &Directory) -> OwnedCollaboration {
    let mut file = CollaborationFile::create(
        directory.0.join("collaboration.jsonl"),
        CollaborationId::new("collaboration").expect("collaboration"),
        CollaborationLimits::default(),
    )
    .expect("collaboration file");
    file.admit(
        CollaborationItemId::new("create").expect("item"),
        CollaborationEvent::DelegationCreated {
            delegation: delegation(),
            delegator: endpoint("main"),
            worker: endpoint("child"),
            task: CollaborationText::new("Inspect the parser").expect("task"),
        },
    )
    .expect("create delegation");
    OwnedCollaboration::new(
        CollaborationWriter::spawn(file).expect("writer"),
        SchedulerLimits::new(1).expect("limits"),
    )
}

fn empty_owner(directory: &Directory) -> OwnedCollaboration {
    let file = CollaborationFile::create(
        directory.0.join("collaboration.jsonl"),
        CollaborationId::new("collaboration").expect("collaboration"),
        CollaborationLimits::default(),
    )
    .expect("collaboration file");
    OwnedCollaboration::new(
        CollaborationWriter::spawn(file).expect("writer"),
        SchedulerLimits::new(1).expect("limits"),
    )
}

fn catalog(directory: &Directory) -> NativeToolCatalog {
    NativeToolCatalog::open(
        &directory.0,
        "TEST_KEY",
        "/bin/false",
        "/bin/false",
        Vec::new(),
    )
    .expect("catalog")
}

fn artifact_journal(endpoint: &MailEndpoint) -> ConversationJournal {
    let head = HeadName::new("main").expect("head");
    let announced = ConversationEntryId::new("announced").expect("entry");
    let artifact = ConversationEntryId::new("artifact-entry").expect("entry");
    let mut journal = ConversationJournal::new(endpoint.conversation.clone());
    journal
        .apply(JournalRecord::AppendEntry {
            sequence: JournalSequence::new(1),
            record_id: JournalRecordId::new("announce-record").expect("record"),
            head: head.clone(),
            expected_head_revision: HeadRevision::new(0),
            entry: Box::new(ConversationEntry {
                id: announced.clone(),
                parent_id: None,
                payload: JournalEntryPayload::AgentCreated {
                    agent_id: endpoint.agent.clone(),
                    label: "Main".into(),
                    status: AgentStatus::Idle,
                },
            }),
        })
        .expect("announce agent");
    journal
        .apply(JournalRecord::AppendEntry {
            sequence: JournalSequence::new(2),
            record_id: JournalRecordId::new("artifact-record").expect("record"),
            head,
            expected_head_revision: HeadRevision::new(1),
            entry: Box::new(ConversationEntry {
                id: artifact,
                parent_id: Some(announced),
                payload: JournalEntryPayload::ArtifactAnnounced {
                    agent_id: endpoint.agent.clone(),
                    item_id: TranscriptItemId::new("artifact-item").expect("item"),
                    artifact_id: ArtifactId::new("artifact-id").expect("artifact"),
                    label: "Findings".into(),
                    pointer: "artifact://main/findings".into(),
                },
            }),
        })
        .expect("announce artifact");
    journal
}

fn admission_request(call: &str, name: &str, arguments: serde_json::Value) -> AdmissionRequest {
    let mut agent = Agent::new(AgentId::new("main").expect("agent"));
    agent.handle_at(
        Input::Submitted {
            text: "coordinate the work".into(),
        },
        UnixMillis::EPOCH,
    );
    let step_id = agent.active_model_step().expect("active step");
    agent.handle_at(
        Input::Streamed {
            step_id: step_id.clone(),
            event: ModelEvent::Called {
                position: ModelOutputPosition::new(0, 0),
                call: ToolCall {
                    call_id: ToolCallId::new(call).expect("call"),
                    name: name.into(),
                    arguments: arguments.to_string(),
                },
            },
        },
        UnixMillis::EPOCH,
    );
    let stopped = agent.handle_at(
        Input::Streamed {
            step_id,
            event: ModelEvent::Stopped(StopReason::ToolCalls),
        },
        UnixMillis::EPOCH,
    );
    let effects: [Effect; 1] = stopped.effects.try_into().expect("one effect");
    let [Effect::AdmitTool(request)] = effects else {
        panic!("one admission request")
    };
    request
}

async fn admit(
    catalog: &NativeToolCatalog,
    call: &str,
    name: &str,
    arguments: serde_json::Value,
) -> plexmaton_agent::AdmittedToolCall {
    let outcome = catalog
        .admit(
            admission_request(call, name, arguments),
            NativeCancellation::new(),
            false,
        )
        .await;
    let AdmissionOutcome::Admitted(call) = outcome else {
        panic!("collaboration call admitted")
    };
    call
}

async fn execute_and_settle(
    catalog: &NativeToolCatalog,
    owner: &mut OwnedCollaboration,
    call: plexmaton_agent::AdmittedToolCall,
) -> (
    plexmaton_agent::ToolExecutionResult,
    CollaborationIngressSettlement,
) {
    let execution = catalog.execute(call, NativeCancellation::new());
    let settlement = owner.next_ingress();
    let (result, settlement) = tokio::join!(execution, settlement);
    (result, settlement.expect("ingress settlement"))
}

async fn shutdown(owner: &mut OwnedCollaboration) {
    owner.begin_shutdown().await.expect("begin shutdown");
    while owner.next_update().await.is_some() {}
    owner.finish_shutdown().await.expect("finish shutdown");
}

fn unsupported_model_and_key() -> (
    plexmaton_provider::ResolvedModel,
    plexmaton_provider::ApiKey,
) {
    let model = ModelRegistry::parse(
        r#"
active_model = { provider = "fixture", model = "child" }
[providers.fixture]
base_url = "http://127.0.0.1:9/v1"
api_key_env = "TEST_KEY"
api = "openai_responses"
[providers.fixture.models.child]
id = "fixture-child"
context_window_tokens = 8192
max_output_tokens = 1024
output_reserve_tokens = 512
"#,
    )
    .expect("model")
    .active_model()
    .clone();
    let key = resolve_api_key(&model, Some("fixture-only".into())).expect("key");
    (model, key)
}

async fn artifact_runtime(
    directory: &Directory,
    endpoint: &MailEndpoint,
    journal: &ConversationJournal,
    tools: NativeToolCatalog,
) -> LiveRuntime {
    let sessions = ConversationDirectory::under(&directory.0).expect("root session directory");
    let mut file = sessions
        .create(endpoint.conversation.clone(), UnixMillis::EPOCH)
        .expect("root session journal");
    for record in journal.records() {
        file.append(record.clone()).expect("seed artifact fact");
    }
    let (model, key) = unsupported_model_and_key();
    LiveRuntime::provider_with_resumed_journal(endpoint.agent.clone(), model, key, tools, file)
        .await
        .expect("artifact runtime")
        .0
}

async fn resumed_artifact_runtime(
    directory: &Directory,
    endpoint: &MailEndpoint,
    tools: NativeToolCatalog,
) -> LiveRuntime {
    let sessions = ConversationDirectory::under(&directory.0).expect("root session directory");
    let file = sessions
        .resume(&endpoint.conversation)
        .expect("resume root session journal");
    let (model, key) = unsupported_model_and_key();
    LiveRuntime::provider_with_resumed_journal(endpoint.agent.clone(), model, key, tools, file)
        .await
        .expect("resumed artifact runtime")
        .0
}

fn bind_main_runtime(owner: &mut OwnedCollaboration, runtime: &LiveRuntime) {
    owner
        .bind_main_runtime(
            runtime
                .main_collaboration_identity()
                .expect("runtime-issued Main identity"),
        )
        .expect("bind Main runtime");
}

/// CTL-1: Main identity is sealed to the exact ingress carried by its user-owned root runtime.
#[tokio::test]
async fn ctl_1_main_identity_is_issued_by_the_exact_root_catalog() {
    let directory = Directory::new();
    let mut owner = empty_owner(&directory);
    let ingress = owner.open_main_ingress().expect("open Main ingress");
    let tools = catalog(&directory)
        .with_main_collaboration(ingress)
        .expect("Main catalog");
    let mut runtime = LiveRuntime::with_root_driver_for_test(
        AgentId::new("main").expect("agent"),
        "Main".into(),
        FakeDriver::new(Vec::<Script>::new()),
        tools,
    )
    .expect("root runtime");

    let foreign_directory = Directory::new();
    let mut foreign = empty_owner(&foreign_directory);
    foreign.open_main_ingress().expect("foreign Main ingress");
    assert_eq!(
        foreign.bind_main_runtime(
            runtime
                .main_collaboration_identity()
                .expect("runtime-issued Main identity")
        ),
        Err(CollaborationIngressRefusal::CapabilityMismatch)
    );
    owner
        .bind_main_runtime(
            runtime
                .main_collaboration_identity()
                .expect("exact runtime-issued Main identity"),
        )
        .expect("bind exact Main runtime");
    assert_eq!(
        owner.bind_main_runtime(
            runtime
                .main_collaboration_identity()
                .expect("second runtime-issued identity")
        ),
        Err(CollaborationIngressRefusal::CapabilityMismatch)
    );

    runtime.shutdown().await.expect("shutdown root runtime");
    shutdown(&mut owner).await;
    shutdown(&mut foreign).await;
}

/// CTL-1/CTL-2: authenticated roles derive endpoints and current revisions inside the owner.
#[tokio::test]
async fn ctl_1_ingress_derives_mail_endpoints_and_current_task_revision() {
    let directory = Directory::new();
    let mut owner = owner(&directory);
    let main_ingress = owner
        .bind_main_ingress(endpoint("main"))
        .expect("bind Main");
    let target = owner
        .register_collaboration_target(delegation())
        .await
        .expect("register target");
    assert!(target.selector().as_str().starts_with("target-v1-"));
    assert!(!target.selector().as_str().contains(delegation().as_str()));

    let main = catalog(&directory)
        .with_main_collaboration(main_ingress)
        .expect("Main catalog");
    assert_eq!(main.provider_definitions().len(), 9);
    let child = catalog(&directory)
        .with_child_collaboration(target.child_ingress())
        .expect("child catalog")
        .into_read_only();
    assert_eq!(child.provider_definitions().len(), 3);

    let main_mail = admit(
        &main,
        "main-mail",
        SEND_MAIL_TOOL_NAME,
        json!({
            "target": target.selector().as_str(),
            "summary": "Check the cancellation path",
            "artifacts": [],
        }),
    )
    .await;
    let (result, settlement) = execute_and_settle(&main, &mut owner, main_mail).await;
    assert!(matches!(
        result.outcome(),
        ToolOutcome::Succeeded { output } if output.contains("mail_accepted")
    ));
    assert!(matches!(
        settled_outcome(settlement.result()),
        Some(CollaborationIngressOutcome::MailAccepted)
    ));
    let accepted = settlement.result().as_ref().expect("accepted mail result");
    assert_eq!(
        result.collaboration_reference(),
        Some(accepted.reference()),
        "the sender receives only the canonical placement reference"
    );

    let child_mail = admit(
        &child,
        "child-mail",
        SEND_MAIL_TOOL_NAME,
        json!({"summary": "The path is safe", "artifacts": []}),
    )
    .await;
    let (_, settlement) = execute_and_settle(&child, &mut owner, child_mail).await;
    assert!(matches!(
        settled_outcome(settlement.result()),
        Some(CollaborationIngressOutcome::MailAccepted)
    ));

    let update = admit(
        &main,
        "update",
        UPDATE_TASK_TOOL_NAME,
        json!({
            "target": target.selector().as_str(),
            "task": "Inspect the exact retry path",
        }),
    )
    .await;
    let (_, settlement) = execute_and_settle(&main, &mut owner, update).await;
    assert!(matches!(
        settled_outcome(settlement.result()),
        Some(CollaborationIngressOutcome::TaskUpdated)
    ));
    let view = owner
        .delegation_view(delegation())
        .await
        .expect("current view");
    assert_eq!(view.revision, DelegationRevision(1));
    assert_eq!(view.task.as_str(), "Inspect the exact retry path");

    let main_mail = owner
        .mail_snapshot(endpoint("main"))
        .await
        .expect("Main mail");
    assert_eq!(main_mail.items().len(), 2);
    assert_eq!(main_mail.items()[0].envelope().from, endpoint("main"));
    assert_eq!(main_mail.items()[0].envelope().to, endpoint("child"));
    assert_eq!(main_mail.items()[1].envelope().from, endpoint("child"));
    assert_eq!(main_mail.items()[1].envelope().to, endpoint("main"));
    shutdown(&mut owner).await;
}

/// CTL-1: unknown selectors and unproven artifacts fail before any durable append.
#[tokio::test]
async fn ctl_1_ingress_refuses_unknown_targets_and_unbound_artifacts_without_mutation() {
    let directory = Directory::new();
    let path = directory.0.join("collaboration.jsonl");
    let mut owner = owner(&directory);
    let main_ingress = owner
        .bind_main_ingress(endpoint("main"))
        .expect("bind Main");
    let target = owner
        .register_collaboration_target(delegation())
        .await
        .expect("register target");
    let main = catalog(&directory)
        .with_main_collaboration(main_ingress)
        .expect("Main catalog");
    let unchanged = std::fs::read(&path).expect("initial bytes");

    for (call, arguments, expected) in [
        (
            "unknown",
            json!({
                "target": "target-v1-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "summary": "mail",
                "artifacts": [],
            }),
            CollaborationIngressRefusal::UnknownTarget,
        ),
        (
            "artifact",
            json!({
                "target": target.selector().as_str(),
                "summary": "mail",
                "artifacts": [UNKNOWN_ARTIFACT],
            }),
            CollaborationIngressRefusal::UnknownArtifact,
        ),
    ] {
        let mail = admit(&main, call, SEND_MAIL_TOOL_NAME, arguments).await;
        let (result, settlement) = execute_and_settle(&main, &mut owner, mail).await;
        assert!(matches!(result.outcome(), ToolOutcome::Failed { .. }));
        assert!(matches!(
            settlement.result(),
            Err(CollaborationIngressFailure::Refused(found)) if found == &expected
        ));
        assert_eq!(std::fs::read(&path).expect("unchanged bytes"), unchanged);
    }
    shutdown(&mut owner).await;
}

/// CTL-1: artifact selectors resolve only from immutable facts in the sender's journal.
#[tokio::test]
async fn ctl_1_artifact_selector_requires_an_authenticated_sender_journal_fact() {
    let directory = Directory::new();
    let mut owner = owner(&directory);
    let main_endpoint = endpoint("main");
    let main_ingress = owner.open_main_ingress().expect("open Main ingress");
    let journal = artifact_journal(&main_endpoint);
    let foreign_directory = Directory::new();
    let mut foreign_owner = empty_owner(&foreign_directory);
    let foreign_ingress = foreign_owner
        .open_main_ingress()
        .expect("open foreign Main ingress");
    let foreign_main = catalog(&foreign_directory)
        .with_main_collaboration(foreign_ingress)
        .expect("foreign Main catalog");
    let mut foreign_runtime =
        artifact_runtime(&foreign_directory, &main_endpoint, &journal, foreign_main).await;
    bind_main_runtime(&mut foreign_owner, &foreign_runtime);
    let foreign_source = foreign_runtime
        .collaboration_artifact_source()
        .expect("project foreign artifacts")
        .expect("foreign runtime carries its own authority");
    assert!(matches!(
        owner.register_collaboration_artifacts(foreign_source),
        Err(CollaborationArtifactRegistrationError::EndpointMismatch)
    ));
    assert!(
        owner
            .ingress
            .as_ref()
            .expect("ingress")
            .artifacts
            .is_empty()
    );
    foreign_runtime
        .shutdown()
        .await
        .expect("shutdown foreign runtime");
    shutdown(&mut foreign_owner).await;

    let main = catalog(&directory)
        .with_main_collaboration(main_ingress)
        .expect("Main catalog");
    let mut runtime = artifact_runtime(&directory, &main_endpoint, &journal, main.clone()).await;
    bind_main_runtime(&mut owner, &runtime);
    let target = owner
        .register_collaboration_target(delegation())
        .await
        .expect("register target");
    let source = runtime
        .collaboration_artifact_source()
        .expect("project artifacts")
        .expect("runtime carries the Main collaboration authority");
    let artifacts = owner
        .register_collaboration_artifacts(source)
        .expect("register artifact");
    assert_eq!(artifacts.len(), 1);
    assert!(artifacts[0].as_str().starts_with("artifact-v1-"));
    assert!(!artifacts[0].as_str().contains("artifact-id"));
    let child = catalog(&directory)
        .with_child_collaboration(target.child_ingress())
        .expect("child catalog");
    let mail = admit(
        &main,
        "artifact-mail",
        SEND_MAIL_TOOL_NAME,
        json!({
            "target": target.selector().as_str(),
            "summary": "See the findings",
            "artifacts": [artifacts[0].as_str()],
        }),
    )
    .await;
    let (_, settlement) = execute_and_settle(&main, &mut owner, mail).await;
    assert!(matches!(
        settled_outcome(settlement.result()),
        Some(CollaborationIngressOutcome::MailAccepted)
    ));
    let child_mail = owner
        .mail_snapshot(endpoint("child"))
        .await
        .expect("child mail");
    assert_eq!(
        child_mail.items()[0].envelope().artifacts,
        vec![ArtifactReference {
            conversation: main_endpoint.conversation.clone(),
            artifact: ArtifactId::new("artifact-id").expect("artifact"),
        }]
    );
    let foreign = admit(
        &child,
        "foreign-artifact",
        SEND_MAIL_TOOL_NAME,
        json!({
            "summary": "Try a foreign artifact",
            "artifacts": [artifacts[0].as_str()],
        }),
    )
    .await;
    let (_, settlement) = execute_and_settle(&child, &mut owner, foreign).await;
    assert!(matches!(
        settlement.result(),
        Err(CollaborationIngressFailure::Refused(
            CollaborationIngressRefusal::UnknownArtifact
        ))
    ));
    let expected_selector = artifacts[0].as_str().to_owned();
    runtime.shutdown().await.expect("shutdown artifact runtime");
    shutdown(&mut owner).await;

    let writer = CollaborationWriter::spawn(
        CollaborationFile::open(directory.0.join("collaboration.jsonl"))
            .expect("reopen collaboration"),
    )
    .expect("reopened writer");
    let mut resumed = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    let main_ingress = resumed
        .open_main_ingress()
        .expect("open resumed Main ingress");
    let main = catalog(&directory)
        .with_main_collaboration(main_ingress)
        .expect("resumed Main catalog");
    let mut runtime = resumed_artifact_runtime(&directory, &main_endpoint, main).await;
    bind_main_runtime(&mut resumed, &runtime);
    resumed
        .register_collaboration_targets()
        .await
        .expect("restore target");
    let source = runtime
        .collaboration_artifact_source()
        .expect("reproject artifacts")
        .expect("resumed runtime carries the Main collaboration authority");
    let restored = resumed
        .register_collaboration_artifacts(source)
        .expect("restore artifact");
    assert_eq!(restored[0].as_str(), expected_selector);
    runtime.shutdown().await.expect("shutdown artifact runtime");
    shutdown(&mut resumed).await;
}

/// CTL-1: an ambiguous durable artifact identity cannot produce a collaboration selector.
#[tokio::test]
async fn ctl_1_ambiguous_artifact_identity_refuses_before_collaboration_admission() {
    let directory = Directory::new();
    let mut owner = owner(&directory);
    let main_endpoint = endpoint("main");
    let main_ingress = owner.open_main_ingress().expect("open Main ingress");
    let mut journal = artifact_journal(&main_endpoint);
    let head = HeadName::new("main").expect("head");
    for suffix in ["first", "duplicate"] {
        let parent = journal.head_target(&head).expect("head target").cloned();
        journal
            .apply(JournalRecord::AppendEntry {
                sequence: journal.next_sequence(),
                record_id: JournalRecordId::new(format!("ambiguous-{suffix}-record"))
                    .expect("record"),
                head: head.clone(),
                expected_head_revision: journal.head_revision(&head).expect("revision"),
                entry: Box::new(ConversationEntry {
                    id: ConversationEntryId::new(format!("ambiguous-{suffix}-entry"))
                        .expect("entry"),
                    parent_id: parent,
                    payload: JournalEntryPayload::ArtifactAnnounced {
                        agent_id: main_endpoint.agent.clone(),
                        item_id: TranscriptItemId::new(format!("ambiguous-{suffix}-item"))
                            .expect("item"),
                        artifact_id: ArtifactId::new("ambiguous-artifact").expect("artifact"),
                        label: format!("Ambiguous {suffix}"),
                        pointer: format!("artifact://main/{suffix}"),
                    },
                }),
            })
            .expect("append ambiguous artifact fact");
    }
    let main = catalog(&directory)
        .with_main_collaboration(main_ingress)
        .expect("Main catalog");
    let mut runtime = artifact_runtime(&directory, &main_endpoint, &journal, main).await;
    bind_main_runtime(&mut owner, &runtime);
    owner
        .register_collaboration_target(delegation())
        .await
        .expect("register target");
    let source = runtime
        .collaboration_artifact_source()
        .expect("project ambiguous artifacts")
        .expect("runtime carries the Main collaboration authority");
    assert!(matches!(
        owner.register_collaboration_artifacts(source),
        Err(CollaborationArtifactRegistrationError::AmbiguousArtifact(id))
            if id.as_str() == "ambiguous-artifact"
    ));
    assert!(
        owner
            .ingress
            .as_ref()
            .expect("ingress")
            .artifacts
            .is_empty()
    );
    assert!(
        owner
            .mail_snapshot(endpoint("child"))
            .await
            .expect("unchanged mail")
            .items()
            .is_empty()
    );
    runtime.shutdown().await.expect("shutdown artifact runtime");
    shutdown(&mut owner).await;
}

/// CTL-1/SCH-4: an accepted command survives loss of its tool reply and remains observable.
#[tokio::test]
async fn ctl_1_cancelled_tool_wait_does_not_cancel_an_accepted_ingress_mutation() {
    let directory = Directory::new();
    let mut owner = owner(&directory);
    let main_ingress = owner
        .bind_main_ingress(endpoint("main"))
        .expect("bind Main");
    let target = owner
        .register_collaboration_target(delegation())
        .await
        .expect("register target");
    let main = catalog(&directory)
        .with_main_collaboration(main_ingress)
        .expect("Main catalog");
    let update = admit(
        &main,
        "cancelled-update",
        UPDATE_TASK_TOOL_NAME,
        json!({
            "target": target.selector().as_str(),
            "task": "Retain this exact mutation",
        }),
    )
    .await;
    let execution = main.execute(update, NativeCancellation::new());
    let task = tokio::spawn(execution);
    for _ in 0..100 {
        if owner.ingress.as_ref().expect("ingress").receiver.len() == 1 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(
        owner.ingress.as_ref().expect("ingress").receiver.len(),
        1,
        "tool command entered the bounded lane"
    );
    task.abort();
    let _ = task.await;

    let settlement = owner
        .next_ingress()
        .await
        .expect("accepted command settles");
    assert_eq!(settlement.call_id().as_str(), "cancelled-update");
    assert!(!settlement.reply_delivered());
    assert_eq!(settlement.caller(), Some(&endpoint("main")));
    settlement
        .result()
        .as_ref()
        .expect("settled update")
        .reference()
        .validate()
        .expect("canonical update reference");
    assert!(matches!(
        settled_outcome(settlement.result()),
        Some(CollaborationIngressOutcome::TaskUpdated)
    ));
    assert_eq!(
        owner
            .delegation_view(delegation())
            .await
            .expect("updated view")
            .task
            .as_str(),
        "Retain this exact mutation"
    );
    shutdown(&mut owner).await;
}

/// CTL-1/SCH-1: cancelling a child tool wait releases the runtime while its accepted mail settles.
#[tokio::test]
async fn ctl_1_child_mail_cancellation_releases_the_caller_and_retains_the_mutation() {
    let directory = Directory::new();
    let mut owner = owner(&directory);
    owner
        .bind_main_ingress(endpoint("main"))
        .expect("bind Main");
    let child = owner
        .register_collaboration_target(delegation())
        .await
        .expect("register target")
        .child_ingress();
    let cancellation = CancellationToken::new();
    let caller_cancellation = cancellation.clone();
    let execution = tokio::spawn(async move {
        child
            .execute(
                ToolCallId::new("cancelled-child-mail").expect("call"),
                CollaborationToolRequest::ChildMail(ChildMailIntent {
                    summary: CollaborationText::new("Retain this accepted mail").expect("summary"),
                    artifacts: Vec::new(),
                }),
                caller_cancellation,
            )
            .await
    });
    for _ in 0..100 {
        if owner.ingress.as_ref().expect("ingress").receiver.len() == 1 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(
        owner.ingress.as_ref().expect("ingress").receiver.len(),
        1,
        "child mail entered the bounded owner lane"
    );

    cancellation.cancel();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), execution)
            .await
            .expect("cancelled child caller is released")
            .expect("child execution task"),
        Err(CollaborationIngressRefusal::Cancelled)
    );
    let settlement = owner
        .next_ingress()
        .await
        .expect("accepted child mail settles");
    assert!(!settlement.reply_delivered());
    assert_eq!(settlement.caller(), Some(&endpoint("child")));
    settlement
        .result()
        .as_ref()
        .expect("settled mail")
        .reference()
        .validate()
        .expect("canonical mail reference");
    assert!(matches!(
        settled_outcome(settlement.result()),
        Some(CollaborationIngressOutcome::MailAccepted)
    ));
    let main_mail = owner
        .mail_snapshot(endpoint("main"))
        .await
        .expect("Main mail");
    assert_eq!(main_mail.items().len(), 1);
    assert_eq!(main_mail.items()[0].envelope().from, endpoint("child"));
    shutdown(&mut owner).await;
}

/// CTL-1/SCH-2: Stop cancels a child waiting on accepted mail without requiring ingress progress.
#[tokio::test]
async fn ctl_1_child_stop_does_not_deadlock_behind_its_accepted_mail() {
    let directory = Directory::new();
    let mut owner = empty_owner(&directory);
    let main_ingress = owner
        .bind_main_ingress(endpoint("main"))
        .expect("bind Main");
    let children =
        DelegatedConversationDirectory::under(&directory.0).expect("delegated directory");
    let driver = FakeDriver::new([Script::Events(vec![
        ModelEvent::Called {
            position: ModelOutputPosition::new(0, 0),
            call: ToolCall {
                call_id: ToolCallId::new("child-mail").expect("call"),
                name: SEND_MAIL_TOOL_NAME.into(),
                arguments: json!({
                    "summary": "Retain mail while Stop cancels the tool wait",
                    "artifacts": [],
                })
                .to_string(),
            },
        },
        ModelEvent::Stopped(StopReason::ToolCalls),
    ])]);
    owner
        .bind_child_factory(DelegatedChildFactory::synthetic(
            children,
            catalog(&directory),
            driver,
            Arc::new(FixedWallClock(UnixMillis::EPOCH)),
        ))
        .expect("bind factory");
    let main = catalog(&directory)
        .with_main_collaboration(main_ingress)
        .expect("Main catalog");
    let delegate = admit(
        &main,
        "delegate-for-stop",
        DELEGATE_TOOL_NAME,
        json!({"task": "Send one typed mail"}),
    )
    .await;
    let (_, settlement) = execute_and_settle(&main, &mut owner, delegate).await;
    assert!(matches!(
        settled_outcome(settlement.result()),
        Some(CollaborationIngressOutcome::Delegated { .. })
    ));
    let target = owner
        .register_collaboration_targets()
        .await
        .expect("canonical target")
        .pop()
        .expect("one target");
    let child = target.child_ingress().worker().conversation.clone();

    tokio::time::timeout(Duration::from_secs(5), async {
        while owner.ingress.as_ref().expect("ingress").receiver.is_empty() {
            let _progress =
                tokio::time::timeout(Duration::from_millis(10), owner.next_update()).await;
        }
    })
    .await
    .expect("child mail enters ingress");
    tokio::time::timeout(Duration::from_secs(5), owner.stop(&child))
        .await
        .expect("Stop is not blocked by the child tool wait")
        .expect("stop child");

    let settlement = owner
        .next_ingress()
        .await
        .expect("accepted child mail settles after Stop");
    assert!(!settlement.reply_delivered());
    assert!(matches!(
        settled_outcome(settlement.result()),
        Some(CollaborationIngressOutcome::MailAccepted)
    ));
    assert_eq!(
        owner
            .mail_snapshot(endpoint("main"))
            .await
            .expect("Main mail")
            .items()
            .len(),
        1
    );
    shutdown(&mut owner).await;
}

/// CTL-1/SCH-5: new Main mail retains a fresh wake for an already engaged child.
#[tokio::test]
async fn ctl_1_main_mail_wakes_an_engaged_child_for_a_fresh_second_turn() {
    let directory = Directory::new();
    let mut owner = empty_owner(&directory);
    let main_ingress = owner
        .bind_main_ingress(endpoint("main"))
        .expect("bind Main");
    let children =
        DelegatedConversationDirectory::under(&directory.0).expect("delegated directory");
    let driver = FakeDriver::new([
        Script::Events(vec![ModelEvent::Stopped(StopReason::EndOfTurn)]),
        Script::Events(vec![ModelEvent::Stopped(StopReason::EndOfTurn)]),
    ]);
    let child_driver: Arc<dyn crate::runtime::ModelDriver> = driver.clone();
    owner
        .bind_child_factory(DelegatedChildFactory::synthetic(
            children,
            catalog(&directory),
            child_driver,
            Arc::new(FixedWallClock(UnixMillis::EPOCH)),
        ))
        .expect("bind factory");
    let main = catalog(&directory)
        .with_main_collaboration(main_ingress)
        .expect("Main catalog");
    let delegate = admit(
        &main,
        "delegate-for-second-turn",
        DELEGATE_TOOL_NAME,
        json!({"task": "Inspect two rounds"}),
    )
    .await;
    let (_, settlement) = execute_and_settle(&main, &mut owner, delegate).await;
    let target = match settled_outcome(settlement.result()) {
        Some(CollaborationIngressOutcome::Delegated { target }) => target.clone(),
        outcome => panic!("delegation did not settle: {outcome:?}"),
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        while driver.calls().await.is_empty() {
            owner
                .next_update()
                .await
                .expect("first child turn progress");
        }
    })
    .await
    .expect("initial delegation wake dispatches");

    let mail = admit(
        &main,
        "second-round-mail",
        SEND_MAIL_TOOL_NAME,
        json!({
            "target": target.as_str(),
            "summary": "Inspect the follow-up evidence",
            "artifacts": [],
        }),
    )
    .await;
    let (_, settlement) = execute_and_settle(&main, &mut owner, mail).await;
    assert!(matches!(
        settled_outcome(settlement.result()),
        Some(CollaborationIngressOutcome::MailAccepted)
    ));
    tokio::time::timeout(Duration::from_secs(5), async {
        while driver.calls().await.len() < 2 {
            owner
                .next_update()
                .await
                .expect("retained second-turn wake");
        }
    })
    .await
    .expect("Main mail dispatches one fresh child turn");
    assert_eq!(driver.calls().await.len(), 2);
    shutdown(&mut owner).await;
}

/// CTL-2/COL-3: cross-role execution is rejected before an owner command or Handoff append.
#[tokio::test]
async fn ctl_2_catalog_rechecks_role_and_handoff_uses_owner_derived_revision() {
    let directory = Directory::new();
    let mut owner = owner(&directory);
    let main_ingress = owner
        .bind_main_ingress(endpoint("main"))
        .expect("bind Main");
    let target = owner
        .register_collaboration_target(delegation())
        .await
        .expect("register target");
    let main = catalog(&directory)
        .with_main_collaboration(main_ingress)
        .expect("Main catalog");
    let child = catalog(&directory)
        .with_child_collaboration(target.child_ingress())
        .expect("child catalog");

    let main_mail = admit(
        &main,
        "cross-role",
        SEND_MAIL_TOOL_NAME,
        json!({
            "target": target.selector().as_str(),
            "summary": "forged",
            "artifacts": [],
        }),
    )
    .await;
    let result = child.execute(main_mail, NativeCancellation::new()).await;
    assert!(matches!(result.outcome(), ToolOutcome::Failed { .. }));
    assert!(
        tokio::time::timeout(Duration::ZERO, owner.next_ingress())
            .await
            .is_err(),
        "role mismatch sends no owner command"
    );

    let handoff = admit(
        &main,
        "handoff",
        HANDOFF_TOOL_NAME,
        json!({"target": target.selector().as_str()}),
    )
    .await;
    let (_, settlement) = execute_and_settle(&main, &mut owner, handoff).await;
    assert!(matches!(
        settled_outcome(settlement.result()),
        Some(CollaborationIngressOutcome::HandoffCompleted)
    ));
    assert_eq!(
        owner
            .delegation_view(delegation())
            .await
            .expect("handoff view")
            .controller,
        DelegationController::User
    );

    for (catalog, call, arguments) in [
        (
            &main,
            "main-after-handoff",
            json!({
                "target": target.selector().as_str(),
                "summary": "Main remains able to send typed mail",
                "artifacts": [],
            }),
        ),
        (
            &child,
            "child-after-handoff",
            json!({
                "summary": "Child mail capability is unchanged",
                "artifacts": [],
            }),
        ),
    ] {
        let mail = admit(catalog, call, SEND_MAIL_TOOL_NAME, arguments).await;
        let (_, settlement) = execute_and_settle(catalog, &mut owner, mail).await;
        assert!(matches!(
            settled_outcome(settlement.result()),
            Some(CollaborationIngressOutcome::MailAccepted)
        ));
    }

    owner.begin_shutdown().await.expect("begin shutdown");
    let report = owner.finish_shutdown().await.expect("finish shutdown");
    assert!(
        !report
            .settlements()
            .iter()
            .any(|settlement| matches!(settlement, OwnedShutdownSettlement::Ingress(_)))
    );
    assert!(CollaborationFile::open(path_for(&directory)).is_ok());
}

/// CCV-1/CCV-2: authenticated Handoff reports pending before its durable User snapshot.
#[tokio::test]
async fn ccv_1_handoff_activity_projects_pending_before_acknowledgement() {
    let directory = Directory::new();
    let mut owner = owner(&directory);
    let main_ingress = owner
        .bind_main_ingress(endpoint("main"))
        .expect("bind Main");
    let target = owner
        .register_collaboration_target(delegation())
        .await
        .expect("register target");
    let user_target = target.user_input_target();
    let initial = owner
        .child_control_snapshot(&user_target)
        .await
        .expect("initial Main snapshot");
    assert_eq!(initial.revision(), 0);
    assert_eq!(initial.control(), OwnedChildControl::Main);
    let main = catalog(&directory)
        .with_main_collaboration(main_ingress)
        .expect("Main catalog");
    let call = admit(
        &main,
        "visible-handoff",
        HANDOFF_TOOL_NAME,
        json!({"target": target.selector().as_str()}),
    )
    .await;
    let execution = main.execute(call, NativeCancellation::new());
    tokio::pin!(execution);

    let pending = {
        let pending_activity = owner.next_activity();
        tokio::pin!(pending_activity);
        tokio::select! {
            activity = &mut pending_activity => activity.expect("pending control activity"),
            result = &mut execution => panic!("Handoff acknowledged before pending control: {result:?}"),
        }
    };
    assert!(matches!(
        pending,
        OwnedCollaborationActivity::Control(snapshot)
            if snapshot.worker() == target.worker()
                && snapshot.revision() == 1
                && snapshot.control() == OwnedChildControl::HandoffPending
    ));
    assert!(
        tokio::time::timeout(Duration::ZERO, &mut execution)
            .await
            .is_err(),
        "the tool remains unacknowledged while pending is visible"
    );
    let (result, settled) = tokio::join!(execution, owner.next_activity());
    assert!(matches!(result.outcome(), ToolOutcome::Succeeded { .. }));
    assert!(matches!(
        settled,
        Some(OwnedCollaborationActivity::Ingress(settlement))
            if matches!(
                settled_outcome(settlement.result()),
                Some(CollaborationIngressOutcome::HandoffCompleted)
            )
    ));
    let user = owner
        .child_control_snapshot(&user_target)
        .await
        .expect("durable User snapshot");
    assert_eq!(user.revision(), 2);
    assert_eq!(user.control(), OwnedChildControl::User);
    shutdown(&mut owner).await;
}

/// CCV-1/CCV-4: a refused Handoff can return to Main and retry without a stale revision.
#[tokio::test]
async fn ccv_1_failed_handoff_projection_rolls_forward_to_main_before_retry() {
    let directory = Directory::new();
    let mut owner = owner(&directory);
    owner
        .bind_main_ingress(endpoint("main"))
        .expect("bind Main");
    let target = owner
        .register_collaboration_target(delegation())
        .await
        .expect("register target");
    let user_target = target.user_input_target();
    let initial = owner
        .child_control_snapshot(&user_target)
        .await
        .expect("initial Main snapshot");
    let attempt = CollaborationAttempt {
        id: CollaborationItemId::new("retry-handoff").expect("item"),
        event: CollaborationEvent::HandoffCompleted {
            delegation: delegation(),
            expected: DelegationRevision(0),
            author: endpoint("main"),
        },
    };

    let pending = owner
        .pending_handoff_snapshot(&attempt)
        .expect("pending snapshot");
    let repeated = owner
        .pending_handoff_snapshot(&attempt)
        .expect("repeated pending snapshot");
    let rolled_back = owner
        .child_control_snapshot(&user_target)
        .await
        .expect("canonical Main snapshot");
    let retry = owner
        .pending_handoff_snapshot(&attempt)
        .expect("retry pending snapshot");

    assert_eq!(
        (initial.revision(), initial.control()),
        (0, OwnedChildControl::Main)
    );
    assert_eq!(
        (pending.revision(), pending.control()),
        (1, OwnedChildControl::HandoffPending)
    );
    assert_eq!(repeated, pending, "repeated state is a no-op");
    assert_eq!(
        (rolled_back.revision(), rolled_back.control()),
        (2, OwnedChildControl::Main)
    );
    assert_eq!(
        (retry.revision(), retry.control()),
        (3, OwnedChildControl::HandoffPending)
    );
    shutdown(&mut owner).await;
}

/// CTL-1/SCH-4: ingress capacity is hard and shutdown settles every accepted command.
#[tokio::test]
async fn ctl_1_ingress_lane_is_bounded_and_shutdown_retains_all_settlements() {
    let directory = Directory::new();
    let path = directory.0.join("collaboration.jsonl");
    let mut owner = owner(&directory);
    let main = owner
        .bind_main_ingress(endpoint("main"))
        .expect("bind Main");
    let target = owner
        .register_collaboration_target(delegation())
        .await
        .expect("register target");
    let unchanged = std::fs::read(&path).expect("initial bytes");
    let mut tasks = Vec::new();
    for index in 0..INGRESS_CAPACITY {
        let ingress = main.clone();
        let selector = target.selector().clone();
        tasks.push(tokio::spawn(async move {
            ingress
                .execute(
                    ToolCallId::new(format!("queued-{index}")).expect("call"),
                    CollaborationToolRequest::UpdateTask(UpdateTaskIntent {
                        target: selector,
                        task: CollaborationText::new(format!("task {index}")).expect("task"),
                    }),
                    CancellationToken::new(),
                )
                .await
        }));
    }
    for _ in 0..100 {
        if owner.ingress.as_ref().expect("ingress").receiver.len() == INGRESS_CAPACITY {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(
        owner.ingress.as_ref().expect("ingress").receiver.len(),
        INGRESS_CAPACITY
    );
    assert_eq!(std::fs::read(&path).expect("unchanged bytes"), unchanged);
    assert_eq!(
        main.execute(
            ToolCallId::new("overflow").expect("call"),
            CollaborationToolRequest::UpdateTask(UpdateTaskIntent {
                target: target.selector().clone(),
                task: CollaborationText::new("overflow").expect("task"),
            }),
            CancellationToken::new(),
        )
        .await,
        Err(CollaborationIngressRefusal::Busy)
    );

    owner.begin_shutdown().await.expect("begin shutdown");
    let report = owner.finish_shutdown().await.expect("finish shutdown");
    assert_eq!(
        report
            .settlements()
            .iter()
            .filter(|settlement| matches!(settlement, OwnedShutdownSettlement::Ingress(_)))
            .count(),
        INGRESS_CAPACITY
    );
    for task in tasks {
        let result = task.await.expect("tool task");
        assert!(matches!(
            settled_outcome(&result),
            Some(CollaborationIngressOutcome::TaskUpdated)
        ));
    }
}

/// CTL-1: target selectors rebuild identically from canonical creation provenance after reopen.
#[tokio::test]
async fn ctl_1_restart_rebuilds_targets_without_exposing_durable_identities() {
    let directory = Directory::new();
    let path = directory.0.join("collaboration.jsonl");
    let mut first = owner(&directory);
    first
        .bind_main_ingress(endpoint("main"))
        .expect("bind first Main");
    let first_target = first
        .register_collaboration_target(delegation())
        .await
        .expect("first target")
        .selector()
        .as_str()
        .to_owned();
    assert!(!first_target.contains("collaboration"));
    assert!(!first_target.contains("create"));
    assert!(!first_target.contains(delegation().as_str()));
    shutdown(&mut first).await;

    let writer =
        CollaborationWriter::spawn(CollaborationFile::open(path).expect("reopen collaboration"))
            .expect("reopened writer");
    let mut resumed = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    let main_ingress = resumed
        .bind_main_ingress(endpoint("main"))
        .expect("bind resumed Main");
    let targets = resumed
        .register_collaboration_targets()
        .await
        .expect("rebuild targets");
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].selector().as_str(), first_target);

    let main = catalog(&directory)
        .with_main_collaboration(main_ingress)
        .expect("resumed Main catalog");
    let mail = admit(
        &main,
        "resumed-mail",
        SEND_MAIL_TOOL_NAME,
        json!({
            "target": first_target,
            "summary": "Resume uses the same selector",
            "artifacts": [],
        }),
    )
    .await;
    let (_, settlement) = execute_and_settle(&main, &mut resumed, mail).await;
    assert!(matches!(
        settled_outcome(settlement.result()),
        Some(CollaborationIngressOutcome::MailAccepted)
    ));
    shutdown(&mut resumed).await;
}

/// CIN-3/CTL-1: production provider refusal precedes child-file and collaboration creation.
#[tokio::test]
async fn ctl_1_unsupported_provider_refuses_before_child_or_delegation_creation() {
    let directory = Directory::new();
    let path = directory.0.join("collaboration.jsonl");
    let mut owner = empty_owner(&directory);
    let main_ingress = owner
        .bind_main_ingress(endpoint("main"))
        .expect("bind Main");
    let children =
        DelegatedConversationDirectory::under(&directory.0).expect("delegated directory");
    let child_path = children.path().to_path_buf();
    // Every production dialect can carry an attributed collaboration turn, so the driver that
    // cannot is a fixture. The gate still has to hold: a child whose model could not be told what
    // it is for must never reach a canonical record.
    owner
        .bind_child_factory(DelegatedChildFactory::synthetic(
            children,
            catalog(&directory),
            FakeDriver::without_collaboration(Vec::<Script>::new()),
            Arc::new(FixedWallClock(UnixMillis::EPOCH)),
        ))
        .expect("bind factory");
    let main = catalog(&directory)
        .with_main_collaboration(main_ingress)
        .expect("Main catalog");
    let unchanged = std::fs::read(&path).expect("initial bytes");
    let delegate = admit(
        &main,
        "delegate-unsupported",
        DELEGATE_TOOL_NAME,
        json!({"task": "Inspect the parser"}),
    )
    .await;
    let (result, settlement) = execute_and_settle(&main, &mut owner, delegate).await;
    assert!(matches!(result.outcome(), ToolOutcome::Failed { .. }));
    assert!(matches!(
        settlement.result(),
        Err(CollaborationIngressFailure::Refused(
            CollaborationIngressRefusal::ProviderUnsupported
        ))
    ));
    assert_eq!(std::fs::read(path).expect("unchanged bytes"), unchanged);
    assert_eq!(
        std::fs::read_dir(child_path)
            .expect("child directory")
            .count(),
        0
    );
    shutdown(&mut owner).await;
}

/// CTL-1/SCH-1: cancelling root progress retains one exact delegation through acknowledgement.
#[tokio::test]
async fn ctl_1_cancelled_delegate_wait_settles_once_without_duplicate_creation() {
    let directory = Directory::new();
    let collaboration_path = directory.0.join("collaboration.jsonl");
    let mut owner = empty_owner(&directory);
    let main_ingress = owner
        .bind_main_ingress(endpoint("main"))
        .expect("bind Main");
    owner
        .bind_child_factory(DelegatedChildFactory::synthetic(
            DelegatedConversationDirectory::under(&directory.0).expect("delegated directory"),
            catalog(&directory),
            FakeDriver::new(Vec::<Script>::new()),
            Arc::new(FixedWallClock(UnixMillis::EPOCH)),
        ))
        .expect("bind factory");
    let main = catalog(&directory)
        .with_main_collaboration(main_ingress)
        .expect("Main catalog");
    let delegate = admit(
        &main,
        "cancelled-delegate",
        DELEGATE_TOOL_NAME,
        json!({"task": "Retain one exact delegation"}),
    )
    .await;
    let execution = tokio::spawn(main.execute(delegate, NativeCancellation::new()));
    let (entered, release) = owner.hold_writer_for_test();
    entered.recv().expect("writer entered hold");
    assert!(
        tokio::time::timeout(Duration::from_millis(10), owner.next_ingress())
            .await
            .is_err(),
        "root progress was cancelled with the exact ingress command retained"
    );
    release.send(()).expect("release writer");

    let settlement = owner
        .next_ingress()
        .await
        .expect("retained delegation settles");
    let target = match settled_outcome(settlement.result()) {
        Some(CollaborationIngressOutcome::Delegated { target }) => target,
        outcome => panic!("delegation did not settle: {outcome:?}"),
    };
    assert!(target.as_str().starts_with("target-v1-"));
    assert!(matches!(
        execution.await.expect("tool execution").outcome(),
        ToolOutcome::Succeeded { .. }
    ));
    assert_eq!(
        std::fs::read_dir(directory.0.join("delegated-sessions"))
            .expect("delegated directory")
            .count(),
        1
    );
    shutdown(&mut owner).await;
    let reopened = CollaborationFile::open(collaboration_path).expect("reopen collaboration");
    assert_eq!(
        reopened
            .ledger()
            .records()
            .iter()
            .filter(|record| matches!(record.event, CollaborationEvent::DelegationCreated { .. }))
            .count(),
        1
    );
}

/// CTL-1/CHB-3: explicit reopen recovers canonical provisioning without an automatic wake.
#[tokio::test]
async fn ctl_1_explicit_resume_recovers_a_canonical_child_missing_its_journal() {
    let directory = Directory::new();
    let collaboration_path = directory.0.join("collaboration.jsonl");
    let mut initial = owner(&directory);
    shutdown(&mut initial).await;
    let unchanged = std::fs::read(&collaboration_path).expect("canonical creation");

    let writer = CollaborationWriter::spawn(
        CollaborationFile::open(&collaboration_path).expect("reopen collaboration"),
    )
    .expect("reopened writer");
    let mut resumed = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    resumed
        .bind_main_ingress(endpoint("main"))
        .expect("bind resumed Main");
    let driver = FakeDriver::new(Vec::<Script>::new());
    let child_driver: Arc<dyn crate::runtime::ModelDriver> = driver.clone();
    resumed
        .bind_child_factory(DelegatedChildFactory::synthetic(
            DelegatedConversationDirectory::under(&directory.0).expect("delegated directory"),
            catalog(&directory),
            child_driver,
            Arc::new(FixedWallClock(UnixMillis::EPOCH)),
        ))
        .expect("bind child factory");
    let targets = resumed
        .register_collaboration_targets()
        .await
        .expect("rebuild target without starting a child");
    assert_eq!(targets.len(), 1);
    assert_eq!(
        std::fs::read_dir(directory.0.join("delegated-sessions"))
            .expect("delegated directory")
            .count(),
        0
    );

    resumed
        .resume_collaboration_target(targets[0].selector())
        .await
        .expect("resume missing canonical child");
    assert!(
        driver.calls().await.is_empty(),
        "resume does not wake the child"
    );
    assert_eq!(
        std::fs::read(&collaboration_path).expect("unchanged collaboration"),
        unchanged
    );
    assert_eq!(
        std::fs::read_dir(directory.0.join("delegated-sessions"))
            .expect("delegated directory")
            .count(),
        1
    );
    shutdown(&mut resumed).await;
}

/// CTL-1: a post-canonical build failure returns its target and resumes without duplication.
#[tokio::test]
async fn ctl_1_post_canonical_provisioning_failure_returns_target_for_explicit_resume() {
    let directory = Directory::new();
    let collaboration_path = directory.0.join("collaboration.jsonl");
    let mut owner = empty_owner(&directory);
    let main_ingress = owner
        .bind_main_ingress(endpoint("main"))
        .expect("bind Main");
    let driver = FakeDriver::new(Vec::<Script>::new());
    let factory = DelegatedChildFactory::synthetic(
        DelegatedConversationDirectory::under(&directory.0).expect("delegated directory"),
        catalog(&directory),
        driver.clone(),
        Arc::new(FixedWallClock(UnixMillis::EPOCH)),
    );
    factory.fail_next_build_for_test();
    owner.bind_child_factory(factory).expect("bind factory");
    let main = catalog(&directory)
        .with_main_collaboration(main_ingress)
        .expect("Main catalog");
    let delegate = admit(
        &main,
        "provisioning-failure",
        DELEGATE_TOOL_NAME,
        json!({"task": "Recover the canonical child"}),
    )
    .await;

    let (result, settlement) = execute_and_settle(&main, &mut owner, delegate).await;
    let target = match settlement.result() {
        Err(CollaborationIngressFailure::Provisioning { target, source }) => {
            assert!(matches!(
                source.as_ref(),
                CollaborationIngressFailure::Factory(
                    crate::DelegatedChildFactoryError::InjectedBuildFailure
                )
            ));
            target.clone()
        }
        outcome => panic!("post-canonical failure lost its target: {outcome:?}"),
    };
    assert!(matches!(
        result.outcome(),
        ToolOutcome::Failed { message } if message.contains(target.as_str())
    ));
    assert!(driver.calls().await.is_empty());
    let after_failure = std::fs::read(&collaboration_path).expect("canonical creation");
    let canonical_targets = owner
        .register_collaboration_targets()
        .await
        .expect("one canonical target");
    assert_eq!(canonical_targets.len(), 1);
    assert_eq!(canonical_targets[0].selector(), &target);
    assert_eq!(
        std::fs::read_dir(directory.0.join("delegated-sessions"))
            .expect("delegated directory")
            .count(),
        1
    );

    owner
        .resume_collaboration_target(&target)
        .await
        .expect("explicitly resume the exact canonical target");
    assert!(
        driver.calls().await.is_empty(),
        "explicit recovery does not invent a wake"
    );
    assert_eq!(
        std::fs::read(&collaboration_path).expect("unchanged collaboration"),
        after_failure
    );
    shutdown(&mut owner).await;
    let reopened = CollaborationFile::open(&collaboration_path).expect("reopen collaboration");
    assert_eq!(
        reopened
            .ledger()
            .records()
            .iter()
            .filter(|record| matches!(record.event, CollaborationEvent::DelegationCreated { .. }))
            .count(),
        1
    );
}

/// CTL-1: post-build registration failure releases the child journal before recovery.
#[tokio::test]
async fn ctl_1_post_build_registration_failure_cleans_up_before_explicit_resume() {
    let directory = Directory::new();
    let collaboration_path = directory.0.join("collaboration.jsonl");
    let mut owner = empty_owner(&directory);
    let main_ingress = owner
        .bind_main_ingress(endpoint("main"))
        .expect("bind Main");
    let driver = FakeDriver::new(Vec::<Script>::new());
    owner
        .bind_child_factory(DelegatedChildFactory::synthetic(
            DelegatedConversationDirectory::under(&directory.0).expect("delegated directory"),
            catalog(&directory),
            driver.clone(),
            Arc::new(FixedWallClock(UnixMillis::EPOCH)),
        ))
        .expect("bind factory");
    owner.fail_next_registration_for_test();
    let main = catalog(&directory)
        .with_main_collaboration(main_ingress)
        .expect("Main catalog");
    let delegate = admit(
        &main,
        "registration-failure",
        DELEGATE_TOOL_NAME,
        json!({"task": "Recover after registration"}),
    )
    .await;

    let (result, settlement) = execute_and_settle(&main, &mut owner, delegate).await;
    let target = match settlement.result() {
        Err(CollaborationIngressFailure::Provisioning { target, source }) => {
            assert!(matches!(
                source.as_ref(),
                CollaborationIngressFailure::Registration(
                    crate::RunnerRegistrationReason::Injected
                )
            ));
            target.clone()
        }
        outcome => panic!("registration failure lost its target: {outcome:?}"),
    };
    assert!(matches!(
        result.outcome(),
        ToolOutcome::Failed { message } if message.contains(target.as_str())
    ));
    assert!(driver.calls().await.is_empty());
    let after_failure = std::fs::read(&collaboration_path).expect("canonical creation");

    owner
        .resume_collaboration_target(&target)
        .await
        .expect("registration cleanup released the exact child journal");
    assert!(driver.calls().await.is_empty());
    assert_eq!(
        std::fs::read(&collaboration_path).expect("unchanged collaboration"),
        after_failure
    );
    shutdown(&mut owner).await;
    let reopened = CollaborationFile::open(&collaboration_path).expect("reopen collaboration");
    assert_eq!(
        reopened
            .ledger()
            .records()
            .iter()
            .filter(|record| matches!(record.event, CollaborationEvent::DelegationCreated { .. }))
            .count(),
        1
    );
}

/// CTL-1/SCH-2: capacity preflight and explicit resume reuse one canonical child identity.
#[tokio::test]
async fn ctl_1_delegate_preflights_capacity_and_resumes_without_duplicate_creation() {
    let directory = Directory::new();
    let collaboration_path = directory.0.join("collaboration.jsonl");
    let mut owner = empty_owner(&directory);
    let main_ingress = owner
        .bind_main_ingress(endpoint("main"))
        .expect("bind Main");
    let children =
        DelegatedConversationDirectory::under(&directory.0).expect("delegated directory");
    let child_path = children.path().to_path_buf();
    owner
        .bind_child_factory(DelegatedChildFactory::synthetic(
            children,
            catalog(&directory),
            FakeDriver::new(Vec::<Script>::new()),
            Arc::new(FixedWallClock(UnixMillis::EPOCH)),
        ))
        .expect("bind factory");
    let main = catalog(&directory)
        .with_main_collaboration(main_ingress)
        .expect("Main catalog");
    let delegate = admit(
        &main,
        "delegate",
        DELEGATE_TOOL_NAME,
        json!({"task": "Inspect the parser"}),
    )
    .await;
    let (result, settlement) = execute_and_settle(&main, &mut owner, delegate).await;
    let target = match settled_outcome(settlement.result()) {
        Some(CollaborationIngressOutcome::Delegated { target }) => target.clone(),
        outcome => panic!("delegation did not settle: {outcome:?}"),
    };
    assert!(matches!(
        result.outcome(),
        ToolOutcome::Succeeded { output }
            if output.contains("\"status\":\"delegated\"")
                && output.contains(target.as_str())
    ));
    assert_eq!(
        owner
            .register_collaboration_targets()
            .await
            .expect("canonical targets")[0]
            .selector(),
        &target
    );
    assert_eq!(
        std::fs::read_dir(child_path)
            .expect("child directory")
            .count(),
        1
    );
    let unchanged = std::fs::read(&collaboration_path).expect("one delegation");
    let second = admit(
        &main,
        "delegate-over-capacity",
        DELEGATE_TOOL_NAME,
        json!({"task": "This must not allocate"}),
    )
    .await;
    let (result, settlement) = execute_and_settle(&main, &mut owner, second).await;
    assert!(matches!(result.outcome(), ToolOutcome::Failed { .. }));
    assert!(matches!(
        settlement.result(),
        Err(CollaborationIngressFailure::Refused(
            CollaborationIngressRefusal::Busy
        ))
    ));
    assert_eq!(
        std::fs::read(&collaboration_path).expect("unchanged collaboration"),
        unchanged
    );
    assert_eq!(
        std::fs::read_dir(directory.0.join("delegated-sessions"))
            .expect("delegated directory")
            .count(),
        1
    );
    shutdown(&mut owner).await;

    let unchanged = std::fs::read(&collaboration_path).expect("canonical creation after shutdown");
    let writer = CollaborationWriter::spawn(
        CollaborationFile::open(&collaboration_path).expect("reopen collaboration"),
    )
    .expect("reopened writer");
    let mut resumed = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    resumed
        .bind_main_ingress(endpoint("main"))
        .expect("bind resumed Main");
    resumed
        .bind_child_factory(DelegatedChildFactory::synthetic(
            DelegatedConversationDirectory::under(&directory.0).expect("delegated directory"),
            catalog(&directory),
            FakeDriver::new(Vec::<Script>::new()),
            Arc::new(FixedWallClock(UnixMillis::EPOCH)),
        ))
        .expect("bind resumed factory");
    let restored = resumed
        .register_collaboration_targets()
        .await
        .expect("restore canonical targets");
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].selector(), &target);
    let expected_worker = restored[0].child_ingress().worker().clone();
    let identity = resumed
        .resume_collaboration_target(&target)
        .await
        .expect("explicitly resume the canonical child");
    assert_eq!(identity.endpoint(), &expected_worker);
    assert_eq!(
        std::fs::read(&collaboration_path).expect("unchanged canonical creation"),
        unchanged
    );
    assert_eq!(
        std::fs::read_dir(directory.0.join("delegated-sessions"))
            .expect("delegated directory")
            .count(),
        1
    );
    shutdown(&mut resumed).await;
}

/// SCH-2/CTL-1: the root activity boundary wakes for ingress even without a runner update.
#[tokio::test]
async fn ctl_1_root_activity_multiplexes_late_ingress_without_polling() {
    let directory = Directory::new();
    let mut owner = owner(&directory);
    let main_ingress = owner
        .bind_main_ingress(endpoint("main"))
        .expect("bind Main");
    let target = owner
        .register_collaboration_target(delegation())
        .await
        .expect("register target");
    let main = catalog(&directory)
        .with_main_collaboration(main_ingress)
        .expect("Main catalog");
    let mail = admit(
        &main,
        "late-mail",
        SEND_MAIL_TOOL_NAME,
        json!({
            "target": target.selector().as_str(),
            "summary": "Wake the root owner",
            "artifacts": [],
        }),
    )
    .await;
    let execution = main.execute(mail, NativeCancellation::new());
    let activity = owner.next_activity();
    let (result, activity) = tokio::join!(execution, activity);
    assert!(matches!(result.outcome(), ToolOutcome::Succeeded { .. }));
    assert!(matches!(
        activity,
        Some(OwnedCollaborationActivity::Ingress(settlement))
            if matches!(
                settled_outcome(settlement.result()),
                Some(CollaborationIngressOutcome::MailAccepted)
            )
    ));
    shutdown(&mut owner).await;
}

fn path_for(directory: &Directory) -> std::path::PathBuf {
    directory.0.join("collaboration.jsonl")
}
