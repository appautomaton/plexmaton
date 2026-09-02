use std::{io, path::Path, time::Duration};

use anyhow::{Context, bail};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, EventStream},
    execute,
};
use futures_util::StreamExt;
use plexmaton_agent::Input;
use plexmaton_core::AgentId;
use plexmaton_sim::{RuntimeCommand, Scenario, ScriptedRuntime};
use plexmaton_tui::{ApprovalSubmission, Flow, Submission, SubmissionKind, Workspace};
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
        ScriptedRuntime::new(scenario),
        &mut TerminalClipboard::new(io::stdout()),
        working_directory(),
    )
    .await
}

/// Where the process runs, the way a shell prompt shows it: the home directory as `~`.
///
/// `None` when the directory cannot be read, which the status line shows as nothing rather than
/// as an error: it is a label, and a session does not fail over a label.
fn working_directory() -> Option<String> {
    let current = std::env::current_dir().ok()?;
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let shown = match home
        .as_deref()
        .and_then(|home| current.strip_prefix(home).ok())
    {
        Some(rest) if rest == Path::new("") => "~".to_owned(),
        Some(rest) => format!("~/{}", rest.display()),
        None => current.display().to_string(),
    };
    Some(shown)
}

/// The event loop: producer events, terminal events, and the frames they justify.
///
/// Everything the loop decides lives in `Workspace`, so what this function owns is the two things
/// only a real process can: the terminal, and the async wait on two sources at once.
async fn run(
    mut terminal: DefaultTerminal,
    mut runtime: ScriptedRuntime,
    clipboard: &mut impl ClipboardSink,
    working_directory: Option<String>,
) -> anyhow::Result<()> {
    let mut workspace = Workspace::default();
    if let Some(path) = working_directory {
        workspace.set_working_directory(path);
    }
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
                            dispatch(
                                &mut runtime,
                                &mut workspace,
                                route_submission(submission),
                            )?;
                        }
                        if let Some(agent_id) = outcome.interrupted {
                            dispatch(
                                &mut runtime,
                                &mut workspace,
                                route_interrupt(agent_id),
                            )?;
                        }
                        if let Some(approval) = outcome.approval {
                            dispatch(
                                &mut runtime,
                                &mut workspace,
                                route_approval(approval),
                            )?;
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

/// One user input after the TUI has settled both its addressee and delivery boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
struct AddressedInput {
    to: AgentId,
    input: Input,
}

/// Preserves the route named by the visible input as the loop's own vocabulary (COM-4, LOOP-6).
fn route_submission(submission: Submission) -> AddressedInput {
    let input = match submission.kind {
        SubmissionKind::Message => Input::Submitted {
            text: submission.text,
        },
        SubmissionKind::Steering => Input::Steered {
            text: submission.text,
        },
    };
    AddressedInput {
        to: submission.to,
        input,
    }
}

/// Turns the focused conversation identity into the loop's interrupt input (INV-7).
fn route_interrupt(to: AgentId) -> AddressedInput {
    AddressedInput {
        to,
        input: Input::Interrupted,
    }
}

/// Preserves the exact pending identity and typed answer chosen on the approval surface (APV-4).
fn route_approval(approval: ApprovalSubmission) -> AddressedInput {
    AddressedInput {
        to: approval.to,
        input: Input::ApprovalDecided {
            approval_id: approval.approval_id,
            decision: approval.decision,
        },
    }
}

/// Gives addressed user input to today's synthetic adapter and applies what it emits.
///
/// The projection is never written directly here. A message reaches the screen as the runtime's
/// own events or not at all, which is what keeps the transcript to one writer (COM-3).
///
/// The adapter deliberately matches every user-facing [`Input`] variant. The simulator has no turn
/// machine, so it renders steering as user-authored text and reports an interrupt as unsupported;
/// slice 6 replaces only this adapter, not the mapping above.
fn dispatch(
    runtime: &mut ScriptedRuntime,
    workspace: &mut Workspace,
    addressed: AddressedInput,
) -> anyhow::Result<()> {
    let command = match addressed.input {
        Input::Submitted { text } => RuntimeCommand::SendMessage {
            to: addressed.to,
            text,
        },
        Input::Steered { text } => RuntimeCommand::SendMessage {
            to: addressed.to,
            text,
        },
        Input::Interrupted => RuntimeCommand::Interrupt { to: addressed.to },
        Input::ApprovalDecided {
            approval_id,
            decision,
        } => RuntimeCommand::Approval {
            to: addressed.to,
            approval_id,
            decision,
        },
        Input::Streamed(_)
        | Input::Failed(_)
        | Input::ToolAdmissionResolved(_)
        | Input::ToolFinished { .. }
        | Input::ShuttingDown => {
            bail!("the TUI produced an input reserved for the producer")
        }
    };
    let emitted = runtime.submit(command).context("dispatch user input")?;
    workspace.emit(emitted);
    Ok(())
}

#[cfg(test)]
mod tests {
    use plexmaton_core::TranscriptRole;
    use plexmaton_sim::{Scenario, ScriptedRuntime};
    use plexmaton_tui::{ApprovalSubmission, Submission, SubmissionKind, SurfaceId, Workspace};
    use ratatui::{
        Terminal,
        backend::TestBackend,
        crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers},
    };

    use super::{dispatch, route_approval, route_interrupt, route_submission};

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
        let mut runtime = ScriptedRuntime::new(
            Scenario::canonical().unwrap_or_else(|error| panic!("fixture: {error}")),
        );
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
            "the composer is bound to the primary agent and says so (COM-4)"
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

        dispatch(&mut runtime, &mut workspace, route_submission(submission))
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

    /// Stage 2 slice 1 closes here: what the loop emits, the projection accepts.
    ///
    /// `plexmaton-agent` and `plexmaton-tui` both depend on `plexmaton-core` and neither knows the
    /// other exists, so this binary is the only place that can prove the vocabulary they share
    /// lines up. An empty notice log is the assertion: the projection refuses a gap, a repeat, an
    /// unknown agent and an unknown item, so a stream it accepts in full is one it understood.
    #[test]
    fn a_turn_the_loop_drove_is_a_stream_the_projection_accepts() {
        use plexmaton_agent::{Agent, Effect, Input, ModelEvent, StopReason};
        use plexmaton_core::AgentId;

        let mut agent =
            Agent::new(AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}")));
        let mut workspace = Workspace::default();

        workspace.emit(agent.announce("Agent A").events);
        let opened = agent.handle(Input::Submitted {
            text: "hello".to_owned(),
        });
        assert!(
            matches!(opened.effects.as_slice(), [Effect::CallModel(_)]),
            "the loop asks the model, and this crate is what would perform it"
        );
        workspace.emit(opened.events);
        for delta in ["hi ", "there"] {
            workspace.emit(
                agent
                    .handle(Input::Streamed(ModelEvent::TextDelta(delta.to_owned())))
                    .events,
            );
        }
        workspace.emit(
            agent
                .handle(Input::Streamed(ModelEvent::Stopped(StopReason::EndOfTurn)))
                .events,
        );

        assert_eq!(
            workspace.state().notices().count(),
            0,
            "the projection refused something the loop emitted"
        );
        let transcript: Vec<_> = workspace
            .state()
            .primary_agent()
            .unwrap_or_else(|| panic!("the loop announced an agent"))
            .transcript()
            .map(|item| (item.role, item.source.clone()))
            .collect();
        assert_eq!(
            transcript,
            [
                (TranscriptRole::User, "hello".to_owned()),
                (TranscriptRole::Assistant, "hi there".to_owned()),
            ]
        );
    }

    /// LOOP-6 and INV-7 at the composition boundary: the visible input chooses the agent input,
    /// and the addressed interrupt reaches that same running turn rather than ending in TUI state.
    #[test]
    fn production_mapping_preserves_message_steering_interrupt_and_approval() {
        use plexmaton_agent::{Agent, Input};
        use plexmaton_core::{AgentId, ApprovalDecision, ApprovalId};

        let agent_id = AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}"));
        let mut agent = Agent::new(agent_id.clone());
        let mut terminal = Terminal::new(TestBackend::new(120, 24))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        let mut workspace = Workspace::default();
        workspace.emit(agent.announce("Agent A").events);
        workspace
            .draw(&mut terminal)
            .unwrap_or_else(|error| panic!("test render: {error}"));

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
            .unwrap_or_else(|| panic!("the composer must submit"));
        let addressed = route_submission(submission);
        assert_eq!(addressed.to, agent_id);
        let opened = agent.handle(addressed.input);
        workspace.emit(opened.events);
        assert!(agent.is_running());

        let steering = route_submission(Submission {
            to: AgentId::new("agent-b").unwrap_or_else(|error| panic!("fixture: {error}")),
            text: "check the cache".to_owned(),
            kind: SubmissionKind::Steering,
        });
        assert!(matches!(
            steering.input,
            Input::Steered { ref text } if text == "check the cache"
        ));

        let approval = route_approval(ApprovalSubmission {
            to: agent_id.clone(),
            approval_id: ApprovalId::new("approval-1")
                .unwrap_or_else(|error| panic!("fixture: {error}")),
            decision: ApprovalDecision::AllowOnce,
        });
        assert_eq!(approval.to, agent_id);
        assert!(matches!(
            approval.input,
            Input::ApprovalDecided {
                ref approval_id,
                decision: ApprovalDecision::AllowOnce,
            } if approval_id.as_str() == "approval-1"
        ));

        for character in "discard me".chars() {
            workspace.handle(&press(KeyCode::Char(character)));
        }
        assert_eq!(workspace.state().composer().draft(), "discard me");

        let interrupted = workspace.handle(&Event::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        )));
        assert_eq!(workspace.state().composer().draft(), "");
        let target = interrupted
            .interrupted
            .unwrap_or_else(|| panic!("Ctrl-C must name its conversation"));
        let addressed = route_interrupt(target);
        assert_eq!(addressed.to, agent_id);
        agent.handle(addressed.input);

        assert!(
            !agent.is_running(),
            "the TUI command stopped at the boundary"
        );
    }

    #[test]
    fn the_synthetic_adapter_reports_an_interrupt_it_cannot_perform() {
        use plexmaton_core::AgentId;

        let mut runtime = ScriptedRuntime::new(
            Scenario::canonical().unwrap_or_else(|error| panic!("fixture: {error}")),
        );
        let mut workspace = Workspace::default();
        workspace.emit(runtime.ready(u64::MAX));
        let before = workspace.state().notices().count();

        dispatch(
            &mut runtime,
            &mut workspace,
            route_interrupt(
                AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}")),
            ),
        )
        .unwrap_or_else(|error| panic!("the adapter must report its limitation: {error}"));

        assert_eq!(
            workspace.state().notices().count(),
            before.saturating_add(1)
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
