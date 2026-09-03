use std::{fs, io, path::Path};

use anyhow::{Context, bail};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, EventStream},
    execute,
};
use futures_util::StreamExt;
use plexmaton_agent::Input;
use plexmaton_core::AgentId;
use plexmaton_provider::{ProviderConfig, resolve_api_key, resolve_home};
use plexmaton_runtime::{DispatchReport, LiveRuntime};
use plexmaton_tui::{ApprovalSubmission, Flow, Submission, SubmissionKind, Workspace};
use ratatui::DefaultTerminal;

mod clipboard;

use clipboard::{ClipboardSink, TerminalClipboard};

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
    // LIVE-6: all fallible authority and endpoint resolution happens before terminal ownership.
    let runtime = live_runtime_from_process()?;
    // The guard is armed before anything is changed, so even a failure to enable capture restores.
    let _restore_terminal = RestoreTerminal;
    let terminal = ratatui::init();
    execute!(io::stdout(), EnableMouseCapture).context("enable mouse reporting")?;
    // The terminal on the other end of stdout is the one holding the user's clipboard, which over
    // SSH or inside tmux is not the machine this process runs on.
    run(
        terminal,
        runtime,
        &mut TerminalClipboard::new(io::stdout()),
        working_directory(),
    )
    .await
}

fn live_runtime_from_process() -> anyhow::Result<LiveRuntime> {
    let configured_home = std::env::var_os("PLEXMATON_HOME");
    let user_home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let root = resolve_home(configured_home.as_deref(), user_home.as_deref())
        .context("resolve Plexmaton configuration root")?;
    let path = root.join("config.toml");
    let source = fs::read_to_string(&path)
        .with_context(|| format!("read provider configuration at {}", path.display()))?;
    let config = ProviderConfig::parse(&source).context("parse provider configuration")?;
    let profile = config.active().clone();
    let key = resolve_api_key(&profile, std::env::var_os(profile.api_key_env()))
        .context("resolve provider API key")?;
    let agent_id = AgentId::new("agent-primary").context("build primary agent identity")?;
    LiveRuntime::openai(agent_id, "Plexmaton", profile, key)
        .context("configure live provider transport")
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
    mut runtime: LiveRuntime,
    clipboard: &mut impl ClipboardSink,
    working_directory: Option<String>,
) -> anyhow::Result<()> {
    let mut workspace = Workspace::default();
    if let Some(path) = working_directory {
        workspace.set_working_directory(path);
    }
    let loop_result = drive_session(&mut terminal, &mut runtime, clipboard, &mut workspace).await;
    let shutdown = runtime.shutdown().await.context("shut down live runtime");
    loop_result?;
    let _report = shutdown?;
    Ok(())
}

