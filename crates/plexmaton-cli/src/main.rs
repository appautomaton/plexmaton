use std::{io, time::Duration};

use anyhow::Context;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, Event, EventStream},
    execute,
};
use futures_util::StreamExt;
use plexmaton_core::PrototypeEventEnvelope;
use plexmaton_sim::{Runtime, RuntimeCommand, Scenario};
use plexmaton_tui::{
    Palette, PointerIntent, Routed, Router, RouterContext, SurfaceTree, TuiIntent, ViewRevision,
    ViewState,
};
use ratatui::DefaultTerminal;

const TICK_INTERVAL: Duration = Duration::from_millis(180);

/// Whether the event loop continues after an intent.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Flow {
    #[default]
    Continue,
    Quit,
}

/// Returns the terminal to the user on every exit path, including error and panic.
///
/// Mouse capture is not part of `ratatui::restore`, and a leaked one is worse than a leaked
/// alternate screen: the terminal keeps reporting movement into the user's shell after the process
/// is gone, and nothing on screen explains why. Releasing it here rather than at the end of `run`
/// is what makes that true for the panic path as well.
struct RestoreTerminal;

impl Drop for RestoreTerminal {
    fn drop(&mut self) {
        // Best effort, and deliberately unreported: the process is leaving, and writing a
        // diagnostic to a screen mid-restoration is how a corrupted terminal gets handed back.
        let _ = execute!(io::stdout(), DisableMouseCapture);
        ratatui::restore();
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let scenario = Scenario::canonical().context("build the canonical synthetic scenario")?;
    // The guard is armed before anything is changed, so even a failure to enable capture restores.
    let _restore_terminal = RestoreTerminal;
    let terminal = ratatui::init();
    execute!(io::stdout(), EnableMouseCapture).context("enable mouse reporting")?;
    run(terminal, Runtime::new(scenario)).await
}

async fn run(mut terminal: DefaultTerminal, mut runtime: Runtime) -> anyhow::Result<()> {
    let mut state = ViewState::default();
    let mut tick = 0_u64;
    let mut ticker = tokio::time::interval(TICK_INTERVAL);
    let mut terminal_events = EventStream::new();
    let mut painted: Option<ViewRevision> = None;
    let mut router = Router::default();
    // The router hit-tests against the surfaces the last frame actually drew, never against a
    // second layout computed on the side. Before the first frame nothing is registered, so a
    // pointer event resolves to nothing rather than to a guessed region.
    let mut surfaces = SurfaceTree::default();
    // Named colour roles resolve through the user's own terminal theme by default.
    let palette = Palette::default();

    emit(&mut state, runtime.ready(tick));

    loop {
        // Repaint only when the projection actually changed. Ambient background activity and
        // input that the workspace ignores must not cost a full-screen redraw.
        if painted != Some(state.revision()) {
            let mut drawn = SurfaceTree::default();
            terminal
                .draw(|frame| drawn = plexmaton_tui::render(frame, &state, &palette))
                .context("draw TUI frame")?;
            surfaces = drawn;
            painted = Some(state.revision());
        }

        tokio::select! {
            _ = ticker.tick() => {
                tick = tick.saturating_add(1);
                emit(&mut state, runtime.ready(tick));
            }
            terminal_event = terminal_events.next() => {
                match terminal_event {
                    Some(Ok(event)) => {
                        let outcome = route(&mut router, &surfaces, &event, &mut state, &mut painted);
                        if let Some(text) = outcome.submitted {
                            send(&mut runtime, &mut state, text)?;
                        }
                        if outcome.flow == Flow::Quit {
                            break;
                        }
                    }
                    Some(Err(error)) => return Err(error).context("read terminal event"),
                    None => break,
                }
            }
        }
    }

    Ok(())
}

/// What one terminal event left for the composition root to do.
#[derive(Debug, Default, Eq, PartialEq)]
struct Outcome {
    flow: Flow,
    /// Text the user submitted. Only the runtime may turn it into transcript events, so it leaves
    /// the reducer as a value rather than being written anywhere (COM-3).
    submitted: Option<String>,
}

impl Outcome {
    const fn quit() -> Self {
        Self {
            flow: Flow::Quit,
            submitted: None,
        }
    }
}

/// Translates one terminal event and applies whatever it asked for.
fn route(
    router: &mut Router,
    surfaces: &SurfaceTree,
    event: &Event,
    state: &mut ViewState,
    painted: &mut Option<ViewRevision>,
) -> Outcome {
    let context = RouterContext {
        surfaces,
        // Derived from whichever surface holds focus, never asserted here (SURF-3).
        focus: state.keyboard_focus(surfaces),
        dismissible: false,
    };
    match router.translate(event, &context) {
        Routed::Intent(intent) => apply_intent(state, surfaces, intent, painted),
        Routed::Ignored(_) => Outcome::default(),
    }
}

/// Applies one intent to the workspace.
///
/// Intents whose reducer arrives in a later delivery step are listed explicitly rather than caught
/// by a wildcard, so a new intent cannot be added and silently do nothing.
fn apply_intent(
    state: &mut ViewState,
    surfaces: &SurfaceTree,
    intent: TuiIntent,
    painted: &mut Option<ViewRevision>,
) -> Outcome {
    match intent {
        TuiIntent::Quit => return Outcome::quit(),
        TuiIntent::Text(edit) => {
            return Outcome {
                flow: Flow::Continue,
                submitted: state.edit(edit),
            };
        }
        TuiIntent::MoveSelection(direction) => state.move_selection(direction),
        TuiIntent::CycleFocus(direction) => state.cycle_focus(surfaces, direction),
        // A press focuses what it hit; the rest of the gesture is a drag, which has no consumer
        // until a surface has an edge worth dragging.
        TuiIntent::Pointer(PointerIntent::Press { surface, .. }) => {
            state.focus_surface(surfaces, surface);
        }
        // A resize leaves the projection unchanged, so the revision gate has to be told that the
        // painted frame is no longer valid.
        TuiIntent::TerminalResized { .. } => *painted = None,
        // Hover routing: the wheel moves the viewport under the pointer and never touches focus
        // (INV-3). Which surface that is was already decided by viewport eligibility.
        TuiIntent::Scroll { surface, direction } => state.scroll(surfaces, surface, direction),
        TuiIntent::Dismiss
        | TuiIntent::Pointer(
            PointerIntent::Drag { .. }
            | PointerIntent::Release { .. }
            | PointerIntent::Cancel { .. },
        ) => {}
    }
    Outcome::default()
}

fn emit(state: &mut ViewState, events: Vec<PrototypeEventEnvelope>) {
    for envelope in events {
        // A producer contract violation is a visible, typed notice inside the projection rather
        // than a reason to tear down the user's terminal.
        let _outcome = state.apply(envelope);
    }
}

/// Hands a submitted draft to the runtime and applies whatever it emits in response.
///
/// The projection is never written directly here. A message reaches the screen as the runtime's
/// own events or not at all, which is what keeps the transcript to one writer (COM-3).
fn send(runtime: &mut Runtime, state: &mut ViewState, text: String) -> anyhow::Result<()> {
    let Some(to) = state.primary_agent().map(|agent| agent.id.clone()) else {
        // Nothing has been delegated to yet, so there is no session to deliver into. Dropping the
        // text here would lose it silently; it stays in the draft until an agent exists.
        return Ok(());
    };
    let emitted = runtime
        .submit(RuntimeCommand::SendMessage { to, text })
        .context("submit the composed message")?;
    emit(state, emitted);
    Ok(())
}

#[cfg(test)]
mod tests {
    use crossterm::event::{
        Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use plexmaton_sim::{Runtime, Scenario};
    use plexmaton_tui::{Router, SurfaceId, SurfaceTree, ViewState, Viewport, WorkspaceInput};
    use ratatui::layout::Rect;

    use super::{Flow, Outcome, emit, route, send};

    fn press(code: KeyCode, modifiers: KeyModifiers) -> Event {
        Event::Key(KeyEvent::new(code, modifiers))
    }

    fn workspace() -> SurfaceTree {
        plexmaton_tui::workspace(Rect::new(0, 0, 120, 24), WorkspaceInput::default())
    }

    #[test]
    fn quit_stops_the_loop_and_resize_invalidates_the_painted_frame() {
        let mut router = Router::default();
        let surfaces = SurfaceTree::default();
        let mut state = ViewState::default();
        let mut painted = Some(state.revision());

        assert_eq!(
            route(
                &mut router,
                &surfaces,
                &press(KeyCode::Char('x'), KeyModifiers::NONE),
                &mut state,
                &mut painted,
            ),
            Outcome::default()
        );
        assert!(painted.is_some(), "an unbound key must not force a redraw");

        assert_eq!(
            route(
                &mut router,
                &surfaces,
                &Event::Resize(100, 40),
                &mut state,
                &mut painted,
            ),
            Outcome::default()
        );
        assert!(painted.is_none(), "a resize must force the next redraw");

        assert_eq!(
            route(
                &mut router,
                &surfaces,
                &press(KeyCode::Char('c'), KeyModifiers::CONTROL),
                &mut state,
                &mut painted,
            ),
            Outcome::quit()
        );
    }

    /// SURF-3 through the executable: `CycleFocus` and `Press` have consumers, not just tests.
    ///
    /// The router has produced both intents since step 1. Proving them here rather than only in the
    /// reducer is what distinguishes a wired binary from a translated event nobody listens to.
    #[test]
    fn tab_walks_the_ring_and_a_click_focuses_the_region_it_landed_in() {
        let mut router = Router::default();
        let surfaces = workspace();
        let mut state = ViewState::default();
        let mut painted = Some(state.revision());
        let mut deliver = |event: &Event, state: &mut ViewState| {
            route(&mut router, &surfaces, event, state, &mut painted)
        };

        assert_eq!(state.focused(&surfaces), Some(SurfaceId::Agents));

        deliver(&press(KeyCode::Tab, KeyModifiers::NONE), &mut state);
        assert_eq!(state.focused(&surfaces), Some(SurfaceId::Transcript));

        deliver(&press(KeyCode::BackTab, KeyModifiers::SHIFT), &mut state);
        assert_eq!(state.focused(&surfaces), Some(SurfaceId::Agents));

        let activity = surfaces
            .get(SurfaceId::Activity)
            .unwrap_or_else(|| panic!("a wide workspace registers an activity column"))
            .bounds;
        deliver(
            &Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: activity.x,
                row: activity.y,
                modifiers: KeyModifiers::NONE,
            }),
            &mut state,
        );
        assert_eq!(
            state.focused(&surfaces),
            Some(SurfaceId::Activity),
            "a press focuses the surface it hit"
        );
    }

