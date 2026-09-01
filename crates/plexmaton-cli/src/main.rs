use std::{io, time::Duration};

use anyhow::Context;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, EventStream},
    execute,
};
use futures_util::StreamExt;
use plexmaton_sim::{Runtime, RuntimeCommand, Scenario};
use plexmaton_tui::{Flow, Submission, Workspace};
use ratatui::DefaultTerminal;

mod clipboard;

use clipboard::{ClipboardSink, TerminalClipboard};

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
    // The terminal on the other end of stdout is the one holding the user's clipboard, which over
    // SSH or inside tmux is not the machine this process runs on.
    run(
        terminal,
        Runtime::new(scenario),
        &mut TerminalClipboard::new(io::stdout()),
    )
    .await
}

/// The event loop: producer events, terminal events, and the frames they justify.
///
/// Everything the loop decides lives in `Workspace`, so what this function owns is the two things
/// only a real process can: the terminal, and the async wait on two sources at once.
async fn run(
    mut terminal: DefaultTerminal,
    mut runtime: Runtime,
    clipboard: &mut impl ClipboardSink,
) -> anyhow::Result<()> {
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
                        if let Some(submission) = outcome.submitted {
                            send(&mut runtime, &mut workspace, submission)?;
                        }
                        if let Some(request) = outcome.copied {
                            clipboard.copy(&request.text).context("copy to the clipboard")?;
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
///
/// The target rides along with the text. With two inputs on screen, a composition root that picked
/// the recipient itself would be a second answer to a question focus has already settled.
fn send(
    runtime: &mut Runtime,
    workspace: &mut Workspace,
    submission: Submission,
) -> anyhow::Result<()> {
    let emitted = runtime
        .submit(RuntimeCommand::SendMessage {
            to: submission.to,
            text: submission.text,
        })
        .context("submit the composed message")?;
    workspace.emit(emitted);
    Ok(())
}

#[cfg(test)]
mod tests {
    use plexmaton_core::TranscriptRole;
    use plexmaton_sim::{Runtime, Scenario};
    use plexmaton_tui::{SurfaceId, Workspace};
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
        // reaches it. Walked rather than counted: the ring gains and loses stops with the terminal
        // and with what is queued, and what this needs is that the composer is reachable.
        for _ in 0..workspace.surfaces().len() {
            if workspace.state().focused(workspace.surfaces()) == Some(SurfaceId::Composer) {
                break;
            }
            workspace.handle(&press(KeyCode::Tab));
            workspace
                .draw(&mut terminal)
                .unwrap_or_else(|error| panic!("test render: {error}"));
        }
        for character in "hello".chars() {
            workspace.handle(&press(KeyCode::Char(character)));
        }
        let submission = workspace
            .handle(&press(KeyCode::Enter))
            .submitted
            .unwrap_or_else(|| panic!("Enter must submit the draft"));
        assert_eq!(submission.text, "hello");
        assert_eq!(
            submission.to.as_str(),
            "agent-a",
            "the composer is bound to the primary agent and says so (D-017)"
        );

        let roles: Vec<_> = workspace
            .state()
            .primary_agent()
            .map(|agent| agent.transcript().map(|item| item.role).collect())
            .unwrap_or_default();
        assert!(
            !roles.contains(&TranscriptRole::User),
            "nothing may appear in the transcript until the runtime emits it"
        );

        send(&mut runtime, &mut workspace, submission)
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

    /// With no agent there is no cursor, so there is nothing to submit in the first place.
    ///
    /// This replaces a test that submitted into an empty roster and checked the text was not lost.
    /// That case stopped being reachable when a submission started carrying its target: the target
    /// comes from focus, and an empty workspace has no text input to focus.
    #[test]
    fn an_empty_workspace_has_no_cursor_to_type_into() {
        let mut terminal = Terminal::new(TestBackend::new(120, 24))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        let mut workspace = Workspace::default();
        workspace
            .draw(&mut terminal)
            .unwrap_or_else(|error| panic!("test render: {error}"));

        for _ in 0..4 {
            workspace.handle(&press(KeyCode::Tab));
        }
        for character in "hello".chars() {
            workspace.handle(&press(KeyCode::Char(character)));
        }

        assert_eq!(workspace.handle(&press(KeyCode::Enter)).submitted, None);
        assert_eq!(workspace.state().agents().count(), 0);
        assert_eq!(
            workspace.state().notices().count(),
            0,
            "and nothing about that is a producer defect to report"
        );
    }
}
