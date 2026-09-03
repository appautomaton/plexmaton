//! One live agent, its bounded provider channel, and every retained outside operation.

use std::{collections::VecDeque, sync::Arc};

use plexmaton_agent::{
    Agent, Effect, Input, ModelCall, ModelError, ModelEvent, ModelStepId, Reaction,
};
use plexmaton_core::{AgentId, SessionEventEnvelope, TokenUsage};
use plexmaton_provider::{ApiKey, ProviderProfile};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{DispatchReport, HttpSetupError, NativeToolCatalog, RuntimeError, http::OpenAiHttp};

mod model;
mod terminal;
mod tools;

use model::RetainedModelFuture;
pub(crate) use model::{ModelDriver, ModelSignal};
use terminal::QueuedTerminal;
use tools::{ToolResolution, ToolTasks};

const MODEL_SIGNAL_CAPACITY: usize = 32;

struct ActiveModel {
    step_id: ModelStepId,
    cancellation: CancellationToken,
    future: RetainedModelFuture,
    usage_reported: bool,
    terminal: Option<QueuedTerminal>,
}

/// Owner of one live agent and every asynchronous operation it starts (LIVE-1).
pub struct LiveRuntime {
    agent_id: AgentId,
    agent: Agent,
    driver: Arc<dyn ModelDriver>,
    pending: VecDeque<SessionEventEnvelope>,
    signals: mpsc::Sender<ModelSignal>,
    signal_rx: mpsc::Receiver<ModelSignal>,
    active: Option<ActiveModel>,
    tools: ToolTasks,
    report: DispatchReport,
    shutting_down: bool,
}

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

    fn with_driver(
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
            report: DispatchReport::default(),
            shutting_down: false,
        };
        let announced = runtime.agent.announce(label);
        runtime
            .apply_reaction(announced)
            .unwrap_or_else(|_| unreachable!("announcing an idle agent starts no outside work"));
        runtime
    }

    /// Gives one addressed input to the owned agent and performs every resulting effect.
    pub async fn submit(
        &mut self,
        to: AgentId,
        input: Input,
    ) -> Result<DispatchReport, RuntimeError> {
        if self.shutting_down {
            return Err(RuntimeError::ShuttingDown);
        }
        if to != self.agent_id {
            return Err(RuntimeError::WrongAgent {
                expected: self.agent_id.clone(),
                received: to,
            });
        }
        let interrupted = matches!(input, Input::Interrupted);
        if interrupted {
            self.supply_missing_usage()?;
        }
        let reaction = self.agent.handle(input);
        self.apply_reaction(reaction)?;
        if interrupted {
            self.cancel_active().await?;
            self.tools.cancel_and_join().await?;
        }
        Ok(self.take_report())
    }

    /// Returns an event already produced without waiting for provider traffic.
    pub fn try_next_event(&mut self) -> Option<SessionEventEnvelope> {
        self.pending.pop_front()
    }

    /// Waits cancellation-safely for the next semantic event.
    pub async fn next_event(&mut self) -> Result<Option<SessionEventEnvelope>, RuntimeError> {
        loop {
            if let Some(event) = self.try_next_event() {
                return Ok(Some(event));
            }
            if self.shutting_down && !self.has_active_work() {
                return Ok(None);
            }
            match self.wait_for_work().await {
                WaitOutcome::Signal(Some(signal)) => self.apply_signal(signal)?,
                WaitOutcome::Signal(None) => return Ok(None),
                WaitOutcome::ModelEnded(result) => self.model_ended(result)?,
                WaitOutcome::Tool(result) => {
                    if let Some(resolution) = result? {
                        self.apply_tool_resolution(resolution)?;
                    }
                }
            }
        }
    }

    /// Begins orderly shutdown, settles the agent first, then cancels and joins all owned work.
    ///
    /// Cancellation of this future does not make shutdown look complete: calling it again resumes
    /// the retained provider and tool cleanup.
    pub async fn shutdown(&mut self) -> Result<DispatchReport, RuntimeError> {
        if !self.shutting_down {
            self.shutting_down = true;
            self.supply_missing_usage()?;
            let reaction = self.agent.handle(Input::ShuttingDown);
            self.apply_reaction(reaction)?;
        }
        let provider = self.cancel_active().await;
        let tools = self.tools.cancel_and_join().await;
        provider?;
        tools?;
        Ok(self.take_report())
    }

    /// Whether this runtime still owns a provider operation.
    #[must_use]
    pub fn has_active_model(&self) -> bool {
        self.active.is_some()
    }

    /// Whether this runtime still owns provider, admission, or execution work.
    #[must_use]
    pub fn has_active_work(&self) -> bool {
        self.active.is_some() || !self.tools.is_empty()
    }

    /// Takes non-event delivery results accumulated while provider traffic was processed.
    pub fn take_report(&mut self) -> DispatchReport {
        std::mem::take(&mut self.report)
    }

    async fn wait_for_work(&mut self) -> WaitOutcome {
        match (self.active.as_mut(), self.tools.is_empty()) {
            (Some(active), false) => tokio::select! {
                biased;
                signal = self.signal_rx.recv() => WaitOutcome::Signal(signal),
                ended = &mut active.future => WaitOutcome::ModelEnded(ended),
                tool = self.tools.next() => WaitOutcome::Tool(tool),
            },
            (Some(active), true) => tokio::select! {
                biased;
                signal = self.signal_rx.recv() => WaitOutcome::Signal(signal),
                ended = &mut active.future => WaitOutcome::ModelEnded(ended),
            },
            (None, false) => tokio::select! {
                biased;
                signal = self.signal_rx.recv() => WaitOutcome::Signal(signal),
                tool = self.tools.next() => WaitOutcome::Tool(tool),
            },
            (None, true) => WaitOutcome::Signal(self.signal_rx.recv().await),
        }
    }

    fn apply_signal(&mut self, signal: ModelSignal) -> Result<(), RuntimeError> {
        match signal {
            ModelSignal::Event { step_id, event } => {
                if self.active_matches(&step_id)
                    && matches!(event, ModelEvent::Usage(_))
                    && let Some(active) = &mut self.active
                {
                    active.usage_reported = true;
                }
                let reaction = self.agent.handle(Input::Streamed { step_id, event });
                self.apply_reaction(reaction)
            }
            ModelSignal::Terminal { step_id, event } => {
                let terminal = QueuedTerminal::Streamed(event);
                if self.active_matches(&step_id) {
                    self.queue_terminal(step_id, terminal)
                } else {
                    self.deliver_terminal(step_id, terminal)
                }
            }
            ModelSignal::Failed { step_id, error } => {
                let terminal = QueuedTerminal::Failed(error);
                if self.active_matches(&step_id) {
                    self.queue_terminal(step_id, terminal)
                } else {
                    self.deliver_terminal(step_id, terminal)
                }
            }
        }
    }

    fn model_ended(&mut self, result: Result<(), ()>) -> Result<(), RuntimeError> {
        self.drain_ready_signals()?;
        let Some(active) = self.active.take() else {
            return Ok(());
        };
        self.supply_missing_usage_for(active.step_id.clone(), active.usage_reported)?;
        if result.is_err() {
            return self.fail_owned_step(active.step_id, "provider future terminated unexpectedly");
        }
        match active.terminal {
            Some(terminal) => self.deliver_terminal(active.step_id, terminal),
            None => self.fail_owned_step(
                active.step_id,
                "provider future ended without terminal output",
            ),
        }
    }

    fn fail_owned_step(
        &mut self,
        step_id: ModelStepId,
        message: &'static str,
    ) -> Result<(), RuntimeError> {
        let reaction = self.agent.handle(Input::Failed {
            step_id,
            error: ModelError::Transport {
                message: message.to_owned(),
            },
        });
        self.apply_reaction(reaction)
    }

    fn apply_reaction(&mut self, reaction: Reaction) -> Result<(), RuntimeError> {
        let mut reactions = VecDeque::from([reaction]);
        while let Some(mut reaction) = reactions.pop_front() {
            self.pending.extend(reaction.events);
            self.report.undelivered.append(&mut reaction.undelivered);
            self.report
                .unresolved_approvals
                .append(&mut reaction.unresolved_approvals);
            self.report
                .undelivered_model
                .append(&mut reaction.undelivered_model);
            for effect in reaction.effects {
                match effect {
                    Effect::CallModel(call) => self.spawn_model(call)?,
                    Effect::AdmitTool(request) => self.tools.start_admission(request)?,
                    Effect::RunTool(call) => self.tools.start_execution(call)?,
                }
            }
        }
        Ok(())
    }

    fn spawn_model(&mut self, call: ModelCall) -> Result<(), RuntimeError> {
        if self.shutting_down {
            return Err(RuntimeError::ShuttingDown);
        }
        if let Some(active) = &self.active {
            return Err(RuntimeError::ModelAlreadyActive {
                active: active.step_id.clone(),
                requested: call.step_id,
            });
        }
        if !self.tools.is_empty() {
            return Err(RuntimeError::ModelStartedWithToolWork);
        }
        let step_id = call.step_id.clone();
        let cancellation = CancellationToken::new();
        let future = self
            .driver
            .drive(call, self.signals.clone(), cancellation.child_token());
        self.active = Some(ActiveModel {
            step_id,
            cancellation,
            future: RetainedModelFuture::new(future),
            usage_reported: false,
            terminal: None,
        });
        Ok(())
    }

    fn supply_missing_usage(&mut self) -> Result<(), RuntimeError> {
        let missing = self
            .active
            .as_ref()
            .map(|active| (active.step_id.clone(), active.usage_reported));
        if let Some((step_id, reported)) = missing {
            self.supply_missing_usage_for(step_id, reported)?;
        }
        Ok(())
    }

    fn supply_missing_usage_for(
        &mut self,
        step_id: ModelStepId,
        reported: bool,
    ) -> Result<(), RuntimeError> {
        if reported {
            return Ok(());
        }
        let reaction = self.agent.handle(Input::Streamed {
            step_id,
            event: ModelEvent::Usage(TokenUsage::Unavailable),
        });
        self.apply_reaction(reaction)?;
        if let Some(active) = &mut self.active {
            active.usage_reported = true;
        }
        Ok(())
    }

    fn active_matches(&self, step_id: &ModelStepId) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| active.step_id == *step_id)
    }

    fn drain_ready_signals(&mut self) -> Result<(), RuntimeError> {
        while let Ok(signal) = self.signal_rx.try_recv() {
            self.apply_signal(signal)?;
        }
        Ok(())
    }

    fn apply_tool_resolution(&mut self, resolution: ToolResolution) -> Result<(), RuntimeError> {
        let reaction = match resolution {
            ToolResolution::Admission(outcome) => {
                self.agent.handle(Input::ToolAdmissionResolved(outcome))
            }
            ToolResolution::Execution { call_id, outcome } => {
                self.agent.handle(Input::ToolFinished { call_id, outcome })
            }
        };
        self.apply_reaction(reaction)
    }
}

impl Drop for LiveRuntime {
    fn drop(&mut self) {
        if let Some(active) = self.active.take() {
            active.cancellation.cancel();
        }
    }
}

enum WaitOutcome {
    Signal(Option<ModelSignal>),
    ModelEnded(Result<(), ()>),
    Tool(Result<Option<ToolResolution>, RuntimeError>),
}

#[cfg(test)]
mod tests;
