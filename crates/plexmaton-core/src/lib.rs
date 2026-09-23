//! Semantic contracts shared by Plexmaton runtimes and projections.
//!
//! Provider wire events, terminal input, persistence records and animation ticks do not belong
//! here; runtime facts cross this boundary as one typed session-event vocabulary.
//!
//! Every field carries an invariant that a producer or a projection can get wrong, so this crate
//! documents them; the rest of the workspace does not enforce field-level documentation.
#![deny(missing_docs)]

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

mod conversation_tree;
mod permissions;
mod reasoning;
mod server_tool;
mod transcript;
mod tree_edit;
mod tree_snapshot;
mod tree_source;
mod usage;

pub use conversation_tree::{TreeNavigation, TreeNavigationTarget, TreeOrigin, TreeRevision};
pub use permissions::{
    ApprovalDecision, ApprovalReason, PermissionChangeError, PermissionOfferId, PermissionRevision,
    PermissionScope, PermissionScopes, RememberPermissionOffer,
};
pub use permissions::{
    NativeFilePreset, PermissionAction, PermissionGrantView, PermissionIntent,
    PermissionRuleAction, PermissionRuleView, PermissionStateView, ProjectConfigurationView,
    ProjectPermissionSource, SavedProjectPermission,
};
pub use tree_edit::{MAX_TREE_LABEL_BYTES, TreeEdit, TreeEditAction, TreeLabel, TreeLabelError};
pub use tree_snapshot::{
    TreeHead, TreePreview, TreeRewindEligibility, TreeRow, TreeRowKind, TreeSnapshot,
    TreeSnapshotError, TreeSnapshotLimit,
};
pub use tree_source::{MAX_TREE_SOURCE_BYTES, TreeSourceError, TreeSourceRequest};

pub use reasoning::ReasoningEffort;
pub use server_tool::{ServerTool, ServerToolAction, ServerToolCall, ServerToolStatus};
pub use transcript::{
    CommandInvocation, ToolCallStatus, ToolDetail, ToolPresentation, TranscriptRole,
};
pub use usage::{TokenCounts, TokenUsage};

impl ConversationEvent {
    /// The conversation whose projection holds this fact.
    #[must_use]
    pub fn agent(&self) -> &AgentId {
        match self {
            Self::AgentCreated { agent_id, .. }
            | Self::AgentStatusChanged { agent_id, .. }
            | Self::TurnUsageUpdated { agent_id, .. }
            | Self::TranscriptItemStarted { agent_id, .. }
            | Self::TranscriptDelta { agent_id, .. }
            | Self::TranscriptItemFinalized { agent_id, .. }
            | Self::ToolCallChanged { agent_id, .. }
            | Self::ServerToolStarted { agent_id, .. }
            | Self::ServerToolCalled { agent_id, .. }
            | Self::AttentionRequested { agent_id, .. }
            | Self::AttentionResolved { agent_id, .. }
            | Self::TaskAssigned { agent_id, .. }
            | Self::MailDelivered { agent_id, .. }
            | Self::HandoffCompleted { agent_id, .. }
            | Self::ArtifactAnnounced { agent_id, .. }
            | Self::RuntimeWarning { agent_id, .. }
            | Self::RuntimeError { agent_id, .. } => agent_id,
        }
    }

