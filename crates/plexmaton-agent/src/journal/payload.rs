use plexmaton_core::{
    AgentId, AgentStatus, ArtifactId, AttentionId, AttentionRequest, MailId, TokenUsage,
    ToolCallId, ToolCallStatus, ToolPresentation, TranscriptItemId, TranscriptRole, TurnId,
};
use serde::{Deserialize, Serialize};

use crate::{ProviderReplay, ToolCall, ToolOutcome};

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
    /// An agent changed lifecycle state.
    AgentStatusChanged {
        agent_id: AgentId,
        status: AgentStatus,
    },
    /// Provider-reported aggregate usage changed for one turn.
    TurnUsageUpdated {
        agent_id: AgentId,
        turn_id: TurnId,
        usage: TokenUsage,
    },
    /// One complete visible message.
    Message {
        agent_id: AgentId,
        item_id: TranscriptItemId,
        role: TranscriptRole,
        text: String,
    },
    /// Exact provider-owned replay metadata, never projected to the screen.
    ProviderReplay(ProviderReplay),
    /// A model-requested tool call at its first visible state.
    ToolCallRequested {
        agent_id: AgentId,
        item_id: TranscriptItemId,
        call: ToolCall,
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
