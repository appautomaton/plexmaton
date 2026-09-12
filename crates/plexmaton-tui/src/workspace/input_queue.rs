//! The workspace's half of the waiting-input band: what it is told, and what it asks for.

use super::{Outcome, Workspace};

impl Workspace {
    /// Replaces what the producer says is still waiting to be sent.
    ///
    /// A snapshot, not an event: the producer owns these queues, so the projection is replaced
    /// whole rather than accumulated here and reconciled when a boundary claims one (IQU-1).
    pub fn set_queued_input(&mut self, queued: Vec<crate::QueuedInput>) {
        self.state.set_queued_input(queued);
    }

    /// Asks the runtime to take the most recent waiting message back (IQU-4).
    ///
    /// The workspace removes nothing itself: the queue belongs to the producer, and a projection
    /// that dropped an entry locally would be describing a queue that still had it. Nothing
    /// waiting or an occupied draft resolves to no request. Returned text and its skill must not
    /// merge into another draft; the band advertises the empty-draft precondition (IQU-4).
    pub(super) fn withdraw_queued(&self) -> Outcome {
        Outcome {
            withdrawn: self.state.withdraw_target(&self.surfaces),
            ..Outcome::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{QueuedBoundary, QueuedInput, SurfaceId, Workspace};
    use ratatui::{
        Terminal,
        backend::TestBackend,
        crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers},
    };

    /// IQU-4: withdrawing into an occupied composer must not merge two messages or lose a skill.
    #[test]
    fn waiting_input_keeps_an_existing_draft_and_its_skill_separate() {
        let mut workspace = Workspace::default();
        workspace.emit(crate::test_support::canonical_runtime().ready(u64::MAX));
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("test terminal");
        workspace
            .settled_draw(&mut terminal)
            .expect("initial frame");
        workspace
            .state
            .focus_surface(&workspace.surfaces, SurfaceId::Composer);
        let agent = workspace.state.primary_agent().expect("primary").id.clone();
        workspace.return_skill_input(
            agent.clone(),
            "$research keep this draft".to_owned(),
            Some("research".to_owned()),
        );
        workspace.set_queued_input(vec![QueuedInput {
            text: "$review inspect the queue".to_owned(),
            boundary: QueuedBoundary::Turn,
        }]);
        workspace
            .settled_draw(&mut terminal)
            .expect("waiting frame");

        let outcome = workspace.handle(&Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT)));
        assert!(
            outcome.withdrawn.is_none(),
            "the runtime must retain the queued invocation"
        );
        assert_eq!(
            workspace.state.composer().text(),
            "$research keep this draft"
        );
        assert_eq!(workspace.state.selected_skill(&agent), Some("research"));
        assert_eq!(workspace.state.queued_input().len(), 1);
        assert!(
            crate::test_support::snapshot_text(
                terminal.backend().buffer(),
                *terminal.backend().buffer().area(),
            )
            .contains("Alt-↑ needs an empty draft")
        );
    }
}
