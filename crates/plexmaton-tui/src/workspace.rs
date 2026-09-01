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
    intent::{PointerIntent, TuiIntent},
    render::render,
    router::{Routed, Router, RouterContext},
    state::{ViewRevision, ViewState},
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
    /// Text the user submitted. Only the runtime may turn it into transcript events, so it leaves
    /// the workspace as a value rather than being written anywhere (COM-3).
    pub submitted: Option<String>,
}

impl Outcome {
    const fn quit() -> Self {
        Self {
            flow: Flow::Quit,
            submitted: None,
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
            dismissible: false,
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
                    flow: Flow::Continue,
                    submitted: self.state.edit(edit),
                };
            }
            TuiIntent::MoveSelection(direction) => self.state.move_selection(direction),
            TuiIntent::CycleFocus(direction) => self.state.cycle_focus(&self.surfaces, direction),
            // A press focuses what it hit; the rest of the gesture is a drag, which has no consumer
            // until a surface has an edge worth dragging.
            TuiIntent::Pointer(PointerIntent::Press { surface, .. }) => {
                self.state.focus_surface(&self.surfaces, surface);
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
            TuiIntent::Dismiss
            | TuiIntent::Pointer(
                PointerIntent::Drag { .. }
                | PointerIntent::Release { .. }
                | PointerIntent::Cancel { .. },
            ) => {}
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
        let focused = |workspace: &Workspace| workspace.state.focused(&workspace.surfaces);

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

    /// Draws and insists the frame happened, for tests whose subject is what one cost.
    fn frame(workspace: &mut Workspace, terminal: &mut Terminal<TestBackend>) -> super::FrameWork {
        workspace
            .draw(terminal)
            .unwrap_or_else(|error| panic!("test render: {error}"))
            .unwrap_or_else(|| panic!("this frame was expected to paint"))
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
        assert_eq!(outcome.submitted.as_deref(), Some("hi q"));
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
