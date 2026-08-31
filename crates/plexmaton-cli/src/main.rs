use std::{collections::VecDeque, time::Duration};

use anyhow::Context;
use crossterm::event::{Event, EventStream};
use futures_util::StreamExt;
use plexmaton_sim::{Scenario, ScenarioStep};
use plexmaton_tui::{
    KeyboardFocus, Palette, Routed, Router, RouterContext, SurfaceTree, TuiIntent, ViewRevision,
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

struct RestoreTerminal;

impl Drop for RestoreTerminal {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let scenario = Scenario::canonical().context("build the canonical synthetic scenario")?;
    let _restore_terminal = RestoreTerminal;
    let terminal = ratatui::init();
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
        // No composer exists yet, so nothing holds the workspace's single cursor.
        focus: KeyboardFocus::Navigation,
        dismissible: false,
    };
    match router.translate(event, &context) {
        Routed::Intent(intent) => apply_intent(state, intent, painted),
        Routed::Ignored(_) => Flow::Continue,
    }
}

/// Applies one intent to the workspace.
///
/// Intents whose reducer arrives in a later delivery step are listed explicitly rather than caught
/// by a wildcard, so a new intent cannot be added and silently do nothing.
fn apply_intent(
    state: &mut ViewState,
    intent: TuiIntent,
    painted: &mut Option<ViewRevision>,
) -> Flow {
    match intent {
        TuiIntent::Quit => return Flow::Quit,
        TuiIntent::MoveSelection(direction) => state.move_selection(direction),
        // A resize leaves the projection unchanged, so the revision gate has to be told that the
        // painted frame is no longer valid.
        TuiIntent::TerminalResized { .. } => *painted = None,
        TuiIntent::CycleFocus(_)
        | TuiIntent::Dismiss
        | TuiIntent::Scroll { .. }
        | TuiIntent::Pointer(_)
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
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    use plexmaton_tui::{Router, SurfaceTree, ViewState};

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
}
