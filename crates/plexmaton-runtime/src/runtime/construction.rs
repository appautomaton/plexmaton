use std::{collections::VecDeque, sync::Arc};

use plexmaton_agent::{
    Agent, ApprovalPolicy, ConversationJournal, ConversationMetadata, TurnBudget,
};
use plexmaton_core::{AgentId, ConversationId};
use plexmaton_provider::{ApiKey, ResolvedModel};
use plexmaton_session_store::collaboration::DelegatedConversationControl;
use plexmaton_session_store::{AutomaticJournal, JournalRecovery, RootJournalFile};
use tokio::sync::mpsc;

use super::clock::{SystemWallClock, WallClock};
use super::{
    AfterCommit, JournalWriter, LiveRuntime, ModelDriver, RuntimeInputControl, ToolTasks,
    journal::JournalStore,
};
use crate::{
    ConversationRecovery, HttpSetupError, JournalTailRecovery, NativeToolCatalog, RuntimeError,
    http::ProviderHttp,
};

mod delegated;

const MODEL_SIGNAL_CAPACITY: usize = 32;

struct JournalRuntimeOwner {
    store: Box<dyn JournalStore>,
    input_control: RuntimeInputControl,
}

impl JournalRuntimeOwner {
    fn user(store: Box<dyn JournalStore>) -> Self {
        Self {
            store,
            input_control: RuntimeInputControl::User,
        }
    }

    fn awaiting_delegated(store: Box<dyn JournalStore>) -> Self {
        Self {
            store,
            input_control: RuntimeInputControl::AwaitingDelegatedControl,
        }
    }

    fn delegated(store: Box<dyn JournalStore>, control: DelegatedConversationControl) -> Self {
        Self {
            store,
            input_control: RuntimeInputControl::Delegated(Box::new(control)),
        }
    }
}

impl LiveRuntime {
    /// Validates HTTP ownership and announces one idle live agent without touching the network.
    pub fn provider(
        agent_id: AgentId,
        label: impl Into<String>,
        model: ResolvedModel,
        key: ApiKey,
        tools: NativeToolCatalog,
    ) -> Result<Self, RuntimeError> {
        if !tools.excludes_api_key_environment(model.api_key_env()) {
            return Err(HttpSetupError::ToolCredentialEnvironmentMismatch.into());
        }
        let definitions = tools.provider_definitions();
        let clock: Arc<dyn WallClock> = Arc::new(SystemWallClock::new()?);
        let driver = Arc::new(ProviderHttp::new(
            model,
            key,
            definitions,
            Arc::clone(&clock),
        )?);
        Ok(Self::with_driver_and_clock(
            agent_id,
            label.into(),
            driver,
            tools,
            clock,
        ))
    }

    /// Opens one new live agent whose canonical reactions must reach an empty journal first.
    pub async fn provider_with_fresh_journal(
        agent_id: AgentId,
        label: impl Into<String>,
        model: ResolvedModel,
        key: ApiKey,
        tools: NativeToolCatalog,
        journal: RootJournalFile,
    ) -> Result<Self, RuntimeError> {
        if !journal.journal().records().is_empty() {
            return Err(RuntimeError::FreshJournalNotEmpty);
        }
        let metadata = journal.journal().metadata().clone();
        Self::provider_with_new_store(
            agent_id,
            label.into(),
            model,
            key,
            tools,
            metadata,
            JournalRuntimeOwner::user(Box::new(journal)),
        )
        .await
    }

    /// Owns a lazy automatic writer: no file until the first user turn, no effect before its append.
    pub async fn provider_with_automatic_journal(
        agent_id: AgentId,
        label: impl Into<String>,
        model: ResolvedModel,
        key: ApiKey,
        tools: NativeToolCatalog,
        journal: AutomaticJournal,
    ) -> Result<Self, RuntimeError> {
        let metadata = journal.metadata().clone();
        Self::provider_with_new_store(
            agent_id,
            label.into(),
            model,
            key,
            tools,
            metadata,
            JournalRuntimeOwner::user(Box::new(journal)),
        )
        .await
    }

