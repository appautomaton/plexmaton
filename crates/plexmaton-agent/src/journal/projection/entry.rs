//! Dispatch one canonical entry to the semantic and visible projections.

use super::tools::ToolChange;
use super::{JournalEntryPayload, JournalProjectionError, Projector, SessionEntry, SessionEvent};

pub(super) fn project_entry(
    projector: &mut Projector,
    entry: &SessionEntry,
) -> Result<(), JournalProjectionError> {
    let source = entry.id.clone();
    let activation_owner = projector.activation_owner.take();
    match &entry.payload {
        JournalEntryPayload::TurnRetried {
            agent_id, turn_id, ..
        } => {
            projector.turns.insert(turn_id.clone(), agent_id.clone());
            projector.emit(SessionEvent::AgentStatusChanged {
                agent_id: agent_id.clone(),
                status: plexmaton_core::AgentStatus::Running,
            })
        }
        JournalEntryPayload::TurnStarted {
            agent_id,
            item_id,
            turn_id,
            text,
            ..
        } => projector.turn_started(
            source,
            agent_id.clone(),
            item_id.clone(),
            turn_id.clone(),
            text.clone(),
        ),
        JournalEntryPayload::SteeringAccepted {
            agent_id,
            item_id,
            turn_id,
            text,
            ..
        } => projector.steering(
            source,
            agent_id.clone(),
            item_id.clone(),
            turn_id.clone(),
            text.clone(),
        ),
        JournalEntryPayload::SkillActivated {
            agent_id,
            turn_id,
            activation,
        } => projector.skill(
            source,
            agent_id.clone(),
            turn_id.clone(),
            activation.clone(),
            activation_owner,
        ),
        JournalEntryPayload::TurnStatusChanged {
            agent_id,
            turn_id,
            status,
        } => projector.turn_status(agent_id.clone(), turn_id.clone(), *status),
        JournalEntryPayload::AssistantOutput {
            agent_id,
            step_id,
            output,
        } => projector.assistant_output(source, agent_id.clone(), step_id.clone(), output.clone()),
        JournalEntryPayload::ToolCallRequested {
            agent_id,
            call_id,
            presentation,
        } => projector.request_tool(
            source,
            agent_id.clone(),
            call_id.clone(),
            presentation.clone(),
        ),
        JournalEntryPayload::ToolCallChanged {
            agent_id,
            call_id,
            item_revision,
            status,
            presentation,
            outcome,
        } => projector.change_tool(
            source,
            ToolChange {
                agent_id: agent_id.clone(),
                call_id: call_id.clone(),
                item_revision: *item_revision,
                status: *status,
                presentation: presentation.clone(),
                outcome: outcome.clone(),
            },
        ),
        other => projector.visible(other.clone()),
    }
}
