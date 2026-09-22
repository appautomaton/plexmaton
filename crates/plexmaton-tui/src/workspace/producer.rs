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
        let heard = !events.is_empty();
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
