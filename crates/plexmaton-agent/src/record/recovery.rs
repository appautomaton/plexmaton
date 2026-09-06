use std::collections::BTreeMap;

use plexmaton_core::{
    AgentStatus, AttentionId, AttentionRequest, ConversationEvent, ToolCallId, ToolCallStatus,
    ToolPresentation, TranscriptItemId, TurnId,
};

use super::Record;
use crate::{AssistantBlock, ConversationEntry, JournalEntryPayload, JournalProjection};

pub(crate) struct RecoverableTool {
    pub(crate) call_id: ToolCallId,
    pub(crate) item_id: TranscriptItemId,
    pub(crate) item_revision: u64,
    pub(crate) label: String,
    pub(crate) status: ToolCallStatus,
    pub(crate) presentation: ToolPresentation,
    pub(crate) attention_id: Option<AttentionId>,
    pub(crate) requested: bool,
}

pub(crate) struct InterruptedTurnRecovery {
    pub(crate) tools: Vec<RecoverableTool>,
    pub(crate) needs_marker: bool,
    pub(crate) turn_id: Option<TurnId>,
}

impl Record {
    pub(crate) fn interrupted_turn(&self) -> Option<InterruptedTurnRecovery> {
        let open_turn = self.journal.open_turn_on_path(&self.head);
        let projection = self
            .journal
            .project(&self.head)
            .unwrap_or_else(|error| unreachable!("loaded journal remains projectable: {error:?}"));
        let mut status = None;
        let mut order = Vec::new();
        let mut tools = BTreeMap::<ToolCallId, RecoverableTool>::new();
        let mut approvals = BTreeMap::<ToolCallId, AttentionId>::new();
        for envelope in projection.events() {
            match &envelope.event {
                ConversationEvent::AgentCreated {
                    agent_id,
                    status: next,
                    ..
                }
                | ConversationEvent::AgentStatusChanged {
                    agent_id,
                    status: next,
                } if agent_id == &self.agent_id => status = Some(*next),
                ConversationEvent::ToolCallChanged {
                    agent_id,
                    item_id,
                    item_revision,
                    call_id,
                    label,
                    status,
                    presentation,
                } if agent_id == &self.agent_id => {
                    if !tools.contains_key(call_id) {
                        order.push(call_id.clone());
                    }
                    tools.insert(
                        call_id.clone(),
                        RecoverableTool {
                            call_id: call_id.clone(),
                            item_id: item_id.clone(),
                            item_revision: *item_revision,
                            label: label.clone(),
                            status: *status,
                            presentation: presentation.clone(),
                            attention_id: None,
                            requested: true,
                        },
                    );
                }
                ConversationEvent::AttentionRequested {
                    agent_id,
                    attention_id,
                    request: AttentionRequest::Approval { call_id, .. },
                    ..
                } if agent_id == &self.agent_id => {
                    approvals.insert(call_id.clone(), attention_id.clone());
                }
                ConversationEvent::AttentionResolved {
                    agent_id,
                    attention_id,
                } if agent_id == &self.agent_id => {
                    approvals.retain(|_, pending| pending != attention_id);
                }
                _ => {}
            }
        }
        let path = self
            .journal
            .path(&self.head)
            .unwrap_or_else(|error| unreachable!("loaded head remains valid: {error:?}"));
        let last_user = path.iter().rposition(|entry| {
            matches!(
                entry.payload,
                JournalEntryPayload::TurnStarted { .. }
                    | JournalEntryPayload::TurnRetried { .. }
                    | JournalEntryPayload::SteeringAccepted { .. }
            )
        });
        let last_recovery = path.iter().rposition(|entry| {
            matches!(
                entry.payload,
                JournalEntryPayload::TurnInterruptedByRecovery { .. }
            )
        });
        include_unrequested_calls(&projection, &path, &mut order, &mut tools);
        let needs_marker =
            last_recovery.is_none_or(|done| last_user.is_none_or(|user| done < user));
        // An unanswered user atom may belong to a failed or cancelled, already finished turn.
        // Turn terminals, not the shape of the model input, establish whether work is unfinished.
        let incomplete_request = projection.recovery().is_some();
        if open_turn.is_none()
            && !matches!(status, Some(AgentStatus::Running | AgentStatus::Waiting))
            && !incomplete_request
        {
            return None;
        }
        let interrupted = order
            .into_iter()
            .filter_map(|call_id| tools.remove(&call_id))
            .filter(|tool| {
                !matches!(
                    tool.status,
                    ToolCallStatus::Succeeded
                        | ToolCallStatus::Failed
                        | ToolCallStatus::Denied
                        | ToolCallStatus::Cancelled
                )
            })
            .map(|mut tool| {
                tool.attention_id = approvals.remove(&tool.call_id);
                tool
            })
            .collect();
        Some(InterruptedTurnRecovery {
            tools: interrupted,
            needs_marker,
            turn_id: open_turn,
        })
    }
}

fn include_unrequested_calls(
    projection: &JournalProjection,
    path: &[&ConversationEntry],
    order: &mut Vec<ToolCallId>,
    tools: &mut BTreeMap<ToolCallId, RecoverableTool>,
) {
    let omitted = projection
        .recovery()
        .map(|recovery| recovery.omitted_batch_calls())
        .unwrap_or_default();
    let incomplete_blocks = path.iter().rev().find_map(|entry| match &entry.payload {
        JournalEntryPayload::AssistantOutput { output, .. }
            if output
                .tool_calls()
                .any(|call| omitted.contains(&call.call_id)) =>
        {
            Some(output.blocks())
        }
        _ => None,
    });
    for block in incomplete_blocks.unwrap_or_default() {
        let AssistantBlock::ToolCall { item_id, call } = block else {
            continue;
        };
        if omitted.contains(&call.call_id) && !tools.contains_key(&call.call_id) {
            order.push(call.call_id.clone());
            tools.insert(
                call.call_id.clone(),
                RecoverableTool {
                    call_id: call.call_id.clone(),
                    item_id: item_id.clone(),
                    item_revision: 0,
                    label: call.name.clone(),
                    status: ToolCallStatus::Queued,
                    presentation: ToolPresentation::default(),
                    attention_id: None,
                    requested: false,
                },
            );
        }
    }
}
