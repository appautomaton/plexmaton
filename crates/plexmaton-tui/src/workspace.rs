//! One iteration of the workspace: events in, terminal events in, at most one frame out.
//!
//! This exists so that "a frame happens only when something changed" is a value a caller can read
//! rather than a side effect buried in an `async fn` around a real terminal. The executable and the
//! measurement harness drive the same object, which is what makes a measured frame the same frame
//! the user gets. The contract is
//! [`specs/frame-loop.md`](../../../.agents/specs/frame-loop.md).

use plexmaton_core::{AgentId, SessionEventEnvelope};
use ratatui::{
    Terminal,
    backend::Backend,
    crossterm::event::{Event, KeyEventKind},
};

use crate::{
    content,
    intent::{PointerIntent, SelectionIntent, TuiIntent},
    render::render,
    router::{Routed, Router, RouterContext},
    state::{
        ApprovalSubmission, CopyRequest, QuitPress, Submission, ViewRevision, ViewState,
        inner_width,
    },
    surface::{Point, SurfaceId, SurfaceTree},
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
    /// Which agent the user asked to interrupt. The target is resolved from the focused
    /// conversation before the command crosses the composition boundary (INV-7).
    pub interrupted: Option<AgentId>,
    /// An answer to the exact approval the user explicitly opened. The runtime routes it to the
    /// loop that owns the pending call (APV-4).
    pub approval: Option<ApprovalSubmission>,
    /// What the user asked to copy. Leaves as a value for the same reason: the clipboard is the
    /// host's, and nothing in this crate may reach for it (SEL-4).
    pub copied: Option<CopyRequest>,
}

