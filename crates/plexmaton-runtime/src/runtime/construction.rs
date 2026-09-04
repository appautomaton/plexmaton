use std::{collections::VecDeque, sync::Arc};

use plexmaton_agent::{Agent, ApprovalPolicy, TurnBudget};
use plexmaton_core::{AgentId, SessionId};
use plexmaton_provider::{ApiKey, ProviderProfile};
use plexmaton_session_store::JournalFile;
use tokio::sync::mpsc;

use super::{
    AfterCommit, JournalWriter, LiveRuntime, ModelDriver, ToolTasks, journal::JournalStore,
};
use crate::{HttpSetupError, NativeToolCatalog, RuntimeError, http::OpenAiHttp};

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
}
