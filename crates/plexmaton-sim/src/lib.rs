//! Deterministic semantic timelines for Phase 00.

mod runtime;
mod workload;

use plexmaton_core::{
    AgentId, AgentStatus, ArtifactId, AttentionId, AttentionKind, IdError, MailId, SessionEvent,
    ToolCallId, ToolCallStatus, TranscriptItemId, TranscriptRole,
};

pub use runtime::{RuntimeCommand, ScriptedRuntime};

/// One event scheduled on a deterministic logical clock.
///
/// A step carries no sequence number. Numbering is the emitting runtime's job, and a scenario is
/// one of two sources feeding a single monotonic stream — the user is the other.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScenarioStep {
    pub at_tick: u64,
    pub event: SessionEvent,
}

/// A replayable synthetic scenario.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Scenario {
    steps: Vec<ScenarioStep>,
}

impl Scenario {
    /// The initial A-delegates-to-B experience used by the runnable skeleton.
    pub fn canonical() -> Result<Self, IdError> {
        let agent_a = AgentId::new("agent-a")?;
        let agent_b = AgentId::new("agent-b")?;
        let item_a = TranscriptItemId::new("item-a-1")?;
        let item_b = TranscriptItemId::new("item-b-1")?;

        let events = vec![
            (
                0,
                SessionEvent::AgentCreated {
                    agent_id: agent_a.clone(),
                    label: "Agent A · primary".into(),
                    status: AgentStatus::Running,
                },
            ),
            (
                1,
                SessionEvent::TranscriptItemStarted {
                    agent_id: agent_a.clone(),
                    item_id: item_a.clone(),
                    role: TranscriptRole::Assistant,
                },
            ),
            (
                2,
                SessionEvent::TranscriptDelta {
                    agent_id: agent_a.clone(),
                    item_id: item_a.clone(),
                    item_revision: 1,
                    text: "I will inspect the workspace and delegate the focused UI study. ".into(),
                },
            ),
            (
                4,
                SessionEvent::AgentCreated {
                    agent_id: agent_b.clone(),
                    label: "Agent B · UI study".into(),
                    status: AgentStatus::Running,
                },
            ),
            (
                5,
                SessionEvent::TranscriptDelta {
                    agent_id: agent_a.clone(),
                    item_id: item_a.clone(),
                    item_revision: 2,
                    text: "Agent B is running independently; this transcript remains interactive."
                        .into(),
                },
            ),
            (
                6,
                SessionEvent::TranscriptItemFinalized {
                    agent_id: agent_a.clone(),
                    item_id: item_a.clone(),
                    item_revision: 3,
                },
            ),
            (
                7,
                SessionEvent::TranscriptItemStarted {
                    agent_id: agent_b.clone(),
                    item_id: item_b.clone(),
                    role: TranscriptRole::Assistant,
                },
            ),
            (
                8,
                SessionEvent::TranscriptDelta {
                    agent_id: agent_b.clone(),
                    item_id: item_b.clone(),
                    item_revision: 1,
                    text: "I found the surface-routing boundary and am checking overlap behavior."
                        .into(),
                },
            ),
            (
                10,
                SessionEvent::ToolCallChanged {
                    agent_id: agent_b.clone(),
                    call_id: ToolCallId::new("tool-b-1")?,
                    label: "inspect interaction fixtures".into(),
                    status: ToolCallStatus::Running,
                },
            ),
            (
                12,
                SessionEvent::AttentionRequested {
                    agent_id: agent_b.clone(),
                    attention_id: AttentionId::new("attention-b-1")?,
                    kind: AttentionKind::Clarification,
                    summary: "Choose whether the overlap study should cover narrow screens.".into(),
                },
            ),
            (
                14,
                SessionEvent::ToolCallChanged {
                    agent_id: agent_b.clone(),
                    call_id: ToolCallId::new("tool-b-1")?,
                    label: "inspect interaction fixtures".into(),
                    status: ToolCallStatus::Succeeded,
                },
            ),
            (
                15,
                SessionEvent::ArtifactAnnounced {
                    agent_id: agent_b.clone(),
                    artifact_id: ArtifactId::new("artifact-b-1")?,
                    label: "interaction findings".into(),
                    pointer: "artifact://agent-b/interaction-findings".into(),
                },
            ),
            (
                16,
                SessionEvent::MailDelivered {
                    mail_id: MailId::new("mail-b-a-1")?,
                    from: agent_b.clone(),
                    to: agent_a.clone(),
                    summary: "Overlap routing should remain centralized and z-ordered.".into(),
                },
            ),
            (
                17,
                SessionEvent::AgentStatusChanged {
                    agent_id: agent_b,
                    status: AgentStatus::Completed,
                },
            ),
        ];

        let steps = events
            .into_iter()
            .map(|(at_tick, event)| ScenarioStep { at_tick, event })
            .collect();

        Ok(Self { steps })
    }

    /// Returns the immutable logical timeline.
    #[must_use]
    pub fn steps(&self) -> &[ScenarioStep] {
        &self.steps
    }

    /// Consumes the scenario into its replay steps.
    #[must_use]
    pub fn into_steps(self) -> Vec<ScenarioStep> {
        self.steps
    }
}

#[cfg(test)]
mod tests {
    use super::Scenario;

    #[test]
    fn canonical_scenario_is_monotonic_and_repeatable() {
        let first =
            Scenario::canonical().unwrap_or_else(|error| panic!("invalid fixture: {error}"));
        let second =
            Scenario::canonical().unwrap_or_else(|error| panic!("invalid fixture: {error}"));

        assert_eq!(
            first, second,
            "replay must be identical, not merely similar"
        );
        assert!(
            first
                .steps()
                .windows(2)
                .all(|pair| pair[0].at_tick <= pair[1].at_tick),
            "a step must never be scheduled before the one in front of it"
        );
    }
}
