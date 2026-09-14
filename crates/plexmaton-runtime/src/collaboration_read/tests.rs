use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use plexmaton_agent::collaboration::{
    CollaborationEvent, CollaborationLimits, CollaborationText, MailEndpoint, MailEnvelope,
    Preparation,
};
use plexmaton_agent::{
    Agent, ApprovalPolicy, ConversationJournal, ConversationMetadata, TurnBudget, UnixMillis,
};
use plexmaton_core::{
    AgentId, CollaborationId, CollaborationItemId, ConversationId, MailId, TurnId,
};
use plexmaton_provider::{ModelRegistry, resolve_api_key};
use plexmaton_session_store::collaboration::{CollaborationAttempt, CollaborationFile};
use plexmaton_session_store::{ConversationDirectory, DelegatedConversationDirectory};

use super::*;
use crate::runtime::FixedWallClock;
use crate::runtime::tests::{FakeDriver, Script};
use crate::{
    CollaborationWriter, LiveRuntime, NativeToolCatalog, ScheduledTurnRequest, SchedulerLimits,
};

struct Directory(std::path::PathBuf);

impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "plexmaton-collaboration-read-{}",
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir(&path).expect("create directory");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                .expect("protect directory");
        }
        Self(path)
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove directory");
    }
}

fn endpoint(name: &str) -> MailEndpoint {
    MailEndpoint {
        agent: AgentId::new(name).expect("agent"),
        conversation: ConversationId::new(format!("conversation-{name}")).expect("conversation"),
    }
}

fn item(value: &str) -> CollaborationItemId {
    CollaborationItemId::new(value).expect("item")
}

fn mail(id: &str, from: MailEndpoint, to: MailEndpoint) -> CollaborationAttempt {
    CollaborationAttempt {
        id: item(&format!("item-{id}")),
        event: CollaborationEvent::MailAccepted {
            mail: MailEnvelope {
                id: MailId::new(id).expect("mail"),
                from,
                to,
                summary: CollaborationText::new(format!("mail {id}")).expect("summary"),
                artifacts: Vec::new(),
            },
        },
    }
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

async fn session_runtime(
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
        file.append(record.clone()).expect("seed session fact");
    }
    let model = ModelRegistry::parse(
        r#"
active_model = { provider = "fixture", model = "main" }
[providers.fixture]
base_url = "http://127.0.0.1:9/v1"
api_key_env = "TEST_KEY"
api = "openai_responses"
[providers.fixture.models.main]
id = "fixture-main"
context_window_tokens = 8192
max_output_tokens = 1024
output_reserve_tokens = 512
"#,
    )
    .expect("model")
    .active_model()
    .clone();
    let key = resolve_api_key(&model, Some("fixture-only".into())).expect("key");
    LiveRuntime::provider_with_resumed_journal(endpoint.agent.clone(), model, key, tools, file)
        .await
        .expect("session runtime")
        .0
}

