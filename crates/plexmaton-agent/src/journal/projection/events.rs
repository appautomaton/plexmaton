use plexmaton_core::SessionEvent;

use super::super::JournalEntryPayload;

pub(super) fn visible_event(payload: JournalEntryPayload) -> SessionEvent {
    match payload {
        JournalEntryPayload::AgentCreated {
            agent_id,
            label,
            status,
        } => SessionEvent::AgentCreated {
            agent_id,
            label,
            status,
        },
        JournalEntryPayload::AttentionRequested {
            agent_id,
            attention_id,
            request,
        } => SessionEvent::AttentionRequested {
            agent_id,
            attention_id,
            request,
        },
        JournalEntryPayload::AttentionResolved {
            agent_id,
            attention_id,
        } => SessionEvent::AttentionResolved {
            agent_id,
            attention_id,
        },
        JournalEntryPayload::MailDelivered {
            item_id,
            mail_id,
            from,
            to,
            summary,
        } => SessionEvent::MailDelivered {
            item_id,
            mail_id,
            from,
            to,
            summary,
        },
        JournalEntryPayload::ArtifactAnnounced {
            agent_id,
            item_id,
            artifact_id,
            label,
            pointer,
        } => SessionEvent::ArtifactAnnounced {
            agent_id,
            item_id,
            artifact_id,
            label,
            pointer,
        },
        JournalEntryPayload::RuntimeWarning {
            agent_id,
            item_id,
            message,
        } => SessionEvent::RuntimeWarning {
            agent_id,
            item_id,
            message,
        },
        JournalEntryPayload::RuntimeError {
            agent_id,
            item_id,
            message,
        } => SessionEvent::RuntimeError {
            agent_id,
            item_id,
            message,
        },
        JournalEntryPayload::TurnInterruptedByRecovery { agent_id, item_id } => {
            SessionEvent::RuntimeWarning {
                agent_id,
                item_id,
                message: super::super::PROCESS_RECOVERY_MESSAGE.to_owned(),
            }
        }
        JournalEntryPayload::AssistantOutput { .. }
        | JournalEntryPayload::TurnStatusChanged { .. }
        | JournalEntryPayload::TurnStarted { .. }
        | JournalEntryPayload::SteeringAccepted { .. }
        | JournalEntryPayload::ToolCallRequested { .. }
        | JournalEntryPayload::ToolCallChanged { .. } => {
            unreachable!("model-bearing payloads are projected separately")
        }
    }
}
