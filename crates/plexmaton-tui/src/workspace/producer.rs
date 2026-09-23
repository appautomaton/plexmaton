//! What producers hand the workspace: conversation events, and user input a runtime returned.

use std::time::Instant;

use plexmaton_core::{AgentId, ConversationEventEnvelope};

use super::Workspace;

impl Workspace {
    /// Applies producer events to the projection.
    ///
    /// A producer contract violation is a visible, typed notice inside the projection rather than a
    /// reason to tear down the user's terminal (`state::notices`), so nothing is returned to check
    /// here.
    pub fn emit(&mut self, events: Vec<ConversationEventEnvelope>) {
        self.emit_at(events, Instant::now());
    }

    /// Apply producer events heard at `now`, which dates the activity line's readings.
    pub fn emit_at(&mut self, events: Vec<ConversationEventEnvelope>, now: Instant) {
        let before = self.state.revision();
        // Quiet is the primary's route saying nothing: a child's stream is not the primary heard.
        let heard = {
            let primary = self.state.primary_agent().map(|agent| &agent.id);
            events
                .iter()
                .any(|envelope| primary.is_none_or(|primary| envelope.event.agent() == primary))
        };
        for envelope in events {
            let _outcome = self.state.apply(envelope);
        }
        self.state.observe_activity(now, heard);
        if self.state.revision() != before {
            self.validate_text_selection();
            self.reconcile_copy();
            // Producer changes can move rows under a stationary pointer. The next motion resolves
            // a fresh frame target; keeping the old identity would make the accent move with it.
            self.state.hover_entry(None);
        }
    }

    /// Restores user text a runtime returned instead of silently discarding its ownership.
    pub fn return_input(&mut self, to: AgentId, text: String) {
        self.state.return_input(to, text);
        // Composer growth can change the transcript viewport beneath a stationary pointer.
        self.state.hover_entry(None);
    }

    /// Restores returned text and its deliberate skill binding when the composer is otherwise empty.
    pub fn return_skill_input(&mut self, to: AgentId, text: String, skill: Option<String>) {
        self.state.return_skill_input(to, text, skill);
        self.state.hover_entry(None);
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use plexmaton_core::AgentId;

    use crate::{Workspace, test_support::Conversation};

    /// COM-5: quiet is the primary's route saying nothing. A child streaming meanwhile does not
    /// keep the primary's line from saying so; the primary's own delta does.
    #[test]
    fn a_child_streaming_does_not_break_the_primary_quiet() {
        let mut conversation = Conversation::canonical();
        let mut workspace = Workspace::default();
        let start = Instant::now();
        workspace.emit_at(conversation.drain(), start);
        let child = AgentId::new("agent-b").expect("the canonical child");
        for second in 1..=6 {
            conversation.extend_agent(&child, 1);
            workspace.emit_at(conversation.drain(), start + Duration::from_secs(second));
        }
        assert_eq!(
            workspace.state.activity_quiet(),
            Some(Duration::from_secs(6)),
            "six seconds of the child is six seconds of the primary's quiet"
        );

        conversation.extend(1);
        workspace.emit_at(conversation.drain(), start + Duration::from_secs(7));
        assert_eq!(
            workspace.state.activity_quiet(),
            None,
            "the primary is heard"
        );
    }
}
