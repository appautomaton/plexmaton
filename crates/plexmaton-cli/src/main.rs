use std::{collections::VecDeque, time::Duration};

use anyhow::Context;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use plexmaton_sim::{Scenario, ScenarioStep};
use plexmaton_tui::{ViewRevision, ViewState};
use ratatui::DefaultTerminal;

const TICK_INTERVAL: Duration = Duration::from_millis(180);

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

    apply_ready(&mut state, &mut timeline, tick);

    loop {
        // Repaint only when the projection actually changed. Ambient background activity and
        // input that the workspace ignores must not cost a full-screen redraw.
        if painted != Some(state.revision()) {
            terminal
                .draw(|frame| plexmaton_tui::render(frame, &state))
                .context("draw TUI frame")?;
            painted = Some(state.revision());
        }

        tokio::select! {
            _ = ticker.tick() => {
                tick = tick.saturating_add(1);
                apply_ready(&mut state, &mut timeline, tick);
            }
            terminal_event = terminal_events.next() => {
                match terminal_event {
                    Some(Ok(Event::Key(key))) if should_quit(key) => break,
                    Some(Ok(event)) => {
                        if invalidates_frame(&event) {
                            painted = None;
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

/// Terminal events that invalidate the painted frame without changing the projection.
fn invalidates_frame(event: &Event) -> bool {
    matches!(event, Event::Resize(_, _))
}

fn should_quit(key: KeyEvent) -> bool {
    if key.kind != KeyEventKind::Press {
        return false;
    }
    matches!(key.code, KeyCode::Esc | KeyCode::Char('q'))
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
}

#[cfg(test)]
mod tests {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

    use super::{invalidates_frame, should_quit};

    #[test]
    fn explicit_quit_keys_are_recognized() {
        assert!(should_quit(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        assert!(should_quit(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL
        )));
        assert!(!should_quit(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE
        )));
    }

    #[test]
    fn only_resize_invalidates_the_painted_frame() {
        assert!(invalidates_frame(&Event::Resize(100, 40)));
        assert!(!invalidates_frame(&Event::Key(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE
        ))));
        assert!(!invalidates_frame(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            column: 4,
            row: 2,
            modifiers: KeyModifiers::NONE,
        })));
    }
}
