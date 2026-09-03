//! Synthetic load, for the workloads Phase 00 has to measure rather than describe.
//!
//! These are timelines, not fixtures: one event per tick, so a harness can advance them a single
//! event at a time and time the frame that follows. The canonical scenario stays what it is — a
//! scripted story with named moments — and these stay what they are, traffic at a stated volume.

use plexmaton_core::{
    AgentId, AgentStatus, IdError, SessionEvent, ToolCallId, ToolCallStatus, ToolDetail,
    ToolPresentation, TranscriptItemId, TranscriptRole,
};

use crate::{Scenario, ScenarioStep};

/// Sentences the filler is assembled from.
///
/// Lengths differ so that items wrap to different heights. A corpus of equal-height messages would
/// make every offset in the transcript a multiple of one number, and the arithmetic that resolves a
/// reading position would look correct while being untested.
const FRAGMENTS: [&str; 4] = [
    "Checked the routing boundary and the surfaces it registers.",
    "The overlap case needs a decision about z-order before the shelf lands, because a promoted \
     surface that does not also take the pointer is worse than one that never rises at all.",
    "Reading the fixtures now.",
    "Summary: hit testing stays centralized, the wheel follows the pointer rather than focus, and \
     an exhausted viewport stops rather than passing the notch upward.",
];

impl Scenario {
    /// One conversation streaming `items` assistant messages.
    ///
    /// The rapid-delta workload. Every item arrives as a started event, two deltas, and a
    /// finalization, which is the traffic shape a real streaming producer has.
    pub fn streaming(items: usize) -> Result<Self, IdError> {
        Self::from_agents(1, items)
    }

    /// `agents` conversations streaming in round-robin, with tool activity changing alongside.
    ///
    /// The interleaved workload. Only one conversation is on screen, so this is also what proves a
    /// background agent's traffic does not cost the foreground a re-measure.
    pub fn interleaved(agents: usize, items_each: usize) -> Result<Self, IdError> {
        Self::from_agents(agents, items_each)
    }

    /// One conversation containing openable compact tool entries.
    ///
    /// Kept separate from [`Self::streaming`] because disclosure is a frame workload, not another
    /// message shape. Each item first appears queued with canonical invocation detail, exactly as a
    /// production admission does: queued has no trusted presentation, then running carries the
    /// canonical invocation at the next revision (ENT-2).
    pub fn tool_entries(items: usize) -> Result<Self, IdError> {
        let agent_id = AgentId::new("agent-0")?;
        let mut events = Vec::with_capacity(items.saturating_mul(2).saturating_add(1));
        events.push(SessionEvent::AgentCreated {
            agent_id: agent_id.clone(),
            label: "Agent 0 · tool workload".to_owned(),
            status: AgentStatus::Running,
        });
        for item in 0..items {
            let item_id = TranscriptItemId::new(format!("tool-entry-0-{item}"))?;
            let call_id = ToolCallId::new(format!("tool-0-{item}"))?;
            events.push(SessionEvent::ToolCallChanged {
                agent_id: agent_id.clone(),
                item_id: item_id.clone(),
                item_revision: 0,
                call_id: call_id.clone(),
                label: "read_file".to_owned(),
                status: ToolCallStatus::Queued,
                presentation: ToolPresentation::default(),
            });
            events.push(SessionEvent::ToolCallChanged {
                agent_id: agent_id.clone(),
                item_id,
                item_revision: 1,
                call_id,
                label: "read_file".to_owned(),
                status: ToolCallStatus::Running,
                presentation: ToolPresentation {
                    invocation: Some(ToolDetail::Text {
                        source: format!("path: fixture/{item}.txt\noffset: null\nlimit: 200"),
                        omitted_bytes: 0,
                    }),
                    outcome: None,
                },
            });
        }
        Ok(Self {
            steps: events
                .into_iter()
                .enumerate()
                .map(|(at_tick, event)| ScenarioStep {
                    at_tick: at_tick as u64,
                    event,
                })
                .collect(),
        })
    }

    fn from_agents(agents: usize, items_each: usize) -> Result<Self, IdError> {
        let ids: Vec<AgentId> = (0..agents)
            .map(|index| AgentId::new(format!("agent-{index}")))
            .collect::<Result<_, _>>()?;
        let mut events = Vec::new();

        for (index, agent_id) in ids.iter().enumerate() {
            events.push(SessionEvent::AgentCreated {
                agent_id: agent_id.clone(),
                label: format!("Agent {index} · workload"),
                status: AgentStatus::Running,
            });
        }

        // Round-robin, so that stepping one event at a time interleaves the conversations the way
        // concurrent agents do rather than finishing one before starting the next.
        for item in 0..items_each {
            for (index, agent_id) in ids.iter().enumerate() {
                events.extend(message(agent_id, index, item)?);
                if item.is_multiple_of(8) {
                    events.push(tool_change(agent_id, index, item)?);
                }
            }
        }

        Ok(Self {
            steps: events
                .into_iter()
                .enumerate()
                .map(|(at_tick, event)| ScenarioStep {
                    at_tick: at_tick as u64,
                    event,
                })
                .collect(),
        })
    }
}