    /// The conversation whose projection holds this fact, so a reader can re-address it.
    ///
    /// A delegated child numbers its own conversation and names itself by its own agent identity.
    /// A root that shows what that child did is showing it inside the root's projection, under the
    /// name the roster gave it, so every forwarded fact is re-addressed here rather than at each
    /// call site — and a new variant is a compile error until it says which conversation it is in.
    pub fn agent_mut(&mut self) -> &mut AgentId {
        match self {
            Self::AgentCreated { agent_id, .. }
            | Self::AgentStatusChanged { agent_id, .. }
            | Self::TurnUsageUpdated { agent_id, .. }
            | Self::TranscriptItemStarted { agent_id, .. }
            | Self::TranscriptDelta { agent_id, .. }
            | Self::TranscriptItemFinalized { agent_id, .. }
            | Self::ToolCallChanged { agent_id, .. }
            | Self::ServerToolStarted { agent_id, .. }
            | Self::ServerToolCalled { agent_id, .. }
            | Self::AttentionRequested { agent_id, .. }
            | Self::AttentionResolved { agent_id, .. }
            | Self::TaskAssigned { agent_id, .. }
            | Self::MailDelivered { agent_id, .. }
            | Self::HandoffCompleted { agent_id, .. }
            | Self::ArtifactAnnounced { agent_id, .. }
            | Self::RuntimeWarning { agent_id, .. }
            | Self::RuntimeError { agent_id, .. } => agent_id,
        }
    }
}

/// Rejected stable identifier input.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum IdError {
    /// Stable identities must not be empty or whitespace-only.
    #[error("{kind} must not be empty")]
    Empty {
        /// Human-readable name of the identifier kind that was rejected.
        kind: &'static str,
    },
}

macro_rules! stable_id {
    ($name:ident, $kind:literal) => {
        #[doc = concat!("Stable identity for a ", $kind, ".")]
        ///
        /// The constructor is the only way in, deserialization included. A derived `Deserialize`
        /// would have let `""` through the inner string while `new` still refused it, which is an
        /// invariant that holds only on the paths that happen to use the constructor.
        #[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Creates an identifier after enforcing its non-empty invariant.
            pub fn new(value: impl Into<String>) -> Result<Self, IdError> {
                let value = value.into();
                if value.trim().is_empty() {
                    return Err(IdError::Empty { kind: $kind });
                }
                Ok(Self(value))
            }

            /// Returns the stable external representation.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = IdError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        // Written out rather than derived, because `serde(transparent)` and `serde(try_from)`
        // cannot both describe the same type: the wire form is a bare string either way, and only
        // this spelling puts the constructor on the decoding path.
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(serde::de::Error::custom)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

stable_id!(AgentId, "agent id");
stable_id!(ApprovalId, "approval id");
stable_id!(ArtifactId, "artifact id");
stable_id!(AttentionId, "attention id");
stable_id!(MailId, "mail id");
stable_id!(CollaborationId, "collaboration id");
stable_id!(CollaborationItemId, "collaboration item id");
stable_id!(DelegationId, "delegation id");
stable_id!(ConversationEntryId, "conversation entry id");
stable_id!(CodingSessionId, "coding session id");
stable_id!(PermissionGrantId, "permission grant id");
stable_id!(ProjectPermissionStoreId, "project permission store id");
stable_id!(ConversationId, "conversation id");
stable_id!(HeadName, "session head name");
stable_id!(JournalRecordId, "journal record id");
stable_id!(ToolDefinitionId, "tool definition id");
stable_id!(ToolCallId, "tool call id");
stable_id!(TranscriptItemId, "transcript item id");
stable_id!(TurnId, "turn id");

/// A personal project's durable policy version; a reset changes its store identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProjectPermissionRevision {
    /// No project policy has ever been written under the stable lock.
    Absent,
    /// A validated store incarnation and monotonic mutation sequence.
    Present {
        /// Prevents a reset from making an old revision current again.
        store: ProjectPermissionStoreId,
        /// Last acknowledged mutation in this incarnation.
        sequence: u64,
    },
}

/// Monotonic sequence assigned by one semantic event producer.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct EventSequence(u64);

impl EventSequence {
    /// Creates a sequence value. Producers conventionally begin at one.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the numeric sequence.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Lifecycle visible to the Phase 00 interface.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    /// Admitted no work and is waiting for input.
    Idle,
    /// Producing model output or running a tool right now.
    Running,
    /// Blocked on a tool, a descendant agent, or a pending user decision.
    Waiting,
    /// Finished its assigned work; the session remains inspectable.
    Completed,
    /// Stopped by an error it could not recover from.
    Failed,
    /// Stopped by an explicit user or parent decision rather than by failure.
    Cancelled,
}

/// A typed capability an admitted tool call may exercise.
///
/// Capabilities compose: a command may both spawn a process and write files. Tool names and prompt
/// prose are not authority, so approval and policy reason over this vocabulary instead.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCapability {
    /// Mutate the authenticated collaboration owner through its bounded command boundary.
    Collaboration,
    /// Read files through the workspace filesystem boundary.
    FileRead,
    /// Create, replace or remove files through the workspace filesystem boundary.
    FileWrite,
    /// Start a host process.
    ProcessSpawn,
}

