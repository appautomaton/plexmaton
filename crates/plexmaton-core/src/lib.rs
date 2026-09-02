//! Semantic contracts shared by Plexmaton runtimes and projections.
//!
//! Phase 00 deliberately exposes only the event vocabulary required by the synthetic
//! multi-agent experience. Provider wire events and terminal input do not belong here.
//!
//! Every field carries an invariant that a producer or a projection can get wrong, so this crate
//! documents them; the rest of the workspace does not enforce field-level documentation.
#![deny(missing_docs)]

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

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
stable_id!(ArtifactId, "artifact id");
stable_id!(AttentionId, "attention id");
stable_id!(MailId, "mail id");
stable_id!(ToolCallId, "tool call id");
stable_id!(TranscriptItemId, "transcript item id");

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

/// Semantic author of a transcript item.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptRole {
    /// Authored by the person using the workspace.
    User,
    /// Authored by the agent that owns this transcript.
    Assistant,
    /// Runtime-authored notice that belongs in the transcript rather than in the notice log.
    System,
}

/// Lifecycle of one visible tool call.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    /// Admitted by the scheduler but not started.
    Queued,
    /// Executing now.
    Running,
    /// Finished and produced a usable result.
    Succeeded,
    /// Finished without a usable result.
    Failed,
    /// Stopped before completion by an explicit decision.
    Cancelled,
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

/// One UI-facing semantic transition.
///
/// This is intentionally not a universal application event: it excludes provider wire data,
/// terminal input, persistence records, and animation ticks.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionEvent {
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
    /// Repeating a `call_id` updates that tool call in place rather than adding a second one.
    ToolCallChanged {
        /// Agent running the tool.
        agent_id: AgentId,
        /// Identity of the tool call being created or updated.
        call_id: ToolCallId,
        /// Display label for the tool call.
        label: String,
        /// Lifecycle state the tool call moved to.
        status: ToolCallStatus,
    },
    /// A background agent needs a user decision.
    ///
    /// Delivery must never move focus or open a modal surface; the item joins the Attention queue.
    AttentionRequested {
        /// Agent that needs the decision.
        agent_id: AgentId,
        /// Identity of the queued request.
        attention_id: AttentionId,
        /// Whether the agent is blocked or merely needs information.
        kind: AttentionKind,
        /// Bounded summary of what is being asked.
        summary: String,
    },
    /// Typed mail was delivered from one session to another.
    MailDelivered {
        /// Identity of the delivered mail.
        mail_id: MailId,
        /// Sending agent. Part of the product contract, so recipients must retain it.
        from: AgentId,
        /// Receiving agent, whose inbox gains the item.
        to: AgentId,
        /// Bounded summary. Bulk findings stay in artifacts or the sender's session.
        summary: String,
    },
    /// An agent published a durable work product.
    ArtifactAnnounced {
        /// Agent that produced the artifact.
        agent_id: AgentId,
        /// Identity of the artifact.
        artifact_id: ArtifactId,
        /// Display label for the artifact.
        label: String,
        /// Stable reference to the content. Copy actions return this, not the display label.
        pointer: String,
    },
    /// The producer reported a condition the user should see but that blocks nothing.
    RuntimeWarning {
        /// Human-readable description of the condition.
        message: String,
    },
}

/// An ordered event at the runtime-to-projection boundary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionEventEnvelope {
    /// Position in the producer's monotonic stream, used to detect loss and duplication.
    pub sequence: EventSequence,
    /// The semantic transition being reported.
    pub event: SessionEvent,
}

#[cfg(test)]
mod tests {
    use super::{
        AgentId, AgentStatus, AttentionId, AttentionKind, EventSequence, IdError, MailId,
        SessionEvent, SessionEventEnvelope, ToolCallId, ToolCallStatus, TranscriptItemId,
        TranscriptRole,
    };

    fn agent(value: &str) -> AgentId {
        AgentId::new(value).unwrap_or_else(|error| panic!("invalid fixture: {error}"))
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
            serde_json::from_str::<SessionEventEnvelope>(
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
            SessionEvent::AgentCreated {
                agent_id: agent("agent-a"),
                label: "Agent A".into(),
                status: AgentStatus::Running,
            },
            SessionEvent::AgentStatusChanged {
                agent_id: agent("agent-a"),
                status: AgentStatus::Cancelled,
            },
            SessionEvent::TranscriptItemStarted {
                agent_id: agent("agent-a"),
                item_id: item.clone(),
                role: TranscriptRole::Assistant,
            },
            SessionEvent::TranscriptDelta {
                agent_id: agent("agent-a"),
                item_id: item.clone(),
                item_revision: 1,
                // Multi-byte and combining text, because transcripts carry both.
                text: "δ 汉字 e\u{301}\n".into(),
            },
            SessionEvent::TranscriptItemFinalized {
                agent_id: agent("agent-a"),
                item_id: item,
                item_revision: 2,
            },
            SessionEvent::ToolCallChanged {
                agent_id: agent("agent-a"),
                call_id: ToolCallId::new("tool-1")
                    .unwrap_or_else(|error| panic!("invalid fixture: {error}")),
                label: "read".into(),
                status: ToolCallStatus::Queued,
            },
            SessionEvent::AttentionRequested {
                agent_id: agent("agent-b"),
                attention_id: AttentionId::new("attention-1")
                    .unwrap_or_else(|error| panic!("invalid fixture: {error}")),
                kind: AttentionKind::Approval,
                summary: "approve the write".into(),
            },
            SessionEvent::MailDelivered {
                mail_id: MailId::new("mail-1")
                    .unwrap_or_else(|error| panic!("invalid fixture: {error}")),
                from: agent("agent-b"),
                to: agent("agent-a"),
                summary: "findings".into(),
            },
            SessionEvent::ArtifactAnnounced {
                agent_id: agent("agent-b"),
                artifact_id: super::ArtifactId::new("artifact-1")
                    .unwrap_or_else(|error| panic!("invalid fixture: {error}")),
                label: "findings".into(),
                pointer: "artifact://agent-b/findings".into(),
            },
            SessionEvent::RuntimeWarning {
                message: "degraded".into(),
            },
        ];

        for (index, event) in events.into_iter().enumerate() {
            let envelope = SessionEventEnvelope {
                sequence: EventSequence::new(index as u64 + 1),
                event,
            };
            let encoded = serde_json::to_string(&envelope)
                .unwrap_or_else(|error| panic!("serialize {envelope:?}: {error}"));
            let decoded: SessionEventEnvelope = serde_json::from_str(&encoded)
                .unwrap_or_else(|error| panic!("deserialize {encoded}: {error}"));

            assert_eq!(decoded, envelope);
        }
    }

    #[test]
    fn the_event_tag_is_the_stable_external_name() {
        // The tag is what an out-of-process producer writes. Renaming a variant without noticing
        // would be a silent wire break, so one tag is pinned here as the canary for the scheme.
        let encoded = serde_json::to_string(&SessionEvent::RuntimeWarning {
            message: "degraded".into(),
        })
        .unwrap_or_else(|error| panic!("serialize: {error}"));

        assert_eq!(
            encoded,
            r#"{"type":"runtime_warning","message":"degraded"}"#
        );
    }
}