    async fn provider_with_new_store(
        agent_id: AgentId,
        label: String,
        model: ResolvedModel,
        key: ApiKey,
        tools: NativeToolCatalog,
        metadata: ConversationMetadata,
        owner: JournalRuntimeOwner,
    ) -> Result<Self, RuntimeError> {
        if !tools.excludes_api_key_environment(model.api_key_env()) {
            return Err(HttpSetupError::ToolCredentialEnvironmentMismatch.into());
        }
        let definitions = tools.provider_definitions();
        let clock: Arc<dyn WallClock> = Arc::new(SystemWallClock::new()?);
        let driver = Arc::new(ProviderHttp::new(
            model,
            key,
            definitions,
            Arc::clone(&clock),
        )?);
        Self::with_driver_store_and_owner(agent_id, label, driver, tools, metadata, owner, clock)
            .await
    }

    /// Rebuilds one live owner from an existing journal and settles work orphaned by process death.
    pub async fn provider_with_resumed_journal(
        agent_id: AgentId,
        model: ResolvedModel,
        key: ApiKey,
        tools: NativeToolCatalog,
        journal: RootJournalFile,
    ) -> Result<(Self, ConversationRecovery), RuntimeError> {
        Self::provider_with_resumed_journal_control(
            agent_id,
            model,
            key,
            tools,
            journal,
            RuntimeInputControl::User,
        )
        .await
    }

    async fn provider_with_resumed_journal_control(
        agent_id: AgentId,
        model: ResolvedModel,
        key: ApiKey,
        tools: NativeToolCatalog,
        journal: RootJournalFile,
        input_control: RuntimeInputControl,
    ) -> Result<(Self, ConversationRecovery), RuntimeError> {
        let recovery = journal.recovery().clone();
        let snapshot = journal.journal().clone();
        let owner = match input_control {
            RuntimeInputControl::User => JournalRuntimeOwner::user(Box::new(journal)),
            RuntimeInputControl::AwaitingDelegatedControl => {
                JournalRuntimeOwner::awaiting_delegated(Box::new(journal))
            }
            RuntimeInputControl::Delegated(_) => {
                unreachable!("delegated control attaches only after runtime construction")
            }
        };
        Self::provider_with_resumed_store(agent_id, model, key, tools, snapshot, recovery, owner)
            .await
    }

    async fn provider_with_resumed_store(
        agent_id: AgentId,
        model: ResolvedModel,
        key: ApiKey,
        tools: NativeToolCatalog,
        snapshot: ConversationJournal,
        recovery: JournalRecovery,
        owner: JournalRuntimeOwner,
    ) -> Result<(Self, ConversationRecovery), RuntimeError> {
        if !tools.excludes_api_key_environment(model.api_key_env()) {
            return Err(HttpSetupError::ToolCredentialEnvironmentMismatch.into());
        }
        let definitions = tools.provider_definitions();
        let clock: Arc<dyn WallClock> = Arc::new(SystemWallClock::new()?);
        let driver = Arc::new(ProviderHttp::new(
            model,
            key,
            definitions,
            Arc::clone(&clock),
        )?);
        let agent = Agent::from_journal(
            agent_id.clone(),
            snapshot,
            TurnBudget::default(),
            ApprovalPolicy::default(),
        )
        .map_err(RuntimeError::JournalProjection)?;
        Self::with_resumed_driver_and_owner(agent_id, agent, driver, tools, owner, recovery, clock)
            .await
    }

    #[cfg(test)]
    pub(super) fn with_driver(
        agent_id: AgentId,
        label: String,
        driver: Arc<dyn ModelDriver>,
        tools: NativeToolCatalog,
    ) -> Result<Self, RuntimeError> {
        Ok(Self::with_driver_and_clock(
            agent_id,
            label,
            driver,
            tools,
            Arc::new(SystemWallClock::new()?),
        ))
    }

    #[cfg(test)]
    pub(crate) fn with_root_driver_for_test(
        agent_id: AgentId,
        label: String,
        driver: Arc<dyn ModelDriver>,
        tools: NativeToolCatalog,
    ) -> Result<Self, RuntimeError> {
        Self::with_driver(agent_id, label, driver, tools)
    }

