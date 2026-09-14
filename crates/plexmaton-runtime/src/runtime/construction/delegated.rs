//! Construction that never exposes a delegated runtime before durable control is bound.

use plexmaton_core::{AgentId, ConversationId};
use plexmaton_provider::{ApiKey, ResolvedModel};
use plexmaton_session_store::DelegatedJournalFile;

use super::{JournalRuntimeOwner, LiveRuntime};
use crate::{ConversationRecovery, DelegatedRuntimeBinding, NativeToolCatalog, RuntimeError};

impl LiveRuntime {
    #[cfg(test)]
    pub(crate) async fn with_fresh_bound_delegated_driver(
        agent_id: AgentId,
        label: String,
        driver: std::sync::Arc<dyn super::super::ModelDriver>,
        tools: NativeToolCatalog,
        journal: DelegatedJournalFile,
        binding: DelegatedRuntimeBinding,
        clock: std::sync::Arc<dyn super::super::WallClock>,
    ) -> Result<Self, RuntimeError> {
        validate_binding(&binding, &agent_id, journal.journal().conversation_id())?;
        if !journal.journal().records().is_empty() {
            return Err(RuntimeError::FreshJournalNotEmpty);
        }
        let metadata = journal.journal().metadata().clone();
        Self::with_driver_store_and_owner(
            agent_id,
            label,
            driver,
            tools.into_read_only(),
            metadata,
            JournalRuntimeOwner::delegated(Box::new(journal), binding.control),
            clock,
        )
        .await
    }

    #[cfg(test)]
    pub(crate) async fn with_resumed_bound_delegated_driver(
        agent_id: AgentId,
        driver: std::sync::Arc<dyn super::super::ModelDriver>,
        tools: NativeToolCatalog,
        journal: DelegatedJournalFile,
        binding: DelegatedRuntimeBinding,
        clock: std::sync::Arc<dyn super::super::WallClock>,
    ) -> Result<Self, RuntimeError> {
        validate_binding(&binding, &agent_id, journal.journal().conversation_id())?;
        let recovery = journal.recovery().clone();
        let snapshot = journal.journal().clone();
        let owner = JournalRuntimeOwner::delegated(Box::new(journal), binding.control);
        let agent = plexmaton_agent::Agent::from_journal(
            agent_id.clone(),
            snapshot,
            plexmaton_agent::TurnBudget::default(),
            plexmaton_agent::ApprovalPolicy::default(),
        )
        .map_err(RuntimeError::JournalProjection)?;
        Self::with_resumed_driver_and_owner(
            agent_id,
            agent,
            driver,
            tools.into_read_only(),
            owner,
            recovery,
            clock,
        )
        .await
        .map(|(runtime, _)| runtime)
    }

    /// Opens a delegated Conversation only after binding its exact durable controller.
    pub async fn provider_with_fresh_delegated_journal(
        agent_id: AgentId,
        label: impl Into<String>,
        model: ResolvedModel,
        key: ApiKey,
        tools: NativeToolCatalog,
        journal: DelegatedJournalFile,
        binding: DelegatedRuntimeBinding,
    ) -> Result<Self, RuntimeError> {
        validate_binding(&binding, &agent_id, journal.journal().conversation_id())?;
        if !journal.journal().records().is_empty() {
            return Err(RuntimeError::FreshJournalNotEmpty);
        }
        let metadata = journal.journal().metadata().clone();
        let tools = tools.into_read_only();
        Self::provider_with_new_store(
            agent_id,
            label.into(),
            model,
            key,
            tools,
            metadata,
            JournalRuntimeOwner::delegated(Box::new(journal), binding.control),
        )
        .await
    }

    /// Reopens a delegated Conversation only after binding its exact durable controller.
    pub async fn provider_with_resumed_delegated_journal(
        agent_id: AgentId,
        model: ResolvedModel,
        key: ApiKey,
        tools: NativeToolCatalog,
        journal: DelegatedJournalFile,
        binding: DelegatedRuntimeBinding,
    ) -> Result<(Self, ConversationRecovery), RuntimeError> {
        validate_binding(&binding, &agent_id, journal.journal().conversation_id())?;
        let recovery = journal.recovery().clone();
        let snapshot = journal.journal().clone();
        let tools = tools.into_read_only();
        let owner = JournalRuntimeOwner::delegated(Box::new(journal), binding.control);
        Self::provider_with_resumed_store(agent_id, model, key, tools, snapshot, recovery, owner)
            .await
    }
}

