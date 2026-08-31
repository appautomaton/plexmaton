use std::{collections::VecDeque, io, time::Duration};

use anyhow::Context;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, Event, EventStream},
    execute,
};
use futures_util::StreamExt;
use plexmaton_sim::{Scenario, ScenarioStep};
use plexmaton_tui::{
    Palette, PointerIntent, Routed, Router, RouterContext, SurfaceTree, TuiIntent, ViewRevision,
    ViewState,
};
use ratatui::DefaultTerminal;

const TICK_INTERVAL: Duration = Duration::from_millis(180);

/// Whether the event loop continues after an intent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Flow {
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
    run(terminal, scenario.into_steps()).await
}

async fn run(mut terminal: DefaultTerminal, steps: Vec<ScenarioStep>) -> anyhow::Result<()> {
    let mut state = ViewState::default();
    let mut timeline: VecDeque<_> = steps.into();
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

    apply_ready(&mut state, &mut timeline, tick);

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
                apply_ready(&mut state, &mut timeline, tick);
            }
            terminal_event = terminal_events.next() => {
                match terminal_event {
                    Some(Ok(event)) => {
                        if route(&mut router, &surfaces, &event, &mut state, &mut painted) == Flow::Quit {
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

/// Translates one terminal event and applies whatever it asked for.
fn route(
    router: &mut Router,
    surfaces: &SurfaceTree,
    event: &Event,
    state: &mut ViewState,
    painted: &mut Option<ViewRevision>,
) -> Flow {
    let context = RouterContext {
        surfaces,
        // Derived from whichever surface holds focus, never asserted here (SURF-3). Until a
        // composer exists no kind returns `TextInput`, so this is navigation by consequence rather
        // than by assumption.
        focus: state.keyboard_focus(surfaces),
        dismissible: false,
    };
    match router.translate(event, &context) {
        Routed::Intent(intent) => apply_intent(state, surfaces, intent, painted),
        Routed::Ignored(_) => Flow::Continue,
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
) -> Flow {
    match intent {
        TuiIntent::Quit => return Flow::Quit,
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
        TuiIntent::Dismiss
        | TuiIntent::Scroll { .. }
        | TuiIntent::Pointer(
            PointerIntent::Drag { .. }
            | PointerIntent::Release { .. }
            | PointerIntent::Cancel { .. },
        )
        | TuiIntent::Text(_) => {}
    }
    Flow::Continue
}

fn apply_ready(state: &mut ViewState, timeline: &mut VecDeque<ScenarioStep>, tick: u64) {
    while timeline.front().is_some_and(|step| step.at_tick <= tick) {
        let Some(step) = timeline.pop_front() else {
            break;
        };
        // A producer contract violation is a visible, typed notice inside the projection rather
        // than a reason to tear down the user's terminal.
        let _outcome = state.apply(step.envelope);
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{
        Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use plexmaton_tui::{Router, SurfaceId, SurfaceTree, ViewState};
    use ratatui::layout::Rect;

    use super::{Flow, route};

    fn press(code: KeyCode, modifiers: KeyModifiers) -> Event {
        Event::Key(KeyEvent::new(code, modifiers))
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
            Flow::Continue
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
            Flow::Continue
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
            Flow::Quit
        );
    }

    /// SURF-3 through the executable: `CycleFocus` and `Press` have consumers, not just tests.
    ///
    /// The router has produced both intents since step 1. Proving them here rather than only in the
    /// reducer is what distinguishes a wired binary from a translated event nobody listens to.
    #[test]
    fn tab_walks_the_ring_and_a_click_focuses_the_region_it_landed_in() {
        let mut router = Router::default();
        let surfaces = plexmaton_tui::workspace(Rect::new(0, 0, 120, 24), false);
        let mut state = ViewState::default();
        let mut painted = Some(state.revision());
        let mut send = |event: &Event, state: &mut ViewState| {
            route(&mut router, &surfaces, event, state, &mut painted)
        };

        assert_eq!(state.focused(&surfaces), Some(SurfaceId::Agents));

        send(&press(KeyCode::Tab, KeyModifiers::NONE), &mut state);
        assert_eq!(state.focused(&surfaces), Some(SurfaceId::Transcript));

        send(&press(KeyCode::BackTab, KeyModifiers::SHIFT), &mut state);
        assert_eq!(state.focused(&surfaces), Some(SurfaceId::Agents));

        let activity = surfaces
            .get(SurfaceId::Activity)
            .unwrap_or_else(|| panic!("a wide workspace registers an activity column"))
            .bounds;
        send(
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
}