    pub(super) fn with_driver_and_clock(
        agent_id: AgentId,
        label: String,
        driver: Arc<dyn ModelDriver>,
        tools: NativeToolCatalog,
        clock: Arc<dyn WallClock>,
    ) -> Self {
        let created_at_unix_ms = clock.now();
        let session_id = ConversationId::new(format!("conversation-{}", uuid::Uuid::now_v7()))
            .unwrap_or_else(|error| unreachable!("a formatted identity is valid: {error}"));
        let metadata = ConversationMetadata::new(session_id, created_at_unix_ms);
        let (signals, signal_rx) = mpsc::channel(MODEL_SIGNAL_CAPACITY);
        let mut runtime = Self {
            agent_id: agent_id.clone(),
            agent: Agent::for_conversation(
                agent_id,
                metadata,
                TurnBudget::default(),
                ApprovalPolicy::default(),
            ),
            driver,
            pending: VecDeque::new(),
            signals,
            signal_rx,
            active: None,
            pending_model_start: None,
            collaboration_context: Default::default(),
            input_control: RuntimeInputControl::User,
            collaboration_permit: None,
            deferred_model_call: None,
            permissions: crate::CodingSessionPermissions::new(&tools),
            deferred_compaction_failure: None,
            compaction: None,
            compaction_budget: Default::default(),
            compaction_timeout: super::compaction::DEFAULT_COMPACTION_TIMEOUT,
            tools: ToolTasks::new(tools),
            report: crate::DispatchReport::default(),
            journal: None,
            pending_commit: None,
            after_commit: None,
            pending_inputs: VecDeque::new(),
            preparing_input: None,
            journal_failed: false,
            shutdown_state: super::ShutdownState::Open,
            clock,
            collaboration_identity:
                crate::collaboration_ingress::RuntimeCollaborationIdentity::fresh(),
        };
        let announced = runtime.agent.announce(label);
        runtime
            .apply_ready_reaction(announced)
            .unwrap_or_else(|_| unreachable!("announcing an idle agent starts no outside work"));
        runtime
    }

    #[cfg(test)]
    pub(super) async fn with_driver_store_and_clock(
        agent_id: AgentId,
        label: String,
        driver: Arc<dyn ModelDriver>,
        tools: NativeToolCatalog,
        metadata: ConversationMetadata,
        store: Box<dyn JournalStore>,
        clock: Arc<dyn WallClock>,
    ) -> Result<Self, RuntimeError> {
        Self::with_driver_store_and_owner(
            agent_id,
            label,
            driver,
            tools,
            metadata,
            JournalRuntimeOwner::user(store),
            clock,
        )
        .await
    }

    #[cfg(test)]
    pub(super) async fn with_delegated_driver_store_and_clock(
        agent_id: AgentId,
        label: String,
        driver: Arc<dyn ModelDriver>,
        tools: NativeToolCatalog,
        metadata: ConversationMetadata,
        store: Box<dyn JournalStore>,
        clock: Arc<dyn WallClock>,
    ) -> Result<Self, RuntimeError> {
        Self::with_driver_store_and_owner(
            agent_id,
            label,
            driver,
            tools,
            metadata,
            JournalRuntimeOwner::awaiting_delegated(store),
            clock,
        )
        .await
    }

    async fn with_driver_store_and_owner(
        agent_id: AgentId,
        label: String,
        driver: Arc<dyn ModelDriver>,
        tools: NativeToolCatalog,
        metadata: ConversationMetadata,
        owner: JournalRuntimeOwner,
        clock: Arc<dyn WallClock>,
    ) -> Result<Self, RuntimeError> {
        let JournalRuntimeOwner {
            store,
            input_control,
        } = owner;
        let (signals, signal_rx) = mpsc::channel(MODEL_SIGNAL_CAPACITY);
        let mut runtime = Self {
            agent_id: agent_id.clone(),
            agent: Agent::for_conversation(
                agent_id,
                metadata,
                TurnBudget::default(),
                ApprovalPolicy::default(),
            ),
            driver,
            pending: VecDeque::new(),
            signals,
            signal_rx,
            active: None,
            pending_model_start: None,
            collaboration_context: Default::default(),
            input_control,
            collaboration_permit: None,
            deferred_model_call: None,
            permissions: crate::CodingSessionPermissions::new(&tools),
            deferred_compaction_failure: None,
            compaction: None,
            compaction_budget: Default::default(),
            compaction_timeout: super::compaction::DEFAULT_COMPACTION_TIMEOUT,
            tools: ToolTasks::new(tools),
            report: crate::DispatchReport::default(),
            journal: Some(
                JournalWriter::spawn(store).map_err(|_| RuntimeError::JournalWriterUnavailable)?,
            ),
            pending_commit: None,
            after_commit: None,
            pending_inputs: VecDeque::new(),
            preparing_input: None,
            journal_failed: false,
            shutdown_state: super::ShutdownState::Open,
            clock,
            collaboration_identity:
                crate::collaboration_ingress::RuntimeCollaborationIdentity::fresh(),
        };
        let reaction = runtime.agent.announce(label);
        runtime.begin_transition(reaction, Vec::new(), AfterCommit::None)?;
        runtime.finish_transition().await?;
        Ok(runtime)
    }

