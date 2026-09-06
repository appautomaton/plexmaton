use plexmaton_core::ConversationEvent;

use super::super::JournalEntryPayload;

pub(super) fn visible_event(payload: JournalEntryPayload) -> ConversationEvent {
    match payload {
        JournalEntryPayload::AgentCreated {
            agent_id,
            label,
            status,
        } => ConversationEvent::AgentCreated {
            agent_id,
            label,
            status,
        },
        JournalEntryPayload::AttentionRequested {
            agent_id,
            attention_id,
            request,
        } => ConversationEvent::AttentionRequested {
            agent_id,
            attention_id,
            request,
        },
        JournalEntryPayload::AttentionResolved {
            agent_id,
            attention_id,
        } => ConversationEvent::AttentionResolved {
            agent_id,
            attention_id,
        },
        JournalEntryPayload::MailDelivered {
            item_id,
            mail_id,
            from,
            to,
            summary,
        } => ConversationEvent::MailDelivered {
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
        } => ConversationEvent::ArtifactAnnounced {
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
        } => ConversationEvent::RuntimeWarning {
            agent_id,
            item_id,
            message,
        },
        JournalEntryPayload::RuntimeError {
            agent_id,
            item_id,
            message,
        } => ConversationEvent::RuntimeError {
            agent_id,
            item_id,
            message,
        },
        JournalEntryPayload::TurnInterruptedByRecovery { agent_id, item_id } => {
            ConversationEvent::RuntimeWarning {
                agent_id,
                item_id,
                message: super::super::PROCESS_RECOVERY_MESSAGE.to_owned(),
            }
        }
        JournalEntryPayload::AssistantOutput { .. }
        | JournalEntryPayload::TurnStatusChanged { .. }
        | JournalEntryPayload::TurnStarted { .. }
        | JournalEntryPayload::TurnRetried { .. }
        | JournalEntryPayload::SteeringAccepted { .. }
        | JournalEntryPayload::SkillActivated { .. }
        | JournalEntryPayload::ToolPermissionDecided { .. }
        | JournalEntryPayload::ToolCallRequested { .. }
        | JournalEntryPayload::ToolCallChanged { .. } => {
            unreachable!("model-bearing payloads are projected separately")
        }
    }
}
