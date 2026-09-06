//! The canonical demonstration, driven end to end by a keyboard.
//!
//! Phase 00 is organized around one scenario, and its exit gate asks that the scenario runs in the
//! real event loop rather than being argued for from the parts. Every other test in this crate
//! isolates one mechanism; this one refuses to, because the failures it is looking for are the ones
//! that only appear when the mechanisms are used together — a selection that survives its own
//! surface closing, a reading position that a shelf quietly moved, a request that arrived while the
//! user was mid-sentence.
//!
//! It is a module rather than an integration test so it can use the same fixtures the unit tests
//! do, and so the file-length sentinel measures it as tests rather than as code.

#[cfg(test)]
mod tests {
    use plexmaton_core::AgentId;
    use plexmaton_sim::{RuntimeCommand, Scenario, ScriptedRuntime};
    use ratatui::{
        Terminal,
        backend::TestBackend,
        crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind},
        layout::Rect,
    };

    use crate::{Outcome, SurfaceId, Workspace, test_support::region_text};

    fn agent(value: &str) -> AgentId {
        AgentId::new(value).unwrap_or_else(|error| panic!("fixture: {error}"))
    }

    /// The workspace, its terminal, and the runtime feeding it, stepped the way the binary steps
    /// them: one event, then the frame it justifies.
    struct Journey {
        workspace: Workspace,
        terminal: Terminal<TestBackend>,
        runtime: ScriptedRuntime,
        tick: u64,
    }

    impl Journey {
        fn open(width: u16, height: u16) -> Self {
            let mut journey = Self {
                workspace: Workspace::default(),
                terminal: Terminal::new(TestBackend::new(width, height))
                    .unwrap_or_else(|error| panic!("test terminal: {error}")),
                runtime: ScriptedRuntime::new(
                    Scenario::canonical().unwrap_or_else(|error| panic!("fixture: {error}")),
                ),
                tick: 0,
            };
            journey.advance(0);
            journey
        }

        /// Runs the timeline forward and draws what it produced.
        fn advance(&mut self, ticks: u64) -> &mut Self {
            self.tick = self.tick.saturating_add(ticks);
            let emitted = self.runtime.ready(self.tick);
            self.workspace.emit(emitted);
            self.draw()
        }

        fn draw(&mut self) -> &mut Self {
            let _frame = self
                .workspace
                .settled_draw(&mut self.terminal)
                .unwrap_or_else(|error| panic!("test render: {error}"));
            self
        }

        fn event(&mut self, event: &Event) -> Outcome {
            let outcome = self.workspace.handle(event);
            self.draw();
            outcome
        }

        fn press(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Outcome {
            self.event(&Event::Key(KeyEvent::new(code, modifiers)))
        }

        fn key(&mut self, code: KeyCode) -> &mut Self {
            self.press(code, KeyModifiers::NONE);
            self
        }

        fn wheel(&mut self, column: u16, row: u16) -> &mut Self {
            self.event(&Event::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            }));
            self
        }

        fn resize(&mut self, width: u16, height: u16) -> &mut Self {
            self.terminal.backend_mut().resize(width, height);
            self.event(&Event::Resize(width, height));
            self
        }

        /// Types a message into whichever input has the cursor and submits it through the runtime.
        ///
        /// The round trip is the point: nothing writes to the projection here, so a message that
        /// appears has been through the boundary a real runtime will occupy (COM-3).
        fn write(&mut self, text: &str) -> &mut Self {
            for character in text.chars() {
                self.press(KeyCode::Char(character), KeyModifiers::NONE);
            }
            let submission = self
                .press(KeyCode::Enter, KeyModifiers::NONE)
                .submitted
                .unwrap_or_else(|| panic!("Enter over a cursor must submit"));
            let emitted = self
                .runtime
                .submit(RuntimeCommand::SendMessage {
                    to: submission.to,
                    text: submission.text,
                })
                .unwrap_or_else(|error| panic!("the runtime accepts the message: {error}"));
            self.workspace.emit(emitted);
            self.draw()
        }

        /// Walks the focus ring to one surface, which is all a keyboard-only user can do.
        fn focus(&mut self, target: SurfaceId) -> &mut Self {
            for _ in 0..=self.workspace.surfaces().len() {
                if self.focused() == Some(target) {
                    return self;
                }
                self.key(KeyCode::Tab);
            }
            panic!("{target:?} is not reachable by keyboard on this frame")
        }

        fn focused(&self) -> Option<SurfaceId> {
            self.workspace.state().focused(self.workspace.surfaces())
        }

        fn selected(&self) -> String {
            self.workspace
                .state()
                .selected_agent()
                .map_or_else(|| "none".to_owned(), |agent| agent.id.to_string())
        }

        fn bounds(&self, surface_id: SurfaceId) -> Rect {
            self.workspace
                .surfaces()
                .get(surface_id)
                .unwrap_or_else(|| panic!("{surface_id:?} must be registered"))
                .bounds
        }

        fn painted(&self, surface_id: SurfaceId) -> String {
            region_text(self.terminal.backend().buffer(), self.bounds(surface_id))
        }

        /// The conversation's rows beneath the second window: what the user can still read of it.
        fn beneath(&self) -> String {
            let conversation = self.bounds(SurfaceId::Transcript);
            let shelf = self.bounds(SurfaceId::Inspector);
            self.painted(SurfaceId::Transcript)
                .lines()
                .skip(usize::from(shelf.bottom().saturating_sub(conversation.y)))
                .collect::<Vec<_>>()
                .join("\n")
        }
    }

    /// Journey steps 1 to 5: A converses, delegates, and both stream while the user keeps their
    /// place in the first conversation.
    #[test]
    fn the_journey_reaches_two_agents_without_losing_the_first() {
        let mut journey = Journey::open(120, 24);

        // 1. The user converses with A.
        journey.advance(3).focus(SurfaceId::Composer);
        journey.write("what is the status?");
        assert!(
            journey.painted(SurfaceId::Transcript).contains("status?"),
            "a submitted message reaches the screen as the runtime's own event"
        );

        // 2 and 3. A delegates; B starts in the background and takes nothing.
        let before = journey.focused();
        journey.advance(6);
        assert_eq!(journey.workspace.state().agents().count(), 2);
        assert_eq!(journey.selected(), "none", "B did not steal the screen");
        assert_eq!(journey.focused(), before, "nor the keyboard");

        // Enough conversation that A has a reading position worth losing, then park away from the
        // tail so that "where the user was" is a fact and not a coincidence (TR-5).
        for message in [
            "one", "two", "three", "four", "five", "six", "seven", "eight",
        ] {
            journey.focus(SurfaceId::Composer).write(message);
        }
        journey.focus(SurfaceId::Transcript);
        for _ in 0..4 {
            journey.key(KeyCode::Up);
        }
        let parked = journey.painted(SurfaceId::Transcript);
        assert!(
            !parked.contains("eight"),
            "the reader is above the newest message, or this proves nothing"
        );

        // 4 and 5. Look at B. That is all opening is: the primary's conversation stays where it
        // is and B's opens over it (INS-1), so two different agents are on screen at once.
        journey.focus(SurfaceId::Agents).key(KeyCode::Down);
        assert_eq!(journey.selected(), "agent-b");

        let inspected = journey.painted(SurfaceId::Inspector);
        let conversation = journey.beneath();
        assert!(inspected.contains("Agent B"));
        assert!(journey.painted(SurfaceId::Transcript).contains("Agent A"));
        assert!(
            inspected.contains("surface-routing boundary"),
            "the second window holds B's conversation, not a regrouped detail list"
        );
        assert!(
            !conversation.contains("surface-routing boundary"),
            "two conversations are on screen, and they are different conversations"
        );

        journey.focus(SurfaceId::Inspector).key(KeyCode::Esc);
        assert_eq!(
            journey.selected(),
            "none",
            "closing is looking at nobody else"
        );
        assert_eq!(
            journey.painted(SurfaceId::Transcript),
            parked,
            "A's reading position survived a second agent being opened over it (SURF-5)"
        );
    }

    /// Journey steps 6 to 8: hover scrolling, shelf manipulation, and a request that interrupts
    /// nothing.
    #[test]
    fn the_journey_keeps_a_second_agent_on_screen_and_takes_a_request_without_being_interrupted() {
        let mut journey = Journey::open(120, 40);
        journey.advance(10);

        // 6. The wheel moves whatever is under the pointer and never touches focus (INV-3).
        journey.focus(SurfaceId::Agents);
        let transcript = journey.bounds(SurfaceId::Transcript);
        journey.wheel(transcript.x + 2, transcript.y + 2);
        assert_eq!(
            journey.focused(),
            Some(SurfaceId::Agents),
            "hover routing is not a focus change"
        );

        // 7. Look at B, enter its window, resize it, maximize and restore, then go back to typing
        // to A with B still on screen.
        //
        // Z-order promotion itself is not exercised: the inspector already floats over the
        // primary conversation at z-index one, and it is the only floating sibling here, so there
        // is nothing to promote it over.
        journey.key(KeyCode::Down).key(KeyCode::Enter);
        assert_eq!(journey.focused(), Some(SurfaceId::Inspector));
        let shelf = journey.bounds(SurfaceId::Inspector).height;
        journey.press(KeyCode::Down, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
        assert!(
            journey.bounds(SurfaceId::Inspector).height > shelf,
            "the keyboard moves the edge the pointer would drag (ui-ux §drag scope)"
        );

        journey.press(KeyCode::Char('f'), KeyModifiers::CONTROL);
        assert!(
            journey
                .workspace
                .surfaces()
                .get(SurfaceId::Transcript)
                .is_none(),
            "maximized takes the region outright (INS-2)"
        );
        journey.press(KeyCode::Char('f'), KeyModifiers::CONTROL);

        journey.focus(SurfaceId::Composer);
        assert_eq!(
            journey.selected(),
            "agent-b",
            "working elsewhere does not close the window"
        );
        assert!(
            journey.painted(SurfaceId::Inspector).contains("Agent B"),
            "B stays on screen while the user types to A (INS-1)"
        );
        assert!(journey.painted(SurfaceId::Transcript).contains("Agent A"));

        // 8. B asks for a decision. It queues, and it takes nothing.
        let before = journey.focused();
        journey.advance(2);
        assert_eq!(journey.workspace.state().attention_pending(), 1);
        assert!(
            journey
                .workspace
                .surfaces()
                .get(SurfaceId::Attention)
                .is_some()
        );
        assert_eq!(journey.focused(), before);
        assert_eq!(journey.selected(), "agent-b");
    }

    /// Journey steps 9 and 10: evidence is produced, copied by its underlying value, and the
    /// workspace comes back to where it was.
    #[test]
    fn the_journey_copies_evidence_and_returns_to_the_prior_state() {
        let mut journey = Journey::open(120, 40);

        // 9. B finishes: a tool result, an artifact pointer, and typed mail back to A.
        journey.advance(17);
        assert_eq!(journey.workspace.state().attention_count(), 1);
        assert_eq!(
            journey
                .workspace
                .state()
                .agent(&agent("agent-b"))
                .map_or(0, |agent| agent.mail().count()),
            1,
            "the producer's conversation retains the delivered mail"
        );

        // The user chooses to go to the agent that asked. Nothing before this moved them there.
        journey.focus(SurfaceId::Attention).key(KeyCode::Enter);
        assert_eq!(journey.selected(), "agent-b");
        assert!(
            journey
                .painted(SurfaceId::Inspector)
                .contains("Routing stays"),
            "looking at B reveals the mail B sent"
        );
        assert_eq!(journey.workspace.state().attention_pending(), 0);
        assert_eq!(
            journey.workspace.state().attention_count(),
            1,
            "acknowledging is not resolving (ATT-3)"
        );

        // 10. Copy evidence: the artifact's stable pointer, not the label on screen.
        journey.press(KeyCode::Up, KeyModifiers::SHIFT);
        journey.press(KeyCode::Up, KeyModifiers::SHIFT);
        let copied = journey
            .press(KeyCode::Char('y'), KeyModifiers::CONTROL)
            .copied
            .unwrap_or_else(|| panic!("a selection must copy to something"));
        assert_eq!(
            copied.text,
            "artifact://agent-b/interaction-findings\nagent-a: Routing stays centralized and z-ordered."
        );
        assert!(
            journey
                .painted(SurfaceId::Inspector)
                .contains("interaction findings"),
            "while the label is what was painted"
        );

        // And back: one Escape drops the selection, and the workspace is where it was.
        journey.key(KeyCode::Esc);
        assert!(journey.workspace.state().selection().is_none());
        journey.key(KeyCode::Esc);
        assert_eq!(
            journey.selected(),
            "none",
            "the second Escape closes B's window"
        );
        assert!(journey.painted(SurfaceId::Transcript).contains("Agent A"));
    }

    /// Journey step 11: the same journey means the same thing at every supported width.
    #[test]
    fn the_journey_survives_wide_medium_and_narrow() {
        let mut journey = Journey::open(140, 40);
        journey.advance(17);
        journey.focus(SurfaceId::Agents).key(KeyCode::Down);

        // Ultrawide, wide, medium, narrow, and back — with the second window open and a request
        // queued throughout, which is the state that has the most to lose.
        for (width, height) in [(140, 40), (120, 30), (80, 24), (60, 20), (140, 40)] {
            journey.resize(width, height);
            let state = journey.workspace.state();
            assert_eq!(state.attention_count(), 1, "{width}x{height}: still queued");
            assert!(
                state.inspector().is_some(),
                "{width}x{height}: the window is the selection, so no resize may drop it (INS-3)"
            );
            assert!(
                journey.painted(SurfaceId::Inspector).contains("Agent B"),
                "{width}x{height}: and it is still showing the agent the user looked at"
            );
            assert!(
                journey
                    .workspace
                    .surfaces()
                    .get(SurfaceId::Composer)
                    .is_some(),
                "{width}x{height}: the composer is never covered, at any size"
            );
        }
    }
}
