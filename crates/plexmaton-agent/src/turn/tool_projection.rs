//! Canonical tool facts and their immediate live presentation.

use plexmaton_core::{AttentionId, ConversationEvent, ToolCallId, ToolCallStatus};

use super::{Agent, Turn};
use crate::interface::Reaction;
use crate::journal::JournalEntryPayload;

impl Agent {
    pub(super) fn emit_tool_status(
        &mut self,
        call_id: ToolCallId,
        status: ToolCallStatus,
        reaction: &mut Reaction,
    ) {
        let entry =
            match &self.turn {
                Turn::Working { batch, .. } => batch.snapshot(&call_id).map(
                    |(item_id, revision, call, presentation, outcome)| {
                        (
                            item_id.clone(),
                            revision,
                            call.name.clone(),
                            presentation.clone(),
                            outcome.cloned(),
                        )
                    },
                ),
                Turn::Idle | Turn::Streaming { .. } => None,
            };
        let Some((item_id, item_revision, label, presentation, outcome)) = entry else {
            self.warn(reaction, "tool state changed without its transcript entry");
            return;
        };
        self.record.commit(
            JournalEntryPayload::ToolCallChanged {
                agent_id: self.record.agent_id().clone(),
                call_id: call_id.clone(),
                item_revision,
                status,
                presentation: presentation.clone(),
                outcome,
            },
            reaction,
        );
        self.record.emit(
            reaction,
            ConversationEvent::ToolCallChanged {
                agent_id: self.record.agent_id().clone(),
                item_id,
                item_revision,
                label,
                call_id,
                status,
                presentation,
            },
        );
    }

    pub(super) fn emit_tool_request(&mut self, call_id: ToolCallId, reaction: &mut Reaction) {
        let snapshot =
            match &self.turn {
                Turn::Working { batch, .. } => batch.snapshot(&call_id).map(
                    |(item_id, revision, call, presentation, outcome)| {
                        (
                            item_id.clone(),
                            revision,
                            call.call_id.clone(),
                            call.name.clone(),
                            presentation.clone(),
                            outcome.cloned(),
                        )
                    },
                ),
                Turn::Idle | Turn::Streaming { .. } => None,
            };
        let Some((item_id, item_revision, call_id, label, presentation, outcome)) = snapshot else {
            self.warn(reaction, "tool request has no transcript entry");
            return;
        };
        debug_assert_eq!(item_revision, 0);
        debug_assert!(outcome.is_none());
        self.record.commit(
            JournalEntryPayload::ToolCallRequested {
                agent_id: self.record.agent_id().clone(),
                call_id: call_id.clone(),
                presentation: presentation.clone(),
            },
            reaction,
        );
        self.record.emit(
            reaction,
            ConversationEvent::ToolCallChanged {
                agent_id: self.record.agent_id().clone(),
                item_id,
                item_revision,
                call_id,
                label,
                status: ToolCallStatus::Queued,
                presentation,
            },
        );
    }

    pub(super) fn resolve_attention(&mut self, attention_id: AttentionId, reaction: &mut Reaction) {
        self.record.commit(
            JournalEntryPayload::AttentionResolved {
                agent_id: self.record.agent_id().clone(),
                attention_id: attention_id.clone(),
            },
            reaction,
        );
        self.record.emit(
            reaction,
            ConversationEvent::AttentionResolved {
                agent_id: self.record.agent_id().clone(),
                attention_id,
            },
        );
    }
}
