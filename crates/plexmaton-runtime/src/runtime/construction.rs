use std::{collections::VecDeque, sync::Arc};

use plexmaton_agent::{Agent, ApprovalPolicy, SessionMetadata, TurnBudget};
use plexmaton_core::{AgentId, SessionId};
use plexmaton_provider::{ApiKey, ResolvedModel};
use plexmaton_session_store::{AutomaticJournal, JournalFile, JournalRecovery};
use tokio::sync::mpsc;

use super::clock::{SystemWallClock, WallClock};
use super::{
    AfterCommit, JournalWriter, LiveRuntime, ModelDriver, ToolTasks, journal::JournalStore,
};
use crate::{
    HttpSetupError, JournalTailRecovery, NativeToolCatalog, RuntimeError, SessionRecovery,
    http::ProviderHttp,
};

const MODEL_SIGNAL_CAPACITY: usize = 32;

impl LiveRuntime {
    /// Validates HTTP ownership and announces one idle live agent without touching the network.
    pub fn provider(
        agent_id: AgentId,
        label: impl Into<String>,
        model: ResolvedModel,
        key: ApiKey,
        tools: NativeToolCatalog,
    ) -> Result<Self, RuntimeError> {
        if !tools.matches_api_key_environment(model.api_key_env()) {
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
        journal: JournalFile,
    ) -> Result<Self, RuntimeError> {
        let metadata = journal.journal().metadata().clone();
        Self::provider_with_new_store(
            agent_id,
            label.into(),
            model,
            key,
            tools,
            metadata,
            Box::new(journal),
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
            Box::new(journal),
        )
        .await
    }

    async fn provider_with_new_store(
        agent_id: AgentId,
        label: String,
        model: ResolvedModel,
        key: ApiKey,
        tools: NativeToolCatalog,
        metadata: SessionMetadata,
        store: Box<dyn JournalStore>,
    ) -> Result<Self, RuntimeError> {
        if !tools.matches_api_key_environment(model.api_key_env()) {
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
        Self::with_driver_store_and_clock(agent_id, label, driver, tools, metadata, store, clock)
            .await
    }

    /// Rebuilds one live owner from an existing journal and settles work orphaned by process death.
    pub async fn provider_with_resumed_journal(
        agent_id: AgentId,
        model: ResolvedModel,
        key: ApiKey,
        tools: NativeToolCatalog,
        journal: JournalFile,
    ) -> Result<(Self, SessionRecovery), RuntimeError> {
        if !tools.matches_api_key_environment(model.api_key_env()) {
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
        let recovery = journal.recovery().clone();
        let agent = Agent::from_journal(
            agent_id.clone(),
            journal.journal().clone(),
            TurnBudget::default(),
            ApprovalPolicy::default(),
        )
        .map_err(RuntimeError::JournalProjection)?;
        Self::with_resumed_driver_and_store(
            agent_id,
            agent,
            driver,
            tools,
            Box::new(journal),
            recovery,
            clock,
        )
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

    pub(super) fn with_driver_and_clock(
        agent_id: AgentId,
        label: String,
        driver: Arc<dyn ModelDriver>,
        tools: NativeToolCatalog,
        clock: Arc<dyn WallClock>,
    ) -> Self {
        let created_at_unix_ms = clock.now();
        let session_id = SessionId::new(format!("{agent_id}-session"))
            .unwrap_or_else(|error| unreachable!("a formatted identity is valid: {error}"));
        let metadata = SessionMetadata::new(session_id, created_at_unix_ms);
        let (signals, signal_rx) = mpsc::channel(MODEL_SIGNAL_CAPACITY);
        let mut runtime = Self {
            agent_id: agent_id.clone(),
            agent: Agent::for_session(
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
            deferred_model_call: None,
            tools: ToolTasks::new(tools),
            report: crate::DispatchReport::default(),
            journal: None,
            pending_commit: None,
            after_commit: None,
            pending_inputs: VecDeque::new(),
            preparing_input: None,
            journal_failed: false,
            shutting_down: false,
            clock,
        };
        let announced = runtime.agent.announce(label);
        runtime
            .apply_ready_reaction(announced)
            .unwrap_or_else(|_| unreachable!("announcing an idle agent starts no outside work"));
        runtime
    }

    pub(super) async fn with_driver_store_and_clock(
        agent_id: AgentId,
        label: String,
        driver: Arc<dyn ModelDriver>,
        tools: NativeToolCatalog,
        metadata: SessionMetadata,
        store: Box<dyn JournalStore>,
        clock: Arc<dyn WallClock>,
    ) -> Result<Self, RuntimeError> {
        let (signals, signal_rx) = mpsc::channel(MODEL_SIGNAL_CAPACITY);
        let mut runtime = Self {
            agent_id: agent_id.clone(),
            agent: Agent::for_session(
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
            deferred_model_call: None,
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
            shutting_down: false,
            clock,
        };
        let reaction = runtime.agent.announce(label);
        runtime.begin_transition(reaction, Vec::new(), AfterCommit::None)?;
        runtime.finish_transition().await?;
        Ok(runtime)
    }

    async fn with_resumed_driver_and_store(
        agent_id: AgentId,
        mut agent: Agent,
        driver: Arc<dyn ModelDriver>,
        tools: NativeToolCatalog,
        store: Box<dyn JournalStore>,
        tail_recovery: JournalRecovery,
        clock: Arc<dyn WallClock>,
    ) -> Result<(Self, SessionRecovery), RuntimeError> {
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
            deferred_model_call: None,
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
            shutting_down: false,
            clock,
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
            SessionRecovery {
                tail,
                interrupted_turn,
            },
        ))
    }
}