    /// The wheel has a consumer, and before the first frame it resolves to nothing.
    ///
    /// A tree with no measured viewport is what the loop holds until the first paint. Guessing a
    /// target there would scroll a surface whose size nothing has established.
    #[test]
    fn the_wheel_moves_a_measured_viewport_and_nothing_before_one_exists() {
        let mut router = Router::default();
        let mut surfaces = workspace();
        let mut state = ViewState::default();
        let mut painted = None;
        let bounds = surfaces
            .get(SurfaceId::Transcript)
            .unwrap_or_else(|| panic!("the conversation is always registered"))
            .bounds;
        let wheel = Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: bounds.x.saturating_add(1),
            row: bounds.y.saturating_add(1),
            modifiers: KeyModifiers::NONE,
        });

        route(&mut router, &surfaces, &wheel, &mut state, &mut painted);
        assert_eq!(
            state.scroll_offset(SurfaceId::Transcript),
            None,
            "an unmeasured workspace has no viewport to move"
        );

        surfaces.set_viewport(
            SurfaceId::Transcript,
            Viewport {
                content_rows: 100,
                visible_rows: 10,
                offset: 0,
            },
        );
        route(&mut router, &surfaces, &wheel, &mut state, &mut painted);
        assert!(
            state
                .scroll_offset(SurfaceId::Transcript)
                .is_some_and(|offset| offset > 0),
            "a measured viewport moves"
        );
    }

    /// COM-1 to COM-3 through the executable: typing reaches the runtime and comes back as a
    /// transcript item.
    ///
    /// The round trip is the point. Nothing here writes to the projection, so a message that
    /// appears has been through the same boundary a real runtime will occupy.
    #[test]
    fn a_typed_message_reaches_the_transcript_by_way_of_the_runtime() {
        let mut router = Router::default();
        let surfaces = workspace();
        let mut runtime =
            Runtime::new(Scenario::canonical().unwrap_or_else(|error| panic!("fixture: {error}")));
        let mut state = ViewState::default();
        let mut painted = None;
        emit(&mut state, runtime.ready(u64::MAX));

        state.focus_surface(&surfaces, SurfaceId::Composer);
        assert_eq!(
            state.focused(&surfaces),
            Some(SurfaceId::Composer),
            "the composer is a focus stop"
        );

        let mut deliver = |event: &Event, state: &mut ViewState| {
            route(&mut router, &surfaces, event, state, &mut painted)
        };
        for character in "hi q".chars() {
            let outcome = deliver(
                &press(KeyCode::Char(character), KeyModifiers::NONE),
                &mut state,
            );
            assert_eq!(outcome.flow, Flow::Continue, "typing must never quit");
            assert!(outcome.submitted.is_none());
        }
        assert_eq!(
            state.composer().draft(),
            "hi q",
            "`q` is a letter while the cursor is in the composer (INV-7)"
        );

        let outcome = deliver(&press(KeyCode::Enter, KeyModifiers::NONE), &mut state);
        let submitted = outcome
            .submitted
            .unwrap_or_else(|| panic!("Enter must submit the draft"));
        assert_eq!(submitted, "hi q");
        assert_eq!(state.composer().draft(), "", "and the draft is cleared");

        let before: Vec<_> = state
            .primary_agent()
            .map(|agent| agent.transcript().map(|item| item.role).collect())
            .unwrap_or_default();
        assert!(
            !before.contains(&plexmaton_core::TranscriptRole::User),
            "nothing may appear in the transcript until the runtime emits it"
        );

        send(&mut runtime, &mut state, submitted)
            .unwrap_or_else(|error| panic!("the runtime accepts the message: {error}"));

        let user_items: Vec<_> = state
            .primary_agent()
            .unwrap_or_else(|| panic!("the canonical timeline creates a primary agent"))
            .transcript()
            .filter(|item| item.role == plexmaton_core::TranscriptRole::User)
            .map(|item| item.source.clone())
            .collect();
        assert_eq!(user_items, ["hi q"]);
        assert_eq!(
            state.notices().count(),
            0,
            "a submitted message must not break the sequence the projection is checking"
        );
    }
}
