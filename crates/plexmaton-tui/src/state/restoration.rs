//! Per-conversation notes: restoration and compaction feedback, outside semantic events and
//! diagnostic queues, painted once after the last entry (JRN-5, CPL-9).

use plexmaton_core::AgentId;

use super::ViewState;

/// Why `/compact` did not start, as the runtime refused it (CPL-9). The composition root maps
/// the runtime's refusal here; the conversation shows one sentence for it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompactRefusal {
    TurnActive,
    ApprovalPending,
    CompactionActive,
    ShuttingDown,
    BudgetUnavailable,
    NothingToCompact,
    HistoryTooLarge,
    SourceUnavailable,
}

impl CompactRefusal {
    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::TurnActive => "Could not compact: the turn is still running.",
            Self::ApprovalPending => "Could not compact: a tool call is waiting for your decision.",
            Self::CompactionActive => "Could not compact: a compaction is already running.",
            Self::ShuttingDown => "Could not compact: Plexmaton is shutting down.",
            Self::BudgetUnavailable => "Could not compact: the model's budget is unavailable.",
            Self::NothingToCompact => "Nothing to compact yet.",
            Self::HistoryTooLarge => {
                "Could not compact: the history exceeds what the model can read at once."
            }
            Self::SourceUnavailable => "Could not compact: the conversation could not be read.",
        }
    }
}

/// What the composition root reports about a compaction the user asked for (CPL-9).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompactionNote {
    /// The runtime owns the summarizer; the activity line says `Compacting…` until it ends.
    Started,
    Refused(CompactRefusal),
    Published,
    /// The attempt ended without a checkpoint; `reason` is the runtime's own sentence for it.
    Failed {
        reason: String,
    },
}

/// One presentation-only line after the conversation's last entry, never a semantic entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConversationNote {
    Restored(ConversationRestoration),
    Compacted,
    CompactionRefused(CompactRefusal),
    CompactionFailed { reason: String },
    SwitchRefused(super::SwitchRefusal),
}

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
        self.report_note(&id, ConversationNote::Restored(summary));
    }

    /// Where a requested compaction stands, on the conversation it was asked for (CPL-9).
    pub(crate) fn report_compaction(&mut self, agent: &AgentId, note: CompactionNote) {
        match note {
            CompactionNote::Started => {
                self.compacting = Some(agent.clone());
                self.touch();
            }
            CompactionNote::Refused(refusal) => {
                self.report_note(agent, ConversationNote::CompactionRefused(refusal));
            }
            CompactionNote::Published => {
                self.compacting = None;
                self.report_note(agent, ConversationNote::Compacted);
            }
            CompactionNote::Failed { reason } => {
                self.compacting = None;
                self.report_note(agent, ConversationNote::CompactionFailed { reason });
            }
        }
    }

    /// Whether the primary conversation's requested compaction is still running.
    pub(crate) fn compacting(&self, agent: &AgentId) -> bool {
        self.compacting.as_ref() == Some(agent)
    }

    pub(super) fn report_note(&mut self, agent: &AgentId, note: ConversationNote) {
        if self
            .agents
            .get_mut(agent)
            .is_ok_and(|view| view.report_note(note))
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
            assert_eq!(
                state.copy_entries().expect("semantic copy").text,
                "a new question"
            );
        }
    }
}