/// One assistant message, as the four events a streaming producer actually sends.
fn message(agent_id: &AgentId, agent: usize, item: usize) -> Result<Vec<SessionEvent>, IdError> {
    let item_id = TranscriptItemId::new(format!("m-{agent}-{item}"))?;
    let body = FRAGMENTS
        .get(item % FRAGMENTS.len())
        .copied()
        .unwrap_or_default();
    Ok(vec![
        SessionEvent::TranscriptItemStarted {
            agent_id: agent_id.clone(),
            item_id: item_id.clone(),
            role: TranscriptRole::Assistant,
        },
        SessionEvent::TranscriptDelta {
            agent_id: agent_id.clone(),
            item_id: item_id.clone(),
            item_revision: 1,
            text: format!("Message {item}. "),
        },
        SessionEvent::TranscriptDelta {
            agent_id: agent_id.clone(),
            item_id: item_id.clone(),
            item_revision: 2,
            text: body.to_owned(),
        },
        SessionEvent::TranscriptItemFinalized {
            agent_id: agent_id.clone(),
            item_id,
            item_revision: 3,
        },
    ])
}

/// A tool appearing among messages, so the unified conversation has mixed entry traffic too.
fn tool_change(agent_id: &AgentId, agent: usize, item: usize) -> Result<SessionEvent, IdError> {
    Ok(SessionEvent::ToolCallChanged {
        agent_id: agent_id.clone(),
        item_id: TranscriptItemId::new(format!("tool-entry-{agent}-{item}"))?,
        item_revision: 0,
        call_id: ToolCallId::new(format!("tool-{agent}-{item}"))?,
        label: format!("inspect fixture {item}"),
        status: ToolCallStatus::Queued,
        presentation: ToolPresentation::default(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use plexmaton_core::{SessionEvent, ToolCallStatus};

    use crate::{Scenario, ScriptedRuntime};

    /// One event per tick, which is what lets a harness advance a workload a single event at a
    /// time and attribute the frame that follows to it.
    #[test]
    fn a_workload_schedules_one_event_per_tick() {
        let scenario = Scenario::streaming(20).unwrap_or_else(|error| panic!("fixture: {error}"));
        let mut runtime = ScriptedRuntime::new(scenario.clone());

        for tick in 0..scenario.steps().len() as u64 {
            assert_eq!(
                runtime.ready(tick).len(),
                1,
                "tick {tick} released more than one event"
            );
        }
        assert!(!runtime.is_replaying(), "the timeline is drained");
    }

    /// Conversations interleave rather than running one to completion, because concurrent agents
    /// are what the responsiveness question is about.
    #[test]
    fn interleaved_agents_take_turns() {
        let scenario =
            Scenario::interleaved(4, 3).unwrap_or_else(|error| panic!("fixture: {error}"));
        let starts: Vec<String> = scenario
            .steps()
            .iter()
            .filter_map(|step| match &step.event {
                SessionEvent::TranscriptItemStarted { agent_id, .. } => Some(agent_id.to_string()),
                _ => None,
            })
            .take(5)
            .collect();

        assert_eq!(
            starts,
            ["agent-0", "agent-1", "agent-2", "agent-3", "agent-0"],
            "the first round has to reach every agent before the second begins"
        );
    }

    /// Items differ in height, or every offset in the transcript would be a multiple of one number.
    ///
    /// A workload of equal-height messages measures a renderer nobody has: the arithmetic that
    /// turns a row into an item would be a division, and every rounding error in it would cancel.
    #[test]
    fn messages_differ_in_length_and_are_long_enough_to_wrap() {
        /// Wider than the conversation panel at any layout class this workspace lays out.
        const WRAPS: usize = 120;

        let scenario = Scenario::streaming(8).unwrap_or_else(|error| panic!("fixture: {error}"));
        let mut by_item: BTreeMap<String, usize> = BTreeMap::new();
        for step in scenario.steps() {
            if let SessionEvent::TranscriptDelta { item_id, text, .. } = &step.event {
                *by_item.entry(item_id.to_string()).or_default() += text.chars().count();
            }
        }
        let lengths: Vec<usize> = by_item.into_values().collect();

        assert_eq!(lengths.len(), 8, "every message contributed text");
        assert!(
            lengths.iter().any(|length| *length > WRAPS),
            "some message has to be wider than a panel, or nothing in the workload wraps: \
             {lengths:?}"
        );
        assert!(
            lengths.iter().min() != lengths.iter().max(),
            "and they must not all be the same height: {lengths:?}"
        );
    }

    /// ENT-2: the disclosure workload uses first-appearance tool facts rather than fake text.
    #[test]
    fn tool_workload_entries_follow_production_admission_order() {
        let scenario = Scenario::tool_entries(3).unwrap_or_else(|error| panic!("fixture: {error}"));
        let tools: Vec<_> = scenario
            .steps()
            .iter()
            .filter_map(|step| match &step.event {
                SessionEvent::ToolCallChanged {
                    item_revision,
                    status,
                    presentation,
                    ..
                } => Some((*item_revision, *status, presentation.invocation.is_some())),
                _ => None,
            })
            .collect();

        assert_eq!(
            tools,
            [
                (0, ToolCallStatus::Queued, false),
                (1, ToolCallStatus::Running, true),
                (0, ToolCallStatus::Queued, false),
                (1, ToolCallStatus::Running, true),
                (0, ToolCallStatus::Queued, false),
                (1, ToolCallStatus::Running, true),
            ]
        );
    }
}