/// CMP-2/CIN-2: queued versus included comes only from the selected recipient session.
#[tokio::test]
async fn cmp_2_session_mail_joins_exact_inclusion_and_leaves_sent_status_remote() {
    let directory = Directory::new();
    let main = endpoint("main");
    let child = endpoint("child");
    let delegation = plexmaton_core::DelegationId::new("delegation").expect("delegation");
    let mut file = CollaborationFile::create(
        directory.0.join("collaboration.jsonl"),
        CollaborationId::new("collaboration").expect("collaboration"),
        CollaborationLimits::default(),
    )
    .expect("collaboration file");
    file.admit(
        item("create"),
        CollaborationEvent::DelegationCreated {
            delegation,
            delegator: main.clone(),
            worker: child.clone(),
            task: CollaborationText::new("Inspect the parser").expect("task"),
        },
    )
    .expect("create delegation");
    let first = mail("first", child.clone(), main.clone());
    file.admit(first.id, first.event).expect("first mail");

    let mut agent = Agent::for_conversation(
        main.agent.clone(),
        ConversationMetadata::new(main.conversation.clone(), UnixMillis::EPOCH),
        TurnBudget::default(),
        ApprovalPolicy::default(),
    );
    agent.announce("Main");
    let boundary = agent
        .collaboration_boundary(TurnId::new("turn").expect("turn"))
        .expect("boundary");
    let Preparation::Append(record) = file
        .ledger()
        .prepare_turn(item("admission"), boundary, None)
        .expect("prepare admission")
    else {
        panic!("fresh admission")
    };
    file.admit(record.id, record.event)
        .expect("admit collaboration turn");
    let reference = file
        .ledger()
        .item_reference(&item("admission"))
        .expect("admission reference");
    let resolved = file
        .ledger()
        .resolve_turn(&reference)
        .expect("resolve turn");
    agent
        .start_collaboration_turn(&resolved, UnixMillis::EPOCH)
        .expect("start session turn");

    let writer = CollaborationWriter::spawn(file).expect("writer");
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    let ingress = owner.open_main_ingress().expect("open Main ingress");
    let tools = catalog(&directory)
        .with_main_collaboration(ingress)
        .expect("Main catalog");
    let mut runtime = session_runtime(&directory, &main, agent.journal(), tools.clone()).await;
    owner
        .bind_main_runtime(
            runtime
                .main_collaboration_identity()
                .expect("runtime-issued Main identity"),
        )
        .expect("bind runtime identity");
    let counterfeit_directory = Directory::new();
    let mut counterfeit =
        session_runtime(&counterfeit_directory, &main, agent.journal(), tools).await;
    assert!(matches!(
        owner
            .session_mail_snapshot(
                counterfeit
                    .collaboration_session_source()
                    .expect("same-ingress counterfeit source"),
            )
            .await,
        Err(CollaborationReadError::SourceMismatch)
    ));
    counterfeit
        .shutdown()
        .await
        .expect("shutdown counterfeit runtime");
    for attempt in [
        mail("second", child.clone(), main.clone()),
        mail("reply", main.clone(), child),
    ] {
        owner.admit(attempt).await.expect("append mail");
    }

    let foreign_directory = Directory::new();
    let foreign_file = CollaborationFile::create(
        foreign_directory.0.join("collaboration.jsonl"),
        CollaborationId::new("foreign-collaboration").expect("collaboration"),
        CollaborationLimits::default(),
    )
    .expect("foreign collaboration file");
    let foreign_writer = CollaborationWriter::spawn(foreign_file).expect("foreign writer");
    let mut foreign_owner =
        OwnedCollaboration::new(foreign_writer, SchedulerLimits::new(1).expect("limits"));
    foreign_owner
        .bind_main_ingress(main.clone())
        .expect("bind foreign Main");
    assert!(matches!(
        foreign_owner
            .session_mail_snapshot(
                runtime
                    .collaboration_session_source()
                    .expect("runtime-issued session source"),
            )
            .await,
        Err(CollaborationReadError::SourceMismatch)
    ));

    let snapshot = owner
        .session_mail_snapshot(
            runtime
                .collaboration_session_source()
                .expect("runtime-issued session source"),
        )
        .await
        .expect("session-aware snapshot");
    assert_eq!(snapshot.items().len(), 3);
    assert!(matches!(
        snapshot.items()[0].inclusion(),
        SessionMailInclusion::Included { admission } if admission == &reference
    ));
    assert_eq!(
        snapshot.items()[1].inclusion(),
        &SessionMailInclusion::Queued
    );
    assert_eq!(
        snapshot.items()[2].inclusion(),
        &SessionMailInclusion::OtherRecipient
    );

    runtime.shutdown().await.expect("shutdown runtime");
    foreign_owner
        .begin_shutdown()
        .await
        .expect("begin foreign shutdown");
    foreign_owner
        .finish_shutdown()
        .await
        .expect("finish foreign shutdown");
    owner.begin_shutdown().await.expect("begin shutdown");
    owner.finish_shutdown().await.expect("finish shutdown");
}

