//! One iteration of the workspace: events in, terminal events in, at most one frame out.
//!
//! This exists so that "a frame happens only when something changed" is a value a caller can read
//! rather than a side effect buried in an `async fn` around a real terminal. The executable and the
//! measurement harness drive the same object, which is what makes a measured frame the same frame
//! the user gets. The contract is
//! [`specs/frame-loop.md`](../../../.agents/specs/frame-loop.md).

use plexmaton_core::PrototypeEventEnvelope;
use ratatui::{Terminal, backend::Backend, crossterm::event::Event};

use crate::{
    intent::{PointerIntent, SelectionIntent, TuiIntent},
    render::render,
    router::{Routed, Router, RouterContext},
    state::{CopyRequest, Submission, ViewRevision, ViewState},
    surface::SurfaceTree,
    theme::Palette,
    transcript::TranscriptMetrics,
};

/// Whether the event loop continues after an intent.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Flow {
    /// Keep running.
    #[default]
    Continue,
    /// The user asked to leave.
    Quit,
}

/// What one terminal event left for the executable to do.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Outcome {
    /// Whether the loop continues.
    pub flow: Flow,
    /// What the user submitted, and who to. Only the runtime may turn it into transcript events,
    /// so it leaves the workspace as a value rather than being written anywhere (COM-3).
    pub submitted: Option<Submission>,
    /// What the user asked to copy. Leaves as a value for the same reason: the clipboard is the
    /// host's, and nothing in this crate may reach for it (SEL-4).
    pub copied: Option<CopyRequest>,
}

impl Outcome {
    const fn quit() -> Self {
        Self {
            flow: Flow::Quit,
            submitted: None,
            copied: None,
        }
    }
}

/// What one frame cost, in work rather than in time.
///
/// Work is the half of a performance claim that is the same on every machine, so it is what tests
/// assert and what a report prints beside its timings (FR-3).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FrameWork {
    /// Transcript items whose height had to be wrapped for this frame.
    pub items_wrapped: usize,
    /// Conversation lines this frame built.
    pub lines_built: usize,
}

/// The projection, its router, its retained layout cache, and the last frame's registry.
///
/// Held together because they are only correct together: routing hit-tests the surfaces the last
/// frame drew (SURF-1), the wheel resolves a reading position against the heights that frame
/// measured (TR-3), and the repaint gate compares against the revision that frame painted (D-004).
/// Assembling these separately at each call site is three chances to wire one of them wrong.
#[derive(Debug, Default)]
pub struct Workspace {
    state: ViewState,
    router: Router,
    surfaces: SurfaceTree,
    metrics: TranscriptMetrics,
    palette: Palette,
    painted: Option<ViewRevision>,
    frames: u64,
}

impl Workspace {
    /// The projection, for whatever the executable needs to read out of it.
    #[must_use]
    pub const fn state(&self) -> &ViewState {
        &self.state
    }

    /// Retained transcript layout work, for a harness to count.
    #[must_use]
    pub const fn metrics(&self) -> &TranscriptMetrics {
        &self.metrics
    }

    /// The surfaces the last frame drew, which is what a pointer event resolves against.
    ///
    /// Read-only, and read-only is the whole point: a caller that could register into this could
    /// give routing geometry no frame ever painted, which is the defect SURF-1 exists to prevent.
    #[must_use]
    pub const fn surfaces(&self) -> &SurfaceTree {
        &self.surfaces
    }

    /// Frames painted since this workspace started.
    #[must_use]
    pub const fn frames(&self) -> u64 {
        self.frames
    }

    /// Applies producer events to the projection.
    ///
    /// A producer contract violation is a visible, typed notice inside the projection rather than a
    /// reason to tear down the user's terminal (D-003), so nothing is returned to check here.
    pub fn emit(&mut self, events: Vec<PrototypeEventEnvelope>) {
        for envelope in events {
            let _outcome = self.state.apply(envelope);
        }
    }

    /// Translates one terminal event and applies whatever it asked for.
    pub fn handle(&mut self, event: &Event) -> Outcome {
        let Self {
            state,
            router,
            surfaces,
            ..
        } = self;
        let context = RouterContext {
            surfaces,
            // Derived from whichever surface holds focus, never asserted here (SURF-3).
            focus: state.keyboard_focus(surfaces),
            focused: state.focused(surfaces),
            // A fact about the frame that was drawn, not about intent: `Escape` resolves the
            // layer the user can see (FR-3).
            dismissible: surfaces.has_dismissible(),
            selecting: state.selection().is_some(),
        };
        match router.translate(event, &context) {
            Routed::Intent(intent) => self.apply(intent),
            Routed::Ignored(_) => Outcome::default(),
        }
    }