    #[cfg(test)]
    pub(super) async fn with_resumed_driver_and_store(
        agent_id: AgentId,
        agent: Agent,
        driver: Arc<dyn ModelDriver>,
        tools: NativeToolCatalog,
        store: Box<dyn JournalStore>,
        tail_recovery: JournalRecovery,
        clock: Arc<dyn WallClock>,
    ) -> Result<(Self, ConversationRecovery), RuntimeError> {
        Self::with_resumed_driver_and_owner(
            agent_id,
            agent,
            driver,
            tools,
            JournalRuntimeOwner::user(store),
            tail_recovery,
            clock,
        )
        .await
    }

    #[cfg(test)]
    pub(super) async fn with_resumed_delegated_driver_and_store(
        agent_id: AgentId,
        agent: Agent,
        driver: Arc<dyn ModelDriver>,
        tools: NativeToolCatalog,
        store: Box<dyn JournalStore>,
        tail_recovery: JournalRecovery,
        clock: Arc<dyn WallClock>,
    ) -> Result<(Self, ConversationRecovery), RuntimeError> {
        Self::with_resumed_driver_and_owner(
            agent_id,
            agent,
            driver,
            tools,
            JournalRuntimeOwner::awaiting_delegated(store),
            tail_recovery,
            clock,
        )
        .await
    }

    async fn with_resumed_driver_and_owner(
        agent_id: AgentId,
        mut agent: Agent,
        driver: Arc<dyn ModelDriver>,
        tools: NativeToolCatalog,
        owner: JournalRuntimeOwner,
        tail_recovery: JournalRecovery,
        clock: Arc<dyn WallClock>,
    ) -> Result<(Self, ConversationRecovery), RuntimeError> {
        let JournalRuntimeOwner {
            store,
            input_control,
        } = owner;
        let projection = agent
            .rebuild_projection()
            .unwrap_or_else(|_| unreachable!("a newly restored agent has no transient work"));
        let pending = VecDeque::from(projection.events().to_vec());
        let recovered = agent.recover_after_process_death_at(clock.now());
        let (signals, signal_rx) = mpsc::channel(MODEL_SIGNAL_CAPACITY);
        let mut runtime = Self {
            agent_id,
            agent,
            driver,
            pending,
            signals,
            signal_rx,
            active: None,
            pending_model_start: None,
            collaboration_context: Default::default(),
            input_control,
            collaboration_permit: None,
            deferred_model_call: None,
            permissions: crate::CodingSessionPermissions::new(&tools),
            deferred_compaction_failure: None,
            compaction: None,
            compaction_budget: Default::default(),
            compaction_timeout: super::compaction::DEFAULT_COMPACTION_TIMEOUT,
            tools: ToolTasks::new(tools),
            report: crate::DispatchReport::default(),
            journal: Some(
                JournalWriter::spawn(store).map_err(|_| RuntimeError::JournalWriterUnavailable)?,
            ),
            pending_commit: None,
            after_commit: None,
            pending_inputs: VecDeque::new(),
            preparing_input: None,
            journal_failed: false,
            shutdown_state: super::ShutdownState::Open,
            clock,
            collaboration_identity:
                crate::collaboration_ingress::RuntimeCollaborationIdentity::fresh(),
        };
        let interrupted_turn = recovered.is_some();
        if let Some(reaction) = recovered {
            runtime.begin_transition(reaction, Vec::new(), AfterCommit::None)?;
            runtime.finish_transition().await?;
        }
        let tail = match tail_recovery {
            JournalRecovery::Clean => None,
            JournalRecovery::AddedFinalNewline => Some(JournalTailRecovery::AddedFinalNewline),
            JournalRecovery::IsolatedFinalTail { bytes, .. } => {
                Some(JournalTailRecovery::IsolatedFinalTail { bytes })
            }
        };
        Ok((
            runtime,
            ConversationRecovery {
                tail,
                interrupted_turn,
            },
        ))
    }
}
