use std::{collections::VecDeque, sync::Arc};

use plexmaton_agent::{Agent, ApprovalPolicy, TurnBudget};
use plexmaton_core::{AgentId, SessionId};
use plexmaton_provider::{ApiKey, ProviderProfile};
use plexmaton_session_store::{JournalFile, JournalRecovery};
use tokio::sync::mpsc;

use super::{
    AfterCommit, JournalWriter, LiveRuntime, ModelDriver, ToolTasks, journal::JournalStore,
};
use crate::{
    HttpSetupError, JournalTailRecovery, NativeToolCatalog, RuntimeError, SessionRecovery,
    http::OpenAiHttp,
};

const MODEL_SIGNAL_CAPACITY: usize = 32;

impl LiveRuntime {
    /// Validates HTTP ownership and announces one idle live agent without touching the network.
    pub fn openai(
        agent_id: AgentId,
        label: impl Into<String>,
        profile: ProviderProfile,
        key: ApiKey,
        tools: NativeToolCatalog,
    ) -> Result<Self, HttpSetupError> {
        if !tools.matches_api_key_environment(profile.api_key_env()) {
            return Err(HttpSetupError::ToolCredentialEnvironmentMismatch);
        }
        let definitions = tools.provider_definitions();
        let driver = Arc::new(OpenAiHttp::new(profile, key, definitions)?);
        Ok(Self::with_driver(agent_id, label.into(), driver, tools))
    }

    /// Opens one new live agent whose canonical reactions must reach an empty journal first.
    pub async fn openai_with_fresh_journal(
        agent_id: AgentId,
        label: impl Into<String>,
        profile: ProviderProfile,
        key: ApiKey,
        tools: NativeToolCatalog,
        journal: JournalFile,
    ) -> Result<Self, RuntimeError> {
        if !tools.matches_api_key_environment(profile.api_key_env()) {
            return Err(HttpSetupError::ToolCredentialEnvironmentMismatch.into());
        }
        let definitions = tools.provider_definitions();
        let driver = Arc::new(OpenAiHttp::new(profile, key, definitions)?);
        let session_id = journal.journal().session_id().clone();
        Self::with_driver_and_store(
            agent_id,
            label.into(),
            driver,
            tools,
            session_id,
            Box::new(journal),
        )
        .await
    }

    /// Rebuilds one live owner from an existing journal and settles work orphaned by process death.
    pub async fn openai_with_resumed_journal(
        agent_id: AgentId,
        profile: ProviderProfile,
        key: ApiKey,
        tools: NativeToolCatalog,
        journal: JournalFile,
    ) -> Result<(Self, SessionRecovery), RuntimeError> {
        if !tools.matches_api_key_environment(profile.api_key_env()) {
            return Err(HttpSetupError::ToolCredentialEnvironmentMismatch.into());
        }
        let definitions = tools.provider_definitions();
        let driver = Arc::new(OpenAiHttp::new(profile, key, definitions)?);
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
        )
        .await
    }

    pub(super) fn with_driver(
        agent_id: AgentId,
        label: String,
        driver: Arc<dyn ModelDriver>,
        tools: NativeToolCatalog,
    ) -> Self {
        let (signals, signal_rx) = mpsc::channel(MODEL_SIGNAL_CAPACITY);
        let mut runtime = Self {
            agent_id: agent_id.clone(),
            agent: Agent::new(agent_id),
            driver,
            pending: VecDeque::new(),
            signals,
            signal_rx,
            active: None,
            tools: ToolTasks::new(tools),
            report: crate::DispatchReport::default(),
            journal: None,
            pending_commit: None,
            after_commit: None,
            journal_failed: false,
            shutting_down: false,
        };
        let announced = runtime.agent.announce(label);
        runtime
            .apply_ready_reaction(announced)
            .unwrap_or_else(|_| unreachable!("announcing an idle agent starts no outside work"));
        runtime
    }

    pub(super) async fn with_driver_and_store(
        agent_id: AgentId,
        label: String,
        driver: Arc<dyn ModelDriver>,
        tools: NativeToolCatalog,
        session_id: SessionId,
        store: Box<dyn JournalStore>,
    ) -> Result<Self, RuntimeError> {
        let (signals, signal_rx) = mpsc::channel(MODEL_SIGNAL_CAPACITY);
        let mut runtime = Self {
            agent_id: agent_id.clone(),
            agent: Agent::for_session(
                agent_id,
                session_id,
                TurnBudget::default(),
                ApprovalPolicy::default(),
            ),
            driver,
            pending: VecDeque::new(),
            signals,
            signal_rx,
            active: None,
            tools: ToolTasks::new(tools),
            report: crate::DispatchReport::default(),
            journal: Some(
                JournalWriter::spawn(store).map_err(|_| RuntimeError::JournalWriterUnavailable)?,
            ),
            pending_commit: None,
            after_commit: None,
            journal_failed: false,
            shutting_down: false,
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
    ) -> Result<(Self, SessionRecovery), RuntimeError> {
        let projection = agent
            .rebuild_projection()
            .unwrap_or_else(|_| unreachable!("a newly restored agent has no transient work"));
        let pending = VecDeque::from(projection.events().to_vec());
        let recovered = agent.recover_after_process_death();
        let (signals, signal_rx) = mpsc::channel(MODEL_SIGNAL_CAPACITY);
        let mut runtime = Self {
            agent_id,
            agent,
            driver,
            pending,
            signals,
            signal_rx,
            active: None,
            tools: ToolTasks::new(tools),
            report: crate::DispatchReport::default(),
            journal: Some(
                JournalWriter::spawn(store).map_err(|_| RuntimeError::JournalWriterUnavailable)?,
            ),
            pending_commit: None,
            after_commit: None,
            journal_failed: false,
            shutting_down: false,
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