/// Why a background agent needs the user's attention.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionKind {
    /// The agent cannot proceed until the user permits an action.
    Approval,
    /// The agent can proceed but needs a decision or missing information first.
    Clarification,
}

/// What one Attention item asks the user to resolve.
///
/// The enum prevents a clarification from carrying an approval identity or an approval from
/// arriving without the exact call and capabilities the decision applies to.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttentionRequest {
    /// A tool call is parked until the user permits or denies it.
    Approval {
        /// Identity a decision must echo.
        approval_id: ApprovalId,
        /// Exact model call the request blocks.
        call_id: ToolCallId,
        /// Stable display label of the admitted tool definition.
        tool: String,
        /// Canonical capabilities policy evaluated for this call.
        capabilities: Vec<ToolCapability>,
        /// Bounded explanation of the concrete operation.
        detail: String,
        /// Policy reason supplied by the producer, separate from the operation detail.
        #[serde(default)]
        reason: ApprovalReason,
        /// Optional backend-issued reusable permission; historical display is never authority.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        remember: Option<RememberPermissionOffer>,
    },
    /// The agent needs information rather than permission.
    Clarification {
        /// Bounded question or summary.
        summary: String,
    },
}

impl AttentionRequest {
    /// Returns the presentation category without duplicating it in serialized state.
    #[must_use]
    pub const fn kind(&self) -> AttentionKind {
        match self {
            Self::Approval { .. } => AttentionKind::Approval,
            Self::Clarification { .. } => AttentionKind::Clarification,
        }
    }

    /// Returns the bounded one-line text used by the Attention queue.
    #[must_use]
    pub fn summary(&self) -> &str {
        match self {
            Self::Approval { detail, .. } => detail,
            Self::Clarification { summary } => summary,
        }
    }
}