/// CMP-2/SCH-2: an active owned child serves its sealed session view off the control lane.
#[tokio::test]
async fn cmp_2_active_owned_child_projects_session_mail_without_blocking_stop() {
    let directory = Directory::new();
    let main = endpoint("main");
    let child = endpoint("child");
    let delegation = plexmaton_core::DelegationId::new("delegation").expect("delegation");
    let mut file = CollaborationFile::create(
        directory.0.join("collaboration.jsonl"),
        CollaborationId::new("collaboration").expect("collaboration"),
        CollaborationLimits::default(),
    )
    .expect("collaboration file");
    file.admit(
        item("create"),
        CollaborationEvent::DelegationCreated {
            delegation: delegation.clone(),
            delegator: main.clone(),
            worker: child.clone(),
            task: CollaborationText::new("Inspect the parser").expect("task"),
        },
    )
    .expect("create delegation");
    let first = mail("first", main.clone(), child.clone());
    file.admit(first.id, first.event).expect("first mail");

    let writer = CollaborationWriter::spawn(file).expect("writer");
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    owner.bind_main_ingress(main.clone()).expect("bind Main");
    let target = owner
        .register_collaboration_target(delegation.clone())
        .await
        .expect("register target");
    let binding = owner
        .delegated_binding(delegation.clone())
        .await
        .expect("child binding");
    let children =
        DelegatedConversationDirectory::under(&directory.0).expect("delegated directory");
    let journal = children
        .create(child.conversation.clone(), UnixMillis::EPOCH)
        .expect("child journal");
    let tools = catalog(&directory)
        .with_child_collaboration(target.child_ingress())
        .expect("child catalog");
    let started = Arc::new(tokio::sync::Notify::new());
    let cancelled = Arc::new(AtomicBool::new(false));
    let driver = FakeDriver::new([Script::WaitForCancellation {
        started: Arc::clone(&started),
        finished: Arc::clone(&cancelled),
    }]);
    let runtime = LiveRuntime::with_fresh_bound_delegated_driver(
        child.agent.clone(),
        "Child".into(),
        driver,
        tools,
        journal,
        binding,
        Arc::new(FixedWallClock(UnixMillis::EPOCH)),
    )
    .await
    .expect("child runtime");
    let (boundary, previous) = runtime
        .collaboration_boundary(TurnId::new("turn").expect("turn"))
        .expect("child boundary");
    let request = ScheduledTurnRequest::new(delegation, item("admission"), boundary, previous);
    owner.register(runtime).await.expect("register child");
    owner.next_update().await.expect("initial child update");
    owner.schedule(request).await.expect("schedule child turn");
    started.notified().await;

    for attempt in [
        mail("second", main.clone(), child.clone()),
        mail("reply", child.clone(), main),
    ] {
        owner.admit(attempt).await.expect("append mail");
    }
    let snapshot = owner
        .child_session_mail_snapshot(&child.conversation)
        .await
        .expect("active child snapshot");
    assert_eq!(snapshot.items().len(), 3);
    assert!(matches!(
        snapshot.items()[0].inclusion(),
        SessionMailInclusion::Included { admission } if admission.item == item("admission")
    ));
    assert_eq!(
        snapshot.items()[1].inclusion(),
        &SessionMailInclusion::Queued
    );
    assert_eq!(
        snapshot.items()[2].inclusion(),
        &SessionMailInclusion::OtherRecipient
    );

    owner
        .stop(&child.conversation)
        .await
        .expect("Stop retains its separate control capacity");
    assert!(cancelled.load(Ordering::SeqCst));
    owner.begin_shutdown().await.expect("begin shutdown");
    loop {
        if owner
            .next_update()
            .await
            .expect("shutdown update")
            .is_finished()
        {
            break;
        }
    }
    owner.finish_shutdown().await.expect("finish shutdown");
}
