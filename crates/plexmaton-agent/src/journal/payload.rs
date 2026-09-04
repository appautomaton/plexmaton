use plexmaton_core::{
    AgentId, AgentStatus, ArtifactId, AttentionId, AttentionRequest, MailId, ToolCallId,
    ToolCallStatus, ToolPresentation, TranscriptItemId, TurnId,
};
use serde::{Deserialize, Serialize};

use crate::{AssistantOutput, ModelStepId, ToolOutcome};

pub(crate) const PROCESS_RECOVERY_MESSAGE: &str =
    "unfinished turn was interrupted during process recovery";

/// One canonical session fact from which model and screen projections are derived (JRN-5).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JournalEntryPayload {
    /// A new agent became visible.
    AgentCreated {
        agent_id: AgentId,
        label: String,
        status: AgentStatus,
    },
    /// A non-terminal status change scoped to one already-open turn (TIM-1).
    TurnStatusChanged {
        agent_id: AgentId,
        turn_id: TurnId,
        status: crate::ActiveTurnStatus,
    },
    /// Initial user input and the turn boundary it starts atomically (TIM-1).
    TurnStarted {
        agent_id: AgentId,
        item_id: TranscriptItemId,
        turn_id: TurnId,
        text: String,
        accepted_at: crate::UnixMillis,
        opened_at: crate::UnixMillis,
    },
    /// User steering claimed by an already-open turn boundary (TIM-1, LOOP-6).
    SteeringAccepted {
        agent_id: AgentId,
        item_id: TranscriptItemId,
        turn_id: TurnId,
        text: String,
        accepted_at: crate::UnixMillis,
    },
    /// One complete ordered model output, including calls and private replay attachments.
    AssistantOutput {
        agent_id: AgentId,
        step_id: ModelStepId,
        output: AssistantOutput,
    },
    /// First visible lifecycle state for a call already declared by `AssistantOutput`.
    ToolCallRequested {
        agent_id: AgentId,
        call_id: ToolCallId,
        presentation: ToolPresentation,
    },
    /// One later state of a known tool call.
    ToolCallChanged {
        agent_id: AgentId,
        call_id: ToolCallId,
        item_revision: u64,
        status: ToolCallStatus,
        presentation: ToolPresentation,
        /// Present exactly once on the terminal transition that supplies the model result.
        outcome: Option<ToolOutcome>,
    },
    /// A user decision became necessary.
    AttentionRequested {
        agent_id: AgentId,
        attention_id: AttentionId,
        request: AttentionRequest,
    },
    /// A prior user decision request stopped blocking.
    AttentionResolved {
        agent_id: AgentId,
        attention_id: AttentionId,
    },
    /// Typed mail was delivered and remains owned by its producer.
    MailDelivered {
        item_id: TranscriptItemId,
        mail_id: MailId,
        from: AgentId,
        to: AgentId,
        summary: String,
    },
    /// A durable work product became visible.
    ArtifactAnnounced {
        agent_id: AgentId,
        item_id: TranscriptItemId,
        artifact_id: ArtifactId,
        label: String,
        pointer: String,
    },
    /// A visible non-blocking warning.
    RuntimeWarning {
        agent_id: AgentId,
        item_id: TranscriptItemId,
        message: String,
    },
    /// A visible failed operation.
    RuntimeError {
        agent_id: AgentId,
        item_id: TranscriptItemId,
        message: String,
    },
    /// A prior process disappeared while this agent still owned an open turn.
    TurnInterruptedByRecovery {
        agent_id: AgentId,
        item_id: TranscriptItemId,
    },
}