    /// Draws a frame if the projection changed since the last one, and reports what it cost.
    ///
    /// `None` means nothing needed painting. Ambient background activity and input the workspace
    /// ignores must not cost a full-screen redraw (FR-1), and a caller that cannot tell the
    /// difference cannot measure how often that gate actually fires.
    pub fn draw<B: Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
    ) -> Result<Option<FrameWork>, B::Error> {
        if self.painted == Some(self.state.revision()) {
            return Ok(None);
        }
        let wrapped = self.metrics.wrapped();
        let built = self.metrics.lines_built();

        let Self {
            state,
            metrics,
            palette,
            ..
        } = self;
        let mut drawn = SurfaceTree::default();
        terminal.draw(|frame| drawn = render(frame, state, palette, metrics))?;

        self.surfaces = drawn;
        self.painted = Some(self.state.revision());
        self.frames = self.frames.saturating_add(1);
        Ok(Some(FrameWork {
            items_wrapped: self.metrics.wrapped().saturating_sub(wrapped),
            lines_built: self.metrics.lines_built().saturating_sub(built),
        }))
    }

    /// Applies one intent to the workspace.
    ///
    /// Intents whose reducer arrives in a later delivery step are listed explicitly rather than
    /// caught by a wildcard, so a new intent cannot be added and silently do nothing.
    fn apply(&mut self, intent: TuiIntent) -> Outcome {
        match intent {
            TuiIntent::Quit => return Outcome::quit(),
            TuiIntent::Text(edit) => {
                return Outcome {
                    submitted: self.state.edit(&self.surfaces, edit),
                    ..Outcome::default()
                };
            }
            TuiIntent::Selection(SelectionIntent::Extend(direction)) => {
                self.state.select(&self.surfaces, direction);
            }
            TuiIntent::Selection(SelectionIntent::Copy) => {
                return Outcome {
                    copied: self.state.copy(),
                    ..Outcome::default()
                };
            }
            TuiIntent::MoveSelection(direction) => self.state.move_selection(direction),
            TuiIntent::CycleFocus(direction) => self.state.cycle_focus(&self.surfaces, direction),
            // A press focuses what it hit; every step of the gesture then reaches the reducer,
            // which is where an edge drag becomes a height.
            TuiIntent::Pointer(pointer) => {
                if let PointerIntent::Press { surface, .. } = pointer {
                    self.state.focus_surface(&self.surfaces, surface);
                }
                self.state.drag(&self.surfaces, pointer);
            }
            TuiIntent::Inspector(inspector) => self.state.inspect(&self.surfaces, inspector),
            TuiIntent::Attention(attention) => self.state.attend(&self.surfaces, attention),
            // The phase's one dismissible layer. `Escape` reaches here only when the router found
            // nothing closer to resolve, which is the ladder's last rung before nothing (INV-6).
            TuiIntent::Dismiss => {
                self.state.dismiss(&self.surfaces);
            }
            // A resize leaves the projection unchanged, so the repaint gate has to be told that the
            // painted frame no longer describes the screen (FR-1).
            TuiIntent::TerminalResized { .. } => self.painted = None,
            // Hover routing: the wheel moves the viewport under the pointer and never touches focus
            // (INV-3). Which surface that is was already decided by viewport eligibility.
            TuiIntent::Scroll { surface, direction } => {
                self.state
                    .scroll(&self.surfaces, &self.metrics, surface, direction);
            }
        }
        Outcome::default()
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{
        Terminal,
        backend::TestBackend,
        crossterm::event::{
            Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
        },
        layout::Rect,
    };

    use plexmaton_core::{AgentId, AttentionId, AttentionKind, PrototypeEvent};

    use super::{Flow, Outcome, Workspace};
    use crate::{
        surface::SurfaceId,
        test_support::{Conversation, canonical_runtime},
    };

    /// A workspace holding the canonical timeline, with one frame already drawn.
    fn drawn(width: u16, height: u16) -> (Workspace, Terminal<TestBackend>) {
        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(width, height))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        workspace.emit(canonical_runtime().ready(u64::MAX));
        workspace
            .draw(&mut terminal)
            .unwrap_or_else(|error| panic!("test render: {error}"));
        (workspace, terminal)
    }

    fn press(code: KeyCode, modifiers: KeyModifiers) -> Event {
        Event::Key(KeyEvent::new(code, modifiers))
    }

    fn mouse(kind: MouseEventKind, column: u16, row: u16) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    }

    fn bounds(workspace: &Workspace, surface_id: SurfaceId) -> Rect {
        workspace
            .surfaces
            .get(surface_id)
            .unwrap_or_else(|| panic!("{surface_id:?} must be registered"))
            .bounds
    }

    /// FR-1: a frame is drawn when something changed and at no other time.
    ///
    /// Both halves matter and they fail differently. Without the gate the workspace repaints on
    /// every idle tick; without the resize arm it never repaints after a resize, because a resize
    /// changes what a frame means without changing anything the revision counts.
    #[test]
    fn a_frame_is_drawn_only_when_something_changed() {
        let (mut workspace, mut terminal) = drawn(120, 24);
        assert_eq!(workspace.frames(), 1, "the first frame always paints");

        let work = workspace
            .draw(&mut terminal)
            .unwrap_or_else(|error| panic!("test render: {error}"));
        assert_eq!(work, None, "an unchanged projection must not repaint");

        assert_eq!(
            workspace.handle(&press(KeyCode::Char('x'), KeyModifiers::NONE)),
            Outcome::default(),
            "an unbound key asks for nothing"
        );
        assert_eq!(
            workspace
                .draw(&mut terminal)
                .unwrap_or_else(|error| panic!("test render: {error}")),
            None,
            "and an unbound key must not force a redraw"
        );

        workspace.handle(&Event::Resize(100, 40));
        assert!(
            workspace
                .draw(&mut terminal)
                .unwrap_or_else(|error| panic!("test render: {error}"))
                .is_some(),
            "a resize changes no projection state, and must still force the next frame"
        );
        assert_eq!(workspace.frames(), 2, "exactly two frames reached a screen");
    }

    #[test]
    fn ctrl_c_quits_from_anywhere() {
        let (mut workspace, _terminal) = drawn(120, 24);

        assert_eq!(
            workspace.handle(&press(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Outcome::quit()
        );
    }

    /// SURF-3 through the loop: `CycleFocus` and `Press` have consumers, not just tests.
    #[test]
    fn tab_walks_the_ring_and_a_click_focuses_the_region_it_landed_in() {
        let (mut workspace, _terminal) = drawn(120, 24);

        assert_eq!(focused(&workspace), Some(SurfaceId::Agents));

        workspace.handle(&press(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(focused(&workspace), Some(SurfaceId::Transcript));

        workspace.handle(&press(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert_eq!(focused(&workspace), Some(SurfaceId::Agents));

        let activity = bounds(&workspace, SurfaceId::Activity);
        workspace.handle(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: activity.x,
            row: activity.y,
            modifiers: KeyModifiers::NONE,
        }));
        assert_eq!(
            focused(&workspace),
            Some(SurfaceId::Activity),
            "a press focuses the surface it hit"
        );
    }

    /// The wheel resolves against the frame that was drawn, and against nothing before one exists.
    ///
    /// A tree with no measured viewport is what the loop holds until the first paint. Guessing a
    /// target there would scroll a surface whose size nothing has established.
    #[test]
    fn the_wheel_moves_a_drawn_viewport_and_nothing_before_one_exists() {
        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(60, 20))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        let mut conversation = Conversation::canonical();
        conversation.extend(8);
        workspace.state = conversation.state;

        // Before the first frame the pointer is over a workspace nothing has laid out.
        let before = workspace.state.revision();
        let blind = Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 30,
            row: 10,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(workspace.handle(&blind), Outcome::default());
        assert_eq!(
            workspace.state.revision(),
            before,
            "an unlaid-out workspace has no viewport to move"
        );

        workspace
            .draw(&mut terminal)
            .unwrap_or_else(|error| panic!("test render: {error}"));
        let transcript = bounds(&workspace, SurfaceId::Transcript);
        workspace.handle(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: transcript.x.saturating_add(1),
            row: transcript.y.saturating_add(1),
            modifiers: KeyModifiers::NONE,
        }));

        assert!(
            workspace.state.revision() > before,
            "a drawn viewport moves under the wheel"
        );
        assert!(
            workspace
                .draw(&mut terminal)
                .unwrap_or_else(|error| panic!("test render: {error}"))
                .is_some(),
            "and the scroll is a visible change, so it repaints"
        );
    }

    /// FR-2, at a scale an argument cannot cover: what a frame builds does not grow with history.
    ///
    /// The two conversations differ by a factor of ten, and a steady frame has to cost the same in
    /// both. Running one size only would pass against a renderer that still builds everything, so
    /// the comparison is the test and the absolute numbers are not.
    ///
    /// The cold frame is the deliberate exception, asserted rather than hidden: knowing how tall a
    /// conversation is means wrapping every item once, and that is what buys every later frame.
    #[test]
    fn frame_work_is_bounded_by_the_viewport_and_not_by_the_history() {
        let mut steady = Vec::new();
        for items in [200_usize, 2000] {
            let mut workspace = Workspace::default();
            let mut terminal = Terminal::new(TestBackend::new(80, 24))
                .unwrap_or_else(|error| panic!("test terminal: {error}"));
            let mut conversation = Conversation::canonical();
            conversation.extend(items);
            workspace.emit(conversation.drain());

            let cold = frame(&mut workspace, &mut terminal);
            assert_eq!(
                cold.items_wrapped,
                items.saturating_add(1),
                "a cold frame measures every item exactly once, the canonical opener included"
            );

            // A streaming delta into the newest item, which is the frame that has to stay cheap.
            conversation.append(" One more sentence of streamed text arrives.");
            workspace.emit(conversation.drain());
            let delta = frame(&mut workspace, &mut terminal);
            assert_eq!(
                delta.items_wrapped, 1,
                "a delta re-measures the item it changed and nothing behind it"
            );

            assert_eq!(
                workspace.metrics().retained(),
                items.saturating_add(1),
                "the cache holds one entry per item and no more"
            );
            steady.push((delta.lines_built, cold.lines_built));
        }

        let [small, large] = steady
            .as_slice()
            .try_into()
            .unwrap_or_else(|_| panic!("two sizes were measured"));
        assert_eq!(
            small, large,
            "a ten-fold longer conversation built a different number of lines, so the frame is \
             still paying for history: {small:?} then {large:?}"
        );
    }

    /// FR-2: moving the reader costs no measurement at all.
    ///
    /// Scrolling changes neither an item's revision nor the panel's width, so every height it needs
    /// is already cached. A frame that re-wrapped here would make the wheel the most expensive
    /// thing in the workspace.
    #[test]
    fn scrolling_a_measured_conversation_wraps_nothing() {
        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(80, 24))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        let mut conversation = Conversation::canonical();
        conversation.extend(400);
        workspace.emit(conversation.drain());
        frame(&mut workspace, &mut terminal);

        let transcript = bounds(&workspace, SurfaceId::Transcript);
        for _ in 0..8 {
            workspace.handle(&Event::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: transcript.x.saturating_add(1),
                row: transcript.y.saturating_add(1),
                modifiers: KeyModifiers::NONE,
            }));
            assert_eq!(
                frame(&mut workspace, &mut terminal).items_wrapped,
                0,
                "the wheel must read cached heights, never recompute them"
            );
        }
    }

    fn painted(
        terminal: &Terminal<TestBackend>,
        workspace: &Workspace,
        surface_id: SurfaceId,
    ) -> String {
        crate::test_support::region_text(terminal.backend().buffer(), bounds(workspace, surface_id))
    }

    /// Resolved against the frame that was drawn, which is the only focus a key can act on (FR-3).
    fn focused(workspace: &Workspace) -> Option<SurfaceId> {
        workspace.state.focused(&workspace.surfaces)
    }

    /// One event and the frame that follows it, the way the loop runs them.
    ///
    /// Batching events without a frame between them is a different thing to test: focus and hit
    /// testing resolve against the registry the *last frame* drew, so two events in a row would
    /// have the second one reading geometry the user never saw (FR-3).
    fn step(workspace: &mut Workspace, terminal: &mut Terminal<TestBackend>, event: &Event) {
        workspace.handle(event);
        let _frame = workspace
            .draw(terminal)
            .unwrap_or_else(|error| panic!("test render: {error}"));
    }

    /// Walks the focus ring to one surface rather than counting presses.
    ///
    /// Counting `Tab`s encodes the ring's current membership into every test that walks it, and the
    /// ring legitimately gains and loses stops. What is being asserted is that the surface is
    /// reachable by keyboard, which is what this asks.
    fn tab_to(workspace: &mut Workspace, terminal: &mut Terminal<TestBackend>, target: SurfaceId) {
        let tab = press(KeyCode::Tab, KeyModifiers::NONE);
        for _ in 0..=workspace.surfaces.len() {
            if focused(workspace) == Some(target) {
                return;
            }
            step(workspace, terminal, &tab);
        }
        panic!("{target:?} is not a stop on this frame's focus ring");
    }

    fn cursor(terminal: &Terminal<TestBackend>) -> Option<ratatui::layout::Position> {
        let backend = terminal.backend();
        backend.cursor_visible().then(|| backend.cursor_position())
    }

    /// Draws and insists the frame happened, for tests whose subject is what one cost.
    fn frame(workspace: &mut Workspace, terminal: &mut Terminal<TestBackend>) -> super::FrameWork {
        workspace
            .draw(terminal)
            .unwrap_or_else(|error| panic!("test render: {error}"))
            .unwrap_or_else(|| panic!("this frame was expected to paint"))
    }

    /// D-026 and INV-6 through the executable: opening focuses, and `Escape` gives focus back.
    ///
    /// The ladder has had no consumer since step 1, so this is the first time `Dismiss` resolves
    /// anything. `Escape` with nothing open must still not quit, which is the other half of INV-6
    /// and the reason the ladder exists at all.
    #[test]
    fn enter_opens_the_inspector_and_escape_returns_focus_to_the_conversation() {
        let (mut workspace, mut terminal) = drawn(120, 40);

        assert!(
            workspace.surfaces.get(SurfaceId::Inspector).is_none(),
            "nothing is open until the user asks"
        );
        assert_eq!(
            workspace.handle(&press(KeyCode::Esc, KeyModifiers::NONE)),
            Outcome::default(),
            "and Escape with nothing to dismiss is not a quit"
        );

        workspace.handle(&press(KeyCode::Enter, KeyModifiers::NONE));
        frame(&mut workspace, &mut terminal);
        assert!(workspace.surfaces.get(SurfaceId::Inspector).is_some());
        assert_eq!(
            focused(&workspace),
            Some(SurfaceId::Inspector),
            "opening one is an explicit action, so it is usable without a second step"
        );

        workspace.handle(&press(KeyCode::Esc, KeyModifiers::NONE));
        frame(&mut workspace, &mut terminal);
        assert!(workspace.surfaces.get(SurfaceId::Inspector).is_none());
        assert_eq!(
            focused(&workspace),
            Some(SurfaceId::Transcript),
            "closing returns the keyboard to the conversation, not to the top of the ring"
        );
    }

    /// A pin is what puts two different agents on the screen at once.
    #[test]
    fn a_pinned_inspector_keeps_its_agent_while_the_conversation_moves_on() {
        let (mut workspace, mut terminal) = drawn(120, 40);
        // Look at agent B and peek it.
        workspace.handle(&press(KeyCode::Down, KeyModifiers::NONE));
        workspace.handle(&press(KeyCode::Enter, KeyModifiers::NONE));
        frame(&mut workspace, &mut terminal);
        assert!(painted(&terminal, &workspace, SurfaceId::Inspector).contains("Agent B"));

        // Returning to the rail is what gives the arrows back their meaning. While the inspector
        // holds focus it holds the cursor, and an arrow under a cursor is not a list movement
        // (INV-2); anywhere else an arrow scrolls the surface it is in (INV-10). Only in the rail
        // does it choose an agent, which is what the collapsed composer row's `⇥` leads back to.
        workspace.handle(&press(KeyCode::BackTab, KeyModifiers::SHIFT));
        workspace.handle(&press(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert_eq!(focused(&workspace), Some(SurfaceId::Agents));
        workspace.handle(&press(KeyCode::Up, KeyModifiers::NONE));
        frame(&mut workspace, &mut terminal);
        assert!(
            painted(&terminal, &workspace, SurfaceId::Inspector).contains("Agent A"),
            "an unpinned peek follows the user rather than ending when they look away"
        );

        // Pin it, then move on: the inspector keeps B while the conversation shows A.
        workspace.handle(&press(KeyCode::Down, KeyModifiers::NONE));
        workspace.handle(&press(KeyCode::Char('p'), KeyModifiers::CONTROL));
        workspace.handle(&press(KeyCode::Up, KeyModifiers::NONE));
        frame(&mut workspace, &mut terminal);

        assert!(
            painted(&terminal, &workspace, SurfaceId::Inspector).contains("Agent B"),
            "a pinned inspector keeps the agent the user pinned"
        );
        assert!(
            painted(&terminal, &workspace, SurfaceId::Transcript).contains("Agent A"),
            "while the conversation underneath is the one they went back to"
        );
    }

    /// COM-1, D-018, D-022 and D-027: two inputs exist, one cursor does, and neither costs the
    /// conversation its rows.
    ///
    /// This is the first time "exactly one cursor" is a claim that could fail. Until now there was
    /// one text input in the workspace, so the invariant held by construction; now the inspector
    /// carries the inspected agent's steer input and focus is the only thing deciding which of the
    /// two has the caret.
    ///
    /// Every event is followed by a frame, the way the loop runs them. Focus cycles against the
    /// tree the last frame drew (FR-3), so batching two focus changes without a frame between
    /// would be asking the ring about a surface that had not been registered yet.
    #[test]
    fn the_inspector_takes_the_cursor_and_the_composer_keeps_one_row() {
        let (mut workspace, mut terminal) = drawn(120, 40);
        let tab = press(KeyCode::Tab, KeyModifiers::NONE);

        // Walk to the composer and leave a draft there.
        tab_to(&mut workspace, &mut terminal, SurfaceId::Composer);
        for character in "to the primary".chars() {
            step(
                &mut workspace,
                &mut terminal,
                &press(KeyCode::Char(character), KeyModifiers::NONE),
            );
        }
        let expanded = bounds(&workspace, SurfaceId::Composer).height;
        assert!(expanded > 1, "an uncollapsed composer is a bordered region");

        // Step off it before opening anything: under a cursor `Enter` submits, and opening an
        // inspector is not something typing can do by accident (INV-2). Then peek a *different*
        // agent — a draft is keyed by who it addresses, so an inspector pointed at the primary
        // agent would correctly be showing the very same draft as the composer.
        tab_to(&mut workspace, &mut terminal, SurfaceId::Agents);
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Down, KeyModifiers::NONE),
        );
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Enter, KeyModifiers::NONE),
        );

        assert_eq!(
            bounds(&workspace, SurfaceId::Composer).height,
            1,
            "one row of jump, not three (D-027)"
        );
        assert!(
            painted(&terminal, &workspace, SurfaceId::Composer).contains("to return"),
            "the collapsed row still says where typing would go and how to get back"
        );
        let caret = cursor(&terminal).unwrap_or_else(|| panic!("a focused input owns the cursor"));
        assert!(
            bounds(&workspace, SurfaceId::Inspector).contains(caret),
            "the caret is at {caret:?}, and it belongs to the input that has focus"
        );
        let focused_conversation = bounds(&workspace, SurfaceId::Transcript).height;

        // Step out of the inspector: its input stops existing, and so does the caret (D-018).
        step(&mut workspace, &mut terminal, &tab);
        assert_eq!(cursor(&terminal), None);
        assert_eq!(
            bounds(&workspace, SurfaceId::Composer).height,
            expanded,
            "and the primary composer comes back to full size"
        );
        assert!(
            focused_conversation >= bounds(&workspace, SurfaceId::Transcript).height,
            "the steer input costs the conversation nothing: its rows come out of the inspector's \
             own budget (D-022), and the collapsing composer gives two more back"
        );

        // Back in, and type: the primary draft is untouched, because a draft belongs to the
        // conversation it addresses rather than to whichever input has focus.
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::BackTab, KeyModifiers::SHIFT),
        );
        for character in "hold on".chars() {
            step(
                &mut workspace,
                &mut terminal,
                &press(KeyCode::Char(character), KeyModifiers::NONE),
            );
        }
        assert_eq!(workspace.state.composer().draft(), "to the primary");
        assert!(painted(&terminal, &workspace, SurfaceId::Inspector).contains("hold on"));

        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Esc, KeyModifiers::NONE),
        );
        assert_eq!(
            cursor(&terminal),
            None,
            "the conversation is not a text input, so no caret is on screen"
        );
        assert_eq!(bounds(&workspace, SurfaceId::Composer).height, expanded);
    }

    fn selected(workspace: &Workspace) -> String {
        workspace
            .state
            .selected_agent()
            .map_or_else(|| "none".to_owned(), |agent| agent.id.to_string())
    }

    /// ATT-1: a request arrives and the workspace carries on.
    ///
    /// This is the exit gate's "background action-required events enter the Attention queue without
    /// stealing focus or opening a modal", asserted against a user who is mid-sentence rather than
    /// against an idle screen — which is the only state in which the claim is worth anything.
    #[test]
    fn a_background_request_takes_no_focus_no_selection_and_no_cursor() {
        let mut runtime = canonical_runtime();
        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 40))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        // Stop the timeline one tick before agent B asks for a decision.
        workspace.emit(runtime.ready(11));
        frame(&mut workspace, &mut terminal);
        assert_eq!(workspace.state.attention_count(), 0, "nothing queued yet");

        tab_to(&mut workspace, &mut terminal, SurfaceId::Composer);
        for character in "half a thought".chars() {
            step(
                &mut workspace,
                &mut terminal,
                &press(KeyCode::Char(character), KeyModifiers::NONE),
            );
        }
        let was_focused = focused(&workspace);
        let was_selected = selected(&workspace);
        let caret = cursor(&terminal);
        assert!(caret.is_some(), "the user is typing");

        workspace.emit(runtime.ready(12));
        frame(&mut workspace, &mut terminal);

        assert_eq!(workspace.state.attention_pending(), 1, "it did queue");
        assert!(
            workspace.surfaces.get(SurfaceId::Attention).is_some(),
            "and queueing is visible, or the user has no way to choose when to answer"
        );
        assert_eq!(focused(&workspace), was_focused, "focus did not move");
        assert_eq!(selected(&workspace), was_selected, "nor did the selection");
        assert_eq!(cursor(&terminal), caret, "nor did the cursor");
        assert_eq!(
            workspace.state.composer().draft(),
            "half a thought",
            "and the half-written sentence is still there"
        );
        assert!(
            !workspace.surfaces.has_dismissible(),
            "nothing opened over the user's work"
        );
    }

    /// ATT-2 and ATT-3: going to a request is a keypress, and being seen is not being answered.
    #[test]
    fn going_to_a_request_is_the_users_move_and_marks_it_seen() {
        let (mut workspace, mut terminal) = drawn(120, 40);
        assert_eq!(selected(&workspace), "agent-a");
        assert_eq!(workspace.state.attention_pending(), 1);

        tab_to(&mut workspace, &mut terminal, SurfaceId::Attention);
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Enter, KeyModifiers::NONE),
        );

        assert_eq!(
            selected(&workspace),
            "agent-b",
            "the user chose to go to the agent that asked"
        );
        assert_eq!(
            focused(&workspace),
            Some(SurfaceId::Transcript),
            "and the keyboard went with them"
        );
        assert_eq!(workspace.state.attention_pending(), 0);
        assert_eq!(
            workspace.state.attention_count(),
            1,
            "the request is still outstanding: the user saw it, nothing granted it"
        );
        assert!(
            painted(&terminal, &workspace, SurfaceId::Agents).contains("attention 0"),
            "and the rail counts what is unanswered, not what is queued"
        );
    }

    /// INV-10 inside the queue: its cursor is its own, and arrows there are not agent selection.
    #[test]
    fn the_queues_cursor_moves_without_touching_the_agent_selection() {
        let mut conversation = Conversation::canonical();
        conversation.emit(PrototypeEvent::AttentionRequested {
            agent_id: AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}")),
            attention_id: AttentionId::new("attention-a-1")
                .unwrap_or_else(|error| panic!("fixture: {error}")),
            kind: AttentionKind::Approval,
            summary: "Approve writing the findings file.".into(),
        });
        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 40))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        workspace.emit(conversation.drain());
        frame(&mut workspace, &mut terminal);
        assert_eq!(workspace.state.attention_count(), 2);

        tab_to(&mut workspace, &mut terminal, SurfaceId::Attention);
        let was_selected = selected(&workspace);
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Down, KeyModifiers::NONE),
        );

        assert_eq!(workspace.state.attention_cursor(), 1);
        assert_eq!(
            selected(&workspace),
            was_selected,
            "an arrow in the queue moves the queue, not the rail"
        );

        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Enter, KeyModifiers::NONE),
        );
        assert_eq!(
            selected(&workspace),
            "agent-a",
            "and Enter goes to whichever request the cursor is on"
        );
    }

    /// SEL-3: copying a detail entry returns the value, not the label that was painted.
    ///
    /// The artifact is the case worth pinning: the panel shows a human label on one row and an
    /// indented pointer on the next, and the pointer is what a paste has to contain.
    #[test]
    fn copying_an_artifact_returns_its_pointer_rather_than_its_label() {
        let (mut workspace, mut terminal) = drawn(120, 40);
        // Agent B is the one with a tool and an artifact.
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Down, KeyModifiers::NONE),
        );
        tab_to(&mut workspace, &mut terminal, SurfaceId::Activity);
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Up, KeyModifiers::SHIFT),
        );

        let copied = workspace
            .handle(&press(KeyCode::Char('y'), KeyModifiers::CONTROL))
            .copied
            .unwrap_or_else(|| panic!("a selection must copy to something"));
        assert_eq!(copied.text, "artifact://agent-b/interaction-findings");
        assert_eq!(copied.entries, 1);
        assert!(
            painted(&terminal, &workspace, SurfaceId::Activity).contains("interaction findings"),
            "while the panel is still showing the label, which is the point"
        );

        // One more entry back takes in the tool above it, in list order rather than in the order
        // the two ends were chosen.
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Up, KeyModifiers::SHIFT),
        );
        let copied = workspace
            .handle(&press(KeyCode::Char('y'), KeyModifiers::CONTROL))
            .copied
            .unwrap_or_else(|| panic!("a selection must copy to something"));
        assert_eq!(
            copied.text,
            "inspect interaction fixtures\nartifact://agent-b/interaction-findings"
        );
    }

    /// INV-6 with three rungs: `Escape` resolves the selection before the surface holding it.
    #[test]
    fn escape_clears_the_selection_before_it_closes_the_inspector() {
        let (mut workspace, mut terminal) = drawn(120, 40);
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Enter, KeyModifiers::NONE),
        );
        assert!(workspace.surfaces.get(SurfaceId::Inspector).is_some());

        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Up, KeyModifiers::SHIFT),
        );
        assert!(
            workspace.state.selection().is_some(),
            "the inspector's own detail is selectable, even though it holds a cursor"
        );

        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Esc, KeyModifiers::NONE),
        );
        assert!(workspace.state.selection().is_none());
        assert!(
            workspace.surfaces.get(SurfaceId::Inspector).is_some(),
            "one layer per press: the surface the selection was made in survives it"
        );

        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Esc, KeyModifiers::NONE),
        );
        assert!(workspace.surfaces.get(SurfaceId::Inspector).is_none());
    }

    /// D-028 and INV-4: the bottom edge follows the pointer, even out of the rectangle.
    ///
    /// Capture is the whole reason a drag is usable: the edge the user grabbed keeps moving after
    /// the pointer has left the surface, which is where a resize gesture spends most of its time.
    /// The clamp is the other half — a drag is a choice inside the ten-row guarantee, never a way
    /// out of it (D-023).
    #[test]
    fn dragging_the_inspectors_edge_resizes_it_and_capture_survives_leaving_the_rectangle() {
        let (mut workspace, mut terminal) = drawn(120, 40);
        workspace.handle(&press(KeyCode::Enter, KeyModifiers::NONE));
        frame(&mut workspace, &mut terminal);

        let shelf = bounds(&workspace, SurfaceId::Inspector);
        let edge = shelf.bottom().saturating_sub(1);
        let column = shelf.x.saturating_add(2);
        workspace.handle(&mouse(
            MouseEventKind::Down(MouseButton::Left),
            column,
            edge,
        ));

        // Well past the bottom of the surface, which is where capture starts mattering.
        workspace.handle(&mouse(
            MouseEventKind::Drag(MouseButton::Left),
            column,
            edge.saturating_add(4),
        ));
        frame(&mut workspace, &mut terminal);
        assert_eq!(
            bounds(&workspace, SurfaceId::Inspector).height,
            shelf.height.saturating_add(4),
            "the edge followed the pointer out of the rectangle"
        );

        // Off the bottom of the terminal entirely: it stops where the guarantee does.
        workspace.handle(&mouse(MouseEventKind::Drag(MouseButton::Left), column, 200));
        frame(&mut workspace, &mut terminal);
        assert_eq!(
            bounds(&workspace, SurfaceId::Transcript).height,
            10,
            "the conversation keeps its ten rows however far the pointer goes"
        );

        let settled = bounds(&workspace, SurfaceId::Inspector).height;
        workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), column, 200));
        workspace.handle(&mouse(
            MouseEventKind::Drag(MouseButton::Left),
            column,
            shelf.y.saturating_add(4),
        ));
        assert_eq!(
            bounds(&workspace, SurfaceId::Inspector).height,
            settled,
            "a drag after release has no capture and must move nothing (INV-5)"
        );
    }

    /// Every mouse interaction has a keyboard equivalent, and both land in the same place.
    #[test]
    fn the_keyboard_moves_the_inspectors_edge_the_same_way_the_pointer_does() {
        let (mut workspace, mut terminal) = drawn(120, 40);
        workspace.handle(&press(KeyCode::Enter, KeyModifiers::NONE));
        frame(&mut workspace, &mut terminal);
        let grow = press(KeyCode::Down, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
        let shrink = press(KeyCode::Up, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
        let rows = |workspace: &Workspace| bounds(workspace, SurfaceId::Inspector).height;

        let opened = rows(&workspace);
        workspace.handle(&grow);
        frame(&mut workspace, &mut terminal);
        assert_eq!(rows(&workspace), opened.saturating_add(1));

        workspace.handle(&shrink);
        frame(&mut workspace, &mut terminal);
        assert_eq!(rows(&workspace), opened, "and back again");

        // A held key at the boundary is idempotent, because each step is measured from the
        // rectangle that was actually drawn rather than from an unclamped running total.
        for _ in 0..40 {
            workspace.handle(&grow);
            workspace
                .draw(&mut terminal)
                .unwrap_or_else(|error| panic!("test render: {error}"));
        }
        let pinned_at_the_guarantee = rows(&workspace);
        assert_eq!(bounds(&workspace, SurfaceId::Transcript).height, 10);

        workspace.handle(&shrink);
        frame(&mut workspace, &mut terminal);
        assert_eq!(
            rows(&workspace),
            pinned_at_the_guarantee.saturating_sub(1),
            "the first press back off the boundary must move it, not undo forty of them"
        );
    }

    /// COM-2 and INV-7 through the loop: `q` is a letter while the cursor is in the composer.
    #[test]
    fn typing_reaches_the_composer_and_submitting_hands_the_text_back() {
        let (mut workspace, mut terminal) = drawn(120, 24);
        let composer = bounds(&workspace, SurfaceId::Composer);
        workspace.handle(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: composer.x.saturating_add(1),
            row: composer.y.saturating_add(1),
            modifiers: KeyModifiers::NONE,
        }));

        for character in "hi q".chars() {
            let outcome = workspace.handle(&press(KeyCode::Char(character), KeyModifiers::NONE));
            assert_eq!(outcome.flow, Flow::Continue, "typing must never quit");
            assert!(outcome.submitted.is_none());
        }
        assert_eq!(workspace.state.composer().draft(), "hi q");

        let outcome = workspace.handle(&press(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            outcome
                .submitted
                .map(|submission| submission.text)
                .as_deref(),
            Some("hi q")
        );
        assert_eq!(
            workspace.state.composer().draft(),
            "",
            "and the draft is cleared"
        );
        assert!(
            workspace
                .draw(&mut terminal)
                .unwrap_or_else(|error| panic!("test render: {error}"))
                .is_some(),
            "typing changed the screen, so the next frame paints"
        );
    }
}
