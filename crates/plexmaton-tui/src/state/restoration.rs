//! Per-conversation restoration feedback, outside semantic events and diagnostic queues.

use super::ViewState;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FeedbackPlacement {
    Before,
    After,
}

/// File repair performed before restoring a conversation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversationTailRepair {
    /// A complete final JSON record lacked its newline.
    AddedFinalNewline,
    /// An incomplete final fragment was retained beside the journal.
    IsolatedFinalTail { bytes: u64 },
}

/// Presentation-only confirmation after the acknowledged history has been installed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationRestoration {
    /// Optional file-tail repair, shown before the success confirmation.
    pub tail: Option<ConversationTailRepair>,
}

impl ViewState {
    pub(crate) fn report_conversation_recovery(&mut self, summary: ConversationRestoration) {
        let Some(id) = self.primary_agent().map(|agent| agent.id.clone()) else {
            return;
        };
        if self
            .agents
            .get_mut(&id)
            .is_ok_and(|agent| agent.report_restoration(summary))
        {
            self.touch();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Palette, TranscriptMetrics, surface::SurfaceId};
    use plexmaton_core::{
        AgentId, AgentStatus, ConversationEvent, ConversationEventEnvelope, EventSequence,
        TranscriptItemId, TranscriptRole,
    };

    #[test]
    fn an_empty_restoration_stays_before_the_first_new_message() {
        // TR-1/TR-3, JRN-5: the empty-history anchor has no invented semantic identity.
        for width in [120, 95, 60] {
            let mut state = ViewState::default();
            let agent_id = AgentId::new("primary").expect("agent");
            state.apply(ConversationEventEnvelope {
                sequence: EventSequence::new(1),
                event: ConversationEvent::AgentCreated {
                    agent_id: agent_id.clone(),
                    label: "Plexmaton".into(),
                    status: AgentStatus::Idle,
                },
            });
            state.report_conversation_recovery(ConversationRestoration { tail: None });
            let before = crate::test_support::draw(&state, width, 24);
            assert!(before.contains("✓ Conversation restored."));
            assert!(!before.contains("Notices"));
            assert_eq!(state.primary_agent().expect("agent").entries().count(), 0);
            let item_id = TranscriptItemId::new("first").expect("item");
            for (sequence, event) in [
                (
                    2,
                    ConversationEvent::TranscriptItemStarted {
                        agent_id: agent_id.clone(),
                        item_id: item_id.clone(),
                        role: TranscriptRole::User,
                    },
                ),
                (
                    3,
                    ConversationEvent::TranscriptDelta {
                        agent_id: agent_id.clone(),
                        item_id,
                        item_revision: 1,
                        text: "a new question".into(),
                    },
                ),
            ] {
                state.apply(ConversationEventEnvelope {
                    sequence: EventSequence::new(sequence),
                    event,
                });
            }
            let after = crate::test_support::draw(&state, width, 24);
            assert!(
                after.find("Conversation restored.").expect("confirmation")
                    < after.find("a new question").expect("new question")
            );
            let mut metrics = TranscriptMetrics::default();
            let inner = width - 2;
            metrics.measure(
                state.primary_agent().expect("agent"),
                &Palette::ansi(),
                inner,
            );
            assert_eq!(metrics.compact_entry_at_row(&agent_id, inner, 0), None);
            assert_eq!(metrics.compact_entry_at_row(&agent_id, inner, 2), Some(0));
            state.begin_selection(SurfaceId::Transcript, agent_id, 0);
            assert_eq!(state.copy().expect("semantic copy").text, "a new question");
        }
    }
}