/// One UI-facing semantic transition.
///
/// This is intentionally not a universal application event: it excludes provider wire data,
/// terminal input, persistence records, and animation ticks.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConversationEvent {
    /// A new agent became visible to the workspace.
    AgentCreated {
        /// Identity the agent keeps for its whole lifetime.
        agent_id: AgentId,
        /// Display label. A routing/display alias, never the durable identity.
        label: String,
        /// Lifecycle state at the moment of creation.
        status: AgentStatus,
    },
    /// An existing agent moved to a new lifecycle state.
    AgentStatusChanged {
        /// Agent whose lifecycle changed.
        agent_id: AgentId,
        /// State the agent moved to.
        status: AgentStatus,
    },
    /// Provider-reported usage for the current turn changed after one model step.
    TurnUsageUpdated {
        /// Agent whose turn consumed the tokens.
        agent_id: AgentId,
        /// Turn the aggregate belongs to.
        turn_id: TurnId,
        /// Checked aggregate across the turn's completed model steps.
        usage: TokenUsage,
    },
    /// A transcript item was opened and may now receive deltas.
    TranscriptItemStarted {
        /// Agent owning the transcript.
        agent_id: AgentId,
        /// Identity used by every later delta and the finalization for this item.
        item_id: TranscriptItemId,
        /// Semantic author of the item.
        role: TranscriptRole,
    },
    /// Text was appended to an open transcript item.
    TranscriptDelta {
        /// Agent owning the transcript.
        agent_id: AgentId,
        /// Item the text belongs to.
        item_id: TranscriptItemId,
        /// Per-item revision. Must be exactly the item's previous revision plus one, so a
        /// projection can detect a lost or duplicated delta without comparing text.
        item_revision: u64,
        /// Exact text to append. Producers must not resend previously delivered text.
        text: String,
    },
    /// A transcript item will receive no further deltas.
    TranscriptItemFinalized {
        /// Agent owning the transcript.
        agent_id: AgentId,
        /// Item being closed.
        item_id: TranscriptItemId,
        /// Per-item revision, continuing the same sequence the deltas used.
        item_revision: u64,
    },
    /// A tool call was created or moved to a new lifecycle state.
    ///
    /// Repeating the entry identity with its next revision updates that tool call in place
    /// (ENT-2); the call identity remains a correlation rather than its transcript position.
    ToolCallChanged {
        /// Agent running the tool.
        agent_id: AgentId,
        /// Transcript position assigned when the call first appeared.
        item_id: TranscriptItemId,
        /// Per-entry revision. The queued first appearance is zero; each transition adds one.
        item_revision: u64,
        /// Identity of the tool call being created or updated.
        call_id: ToolCallId,
        /// Display label for the tool call.
        label: String,
        /// Lifecycle state the tool call moved to.
        status: ToolCallStatus,
        /// Bounded semantic detail used by open and copy presentations.
        presentation: ToolPresentation,
    },
    /// The provider began running a tool on its own side inside one model call.
    ///
    /// Nothing here was queued, admitted or dispatched: the row appears running where the provider
    /// placed the call, and [`Self::ServerToolCalled`] finishes it at the next revision.
    ServerToolStarted {
        /// Agent whose model call the provider is running the tool inside.
        agent_id: AgentId,
        /// Transcript position assigned when the call was first reported.
        item_id: TranscriptItemId,
        /// The tool the provider began running.
        tool: ServerTool,
    },
    /// A tool the provider ran on its own side ended, and this is what it did.
    ///
    /// At revision one it finishes the entry [`Self::ServerToolStarted`] opened; at revision zero
    /// it is the whole entry, which is how a reopened conversation, holding only the finished
    /// call, shows it. It says what the route reported and claims nothing further.
    ServerToolCalled {
        /// Agent whose model call the provider ran the tool inside.
        agent_id: AgentId,
        /// Transcript position the call holds.
        item_id: TranscriptItemId,
        /// Zero for a first appearance, one when it finishes a call that appeared running.
        item_revision: u64,
        /// The tool, what it did with it, and how it ended.
        call: ServerToolCall,
    },
    /// A background agent needs a user decision.
    ///
    /// Delivery must never move focus or open a modal surface; the item joins the Attention queue.
    AttentionRequested {
        /// Agent that needs the decision.
        agent_id: AgentId,
        /// Identity of the queued request.
        attention_id: AttentionId,
        /// Typed request. Its kind and summary are derived rather than stored twice.
        request: AttentionRequest,
    },
    /// A previously requested Attention item no longer blocks its agent.
    AttentionResolved {
        /// Agent whose request was resolved.
        agent_id: AgentId,
        /// Exact queued request to remove.
        attention_id: AttentionId,
    },
    /// Main assigned or reassigned the standing task of one delegated session.
    ///
    /// Announced once per side, like mail: the delegator's conversation shows what it asked for and
    /// the worker's shows what it was asked. A revision of the same delegation is a new item rather
    /// than an update, because what the child was asked at the time is what the reader is looking
    /// for — the current task alone would erase the history of having changed it.
    TaskAssigned {
        /// Whose conversation this item belongs to: the delegator's copy, or the worker's.
        agent_id: AgentId,
        /// Transcript position assigned to this assignment.
        item_id: TranscriptItemId,
        /// The session that assigned it, named on both sides.
        from: AgentId,
        /// The delegated session it was assigned to.
        to: AgentId,
        /// The task as written. Bounded by the collaboration log that holds it.
        task: String,
    },
    /// Typed mail was delivered from one session to another.
    ///
    /// One letter reaches both conversations, so it is announced once per side: the sender's
    /// conversation shows what it sent and the recipient's shows what arrived, each its own
    /// transcript item over the same `mail_id`. Without `agent_id` the owner could only be read
    /// off `from`, and a recipient's conversation had no way to say a letter had come at all.
    MailDelivered {
        /// Whose conversation this item belongs to: the sender's copy, or the recipient's.
        agent_id: AgentId,
        /// Transcript position assigned to this delivery.
        item_id: TranscriptItemId,
        /// Identity of the delivered mail, shared by both sides' items.
        mail_id: MailId,
        /// Sending agent, named on both sides because attribution is the letter's, not the item's.
        from: AgentId,
        /// Receiving agent.
        to: AgentId,
        /// Bounded summary. Bulk findings stay in artifacts or the sender's session.
        summary: String,
    },
    /// Durable transfer of one delegated Conversation from Main to User control.
    ///
    /// The same Handoff appears once in the root and child conversations under distinct item
    /// identities; `agent_id` names which side owns this entry.
    HandoffCompleted {
        /// Conversation whose ordered transcript owns this side of the Handoff.
        agent_id: AgentId,
        /// Transcript position assigned to this side of the Handoff.
        item_id: TranscriptItemId,
        /// Delegated agent whose controller became User.
        child: AgentId,
    },
    /// An agent published a durable work product.
    ArtifactAnnounced {
        /// Agent that produced the artifact.
        agent_id: AgentId,
        /// Transcript position assigned to this announcement.
        item_id: TranscriptItemId,
        /// Identity of the artifact.
        artifact_id: ArtifactId,
        /// Display label for the artifact.
        label: String,
        /// Stable reference to the content. Copy actions return this, not the display label.
        pointer: String,
    },
    /// The producer reported a condition the user should see but that blocks nothing.
    RuntimeWarning {
        /// Agent whose transcript owns the warning.
        agent_id: AgentId,
        /// Transcript position assigned to this warning.
        item_id: TranscriptItemId,
        /// Human-readable description of the condition.
        message: String,
    },
    /// The producer reported a failed operation in the owning agent's transcript.
    RuntimeError {
        /// Agent whose transcript owns the error.
        agent_id: AgentId,
        /// Transcript position assigned to this error.
        item_id: TranscriptItemId,
        /// Human-readable description of the failure.
        message: String,
    },
}

