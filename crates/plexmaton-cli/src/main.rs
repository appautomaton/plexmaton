use std::{io, time::Duration};

use anyhow::Context;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, EventStream},
    execute,
};
use futures_util::StreamExt;
use plexmaton_sim::{Runtime, RuntimeCommand, Scenario};
use plexmaton_tui::{Flow, Workspace};
use ratatui::DefaultTerminal;

const TICK_INTERVAL: Duration = Duration::from_millis(180);

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

/// The event loop: producer events, terminal events, and the frames they justify.
///
/// Everything the loop decides lives in `Workspace`, so what this function owns is the two things
/// only a real process can: the terminal, and the async wait on two sources at once.
async fn run(mut terminal: DefaultTerminal, mut runtime: Runtime) -> anyhow::Result<()> {
    let mut workspace = Workspace::default();
    let mut tick = 0_u64;
    let mut ticker = tokio::time::interval(TICK_INTERVAL);
    let mut terminal_events = EventStream::new();

    workspace.emit(runtime.ready(tick));

    loop {
        workspace.draw(&mut terminal).context("draw TUI frame")?;

        tokio::select! {
            _ = ticker.tick() => {
                tick = tick.saturating_add(1);
                workspace.emit(runtime.ready(tick));
            }
            terminal_event = terminal_events.next() => {
                match terminal_event {
                    Some(Ok(event)) => {
                        let outcome = workspace.handle(&event);
                        if let Some(text) = outcome.submitted {
                            send(&mut runtime, &mut workspace, text)?;
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

/// Hands a submitted draft to the runtime and applies whatever it emits in response.
///
/// The projection is never written directly here. A message reaches the screen as the runtime's
/// own events or not at all, which is what keeps the transcript to one writer (COM-3).
fn send(runtime: &mut Runtime, workspace: &mut Workspace, text: String) -> anyhow::Result<()> {
    let Some(to) = workspace
        .state()
        .primary_agent()
        .map(|agent| agent.id.clone())
    else {
        // Nothing has been delegated to yet, so there is no session to deliver into. Dropping the
        // text here would lose it silently; it stays in the draft until an agent exists.
        return Ok(());
    };
    let emitted = runtime
        .submit(RuntimeCommand::SendMessage { to, text })
        .context("submit the composed message")?;
    workspace.emit(emitted);
    Ok(())
}

#[cfg(test)]
mod tests {
    use plexmaton_core::TranscriptRole;
    use plexmaton_sim::{Runtime, Scenario};
    use plexmaton_tui::Workspace;
    use ratatui::{
        Terminal,
        backend::TestBackend,
        crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers},
    };

    use super::send;

    fn press(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    /// COM-1 to COM-3 through the executable: typing reaches the runtime and comes back as a
    /// transcript item.
    ///
    /// The round trip is the point, and it is this crate's to prove. The workspace hands submitted
    /// text back as a value; only the composition root knows there is a runtime to give it to. So
    /// nothing here writes to the projection, and a message that appears has been through the same
    /// boundary a real runtime will occupy.
    #[test]
    fn a_typed_message_reaches_the_transcript_by_way_of_the_runtime() {
        let mut runtime =
            Runtime::new(Scenario::canonical().unwrap_or_else(|error| panic!("fixture: {error}")));
        let mut terminal = Terminal::new(TestBackend::new(120, 24))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        let mut workspace = Workspace::default();
        workspace.emit(runtime.ready(u64::MAX));
        workspace
            .draw(&mut terminal)
            .unwrap_or_else(|error| panic!("test render: {error}"));

        // The composer is the ring's last stop, and walking there is how a keyboard-only user
        // reaches it.
        for _ in 0..3 {
            workspace.handle(&press(KeyCode::Tab));
        }
        for character in "hello".chars() {
            workspace.handle(&press(KeyCode::Char(character)));
        }
        let text = workspace
            .handle(&press(KeyCode::Enter))
            .submitted
            .unwrap_or_else(|| panic!("Enter must submit the draft"));
        assert_eq!(text, "hello");

        let roles: Vec<_> = workspace
            .state()
            .primary_agent()
            .map(|agent| agent.transcript().map(|item| item.role).collect())
            .unwrap_or_default();
        assert!(
            !roles.contains(&TranscriptRole::User),
            "nothing may appear in the transcript until the runtime emits it"
        );

        send(&mut runtime, &mut workspace, text)
            .unwrap_or_else(|error| panic!("the runtime accepts the message: {error}"));

        let user_items: Vec<_> = workspace
            .state()
            .primary_agent()
            .unwrap_or_else(|| panic!("the canonical timeline creates a primary agent"))
            .transcript()
            .filter(|item| item.role == TranscriptRole::User)
            .map(|item| item.source.clone())
            .collect();
        assert_eq!(user_items, ["hello"]);
        assert_eq!(
            workspace.state().notices().count(),
            0,
            "a submitted message must not break the sequence the projection is checking"
        );
    }

    /// A draft submitted before any agent exists is kept rather than delivered nowhere.
    #[test]
    fn submitting_with_no_agent_is_not_a_producer_defect() {
        let mut runtime =
            Runtime::new(Scenario::canonical().unwrap_or_else(|error| panic!("fixture: {error}")));
        let mut workspace = Workspace::default();

        send(&mut runtime, &mut workspace, "into the void".to_owned())
            .unwrap_or_else(|error| panic!("an empty roster is not an error: {error}"));

        assert_eq!(workspace.state().agents().count(), 0);
        assert_eq!(
            workspace.state().notices().count(),
            0,
            "there was no session to deliver into, and that is not a defect to report"
        );
    }
}