fn validate_binding(
    binding: &DelegatedRuntimeBinding,
    agent: &AgentId,
    conversation: &ConversationId,
) -> Result<(), RuntimeError> {
    if &binding.control.worker().agent != agent
        || &binding.control.worker().conversation != conversation
    {
        return Err(RuntimeError::DelegatedControlMismatch);
    }
    binding.control.controller()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use plexmaton_agent::collaboration::{
        CollaborationEvent, CollaborationLimits, CollaborationText, DelegationController,
        DelegationRevision, MailEndpoint,
    };
    use plexmaton_agent::{
        Agent, ApprovalPolicy, ConversationMetadata, Input, TurnBudget, UnixMillis,
    };
    use plexmaton_core::{
        AgentId, CollaborationId, CollaborationItemId, ConversationId, DelegationId,
    };
    use plexmaton_provider::{ApiKey, ModelRegistry, ResolvedModel, resolve_api_key};
    use plexmaton_session_store::collaboration::CollaborationFile;
    use plexmaton_session_store::{DelegatedConversationDirectory, DelegatedJournalFile};

    use super::*;

    struct Directory(std::path::PathBuf);

    impl Directory {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "plexmaton-delegated-{name}-{}",
                uuid::Uuid::now_v7()
            ));
            std::fs::create_dir(&path).expect("create test directory");
            Self(path)
        }

        fn tools(&self, model: &ResolvedModel) -> NativeToolCatalog {
            NativeToolCatalog::open(
                &self.0,
                model.api_key_env(),
                "/bin/false",
                "/bin/false",
                Vec::new(),
            )
            .expect("native tools")
        }
    }

    impl Drop for Directory {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).expect("remove test directory");
        }
    }

    fn model_and_key() -> (ResolvedModel, ApiKey) {
        let model = ModelRegistry::parse(
            r#"
active_model = { provider = "fixture", model = "child" }
[providers.fixture]
base_url = "http://127.0.0.1:9/v1"
api_key_env = "CHILD_TEST_KEY"
api = "openai_responses"
[providers.fixture.models.child]
id = "fixture-child"
reasoning_effort = "high"
allowed_reasoning_efforts = ["low", "high", "max"]
context_window_tokens = 8192
max_output_tokens = 1024
output_reserve_tokens = 512
"#,
        )
        .expect("model config")
        .active_model()
        .clone();
        let key = resolve_api_key(&model, Some("fixture-only".into())).expect("api key");
        (model, key)
    }

    fn child_endpoint() -> MailEndpoint {
        MailEndpoint {
            agent: AgentId::new("child-agent").expect("child agent"),
            conversation: ConversationId::new("child-conversation").expect("child conversation"),
        }
    }

    fn delegation() -> DelegationId {
        DelegationId::new("inspect-parser").expect("delegation")
    }

    fn collaboration(directory: &Directory) -> CollaborationFile {
        let child = child_endpoint();
        let mut file = CollaborationFile::create(
            directory.0.join("collaboration/log.jsonl"),
            CollaborationId::new("bootstrap").expect("collaboration"),
            CollaborationLimits::default(),
        )
        .expect("collaboration file");
        file.admit(
            CollaborationItemId::new("create-child").expect("item"),
            CollaborationEvent::DelegationCreated {
                delegation: delegation(),
                delegator: MailEndpoint {
                    agent: AgentId::new("main-agent").expect("main agent"),
                    conversation: ConversationId::new("main-conversation")
                        .expect("main conversation"),
                },
                worker: child,
                task: CollaborationText::new("Inspect the parser").expect("task"),
            },
        )
        .expect("create delegation");
        file
    }

    fn binding(file: &CollaborationFile) -> DelegatedRuntimeBinding {
        DelegatedRuntimeBinding::new(
            file.delegated_control(&delegation())
                .expect("delegated control"),
        )
    }

    fn append_reaction(journal: &mut DelegatedJournalFile, reaction: plexmaton_agent::Reaction) {
        for record in reaction.records {
            journal.append(record).expect("append child record");
        }
    }

    /// CHB-1/CHB-2: construction validates the canonical child before publishing a narrowed runtime.
    #[tokio::test]
    async fn chb_2_fresh_child_constructor_is_exact_and_forces_read_only_tools() {
        let directory = Directory::new("fresh");
        let children =
            DelegatedConversationDirectory::under(&directory.0).expect("delegated directory");
        let child = child_endpoint();
        let mut collaboration = collaboration(&directory);

        let journal = children
            .create(child.conversation.clone(), UnixMillis::EPOCH)
            .expect("child journal");
        let path = journal.path().to_path_buf();
        let unchanged = std::fs::read(&path).expect("child bytes");
        let (model, key) = model_and_key();
        let error = LiveRuntime::provider_with_fresh_delegated_journal(
            AgentId::new("wrong-agent").expect("wrong agent"),
            "Child",
            model,
            key,
            directory.tools(&model_and_key().0),
            journal,
            binding(&collaboration),
        )
        .await;
        assert!(matches!(error, Err(RuntimeError::DelegatedControlMismatch)));
        assert_eq!(
            std::fs::read(&path).expect("unchanged child bytes"),
            unchanged
        );

        let wrong_id = ConversationId::new("wrong-conversation").expect("wrong conversation");
        let wrong_journal = children
            .create(wrong_id, UnixMillis::EPOCH)
            .expect("wrong child journal");
        let wrong_path = wrong_journal.path().to_path_buf();
        let wrong_unchanged = std::fs::read(&wrong_path).expect("wrong child bytes");
        let (model, key) = model_and_key();
        let error = LiveRuntime::provider_with_fresh_delegated_journal(
            child.agent.clone(),
            "Child",
            model,
            key,
            directory.tools(&model_and_key().0),
            wrong_journal,
            binding(&collaboration),
        )
        .await;
        assert!(matches!(error, Err(RuntimeError::DelegatedControlMismatch)));
        assert_eq!(
            std::fs::read(&wrong_path).expect("unchanged wrong bytes"),
            wrong_unchanged
        );

        let journal = children
            .resume(&child.conversation)
            .expect("resume exact child journal");
        let (model, key) = model_and_key();
        let exact_binding = binding(&collaboration);
        let delegator = exact_binding.provenance().delegator().clone();
        let mut runtime = LiveRuntime::provider_with_fresh_delegated_journal(
            child.agent.clone(),
            "Child",
            model.clone(),
            key,
            directory.tools(&model),
            journal,
            exact_binding,
        )
        .await
        .expect("construct exact child");
        assert_eq!(runtime.agent_id(), &child.agent);
        assert_eq!(runtime.configured_model(), Some(&model));
        assert_eq!(
            runtime.delegation_controller().expect("controller"),
            Some(DelegationController::Main)
        );
        assert_eq!(
            runtime
                .driver
                .budget_inputs()
                .expect("provider budget inputs")
                .1
                .len(),
            2
        );
        assert!(!runtime.has_active_work());

        collaboration
            .admit(
                CollaborationItemId::new("handoff-child").expect("handoff item"),
                CollaborationEvent::HandoffCompleted {
                    delegation: delegation(),
                    expected: DelegationRevision(0),
                    author: delegator,
                },
            )
            .expect("handoff child");
        assert_eq!(
            runtime.delegation_controller().expect("controller"),
            Some(DelegationController::User)
        );
        assert_eq!(
            runtime
                .driver
                .budget_inputs()
                .expect("provider budget inputs")
                .1
                .len(),
            2
        );
        runtime.shutdown().await.expect("shutdown child");
    }

    /// CHB-3: resume settles an interrupted child without automatically dispatching its work.
    #[tokio::test]
    async fn chb_3_resumed_child_settles_interruption_without_redispatch() {
        let directory = Directory::new("resume");
        let children =
            DelegatedConversationDirectory::under(&directory.0).expect("delegated directory");
        let child = child_endpoint();
        let mut journal = children
            .create(child.conversation.clone(), UnixMillis::EPOCH)
            .expect("child journal");
        let mut agent = Agent::for_conversation(
            child.agent.clone(),
            ConversationMetadata::new(child.conversation.clone(), UnixMillis::EPOCH),
            TurnBudget::default(),
            ApprovalPolicy::default(),
        );
        append_reaction(&mut journal, agent.announce("Child"));
        append_reaction(
            &mut journal,
            agent.handle_at(
                Input::Submitted {
                    text: "unfinished child turn".into(),
                },
                UnixMillis::new(1),
            ),
        );
        drop(journal);

        let collaboration = collaboration(&directory);
        drop(collaboration);
        let collaboration = CollaborationFile::open(directory.0.join("collaboration/log.jsonl"))
            .expect("reopen collaboration");
        let child_path = children.path_for(&child.conversation).expect("child path");
        let unchanged = std::fs::read(&child_path).expect("interrupted child bytes");
        let populated = children
            .resume(&child.conversation)
            .expect("open populated child journal");
        let (model, key) = model_and_key();
        let error = LiveRuntime::provider_with_fresh_delegated_journal(
            child.agent.clone(),
            "Child",
            model.clone(),
            key,
            directory.tools(&model),
            populated,
            binding(&collaboration),
        )
        .await;
        assert!(matches!(error, Err(RuntimeError::FreshJournalNotEmpty)));
        assert_eq!(
            std::fs::read(&child_path).expect("unchanged interrupted bytes"),
            unchanged
        );

        let populated = children
            .resume(&child.conversation)
            .expect("reopen populated child journal");
        let (model, key) = model_and_key();
        let error = LiveRuntime::provider_with_resumed_delegated_journal(
            AgentId::new("wrong-resumed-agent").expect("wrong resumed agent"),
            model.clone(),
            key,
            directory.tools(&model),
            populated,
            binding(&collaboration),
        )
        .await;
        assert!(matches!(error, Err(RuntimeError::DelegatedControlMismatch)));
        assert_eq!(
            std::fs::read(&child_path).expect("unchanged mismatched resume bytes"),
            unchanged
        );

        let journal = children
            .resume(&child.conversation)
            .expect("reopen child journal");
        let (model, key) = model_and_key();
        let (mut runtime, recovery) = LiveRuntime::provider_with_resumed_delegated_journal(
            child.agent.clone(),
            model.clone(),
            key,
            directory.tools(&model),
            journal,
            binding(&collaboration),
        )
        .await
        .expect("resume child");
        assert!(recovery.interrupted_turn);
        assert!(!runtime.has_active_model());
        assert!(!runtime.has_active_work());
        assert_eq!(runtime.configured_model(), Some(&model));
        assert_eq!(
            runtime.delegation_controller().expect("controller"),
            Some(DelegationController::Main)
        );
        runtime.shutdown().await.expect("shutdown child");
    }
}