/// An ordered event at the runtime-to-projection boundary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConversationEventEnvelope {
    /// Position in the producer's monotonic stream, used to detect loss and duplication.
    pub sequence: EventSequence,
    /// The semantic transition being reported.
    pub event: ConversationEvent,
}

#[cfg(test)]
mod tests {
    use super::{
        AgentId, AgentStatus, ApprovalDecision, ApprovalId, AttentionId, AttentionRequest,
        ConversationEvent, ConversationEventEnvelope, EventSequence, IdError, MailId, ServerTool,
        ServerToolAction, ServerToolCall, ServerToolStatus, TokenCounts, TokenUsage, ToolCallId,
        ToolCallStatus, ToolCapability, ToolDetail, ToolPresentation, TranscriptItemId,
        TranscriptRole, TurnId,
    };

    fn agent(value: &str) -> AgentId {
        AgentId::new(value).unwrap_or_else(|error| panic!("invalid fixture: {error}"))
    }

    fn item_id(value: &str) -> TranscriptItemId {
        TranscriptItemId::new(value).unwrap_or_else(|error| panic!("invalid fixture: {error}"))
    }

    #[test]
    fn stable_id_rejects_whitespace() {
        assert_eq!(
            AgentId::new("  \t"),
            Err(IdError::Empty { kind: "agent id" })
        );
    }