impl Outcome {
    const fn quit() -> Self {
        Self {
            flow: Flow::Quit,
            submitted: None,
            interrupted: None,
            approval: None,
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
/// measured (TR-3), and the repaint gate compares against the revision that frame painted (FR-1).
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
    /// Builds a workspace that paints with `palette`.
    ///
    /// The default workspace uses [`Palette::ansi`]. A colourway is a palette, so swapping one is
    /// construction, not a later rewrite of the widgets (`theme`).
    #[must_use]
    pub fn with_palette(palette: Palette) -> Self {
        Self {
            palette,
            ..Self::default()
        }
    }

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
    /// reason to tear down the user's terminal (`state::notices`), so nothing is returned to check
    /// here.
    pub fn emit(&mut self, events: Vec<SessionEventEnvelope>) {
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
        let routed = router.translate(event, &context);
        // Any key but the quit chord withdraws what the status line asked, bound or not: the user
        // pressed something else, so the question is answered (INV-7).
        if let Event::Key(key) = event
            && key.kind != KeyEventKind::Release
            && routed != Routed::Intent(TuiIntent::Quit)
        {
            state.settle_status();
        }
        match routed {
            Routed::Intent(intent) => self.apply(intent),
            Routed::Ignored(_) => Outcome::default(),
        }
    }

    /// Names where the process runs, for the status line. The composition root knows; this crate
    /// never asks the filesystem.
    pub fn set_working_directory(&mut self, path: String) {
        self.state.set_working_directory(path);
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
            TuiIntent::Quit => match self.state.press_quit() {
                QuitPress::Confirmed => return Outcome::quit(),
                QuitPress::Asked => {}
            },
            TuiIntent::Interrupt => {
                return Outcome {
                    interrupted: self.state.interrupt(&self.surfaces),
                    ..Outcome::default()
                };
            }
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
                if let PointerIntent::Press { surface, at } = pointer {
                    self.state.focus_surface(&self.surfaces, surface);
                    // A click in the list is looking at that agent (INS-1), which is what opens
                    // the second window. Read against the painted rows, not the roster's index.
                    if surface == SurfaceId::Agents {
                        self.click_agent(at);
                    }
                }
                self.state.drag(&self.surfaces, pointer);
            }
            TuiIntent::Inspector(inspector) => self.state.inspect(&self.surfaces, inspector),
            TuiIntent::Attention(attention) => self.state.attend(&self.surfaces, attention),
            TuiIntent::Approval(approval) => {
                return Outcome {
                    approval: self.state.decide_approval(approval),
                    ..Outcome::default()
                };
            }
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

    /// Selects the agent painted under a press in the list, if the press landed on one.
    fn click_agent(&mut self, at: Point) {
        let Some(bounds) = self
            .surfaces
            .get(SurfaceId::Agents)
            .map(|surface| surface.bounds)
        else {
            return;
        };
        // Inside the border, then past whatever the list is scrolled by.
        let Some(row) = at.y.checked_sub(bounds.y.saturating_add(1)) else {
            return;
        };
        if row >= bounds.height.saturating_sub(2) {
            return;
        }
        let offset = self
            .surfaces
            .viewport(SurfaceId::Agents)
            .map_or(0, |viewport| viewport.offset);
        let width = inner_width(bounds.width);
        if let Some(agent) = content::agent_at_row(
            &self.state,
            &self.palette,
            width,
            row.saturating_add(offset),
        ) {
            // The agent came from the roster one line ago, so an unknown one is a race with
            // nothing, and selecting it again is the no-op the reducer already makes it.
            let _known = self.state.select_agent(&agent);
        }
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
        style::{Color, Style},
    };

    use plexmaton_core::{
        AgentId, ApprovalDecision, ApprovalId, AttentionId, AttentionRequest, SessionEvent,
        ToolCallId, ToolCapability,
    };

    use super::{Flow, Outcome, Workspace};
    use crate::{
        SubmissionKind,
        surface::{Point, SurfaceId},
        test_support::{Conversation, canonical_runtime},
        theme::{Palette, Role},
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

    #[test]
    fn an_injected_palette_is_the_one_the_frame_paints() {
        let palette = Palette::from_roles(|role| match role {
            Role::Border => Style::new().fg(Color::Magenta),
            role => Palette::ansi().style(role),
        });
        let mut workspace = Workspace::with_palette(palette);
        let mut terminal = Terminal::new(TestBackend::new(120, 24))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        workspace.emit(canonical_runtime().ready(u64::MAX));
        workspace
            .draw(&mut terminal)
            .unwrap_or_else(|error| panic!("test render: {error}"));

        // The conversation is not focused at start, so its corner wears the plain border role.
        let conversation = bounds(&workspace, SurfaceId::Transcript);
        let corner = &terminal.backend().buffer()[(conversation.x, conversation.y)];
        assert_eq!(corner.symbol(), "┌");
        assert_eq!(
            corner.style().fg,
            Some(Color::Magenta),
            "the frame must paint the injected assignment, not the ansi preset"
        );
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

    /// INV-7: the chord asks first, any other key withdraws the question, and only a second press
    /// in a row leaves. The status line is where the asking happens, in the composer's border.
    #[test]
    fn the_quit_chord_asks_once_and_leaves_on_the_second_press() {
        let (mut workspace, mut terminal) = drawn(120, 24);
        workspace.set_working_directory("~/work".to_owned());
        let redraw = |workspace: &mut Workspace, terminal: &mut Terminal<TestBackend>| {
            workspace
                .draw(terminal)
                .unwrap_or_else(|error| panic!("test render: {error}"));
        };
        redraw(&mut workspace, &mut terminal);
        assert!(painted(&terminal, &workspace, SurfaceId::Status).contains("~/work"));

        // From inside a sub-agent's window, because the answer must not depend on which
        // conversation the key was pressed in.
        workspace.handle(&press(KeyCode::Down, KeyModifiers::NONE));
        redraw(&mut workspace, &mut terminal);
        workspace.handle(&press(KeyCode::Enter, KeyModifiers::NONE));
        redraw(&mut workspace, &mut terminal);
        assert_eq!(focused(&workspace), Some(SurfaceId::Inspector));
        assert_eq!(
            workspace.handle(&press(KeyCode::Char('d'), KeyModifiers::CONTROL)),
            Outcome::default(),
            "the first press asks"
        );
        redraw(&mut workspace, &mut terminal);
        let asked = painted(&terminal, &workspace, SurfaceId::Status);
        assert!(asked.contains("press Ctrl-D again to quit"), "{asked}");
        assert!(
            !asked.contains("~/work"),
            "the question replaces the directory"
        );

        assert_eq!(
            workspace
                .handle(&press(KeyCode::Tab, KeyModifiers::NONE))
                .flow,
            Flow::Continue
        );
        redraw(&mut workspace, &mut terminal);
        let withdrawn = painted(&terminal, &workspace, SurfaceId::Status);
        assert!(
            withdrawn.contains("~/work"),
            "any other key withdraws it: {withdrawn}"
        );

        assert_eq!(
            workspace.handle(&press(KeyCode::Char('d'), KeyModifiers::CONTROL)),
            Outcome::default()
        );
        assert_eq!(
            workspace.handle(&press(KeyCode::Char('d'), KeyModifiers::CONTROL)),
            Outcome::quit(),
            "two presses in a row leave"
        );
    }

    /// INV-7: `Ctrl-C` clears the draft, names the addressed turn to interrupt, and never quits.
    #[test]
    fn ctrl_c_clears_the_draft_and_with_none_points_at_the_quit_chord() {
        let (mut workspace, mut terminal) = drawn(120, 24);
        workspace.set_working_directory("~/work".to_owned());
        let composer = bounds(&workspace, SurfaceId::Composer);
        workspace.handle(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: composer.x.saturating_add(1),
            row: composer.y.saturating_add(1),
            modifiers: KeyModifiers::NONE,
        }));
        for character in "hi".chars() {
            workspace.handle(&press(KeyCode::Char(character), KeyModifiers::NONE));
        }
        assert_eq!(workspace.state.composer().draft(), "hi");

        let outcome = workspace.handle(&press(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(outcome.flow, Flow::Continue, "clearing is not quitting");
        assert_eq!(
            outcome.interrupted.as_ref().map(AgentId::as_str),
            Some("agent-a")
        );
        assert_eq!(workspace.state.composer().draft(), "");
        workspace
            .draw(&mut terminal)
            .unwrap_or_else(|error| panic!("test render: {error}"));
        assert!(
            painted(&terminal, &workspace, SurfaceId::Status).contains("~/work"),
            "taking the draft asks nothing"
        );

        let outcome = workspace.handle(&press(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(
            outcome.interrupted.as_ref().map(AgentId::as_str),
            Some("agent-a")
        );
        workspace
            .draw(&mut terminal)
            .unwrap_or_else(|error| panic!("test render: {error}"));
        let hinted = painted(&terminal, &workspace, SurfaceId::Status);
        assert!(hinted.contains("Ctrl-D twice to quit"), "{hinted}");

        let outcome = workspace.handle(&press(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(
            outcome.flow,
            Flow::Continue,
            "however many times: never a quit"
        );
    }

    /// INV-7: the command is addressed by the conversation holding focus, not by selection alone.
    #[test]
    fn ctrl_c_names_the_conversation_it_interrupts() {
        let (mut workspace, mut terminal) = drawn(120, 40);

        workspace.handle(&press(KeyCode::Down, KeyModifiers::NONE));
        frame(&mut workspace, &mut terminal);
        let looking = workspace.handle(&press(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(
            looking.interrupted.as_ref().map(AgentId::as_str),
            Some("agent-a"),
            "looking at a worker does not retarget commands before the user enters its window"
        );

        workspace.handle(&press(KeyCode::Enter, KeyModifiers::NONE));
        frame(&mut workspace, &mut terminal);
        let entered = workspace.handle(&press(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(
            entered.interrupted.as_ref().map(AgentId::as_str),
            Some("agent-b"),
            "the entered conversation owns the interrupt"
        );
    }

    /// SURF-3 through the loop: `CycleFocus` and `Press` have consumers, not just tests.
    #[test]
    fn tab_walks_the_ring_and_a_click_focuses_the_region_it_landed_in() {
        let (mut workspace, _terminal) = drawn(120, 24);

        assert_eq!(focused(&workspace), Some(SurfaceId::Agents));

        workspace.handle(&press(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(
            focused(&workspace),
            Some(SurfaceId::Activity),
            "the ring runs down the agent column first"
        );
        workspace.handle(&press(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(focused(&workspace), Some(SurfaceId::Transcript));

        workspace.handle(&press(KeyCode::BackTab, KeyModifiers::SHIFT));
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

    /// INS-4 and INV-6 through the executable: opening focuses, and `Escape` gives focus back.
    ///
    /// The ladder has had no consumer since step 1, so this is the first time `Dismiss` resolves
    /// anything. `Escape` with nothing open must still not quit, which is the other half of INV-6
    /// and the reason the ladder exists at all.
    #[test]
    fn selecting_another_agent_opens_its_window_and_escape_returns_focus_to_the_conversation() {
        let (mut workspace, mut terminal) = drawn(120, 40);

        assert!(
            workspace.surfaces.get(SurfaceId::Inspector).is_none(),
            "nothing is open until the user looks at someone else"
        );
        assert_eq!(
            workspace.handle(&press(KeyCode::Esc, KeyModifiers::NONE)),
            Outcome::default(),
            "and Escape with nothing to dismiss is not a quit"
        );

        workspace.handle(&press(KeyCode::Down, KeyModifiers::NONE));
        frame(&mut workspace, &mut terminal);
        assert!(workspace.surfaces.get(SurfaceId::Inspector).is_some());
        assert_eq!(
            focused(&workspace),
            Some(SurfaceId::Agents),
            "looking is not entering: the arrows keep working in the list"
        );

        workspace.handle(&press(KeyCode::Enter, KeyModifiers::NONE));
        frame(&mut workspace, &mut terminal);
        assert_eq!(
            focused(&workspace),
            Some(SurfaceId::Inspector),
            "entering is an explicit action, so the window is usable without a second step"
        );

        workspace.handle(&press(KeyCode::Esc, KeyModifiers::NONE));
        frame(&mut workspace, &mut terminal);
        assert!(workspace.surfaces.get(SurfaceId::Inspector).is_none());
        assert_eq!(
            selected(&workspace),
            "none",
            "closing is looking at nobody else"
        );
        assert_eq!(
            focused(&workspace),
            Some(SurfaceId::Transcript),
            "closing returns the keyboard to the conversation, not to the top of the ring"
        );
    }

    /// Rows of the conversation's interior left readable beneath the shelf.
    fn readable_rows(workspace: &Workspace) -> u16 {
        let conversation = bounds(workspace, SurfaceId::Transcript);
        let shelf = bounds(workspace, SurfaceId::Inspector);
        conversation
            .bottom()
            .saturating_sub(1)
            .saturating_sub(shelf.bottom())
    }

    /// The conversation's painted rows beneath the shelf: what the user can still read of it.
    fn painted_beneath(terminal: &Terminal<TestBackend>, workspace: &Workspace) -> String {
        let conversation = bounds(workspace, SurfaceId::Transcript);
        let shelf = bounds(workspace, SurfaceId::Inspector);
        painted(terminal, workspace, SurfaceId::Transcript)
            .lines()
            .skip(usize::from(shelf.bottom().saturating_sub(conversation.y)))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// INS-1: the window shows the agent the user is looking at, floating over the primary's
    /// conversation, which keeps its rectangle and its title; `Escape` closes it. Nothing on
    /// screen is ever shown twice.
    #[test]
    fn the_window_floats_over_the_primary_and_escape_closes_it() {
        let (mut workspace, mut terminal) = drawn(120, 40);
        let before = bounds(&workspace, SurfaceId::Transcript);

        workspace.handle(&press(KeyCode::Down, KeyModifiers::NONE));
        frame(&mut workspace, &mut terminal);
        let conversation = bounds(&workspace, SurfaceId::Transcript);
        let shelf = bounds(&workspace, SurfaceId::Inspector);
        assert_eq!(
            conversation, before,
            "the conversation keeps its whole rectangle"
        );
        assert_eq!(
            conversation.union(shelf),
            conversation,
            "and the window floats inside it"
        );
        assert!(painted(&terminal, &workspace, SurfaceId::Inspector).contains("Agent B"));
        assert!(
            painted(&terminal, &workspace, SurfaceId::Transcript)
                .lines()
                .next()
                .is_some_and(|title| title.contains("Agent A")),
            "the conversation's title stays readable above the window"
        );
        assert!(
            painted_beneath(&terminal, &workspace).contains("assistant"),
            "and the conversation is still readable beneath it"
        );

        workspace.handle(&press(KeyCode::Esc, KeyModifiers::NONE));
        frame(&mut workspace, &mut terminal);
        assert!(
            workspace.surfaces.get(SurfaceId::Inspector).is_none(),
            "Escape is looking at nobody else"
        );
        assert_eq!(selected(&workspace), "none");
    }

    /// A click in the list is the pointer's way of looking at a sub-agent, and it opens the same
    /// window the arrows do (INS-1). Rows are the painted ones, so a wrapped label still hits.
    #[test]
    fn clicking_an_agent_in_the_list_selects_it_and_opens_its_window() {
        let (mut workspace, mut terminal) = drawn(120, 40);
        let list = bounds(&workspace, SurfaceId::Agents);
        let row_of = |label: &str, terminal: &Terminal<TestBackend>, workspace: &Workspace| {
            let index = painted(terminal, workspace, SurfaceId::Agents)
                .lines()
                .position(|line| line.contains(label))
                .unwrap_or_else(|| panic!("{label} is in the list"));
            list.y
                .saturating_add(u16::try_from(index).unwrap_or(u16::MAX))
        };
        let click = |workspace: &mut Workspace, row: u16| {
            workspace.handle(&mouse(
                MouseEventKind::Down(MouseButton::Left),
                list.x.saturating_add(2),
                row,
            ));
            workspace.handle(&mouse(
                MouseEventKind::Up(MouseButton::Left),
                list.x.saturating_add(2),
                row,
            ));
        };
        assert!(
            !painted(&terminal, &workspace, SurfaceId::Agents).contains("Agent A"),
            "the primary is the screen, not a row in the list"
        );

        let row = row_of("Agent B", &terminal, &workspace);
        click(&mut workspace, row);
        frame(&mut workspace, &mut terminal);
        assert_eq!(selected(&workspace), "agent-b");
        assert!(painted(&terminal, &workspace, SurfaceId::Inspector).contains("Agent B"));

        click(&mut workspace, row);
        assert_eq!(
            workspace
                .draw(&mut terminal)
                .unwrap_or_else(|error| panic!("test render: {error}")),
            None,
            "clicking the agent already looked at changes nothing, so it costs no frame (FR-1)"
        );

        click(&mut workspace, list.bottom().saturating_sub(2));
        assert_eq!(
            selected(&workspace),
            "agent-b",
            "a click on an empty row looks at nobody new"
        );

        workspace.handle(&press(KeyCode::Esc, KeyModifiers::NONE));
        frame(&mut workspace, &mut terminal);
        assert!(workspace.surfaces.get(SurfaceId::Inspector).is_none());
    }

    /// COM-1 and INS-5: two inputs exist, one cursor does, and neither costs the
    /// conversation its rows.
    ///
    /// This is the first time "exactly one cursor" is a claim that could fail. Until now there was
    /// one text input in the workspace, so the invariant held by construction; now the inspector
    /// carries the inspected agent's steer input and focus is the only thing deciding which of the
    /// two has the caret.
    ///
    /// COM-1: the one cursor is where the text ends, at a width that made the draft wrap.
    ///
    /// Driven through the real frame rather than through the composer, because the defect this
    /// pins lived in the join: the composer measured itself in newlines, the panel wrapped what it
    /// was given, and the caret was placed against the unwrapped last line. At 60 columns a long
    /// draft painted its tail ending at column 34 while the caret sat at column 59 — on the right
    /// border, which is not a cell any character can be typed into.
    #[test]
    fn a_wrapped_draft_puts_the_caret_at_the_end_of_the_text_not_on_the_border() {
        let (mut workspace, mut terminal) = drawn(60, 24);
        tab_to(&mut workspace, &mut terminal, SurfaceId::Composer);
        // No spaces, so the wrap point is arithmetic rather than a word boundary, and the row the
        // caret must land on is one this test can compute rather than guess.
        for character in "x".repeat(150).chars() {
            step(
                &mut workspace,
                &mut terminal,
                &press(KeyCode::Char(character), KeyModifiers::NONE),
            );
        }

        let composer = bounds(&workspace, SurfaceId::Composer);
        let cursor = terminal
            .get_cursor_position()
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        let inside = composer.width.saturating_sub(2);
        // 150 characters at 58 columns is two full rows and a third holding the remainder.
        let tail = 150_u16 % inside;

        assert_eq!(composer.height, 5, "three wrapped rows and two borders");
        assert_eq!(
            cursor.x,
            composer.x + 1 + tail,
            "the caret sits after the last character, not against the border"
        );
        assert!(
            cursor.x < composer.right() - 1,
            "and the border is not a cell the caret may occupy"
        );
        assert_eq!(
            cursor.y,
            composer.bottom() - 2,
            "on the last of the three wrapped rows, which is the one being typed into"
        );
    }

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
            2,
            "one row and the edge it closes the box with: one row of jump, not three (INS-5)"
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

        // Step out of the window: `Tab` lands on the primary composer, which is what the collapsed
        // row promised, so the window's input stops existing (INS-5) and the caret is now the
        // composer's.
        step(&mut workspace, &mut terminal, &tab);
        assert_eq!(focused(&workspace), Some(SurfaceId::Composer));
        assert_eq!(
            bounds(&workspace, SurfaceId::Composer).height,
            expanded,
            "and the primary composer comes back to full size"
        );
        let caret = cursor(&terminal).unwrap_or_else(|| panic!("the composer owns the cursor now"));
        assert!(
            bounds(&workspace, SurfaceId::Composer).contains(caret),
            "the caret is at {caret:?}, outside the composer"
        );
        assert!(
            focused_conversation >= bounds(&workspace, SurfaceId::Transcript).height,
            "the steer input costs the conversation nothing: its rows come out of the inspector's \
             own budget (INS-5), and the collapsing composer gives two more back"
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

    /// The primary with plenty of history, and B beside it at ultrawide. The two columns are
    /// equals, so the terminal is one cell odd to make them differ by one: two conversations at
    /// two widths in one frame is what the cache has to get right.
    fn with_the_second_agent_beside_the_conversation()
    -> (Workspace, Terminal<TestBackend>, Conversation) {
        let mut conversation = Conversation::canonical();
        conversation.extend(20);

        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(141, 40))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        workspace.emit(conversation.drain());
        workspace
            .draw(&mut terminal)
            .unwrap_or_else(|error| panic!("test render: {error}"));
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Down, KeyModifiers::NONE),
        );
        (workspace, terminal, conversation)
    }

    fn measured(workspace: &Workspace, surface_id: SurfaceId) -> crate::Viewport {
        workspace
            .surfaces
            .viewport(surface_id)
            .unwrap_or_else(|| panic!("{surface_id:?} was drawn, so it has been measured"))
    }

    /// The rows one agent's whole conversation wraps to at `width`, measured the un-virtualized way.
    fn whole_conversation_rows(workspace: &Workspace, agent: &AgentId, width: u16) -> u16 {
        let palette = Palette::default();
        let agent = workspace
            .state
            .agent(agent)
            .unwrap_or_else(|| panic!("the fixture created {agent}"));
        let lines: Vec<_> = agent
            .transcript()
            .flat_map(|item| crate::content::transcript_item(item, &palette, false))
            .collect();
        let rows = ratatui::widgets::Paragraph::new(lines)
            .wrap(ratatui::widgets::Wrap { trim: false })
            .line_count(width);
        u16::try_from(rows).unwrap_or(u16::MAX)
    }

    /// TR-1 through the executable: each panel's rows belong to the width that panel was drawn at.
    ///
    /// Heights were keyed by agent alone, with the width only deciding whether an entry was still
    /// valid — so a conversation drawn at a second width threw away the first's measurements. What
    /// that cost is asserted here rather than argued: a steady frame paints nothing, and a streaming
    /// delta wraps one item at the one width its conversation is on screen at.
    #[test]
    fn a_conversation_drawn_at_two_widths_measures_correctly_at_both() {
        let agent_a = AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}"));
        let agent_b = AgentId::new("agent-b").unwrap_or_else(|error| panic!("fixture: {error}"));
        let (mut workspace, mut terminal, mut conversation) =
            with_the_second_agent_beside_the_conversation();
        let primary = measured(&workspace, SurfaceId::Transcript);
        let inspected = measured(&workspace, SurfaceId::Inspector);

        assert_ne!(
            primary.content_width, inspected.content_width,
            "the two panels have to be different widths or this proves nothing"
        );
        for (agent, viewport) in [(&agent_a, primary), (&agent_b, inspected)] {
            assert_eq!(
                viewport.content_rows,
                whole_conversation_rows(&workspace, agent, viewport.content_width),
                "a panel {} columns wide reported the height of some other width",
                viewport.content_width
            );
        }

        assert_eq!(
            workspace
                .draw(&mut terminal)
                .unwrap_or_else(|error| panic!("test render: {error}")),
            None,
            "and having measured both, an unchanged frame has nothing to repaint"
        );

        conversation.append(" and more streamed text.");
        workspace.emit(conversation.drain());
        let work = frame(&mut workspace, &mut terminal);
        assert_eq!(
            work.items_wrapped, 1,
            "a delta costs one wrap at the width its conversation is drawn at; a whole history is the defect"
        );
    }

    /// TR-3 through the executable: the reader moves by the width of the panel under the wheel.
    ///
    /// The control is the same terminal with nothing open. An inspector showing the same
    /// conversation must not change how far one notch takes its reader — and it did, because the
    /// anchor was resolved against whichever width the cache had measured last, which was the
    /// narrow column's.
    #[test]
    fn a_wheel_notch_moves_the_conversation_the_same_distance_with_an_inspector_open() {
        let notch = |workspace: &mut Workspace, terminal: &mut Terminal<TestBackend>| {
            let over = bounds(workspace, SurfaceId::Transcript);
            let before = measured(workspace, SurfaceId::Transcript).offset;
            step(
                workspace,
                terminal,
                &mouse(MouseEventKind::ScrollUp, over.x + 2, over.y + 2),
            );
            before.saturating_sub(measured(workspace, SurfaceId::Transcript).offset)
        };

        let (mut alone, mut alone_terminal, _events) =
            with_the_second_agent_beside_the_conversation();
        step(
            &mut alone,
            &mut alone_terminal,
            &press(KeyCode::Esc, KeyModifiers::NONE),
        );
        assert!(
            alone.surfaces.get(SurfaceId::Inspector).is_none(),
            "the control has to have closed the inspector"
        );
        let control = notch(&mut alone, &mut alone_terminal);
        assert!(control > 0, "the fixture has to be scrollable");

        let (mut inspecting, mut inspecting_terminal, _events) =
            with_the_second_agent_beside_the_conversation();
        // The second column comes out of the conversation's width (ui-ux §layout classes), so
        // the conversation is
        // narrower with it open; a notch must still move the reader the same number of rows.
        assert!(
            measured(&inspecting, SurfaceId::Transcript).content_width
                < measured(&alone, SurfaceId::Transcript).content_width,
            "the fixture has to change the conversation's width, or this proves nothing"
        );
        assert_eq!(
            notch(&mut inspecting, &mut inspecting_terminal),
            control,
            "one notch moved the reader a different distance because a second panel was open"
        );
    }

    /// INS-7 through the executable: an inspector with no room for its input holds no cursor.
    ///
    /// INS-5 already said a rectangle that cannot hold both keeps the conversation and shows no
    /// input. What it did not say is what focus becomes, and the surface went on reporting text
    /// focus regardless — so the caret was placed at the end of the conversation's last line, as
    /// though a transcript item were an editor, and every keystroke landed in a draft with nothing
    /// on screen to show it. The three now agree: no input, no cursor, no draft.
    #[test]
    fn an_inspector_too_short_for_its_input_takes_no_typing_and_no_cursor() {
        let agent_b = AgentId::new("agent-b").unwrap_or_else(|error| panic!("fixture: {error}"));
        let (mut workspace, mut terminal) = drawn(120, 40);
        let shrink = press(KeyCode::Up, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
        let grow = press(KeyCode::Down, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
        let draft = |workspace: &Workspace| workspace.state.draft(&agent_b).draft().to_owned();

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
        for character in "steer".chars() {
            step(
                &mut workspace,
                &mut terminal,
                &press(KeyCode::Char(character), KeyModifiers::NONE),
            );
        }
        assert_eq!(
            draft(&workspace),
            "steer",
            "there is a draft to be typed into"
        );
        assert!(painted(&terminal, &workspace, SurfaceId::Inspector).contains("Message Agent B"));

        // Down to the height the guarantee clamps at, where a conversation and an input no longer
        // both fit. The loop is bounded by the ring rather than counted: what is asserted is where
        // dragging stops, not how many presses it takes to get there.
        for _ in 0..bounds(&workspace, SurfaceId::Inspector).height {
            step(&mut workspace, &mut terminal, &shrink);
        }
        let squeezed = bounds(&workspace, SurfaceId::Inspector);
        assert_eq!(
            squeezed.height, 3,
            "the shelf is at the smallest height layout will draw"
        );
        assert_eq!(
            focused(&workspace),
            Some(SurfaceId::Inspector),
            "it still holds focus; what changed is what holding focus means"
        );
        assert_eq!(
            workspace.state.keyboard_focus(&workspace.surfaces),
            crate::KeyboardFocus::Navigation,
            "a surface with no input on screen is a navigation surface"
        );
        assert_eq!(
            cursor(&terminal),
            None,
            "and nothing owns a cursor, least of all the conversation"
        );
        assert!(
            !painted(&terminal, &workspace, SurfaceId::Inspector).contains("Message Agent B"),
            "there is no input drawn, which is what INS-5 asks for"
        );

        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Char('x'), KeyModifiers::NONE),
        );
        assert_eq!(
            draft(&workspace),
            "steer",
            "typing must not reach a draft the user cannot see"
        );
        // The consequence of becoming a navigation surface, asserted rather than left to be
        // discovered: with no cursor on screen a letter is a command again, and no bare letter is
        // the quit (INV-7), so this transition does not put an exit under a keypress the user
        // thought was text — `Escape` remains the way out.
        assert_eq!(
            workspace
                .handle(&press(KeyCode::Char('q'), KeyModifiers::NONE))
                .flow,
            Flow::Continue,
            "a bare letter never ends the session"
        );

        // Give the rows back, and the input comes back with the draft that was waiting for it.
        for _ in 0..3 {
            step(&mut workspace, &mut terminal, &grow);
        }
        assert!(painted(&terminal, &workspace, SurfaceId::Inspector).contains("Message Agent B"));
        let caret = cursor(&terminal).unwrap_or_else(|| panic!("the input is back and drawn"));
        assert!(
            bounds(&workspace, SurfaceId::Inspector).contains(caret),
            "the caret is at {caret:?}, outside the inspector"
        );
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Char('!'), KeyModifiers::NONE),
        );
        assert_eq!(
            draft(&workspace),
            "steer!",
            "and typing lands where the caret is"
        );
    }

    fn two_conversations() -> (Workspace, Terminal<TestBackend>) {
        let mut conversation = Conversation::canonical();
        let agent_b = AgentId::new("agent-b").unwrap_or_else(|error| panic!("fixture: {error}"));
        conversation.extend(20).extend_agent(&agent_b, 20);

        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(100, 40))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        workspace.emit(conversation.drain());
        workspace
            .draw(&mut terminal)
            .unwrap_or_else(|error| panic!("test render: {error}"));

        // Look at B: its conversation opens over A's, which stays where it is (INS-1).
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Down, KeyModifiers::NONE),
        );
        assert_eq!(selected(&workspace), "agent-b");
        (workspace, terminal)
    }

    /// The canonical journey's step 5: two agents streaming into independent conversations.
    ///
    /// This is the phase's hardest declared requirement and it had never been exercised — the
    /// inspector drew the same tools, artifacts and mail the activity column already showed for the
    /// selected agent, so there was only ever one conversation on screen. Independence is the part
    /// worth asserting: the reading position belongs to the conversation rather than to the panel
    /// (TR-5), and with two panels that stops being a distinction without a difference.
    #[test]
    fn two_conversations_scroll_independently_and_neither_moves_the_other() {
        let (mut workspace, mut terminal) = two_conversations();

        let conversation = painted_beneath(&terminal, &workspace);
        let inspected = painted(&terminal, &workspace, SurfaceId::Inspector);
        assert_ne!(
            conversation, inspected,
            "two panels showing the same text would prove nothing about independence"
        );

        // Scroll the inspector only. Hover routing puts the wheel where the pointer is, without
        // touching focus, so this is one reader moving and the other staying put (ui-ux §nested
        // scrolling).
        let over_inspector = bounds(&workspace, SurfaceId::Inspector);
        for _ in 0..3 {
            step(
                &mut workspace,
                &mut terminal,
                &mouse(
                    MouseEventKind::ScrollUp,
                    over_inspector.x + 2,
                    over_inspector.y + 2,
                ),
            );
        }

        assert_ne!(
            painted(&terminal, &workspace, SurfaceId::Inspector),
            inspected,
            "the inspected conversation moved"
        );
        assert_eq!(
            painted_beneath(&terminal, &workspace),
            conversation,
            "and the one beneath it did not"
        );

        // Now the other way round, from where each of them is standing.
        let inspected = painted(&terminal, &workspace, SurfaceId::Inspector);
        let over_conversation = bounds(&workspace, SurfaceId::Transcript);
        for _ in 0..3 {
            step(
                &mut workspace,
                &mut terminal,
                &mouse(
                    MouseEventKind::ScrollUp,
                    over_conversation.x + 2,
                    over_conversation.bottom().saturating_sub(2),
                ),
            );
        }

        assert_ne!(painted_beneath(&terminal, &workspace), conversation);
        assert_eq!(
            painted(&terminal, &workspace, SurfaceId::Inspector),
            inspected,
            "the reader the user was not moving stayed exactly where it was"
        );
    }

    /// TR-5 and SURF-5 together: a conversation remembers its reader, and a surface that comes back
    /// comes back where it was left.
    #[test]
    fn an_inspected_conversation_keeps_its_own_reading_position_across_a_close_and_reopen() {
        let (mut workspace, mut terminal) = two_conversations();
        let over_inspector = bounds(&workspace, SurfaceId::Inspector);
        for _ in 0..4 {
            step(
                &mut workspace,
                &mut terminal,
                &mouse(
                    MouseEventKind::ScrollUp,
                    over_inspector.x + 2,
                    over_inspector.y + 2,
                ),
            );
        }
        // The first content row is the anchor: which message the reader is on and how far into it
        // (TR-3). The whole region is the wrong comparison — the height the user dragged to does
        // not survive a close, so the region differs for reasons that are not the reader.
        let anchor = |terminal: &Terminal<TestBackend>, workspace: &Workspace| {
            painted(terminal, workspace, SurfaceId::Inspector)
                .lines()
                .nth(1)
                .unwrap_or_default()
                .to_owned()
        };
        let parked = anchor(&terminal, &workspace);
        assert!(
            parked.contains("Filler") || parked.contains("wrap across") || parked.contains("panel"),
            "the reader must be parked inside a message, not at a boundary: {parked}"
        );

        // Close it from the conversation, so the Escape ladder resolves the inspector rather than a
        // selection, then reopen B the way it was opened the first time.
        tab_to(&mut workspace, &mut terminal, SurfaceId::Transcript);
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Esc, KeyModifiers::NONE),
        );
        assert!(
            workspace.surfaces.get(SurfaceId::Inspector).is_none(),
            "it must actually have closed, or this proves nothing"
        );
        tab_to(&mut workspace, &mut terminal, SurfaceId::Agents);
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Down, KeyModifiers::NONE),
        );

        assert_eq!(
            anchor(&terminal, &workspace),
            parked,
            "the inspected conversation came back on the message its reader had stopped at"
        );
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
        assert_eq!(selected(&workspace), "none");
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
            "the user chose to go to the agent that asked, which opens its window"
        );
        assert_eq!(
            focused(&workspace),
            Some(SurfaceId::Inspector),
            "and the keyboard went with them, into that window"
        );
        assert_eq!(workspace.state.attention_pending(), 0);
        assert_eq!(
            workspace.state.attention_count(),
            1,
            "the request is still outstanding: the user saw it, nothing granted it"
        );
        assert!(
            !painted(&terminal, &workspace, SurfaceId::Agents).contains(" · !"),
            "and the rail's badge counts what is unanswered, so nothing unanswered is no badge"
        );
    }

    /// INV-10 inside the queue: its cursor is its own, and arrows there are not agent selection.
    #[test]
    fn the_queues_cursor_moves_without_touching_the_agent_selection() {
        let mut conversation = Conversation::canonical();
        conversation.emit(SessionEvent::AttentionRequested {
            agent_id: AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}")),
            attention_id: AttentionId::new("attention-a-1")
                .unwrap_or_else(|error| panic!("fixture: {error}")),
            request: AttentionRequest::Approval {
                approval_id: ApprovalId::new("approval-a-1")
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                call_id: ToolCallId::new("tool-a-1")
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                tool: "edit".into(),
                capabilities: vec![ToolCapability::FileWrite],
                detail: "Approve writing the findings file.".into(),
            },
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
            "none",
            "and Enter goes to whichever request the cursor is on: the primary's opens nothing"
        );
    }

    /// APV-4 and SURF-4: only a user action opens the blocking surface, whose answer echoes the
    /// loop's identity; Escape closes presentation without manufacturing a decision (ATT-3).
    #[test]
    fn an_open_approval_blocks_the_workspace_and_returns_only_the_selected_decision() {
        let mut conversation = Conversation::canonical();
        let attention_id = AttentionId::new("attention-b-approval")
            .unwrap_or_else(|error| panic!("fixture: {error}"));
        conversation.emit(SessionEvent::AttentionRequested {
            agent_id: AgentId::new("agent-b").unwrap_or_else(|error| panic!("fixture: {error}")),
            attention_id: attention_id.clone(),
            request: AttentionRequest::Approval {
                approval_id: ApprovalId::new("approval-b-1")
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                call_id: ToolCallId::new("tool-b-write")
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                tool: "edit".into(),
                capabilities: vec![ToolCapability::FileWrite],
                detail: "Change crates/plexmaton-core/src/lib.rs".into(),
            },
        });
        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 40))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        workspace.emit(conversation.drain());
        frame(&mut workspace, &mut terminal);

        tab_to(&mut workspace, &mut terminal, SurfaceId::Attention);
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

        assert_eq!(focused(&workspace), Some(SurfaceId::Approval));
        let card = bounds(&workspace, SurfaceId::Approval);
        assert_eq!(
            workspace.surfaces.hit_test(Point { x: 0, y: 0 }),
            Some(SurfaceId::Approval),
            "a click outside the card cannot reach the workspace below it"
        );
        let card_text = painted(&terminal, &workspace, SurfaceId::Approval);
        assert!(card_text.contains("Change crates/plexmaton-core/src/lib.rs"));
        assert!(
            card_text.contains("> Deny"),
            "safe answer is highlighted: {card_text}"
        );
        assert!(
            card.width < 120 && card.height < 40,
            "the surface is a card, not a replacement screen"
        );

        let denied = workspace.handle(&press(KeyCode::Enter, KeyModifiers::NONE));
        let denied = denied
            .approval
            .unwrap_or_else(|| panic!("Enter returns the highlighted decision"));
        assert_eq!(denied.approval_id.as_str(), "approval-b-1");
        assert_eq!(denied.decision, ApprovalDecision::Deny);
        assert_eq!(
            workspace.state.attention_count(),
            2,
            "the UI did not resolve loop state"
        );

        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Esc, KeyModifiers::NONE),
        );
        assert!(workspace.surfaces.get(SurfaceId::Approval).is_none());
        assert_eq!(
            workspace.state.attention_count(),
            2,
            "Escape keeps the request pending"
        );
        assert_eq!(focused(&workspace), Some(SurfaceId::Inspector));

        tab_to(&mut workspace, &mut terminal, SurfaceId::Attention);
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Enter, KeyModifiers::NONE),
        );
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Up, KeyModifiers::NONE),
        );
        let allowed = workspace
            .handle(&press(KeyCode::Enter, KeyModifiers::NONE))
            .approval
            .unwrap_or_else(|| panic!("Enter returns the highlighted decision"));
        assert_eq!(allowed.approval_id.as_str(), "approval-b-1");
        assert_eq!(allowed.decision, ApprovalDecision::AllowOnce);

        conversation.emit(SessionEvent::AttentionResolved {
            agent_id: AgentId::new("agent-b").unwrap_or_else(|error| panic!("fixture: {error}")),
            attention_id,
        });
        workspace.emit(conversation.drain());
        frame(&mut workspace, &mut terminal);
        assert!(workspace.surfaces.get(SurfaceId::Approval).is_none());
        assert_eq!(workspace.state.attention_count(), 1);
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
            &press(KeyCode::Down, KeyModifiers::NONE),
        );
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

    /// INV-4 across a resize: a gesture in flight when the terminal changes size stays coherent.
    ///
    /// The exit gate asks for this by name, and it is the case where the two owners of a drag can
    /// disagree: the router holds capture across the resize because capture is about the button,
    /// while the surface the gesture is resizing has been relaid out underneath it. The next drag
    /// therefore has to be measured against where the edge *is*, not where it was grabbed.
    #[test]
    fn a_drag_in_flight_survives_the_terminal_changing_size() {
        let (mut workspace, mut terminal) = drawn(120, 40);
        workspace.handle(&press(KeyCode::Down, KeyModifiers::NONE));
        frame(&mut workspace, &mut terminal);

        let shelf = bounds(&workspace, SurfaceId::Inspector);
        let column = shelf.x.saturating_add(2);
        workspace.handle(&mouse(
            MouseEventKind::Down(MouseButton::Left),
            column,
            shelf.bottom().saturating_sub(1),
        ));

        // Narrower and much shorter, but still roomy enough for a shelf: the interesting case is
        // the edge being relaid out under a held button, not the inspector taking the region.
        terminal.backend_mut().resize(100, 30);
        workspace.handle(&Event::Resize(100, 30));
        frame(&mut workspace, &mut terminal);
        assert_eq!(
            workspace.router.capture(),
            Some(SurfaceId::Inspector),
            "capture is about the button, which the user is still holding"
        );

        let after_resize = bounds(&workspace, SurfaceId::Inspector);
        workspace.handle(&mouse(
            MouseEventKind::Drag(MouseButton::Left),
            column,
            after_resize.bottom(),
        ));
        frame(&mut workspace, &mut terminal);
        assert!(
            bounds(&workspace, SurfaceId::Transcript).height >= 10,
            "the ten-row guarantee holds at the new size, measured against the new geometry"
        );

        workspace.handle(&mouse(
            MouseEventKind::Up(MouseButton::Left),
            column,
            after_resize.bottom(),
        ));
        assert_eq!(workspace.router.capture(), None, "and the gesture ended");
    }

    /// INV-4: the bottom edge follows the pointer, even out of the rectangle.
    ///
    /// Capture is the whole reason a drag is usable: the edge the user grabbed keeps moving after
    /// the pointer has left the surface, which is where a resize gesture spends most of its time.
    /// The clamp is the other half — a drag is a choice inside the ten-row guarantee, never a way
    /// out of it (INS-2).
    #[test]
    fn dragging_the_inspectors_edge_resizes_it_and_capture_survives_leaving_the_rectangle() {
        let (mut workspace, mut terminal) = drawn(120, 40);
        workspace.handle(&press(KeyCode::Down, KeyModifiers::NONE));
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
            readable_rows(&workspace),
            10,
            "the conversation keeps its ten readable rows however far the pointer goes"
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
        workspace.handle(&press(KeyCode::Down, KeyModifiers::NONE));
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
        assert_eq!(readable_rows(&workspace), 10);

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
            outcome.submitted.as_ref().map(|submission| submission.kind),
            Some(SubmissionKind::Message),
            "the primary composer names the next-turn route"
        );
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

    /// COM-4: the visible input names both the recipient and the boundary the loop must claim.
    #[test]
    fn the_inspectors_input_submits_steering_for_that_agents_next_step() {
        let (mut workspace, mut terminal) = drawn(120, 40);
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
        for character in "check the cache".chars() {
            workspace.handle(&press(KeyCode::Char(character), KeyModifiers::NONE));
        }

        let submission = workspace
            .handle(&press(KeyCode::Enter, KeyModifiers::NONE))
            .submitted
            .unwrap_or_else(|| panic!("the entered worker input must submit"));

        assert_eq!(submission.to.as_str(), "agent-b");
        assert_eq!(submission.kind, SubmissionKind::Steering);
        assert_eq!(submission.text, "check the cache");
    }
}