/// Runs the interactive select separately so every error returns to the owner that joins runtime.
async fn drive_session(
    terminal: &mut DefaultTerminal,
    runtime: &mut LiveRuntime,
    clipboard: &mut impl ClipboardSink,
    workspace: &mut Workspace,
) -> anyhow::Result<()> {
    let mut terminal_events = EventStream::new();
    loop {
        workspace.draw(terminal).context("draw TUI frame")?;

        tokio::select! {
            runtime_event = runtime.next_event() => {
                match runtime_event.context("receive live runtime event")? {
                    Some(event) => workspace.emit(vec![event]),
                    None => break,
                }
            }
            terminal_event = terminal_events.next() => {
                match terminal_event {
                    Some(Ok(event)) => {
                        let outcome = workspace.handle(&event);
                        if let Some(submission) = outcome.submitted {
                            dispatch_live(
                                runtime,
                                workspace,
                                route_submission(submission),
                            ).await?;
                        }
                        if let Some(agent_id) = outcome.interrupted {
                            dispatch_live(
                                runtime,
                                workspace,
                                route_interrupt(agent_id),
                            ).await?;
                        }
                        if let Some(approval) = outcome.approval {
                            dispatch_live(
                                runtime,
                                workspace,
                                route_approval(approval),
                            ).await?;
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

/// Gives addressed visible input to the live runtime; semantic events return on its event stream.
///
/// The projection is never written directly here. A message reaches the screen as the runtime's
/// own events or not at all, which is what keeps the transcript to one writer (COM-3).
///
async fn dispatch_live(
    runtime: &mut LiveRuntime,
    workspace: &mut Workspace,
    addressed: AddressedInput,
) -> anyhow::Result<()> {
    if matches!(
        addressed.input,
        Input::Streamed { .. }
            | Input::Failed { .. }
            | Input::ToolAdmissionResolved(_)
            | Input::ToolFinished { .. }
            | Input::ShuttingDown
    ) {
        bail!("the TUI produced an input reserved for the producer");
    }
    let to = addressed.to.clone();
    let report = runtime
        .submit(addressed.to, addressed.input)
        .await
        .context("dispatch user input")?;
    restore_undelivered(workspace, to, report);
    Ok(())
}

fn restore_undelivered(workspace: &mut Workspace, to: AgentId, report: DispatchReport) {
    for input in report.undelivered {
        workspace.return_input(to.clone(), input.text);
    }
}

#[cfg(test)]
fn dispatch_synthetic(
    runtime: &mut plexmaton_sim::ScriptedRuntime,
    workspace: &mut Workspace,
    addressed: AddressedInput,
) -> anyhow::Result<()> {
    use plexmaton_sim::RuntimeCommand;

    let command = match addressed.input {
        Input::Submitted { text } | Input::Steered { text } => RuntimeCommand::SendMessage {
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
        Input::Streamed { .. }
        | Input::Failed { .. }
        | Input::ToolAdmissionResolved(_)
        | Input::ToolFinished { .. }
        | Input::ShuttingDown => bail!("the TUI produced an input reserved for the producer"),
    };
    workspace.emit(runtime.submit(command).context("dispatch user input")?);
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

    use super::{
        AddressedInput, dispatch_live, dispatch_synthetic, route_approval, route_interrupt,
        route_submission,
    };

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

        dispatch_synthetic(&mut runtime, &mut workspace, route_submission(submission))
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
        let step_id = agent
            .active_model_step()
            .unwrap_or_else(|| panic!("submission opens one model step"));
        for delta in ["hi ", "there"] {
            workspace.emit(
                agent
                    .handle(Input::Streamed {
                        step_id: step_id.clone(),
                        event: ModelEvent::TextDelta(delta.to_owned()),
                    })
                    .events,
            );
        }
        workspace.emit(
            agent
                .handle(Input::Streamed {
                    step_id,
                    event: ModelEvent::Stopped(StopReason::EndOfTurn),
                })
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

    /// LOOP-6: ownership returned by the live loop goes back to the addressed editable draft.
    #[tokio::test]
    async fn a_live_dispatch_restores_undelivered_user_text() {
        use std::ffi::OsString;

        use plexmaton_agent::Input;
        use plexmaton_core::AgentId;
        use plexmaton_provider::{ProviderConfig, resolve_api_key};
        use plexmaton_runtime::LiveRuntime;

        let agent_id = AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}"));
        let config = ProviderConfig::parse(
            r#"active_provider = "test"

[providers.test]
kind = "openai_compatible"
protocol = "responses"
base_url = "http://127.0.0.1:9/v1"
model = "fixture"
api_key_env = "TEST_KEY"
reasoning_effort = "none"
"#,
        )
        .unwrap_or_else(|error| panic!("test config: {error}"));
        let key = resolve_api_key(config.active(), Some(OsString::from("fixture-only")))
            .unwrap_or_else(|error| panic!("test key: {error}"));
        let mut runtime =
            LiveRuntime::openai(agent_id.clone(), "Agent A", config.active().clone(), key)
                .unwrap_or_else(|error| panic!("test runtime: {error}"));
        let mut workspace = Workspace::default();
        dispatch_live(
            &mut runtime,
            &mut workspace,
            AddressedInput {
                to: agent_id.clone(),
                input: Input::Steered {
                    text: "do not lose this".to_owned(),
                },
            },
        )
        .await
        .unwrap_or_else(|error| panic!("dispatch: {error}"));

        assert_eq!(
            workspace.state().draft(&agent_id).draft(),
            "do not lose this"
        );
        runtime
            .shutdown()
            .await
            .unwrap_or_else(|error| panic!("shutdown: {error}"));
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

        dispatch_synthetic(
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