    /// An invariant that only holds on the paths that use the constructor is not an invariant.
    ///
    /// This crate is the contract an out-of-process producer writes to, so decoding is one of those
    /// paths. Asserted through a whole event as well as a bare identity, because the event is the
    /// shape a producer actually sends and a nested field is where a bypass would go unnoticed.
    #[test]
    fn an_identity_cannot_be_deserialized_past_its_constructor() {
        assert!(serde_json::from_str::<AgentId>(r#""agent-a""#).is_ok());
        assert!(serde_json::from_str::<AgentId>(r#""""#).is_err());
        assert!(serde_json::from_str::<AgentId>(r#""   ""#).is_err());
        assert!(
            serde_json::from_str::<ConversationEventEnvelope>(
                r#"{"sequence":1,"event":{"type":"agent_created","agent_id":"","label":"x","status":"idle"}}"#
            )
            .is_err(),
            "and the same holds for an identity nested in the event a producer sends"
        );
    }

    /// This crate is the contract a future real runtime has to produce, so its wire form has to
    /// survive a round trip. A renamed variant or a field that stops serializing would otherwise
    /// only surface when a producer outside this workspace fails to be understood.
    #[test]
    fn every_event_variant_survives_a_json_round_trip() {
        let item = TranscriptItemId::new("item-1")
            .unwrap_or_else(|error| panic!("invalid fixture: {error}"));
        let events = [
            ConversationEvent::AgentCreated {
                agent_id: agent("agent-a"),
                label: "Agent A".into(),
                status: AgentStatus::Running,
            },
            ConversationEvent::AgentStatusChanged {
                agent_id: agent("agent-a"),
                status: AgentStatus::Cancelled,
            },
            ConversationEvent::TurnUsageUpdated {
                agent_id: agent("agent-a"),
                turn_id: TurnId::new("turn-1").unwrap_or_else(|error| panic!("fixture: {error}")),
                usage: TokenUsage::Complete(TokenCounts {
                    input: 10,
                    cached_input: Some(2),
                    cache_write_input: Some(1),
                    output: 4,
                    reasoning_output: Some(3),
                    total: 14,
                }),
            },
            ConversationEvent::TranscriptItemStarted {
                agent_id: agent("agent-a"),
                item_id: item.clone(),
                role: TranscriptRole::Assistant,
            },
            ConversationEvent::TranscriptDelta {
                agent_id: agent("agent-a"),
                item_id: item.clone(),
                item_revision: 1,
                // Multi-byte and combining text, because transcripts carry both.
                text: "δ 汉字 e\u{301}\n".into(),
            },
            ConversationEvent::TranscriptItemFinalized {
                agent_id: agent("agent-a"),
                item_id: item,
                item_revision: 2,
            },
            ConversationEvent::ToolCallChanged {
                agent_id: agent("agent-a"),
                item_id: TranscriptItemId::new("item-tool-1")
                    .unwrap_or_else(|error| panic!("invalid fixture: {error}")),
                item_revision: 0,
                call_id: ToolCallId::new("tool-1")
                    .unwrap_or_else(|error| panic!("invalid fixture: {error}")),
                label: "read".into(),
                status: ToolCallStatus::Queued,
                presentation: ToolPresentation {
                    invocation: Some(ToolDetail::Text {
                        source: "path: src/lib.rs".into(),
                        omitted_bytes: 7,
                    }),
                    outcome: Some(ToolDetail::Diff {
                        patch: "-old\n+new\n".into(),
                    }),
                },
            },
            ConversationEvent::ServerToolStarted {
                agent_id: agent("agent-a"),
                item_id: item_id("item-search-1"),
                tool: ServerTool::WebSearch,
            },
            ConversationEvent::ServerToolCalled {
                agent_id: agent("agent-a"),
                item_id: item_id("item-search-1"),
                item_revision: 1,
                call: ServerToolCall {
                    tool: ServerTool::WebSearch,
                    action: ServerToolAction::FindInPage {
                        url: "https://example.test/δ".into(),
                        pattern: "汉字".into(),
                    },
                    status: ServerToolStatus::Failed,
                },
            },
            ConversationEvent::AttentionRequested {
                agent_id: agent("agent-b"),
                attention_id: AttentionId::new("attention-1")
                    .unwrap_or_else(|error| panic!("invalid fixture: {error}")),
                request: AttentionRequest::Approval {
                    reason: crate::ApprovalReason::PermissionRequired,
                    remember: None,
                    approval_id: ApprovalId::new("approval-1")
                        .unwrap_or_else(|error| panic!("invalid fixture: {error}")),
                    call_id: ToolCallId::new("tool-1")
                        .unwrap_or_else(|error| panic!("invalid fixture: {error}")),
                    tool: "write".into(),
                    capabilities: vec![ToolCapability::FileWrite],
                    detail: "approve the write".into(),
                },
            },
            ConversationEvent::AttentionResolved {
                agent_id: agent("agent-b"),
                attention_id: AttentionId::new("attention-1")
                    .unwrap_or_else(|error| panic!("invalid fixture: {error}")),
            },
            ConversationEvent::MailDelivered {
                agent_id: agent("agent-b"),
                item_id: TranscriptItemId::new("item-mail-1")
                    .unwrap_or_else(|error| panic!("invalid fixture: {error}")),
                mail_id: MailId::new("mail-1")
                    .unwrap_or_else(|error| panic!("invalid fixture: {error}")),
                from: agent("agent-b"),
                to: agent("agent-a"),
                summary: "findings".into(),
            },
            ConversationEvent::HandoffCompleted {
                agent_id: agent("agent-a"),
                item_id: TranscriptItemId::new("item-handoff-1")
                    .unwrap_or_else(|error| panic!("invalid fixture: {error}")),
                child: agent("agent-b"),
            },
            ConversationEvent::ArtifactAnnounced {
                agent_id: agent("agent-b"),
                item_id: TranscriptItemId::new("item-artifact-1")
                    .unwrap_or_else(|error| panic!("invalid fixture: {error}")),
                artifact_id: super::ArtifactId::new("artifact-1")
                    .unwrap_or_else(|error| panic!("invalid fixture: {error}")),
                label: "findings".into(),
                pointer: "artifact://agent-b/findings".into(),
            },
            ConversationEvent::RuntimeWarning {
                agent_id: agent("agent-a"),
                item_id: TranscriptItemId::new("item-warning-1")
                    .unwrap_or_else(|error| panic!("invalid fixture: {error}")),
                message: "degraded".into(),
            },
            ConversationEvent::RuntimeError {
                agent_id: agent("agent-a"),
                item_id: TranscriptItemId::new("item-error-1")
                    .unwrap_or_else(|error| panic!("invalid fixture: {error}")),
                message: "failed".into(),
            },
        ];

        let _typed_decision = ApprovalDecision::AllowOnce;
        let _turn =
            TurnId::new("turn-1").unwrap_or_else(|error| panic!("invalid fixture: {error}"));

        for (index, event) in events.into_iter().enumerate() {
            let envelope = ConversationEventEnvelope {
                sequence: EventSequence::new(index as u64 + 1),
                event,
            };
            let encoded = serde_json::to_string(&envelope)
                .unwrap_or_else(|error| panic!("serialize {envelope:?}: {error}"));
            let decoded: ConversationEventEnvelope = serde_json::from_str(&encoded)
                .unwrap_or_else(|error| panic!("deserialize {encoded}: {error}"));

            assert_eq!(decoded, envelope);
        }
    }

    #[test]
    fn the_event_tag_is_the_stable_external_name() {
        // The tag is what an out-of-process producer writes. Renaming a variant without noticing
        // would be a silent wire break, so one tag is pinned here as the canary for the scheme.
        let encoded = serde_json::to_string(&ConversationEvent::RuntimeWarning {
            agent_id: agent("agent-a"),
            item_id: TranscriptItemId::new("item-warning-1")
                .unwrap_or_else(|error| panic!("invalid fixture: {error}")),
            message: "degraded".into(),
        })
        .unwrap_or_else(|error| panic!("serialize: {error}"));

        assert_eq!(
            encoded,
            r#"{"type":"runtime_warning","agent_id":"agent-a","item_id":"item-warning-1","message":"degraded"}"#
        );
    }
}
