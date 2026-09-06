use std::{
    ffi::{OsStr, OsString},
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
};

use anyhow::{Context, bail};
use crossterm::event::EventStream;
use plexmaton_agent::Input;
use plexmaton_core::AgentId;
use plexmaton_provider::{resolve_api_key, resolve_home};
use plexmaton_runtime::{
    CleanupFailure, DispatchReport, LiveRuntime, NativeToolCatalog, PersistenceFailure,
    SessionRecovery,
};
use plexmaton_tui::{
    ApprovalSubmission, CleanupNotice, Command, ConfigurationSummary, MarkdownTheme, Palette,
    PersistenceNotice, Submission, SubmissionKind, Workspace,
};

mod clipboard;
mod interaction;
mod output;
mod retry;
mod session;
mod session_picker;
mod statusline;
mod stream_frames;
#[cfg(test)]
mod test_support;

use interaction::drive_session;
use output::{RestoreTerminal, TerminalOutput};
use plexmaton_cli::preparation;
use session::{
    OpenedSession, PersistedSession, SessionSelection, StartupAction, USAGE, open_selected_session,
    parse_startup_action, report_persisted_session, restoration_feedback,
};

const INTERNAL_RG_DRIVER: &str = "--__plexmaton-rg-driver";

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if arguments.first().map(OsString::as_os_str) == Some(OsStr::new(INTERNAL_RG_DRIVER)) {
        return plexmaton_file_tools::run_search_driver(arguments.into_iter().skip(1))
            .context("run internal descriptor-rooted ripgrep driver");
    }
    if arguments.len() == 1
        && arguments.first().map(OsString::as_os_str)
            == Some(OsStr::new(preparation::DRIVER_ARGUMENT))
    {
        return preparation::run_driver().context("run internal presentation preparation");
    }
    let selection = match parse_startup_action(&arguments)? {
        StartupAction::Run(selection) => selection,
        StartupAction::Help => {
            writeln!(io::stdout(), "{USAGE}").context("write help")?;
            return Ok(());
        }
    };
    let render_preparation = preparation::LivePreparation::new(
        std::env::current_exe().context("locate preparation executable")?,
    );
    // LIVE-6: all fallible authority and endpoint resolution happens before terminal ownership.
    let (opened, workspace_root, mut picker, status_line) =
        live_runtime_from_process(selection).await?;
    let OpenedSession {
        runtime,
        recovery,
        persisted,
    } = opened;
    picker.current = persisted;
    // The guard is armed before anything is changed, so even a failure to enable capture restores.
    let restore_terminal = RestoreTerminal::new();
    let output = match TerminalOutput::acquire() {
        Ok(output) => output,
        Err(error) => {
            return failed_terminal_setup(error, runtime, picker, status_line, render_preparation)
                .await;
        }
    };
    let run_result = run(
        output,
        runtime,
        working_directory(&workspace_root),
        recovery,
        picker,
        status_line,
        render_preparation,
    )
    .await;
    drop(restore_terminal);
    match run_result {
        Err(error) => Err(error),
        Ok(persisted) => persisted
            .as_ref()
            .map(|session| report_persisted_session(session, &mut io::stdout()))
            .transpose()
            .map(|_| ()),
    }
}

async fn failed_terminal_setup(
    error: io::Error,
    mut runtime: LiveRuntime,
    mut picker: session_picker::SessionPicker,
    mut status: Option<statusline::StatusLine>,
    mut preparation: preparation::LivePreparation,
) -> anyhow::Result<()> {
    let preparation_shutdown = preparation
        .shutdown()
        .await
        .context("shut down render preparation");
    let picker_shutdown = picker.shutdown().await;
    let status_shutdown = match &mut status {
        Some(status) => status.shutdown().await,
        None => Ok(()),
    };
    let runtime_shutdown = runtime
        .shutdown()
        .await
        .context("shut down live runtime")
        .and_then(surface_shutdown_report);
    session_result(
        Err(error).context("acquire terminal output"),
        runtime_shutdown,
        status_shutdown,
        Ok(()),
        preparation_shutdown,
        picker_shutdown,
    )
}

async fn live_runtime_from_process(
    selection: SessionSelection,
) -> anyhow::Result<(
    OpenedSession,
    PathBuf,
    session_picker::SessionPicker,
    Option<statusline::StatusLine>,
)> {
    let configured_home = std::env::var_os("PLEXMATON_HOME");
    let user_home = std::env::var_os("HOME").map(PathBuf::from);
    let root = resolve_home(configured_home.as_deref(), user_home.as_deref())
        .context("resolve Plexmaton configuration root")?;
    let path = root.join("config.toml");
    let source = fs::read_to_string(&path)
        .with_context(|| format!("read provider configuration at {}", path.display()))?;
    let (config, status_config) = statusline::parse(&source)?;
    let model = config.active_model().clone();
    let key = resolve_api_key(&model, std::env::var_os(model.api_key_env()))
        .context("resolve provider API key")?;
    let workspace_root = std::env::current_dir()
        .context("resolve tool workspace")?
        .canonicalize()
        .context("canonicalize tool workspace")?;
    let ripgrep = resolve_path_executable("rg", std::env::var_os("PATH").as_deref())?;
    let driver = std::env::current_exe()
        .context("resolve Plexmaton executable for the search driver")?
        .canonicalize()
        .context("canonicalize Plexmaton search driver")?;
    let tools = NativeToolCatalog::open(
        &workspace_root,
        model.api_key_env(),
        ripgrep.clone(),
        driver.clone(),
        vec![OsString::from(INTERNAL_RG_DRIVER)],
    )
    .context("configure native workspace tools")?;
    let picker = session_picker::SessionPicker::new(session_picker::Launcher {
        root: root.clone(),
        workspace: workspace_root.clone(),
        model: model.clone(),
        ripgrep,
        driver,
    });
    let agent_id = AgentId::new("agent-primary").context("build primary agent identity")?;
    let status_line = status_config
        .map(|config| statusline::StatusLine::new(config, model.clone(), workspace_root.clone()));
    let opened = open_selected_session(&root, selection, agent_id, model, key, tools).await?;
    Ok((opened, workspace_root, picker, status_line))
}

/// Project only display values from the same model handed to the runtime (INV-12, PRV-6).
fn configuration_summary(model: &plexmaton_provider::ResolvedModel) -> ConfigurationSummary {
    ConfigurationSummary {
        provider: model.provider_name().to_owned(),
        model: model.wire_id().to_owned(),
        reasoning_effort: model.reasoning_effort().as_str().to_owned(),
    }
}

/// Where the process runs, the way a shell prompt shows it: the home directory as `~`.
///
/// `None` when the directory cannot be read, which the status line shows as nothing rather than
/// as an error: it is a label, and a session does not fail over a label.
fn working_directory(current: &Path) -> Option<String> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
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

fn resolve_path_executable(name: &str, path: Option<&OsStr>) -> anyhow::Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt as _;

    let Some(path) = path else {
        bail!("cannot find `{name}` because PATH is absent");
    };
    for directory in std::env::split_paths(path) {
        if !directory.is_absolute() {
            continue;
        }
        let candidate = directory.join(name);
        let Ok(metadata) = fs::metadata(&candidate) else {
            continue;
        };
        if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 {
            return candidate
                .canonicalize()
                .with_context(|| format!("canonicalize `{name}` at {}", candidate.display()));
        }
    }
    bail!("cannot find executable `{name}` in absolute PATH entries")
}

/// The event loop: producer events, terminal events, and the frames they justify.
///
/// Workspace owns projection and interaction state; this composition root owns external effects
/// and settles every acquired owner before the terminal is released.
#[allow(
    clippy::too_many_arguments,
    reason = "the composition root settles each acquired owner before terminal restoration"
)]
async fn run(
    mut output: TerminalOutput,
    mut runtime: LiveRuntime,
    working_directory: Option<String>,
    recovery: Option<SessionRecovery>,
    mut picker: session_picker::SessionPicker,
    mut status_line: Option<statusline::StatusLine>,
    mut render_preparation: preparation::LivePreparation,
) -> anyhow::Result<Option<PersistedSession>> {
    // MD-5: terminal-owned chrome surrounds the user-approved pastel Markdown accents.
    // The script footer retains its independent colors; neither choice rethemes the other.
    let mut workspace = Workspace::with_presentation(
        Palette::ansi().with_markdown_theme(MarkdownTheme::Pastel),
        output.math,
    );
    if let Some(path) = working_directory {
        workspace.set_working_directory(path);
    }
    if let Some(recovery) = restoration_feedback(recovery) {
        // Install the acknowledged replay before anchoring presentation after its final entry.
        while let Some(event) = runtime.try_next_event() {
            workspace.emit(vec![event]);
        }
        workspace.report_session_recovery(recovery);
    }
    retry::sync_actions(&runtime, &mut workspace);
    let loop_result = drive_session(
        &mut output.terminal,
        &mut runtime,
        &mut output.clipboard,
        &mut workspace,
        &mut picker,
        &mut status_line,
        &mut EventStream::new(),
        &mut render_preparation,
        output::write_native,
    )
    .await;
    let clipboard_shutdown = output
        .clipboard
        .shutdown()
        .await
        .context("shut down clipboard delivery");
    let preparation_shutdown = render_preparation
        .shutdown()
        .await
        .context("shut down render preparation");
    let picker_shutdown = picker.shutdown().await;
    let status_shutdown = match &mut status_line {
        Some(status) => status.shutdown().await,
        None => Ok(()),
    };
    let shutdown = runtime.shutdown().await.context("shut down live runtime");
    session_result(
        loop_result,
        shutdown.and_then(surface_shutdown_report),
        status_shutdown,
        clipboard_shutdown,
        preparation_shutdown,
        picker_shutdown,
    )?;
    Ok(picker.current)
}

/// Optional presentation cleanup must never mask retained input or a durable-session failure.
fn session_result(
    loop_result: anyhow::Result<()>,
    runtime_shutdown: anyhow::Result<()>,
    status_shutdown: anyhow::Result<()>,
    clipboard_shutdown: anyhow::Result<()>,
    preparation_shutdown: anyhow::Result<()>,
    picker_shutdown: anyhow::Result<()>,
) -> anyhow::Result<()> {
    let failures: Vec<_> = [
        runtime_shutdown,
        loop_result,
        status_shutdown,
        clipboard_shutdown,
        preparation_shutdown,
        picker_shutdown,
    ]
    .into_iter()
    .filter_map(Result::err)
    .map(|error| format!("{error:#}"))
    .collect();
    if failures.is_empty() {
        Ok(())
    } else {
        bail!("{}", failures.join("; "))
    }
}

fn surface_shutdown_report(report: DispatchReport) -> anyhow::Result<()> {
    if report.undelivered.is_empty()
        && report.unresolved_approvals.is_empty()
        && report.undelivered_model.is_empty()
        && report.persistence_failure.is_none()
        && report.cleanup_failures.is_empty()
    {
        return Ok(());
    }
    let input = report
        .undelivered
        .iter()
        .map(|input| format!("{:?}", input.text))
        .collect::<Vec<_>>()
        .join(", ");
    bail!(
        "shutdown retained input [{input}]; persistence={:?}; cleanup={:?}; unresolved_approvals={}; undelivered_model={}",
        report.persistence_failure,
        report.cleanup_failures,
        report.unresolved_approvals.len(),
        report.undelivered_model.len(),
    )
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
    if let Some(projection) = report.projection_reset {
        workspace.complete_retry_edit();
        workspace.replace_projection(projection);
    }
    for input in report.undelivered {
        workspace.return_input(to.clone(), input.text);
    }
    for failure in report.cleanup_failures {
        let notice = match failure {
            CleanupFailure::Provider => CleanupNotice::Provider,
            CleanupFailure::Tools => CleanupNotice::Tools,
            CleanupFailure::JournalWriter => CleanupNotice::JournalWriter,
        };
        workspace.report_cleanup_failure(notice);
    }
    if let Some(failure) = report.persistence_failure {
        let notice = match failure {
            PersistenceFailure::NotWritten => PersistenceNotice::NotWritten,
            PersistenceFailure::OutcomeUnknown => PersistenceNotice::OutcomeUnknown,
        };
        workspace.report_persistence_failure(notice);
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read as _, Write as _},
        net::{TcpListener, TcpStream},
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
        thread,
        time::{Duration, Instant},
    };

    use plexmaton_core::{
        SessionEvent, SessionEventEnvelope, ToolCallStatus, ToolDetail, TranscriptRole,
    };
    use plexmaton_sim::{Scenario, ScriptedRuntime};
    use plexmaton_tui::{
        ApprovalSubmission, CleanupNotice, NoticeView, PersistenceNotice, Submission,
        SubmissionKind, SurfaceId, TranscriptEntryView, TranscriptTextKind, Workspace,
    };
    use ratatui::{
        Terminal,
        backend::TestBackend,
        crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers},
    };

    use super::{
        AddressedInput, dispatch_live, restore_undelivered, route_approval, route_interrupt,
        route_submission, surface_shutdown_report,
    };

    fn dispatch_synthetic(
        runtime: &mut ScriptedRuntime,
        workspace: &mut Workspace,
        addressed: AddressedInput,
    ) -> anyhow::Result<()> {
        use anyhow::{Context as _, bail};
        use plexmaton_agent::Input;
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

    trait AgentTestExt {
        fn handle(&mut self, input: plexmaton_agent::Input) -> plexmaton_agent::Reaction;
    }

    impl AgentTestExt for plexmaton_agent::Agent {
        fn handle(&mut self, input: plexmaton_agent::Input) -> plexmaton_agent::Reaction {
            self.handle_at(input, plexmaton_agent::UnixMillis::EPOCH)
        }
    }

    fn press(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    /// INV-12, PRV-6: the real composition-root handler opens the resolved model's display.
    #[tokio::test]
    async fn config_and_settings_commands_open_the_resolved_configuration() {
        let registry = plexmaton_provider::ModelRegistry::parse(
            r#"
active_model = { provider = "fixture", model = "chosen" }
[providers.fixture]
base_url = "http://127.0.0.1:9/v1"
api_key_env = "FIXTURE_KEY"
[providers.fixture.models.chosen]
api = "openai_responses"
id = "wire-model"
reasoning_effort = "high"
context_window_tokens = 100000
max_output_tokens = 10000
output_reserve_tokens = 5000
"#,
        )
        .expect("valid registry fixture");
        let mut picker =
            super::session_picker::SessionPicker::new(super::session_picker::Launcher {
                root: PathBuf::new(),
                workspace: PathBuf::new(),
                model: registry.active_model().clone(),
                ripgrep: "/bin/false".into(),
                driver: "/bin/false".into(),
            });
        let root = FixtureWorkspace::new();
        let model = registry.active_model().clone();
        let tools = super::NativeToolCatalog::open(
            root.path(),
            model.api_key_env(),
            "/bin/false",
            "/bin/false",
            Vec::new(),
        )
        .expect("tools");
        let mut runtime = super::LiveRuntime::provider(
            super::AgentId::new("primary").expect("agent"),
            "Plexmaton",
            model.clone(),
            super::resolve_api_key(&model, Some(std::ffi::OsString::from("fixture-only")))
                .expect("key"),
            tools,
        )
        .expect("runtime");
        for query in ["config", "/config", "settings", "/settings"] {
            let mut workspace = Workspace::default();
            workspace.emit(
                ScriptedRuntime::new(Scenario::canonical().expect("scenario")).ready(u64::MAX),
            );
            let mut terminal = Terminal::new(TestBackend::new(95, 40)).expect("terminal");
            workspace.draw(&mut terminal).expect("draw");
            let before = workspace.state().focused(workspace.surfaces());
            workspace.handle(&Event::Key(KeyEvent::new(
                KeyCode::Char('p'),
                KeyModifiers::CONTROL,
            )));
            workspace.draw(&mut terminal).expect("draw palette");
            for character in query.chars() {
                workspace.handle(&press(KeyCode::Char(character)));
            }
            workspace.handle(&press(KeyCode::Left));
            let command = workspace
                .handle(&press(KeyCode::Enter))
                .command
                .expect("matched command");
            picker.execute_command(&mut workspace, &runtime, command);
            workspace.draw(&mut terminal).expect("draw configuration");
            let shown = workspace
                .state()
                .configuration()
                .expect("configuration is open");
            assert_eq!(shown.provider, "fixture");
            assert_eq!(shown.model, "wire-model");
            assert_eq!(shown.reasoning_effort, "high");
            assert_eq!(
                workspace.state().focused(workspace.surfaces()),
                Some(SurfaceId::Configuration)
            );
            workspace.handle(&press(KeyCode::Esc));
            workspace.draw(&mut terminal).expect("draw return");
            assert!(workspace.state().configuration().is_none());
            assert_eq!(
                workspace.state().focused(workspace.surfaces()),
                Some(SurfaceId::CommandPalette)
            );
            let palette = workspace
                .state()
                .command_palette()
                .expect("restored palette");
            assert_eq!(palette.filter().text(), query);
            assert_eq!(palette.filter().cursor(), query.len() - 1);
            assert_eq!(palette.chosen(), Some(plexmaton_tui::Command::Config));
            workspace.handle(&press(KeyCode::Esc));
            workspace.draw(&mut terminal).expect("draw conversation");
            assert_eq!(workspace.state().focused(workspace.surfaces()), before);
        }
        runtime.shutdown().await.expect("shutdown");
    }

    fn succeeded_tool_result(output: &str) -> plexmaton_agent::ToolExecutionResult {
        plexmaton_agent::ToolExecutionResult::new(
            plexmaton_agent::ToolOutcome::Succeeded {
                output: output.to_owned(),
            },
            None,
        )
    }

    fn failed_tool_result(message: &str) -> plexmaton_agent::ToolExecutionResult {
        plexmaton_agent::ToolExecutionResult::new(
            plexmaton_agent::ToolOutcome::Failed {
                message: message.to_owned(),
            },
            None,
        )
    }

    fn assert_tool_lifecycle_projection(workspace: &Workspace) {
        let tools: Vec<_> = workspace
            .state()
            .primary_agent()
            .unwrap_or_else(|| panic!("agent was projected"))
            .tools()
            .map(|tool| (tool.id.as_str(), tool.revision, tool.status))
            .collect();
        assert_eq!(
            tools,
            [
                ("success", 2, ToolCallStatus::Succeeded),
                ("execution-failed", 2, ToolCallStatus::Failed),
                ("refused", 1, ToolCallStatus::Failed),
                ("denied", 2, ToolCallStatus::Denied),
                ("cancelled", 2, ToolCallStatus::Cancelled),
                ("approved", 3, ToolCallStatus::Succeeded),
            ]
        );
        assert_eq!(workspace.state().notices().count(), 0);
    }

    fn finish_tool(
        agent: &mut plexmaton_agent::Agent,
        call_id: &str,
        result: plexmaton_agent::ToolExecutionResult,
    ) -> Vec<SessionEventEnvelope> {
        agent
            .handle(plexmaton_agent::Input::ToolFinished {
                call_id: plexmaton_core::ToolCallId::new(call_id)
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                result,
            })
            .events
    }

    fn assert_native_tools_advertised(requests: &[Vec<u8>]) {
        for request in requests {
            let request = std::str::from_utf8(request)
                .unwrap_or_else(|error| panic!("fixture request body: {error}"));
            for name in [
                "read_file",
                "search",
                "edit_file",
                "create_file",
                "exec_command",
            ] {
                assert!(request.contains(name), "request omitted native tool {name}");
            }
        }
    }

    pub(crate) struct FixtureWorkspace(PathBuf);

    impl FixtureWorkspace {
        pub(crate) fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(1);
            loop {
                let suffix = NEXT.fetch_add(1, Ordering::Relaxed);
                let path = std::env::temp_dir().join(format!(
                    "plexmaton-cli-live-tools-{}-{suffix}",
                    std::process::id()
                ));
                match std::fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => panic!("create fixture workspace: {error}"),
                }
            }
        }

        pub(crate) fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for FixtureWorkspace {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0)
                .unwrap_or_else(|error| panic!("remove fixture workspace: {error}"));
        }
    }

    pub(crate) type FixtureServer = thread::JoinHandle<Result<Vec<Vec<u8>>, String>>;

    pub(crate) fn fixture_http_server<const N: usize>(
        responses: [&'static str; N],
    ) -> (String, FixtureServer) {
        fixture_http_responses(responses.map(|body| (200, body)))
    }

    pub(crate) fn fixture_http_responses<const N: usize>(
        responses: [(u16, &'static str); N],
    ) -> (String, FixtureServer) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .unwrap_or_else(|error| panic!("bind fixture HTTP server: {error}"));
        listener
            .set_nonblocking(true)
            .unwrap_or_else(|error| panic!("make fixture HTTP server nonblocking: {error}"));
        let address = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("read fixture HTTP address: {error}"));
        let handle = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut requests = Vec::with_capacity(responses.len());
            for (status, response) in responses {
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _peer)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            if Instant::now() >= deadline {
                                return Err("timed out waiting for fixture request".to_owned());
                            }
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => return Err(format!("accept fixture request: {error}")),
                    }
                };
                // BSD-derived kernels propagate O_NONBLOCK from the listener to an accepted
                // socket. The listener polls so it can enforce its own deadline; request reads
                // use SO_RCVTIMEO and therefore need the accepted socket back in blocking mode.
                stream
                    .set_nonblocking(false)
                    .map_err(|error| format!("make fixture connection blocking: {error}"))?;
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .map_err(|error| format!("set fixture read timeout: {error}"))?;
                requests.push(read_http_request(&mut stream)?);
                let headers = format!(
                    "HTTP/1.1 {status} Fixture\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    response.len()
                );
                stream
                    .write_all(headers.as_bytes())
                    .and_then(|()| stream.write_all(response.as_bytes()))
                    .map_err(|error| format!("write fixture response: {error}"))?;
            }
            Ok(requests)
        });
        (format!("http://{address}/v1"), handle)
    }

    fn read_http_request(stream: &mut TcpStream) -> Result<Vec<u8>, String> {
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        let (body_start, content_length) = loop {
            let read = stream
                .read(&mut buffer)
                .map_err(|error| format!("read fixture request: {error}"))?;
            if read == 0 {
                return Err("fixture request ended before its headers".to_owned());
            }
            request.extend_from_slice(&buffer[..read]);
            let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n")
            else {
                continue;
            };
            let body_start = header_end + 4;
            let headers = std::str::from_utf8(&request[..header_end])
                .map_err(|error| format!("fixture request headers were not UTF-8: {error}"))?;
            let length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>())
                })
                .transpose()
                .map_err(|error| format!("parse fixture content length: {error}"))?
                .ok_or_else(|| "fixture request omitted Content-Length".to_owned())?;
            break (body_start, length);
        };
        while request.len() < body_start.saturating_add(content_length) {
            let read = stream
                .read(&mut buffer)
                .map_err(|error| format!("read fixture request body: {error}"))?;
            if read == 0 {
                return Err("fixture request ended before its body".to_owned());
            }
            request.extend_from_slice(&buffer[..read]);
        }
        Ok(request[body_start..body_start + content_length].to_vec())
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
        use plexmaton_agent::{Agent, Effect, Input, ModelEvent, ModelOutputPosition, StopReason};
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
                        event: ModelEvent::TextDelta {
                            position: ModelOutputPosition::new(0, 0),
                            delta: delta.to_owned(),
                        },
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

    /// ENT-2: every production lifecycle branch agrees with the projection's entry revisions.
    #[test]
    fn production_tool_lifecycles_replay_as_one_entry_each() {
        use plexmaton_agent::{
            AdmissionRefusal, Agent, Effect, Input, ModelEvent, ModelOutputPosition, StopReason,
            ToolCall, ToolDefinitionRevision,
        };
        use plexmaton_core::{
            AgentId, ApprovalDecision, ToolCallId, ToolCapability, ToolDefinitionId,
        };

        let mut agent =
            Agent::new(AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}")));
        let mut workspace = Workspace::default();
        workspace.emit(agent.announce("Agent A").events);
        workspace.emit(
            agent
                .handle(Input::Submitted {
                    text: "exercise every tool ending".to_owned(),
                })
                .events,
        );
        let step_id = agent
            .active_model_step()
            .unwrap_or_else(|| panic!("submission opens one model step"));
        for (item, call_id) in [
            (0, "success"),
            (1, "execution-failed"),
            (2, "refused"),
            (3, "denied"),
            (4, "cancelled"),
            (5, "approved"),
        ] {
            let reaction = agent.handle(Input::Streamed {
                step_id: step_id.clone(),
                event: ModelEvent::Called {
                    position: ModelOutputPosition::new(item, 0),
                    call: ToolCall {
                        call_id: ToolCallId::new(call_id)
                            .unwrap_or_else(|error| panic!("fixture: {error}")),
                        name: call_id.to_owned(),
                        arguments: "{}".to_owned(),
                    },
                },
            });
            assert!(reaction.events.is_empty());
        }
        let mut dispatched = agent.handle(Input::Streamed {
            step_id,
            event: ModelEvent::Stopped(StopReason::ToolCalls),
        });
        workspace.emit(std::mem::take(&mut dispatched.events));
        let mut requests = dispatched
            .effects
            .into_iter()
            .filter_map(|effect| match effect {
                Effect::AdmitTool(request) => Some(request),
                Effect::CallModel(_) | Effect::RunTool(_) => None,
            });
        for (expected, capability) in [
            ("success", Some(ToolCapability::FileRead)),
            ("execution-failed", Some(ToolCapability::FileRead)),
            ("refused", None),
            ("denied", Some(ToolCapability::FileWrite)),
            ("cancelled", Some(ToolCapability::FileRead)),
            ("approved", Some(ToolCapability::FileWrite)),
        ] {
            let request = requests
                .next()
                .unwrap_or_else(|| panic!("admission request for {expected}"));
            assert_eq!(request.requested().call_id.as_str(), expected);
            let outcome = match capability {
                Some(capability) => request
                    .admit(
                        ToolDefinitionId::new(format!("{expected}-v1"))
                            .unwrap_or_else(|error| panic!("fixture: {error}")),
                        ToolDefinitionRevision::new(1)
                            .unwrap_or_else(|| panic!("fixture revision")),
                        [capability],
                        "{}".to_owned(),
                        format!("{expected} fixture"),
                        None,
                    )
                    .unwrap_or_else(|error| panic!("admit {expected}: {error:?}")),
                None => request.refuse(AdmissionRefusal::UnknownTool),
            };
            workspace.emit(agent.handle(Input::ToolAdmissionResolved(outcome)).events);
        }
        assert!(requests.next().is_none());
        let approvals: Vec<_> = agent
            .pending_approvals()
            .map(|pending| {
                (
                    pending.admitted().requested().call_id.clone(),
                    pending.approval_id().clone(),
                )
            })
            .collect();
        let approval_id = |call_id: &str| {
            approvals
                .iter()
                .find(|(pending_call_id, _)| pending_call_id.as_str() == call_id)
                .map(|(_, approval_id)| approval_id.clone())
                .unwrap_or_else(|| panic!("{call_id} awaits approval"))
        };
        let mut allowed = agent.handle(Input::ApprovalDecided {
            approval_id: approval_id("approved"),
            decision: ApprovalDecision::AllowOnce,
        });
        assert!(matches!(
            allowed.effects.as_slice(),
            [Effect::RunTool(call)] if call.requested().call_id.as_str() == "approved"
        ));
        workspace.emit(std::mem::take(&mut allowed.events));
        workspace.emit(finish_tool(
            &mut agent,
            "approved",
            succeeded_tool_result("approved done"),
        ));
        workspace.emit(
            agent
                .handle(Input::ApprovalDecided {
                    approval_id: approval_id("denied"),
                    decision: ApprovalDecision::Deny,
                })
                .events,
        );
        workspace.emit(finish_tool(
            &mut agent,
            "success",
            succeeded_tool_result("done"),
        ));
        workspace.emit(finish_tool(
            &mut agent,
            "execution-failed",
            failed_tool_result("executor refused fixture"),
        ));
        workspace.emit(agent.handle(Input::Interrupted).events);

        assert_tool_lifecycle_projection(&workspace);
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
        assert_eq!(workspace.state().composer().text(), "discard me");

        let interrupted = workspace.handle(&Event::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        )));
        assert_eq!(workspace.state().composer().text(), "");
        assert_eq!(
            interrupted.interrupted, None,
            "clearing a draft must not also stop the running turn"
        );
        assert!(agent.is_running());

        let interrupted = workspace.handle(&Event::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        )));
        let target = interrupted
            .interrupted
            .unwrap_or_else(|| panic!("Ctrl-C on an empty draft must name its conversation"));
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
        use plexmaton_provider::{ModelRegistry, resolve_api_key};
        use plexmaton_runtime::{LiveRuntime, NativeToolCatalog};

        let agent_id = AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}"));
        let config = ModelRegistry::parse(
            r#"active_model = { provider = "test", model = "fixture" }

[providers.test]
base_url = "http://127.0.0.1:9/v1"
api_key_env = "TEST_KEY"
[providers.test.models.fixture]
api = "openai_responses"
id = "fixture"
reasoning_effort = "none"
context_window_tokens = 100000
max_output_tokens = 10000
output_reserve_tokens = 5000
"#,
        )
        .unwrap_or_else(|error| panic!("test config: {error}"));
        let model = config.active_model();
        let key = resolve_api_key(model, Some(OsString::from("fixture-only")))
            .unwrap_or_else(|error| panic!("test key: {error}"));
        let workspace =
            std::env::current_dir().unwrap_or_else(|error| panic!("test workspace: {error}"));
        let tools = NativeToolCatalog::open(
            &workspace,
            model.api_key_env(),
            "/bin/false",
            "/bin/false",
            Vec::new(),
        )
        .unwrap_or_else(|error| panic!("test native tools: {error}"));
        let mut runtime =
            LiveRuntime::provider(agent_id.clone(), "Agent A", model.clone(), key, tools)
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
            workspace.state().draft(&agent_id).text(),
            "do not lose this"
        );
        runtime
            .shutdown()
            .await
            .unwrap_or_else(|error| panic!("shutdown: {error}"));
    }

    /// JRN-7: the composition root restores text and maps typed persistence failures visibly.
    #[test]
    fn persistence_failure_restores_the_draft_and_opens_one_notice() {
        let agent_id = plexmaton_core::AgentId::new("agent-a")
            .unwrap_or_else(|error| panic!("fixture: {error}"));
        for (failure, expected) in [
            (
                plexmaton_runtime::PersistenceFailure::NotWritten,
                PersistenceNotice::NotWritten,
            ),
            (
                plexmaton_runtime::PersistenceFailure::OutcomeUnknown,
                PersistenceNotice::OutcomeUnknown,
            ),
        ] {
            let mut workspace = Workspace::default();
            restore_undelivered(
                &mut workspace,
                agent_id.clone(),
                plexmaton_runtime::DispatchReport {
                    undelivered: vec![plexmaton_agent::UndeliveredInput {
                        text: "keep this exact draft".to_owned(),
                        reason: plexmaton_agent::UndeliveredReason::PersistenceFailed,
                    }],
                    persistence_failure: Some(failure),
                    ..plexmaton_runtime::DispatchReport::default()
                },
            );

            assert_eq!(
                workspace.state().draft(&agent_id).text(),
                "keep this exact draft"
            );
            assert!(matches!(
                workspace.state().notices().next(),
                Some(NoticeView::PersistenceFailed(actual)) if *actual == expected
            ));
            assert_eq!(workspace.state().notices().count(), 1);
        }
    }

    /// JRN-7: cleanup diagnostics cross the composition root as typed visible notices.
    #[test]
    fn combined_failures_keep_persistence_as_the_visible_tail_notice() {
        let mut workspace = Workspace::default();
        restore_undelivered(
            &mut workspace,
            plexmaton_core::AgentId::new("agent-a")
                .unwrap_or_else(|error| panic!("fixture: {error}")),
            plexmaton_runtime::DispatchReport {
                cleanup_failures: vec![
                    plexmaton_runtime::CleanupFailure::Provider,
                    plexmaton_runtime::CleanupFailure::Tools,
                    plexmaton_runtime::CleanupFailure::JournalWriter,
                ],
                persistence_failure: Some(plexmaton_runtime::PersistenceFailure::OutcomeUnknown),
                ..plexmaton_runtime::DispatchReport::default()
            },
        );

        assert_eq!(
            workspace.state().notices().cloned().collect::<Vec<_>>(),
            [
                NoticeView::CleanupFailed(CleanupNotice::Provider),
                NoticeView::CleanupFailed(CleanupNotice::Tools),
                NoticeView::CleanupFailed(CleanupNotice::JournalWriter),
                NoticeView::PersistenceFailed(PersistenceNotice::OutcomeUnknown),
            ]
        );
    }

    /// JRN-7/LOOP-6: process exit surfaces exact unsent text after the TUI releases the terminal.
    #[test]
    fn shutdown_report_is_not_silently_discarded() {
        let error = surface_shutdown_report(plexmaton_runtime::DispatchReport {
            undelivered: vec![plexmaton_agent::UndeliveredInput {
                text: "exact\ntext".to_owned(),
                reason: plexmaton_agent::UndeliveredReason::Shutdown,
            }],
            persistence_failure: Some(plexmaton_runtime::PersistenceFailure::OutcomeUnknown),
            cleanup_failures: vec![plexmaton_runtime::CleanupFailure::JournalWriter],
            ..plexmaton_runtime::DispatchReport::default()
        })
        .err()
        .unwrap_or_else(|| panic!("non-empty shutdown report was discarded"));
        let shown = error.to_string();

        assert!(shown.contains(r#""exact\ntext""#));
        assert!(shown.contains("OutcomeUnknown"));
        assert!(shown.contains("JournalWriter"));
    }

    /// LIVE-1: the production HTTP, loop, native-read, and projection boundaries compose without
    /// a sequence or item-revision refusal.
    #[tokio::test]
    async fn a_native_tool_round_trip_is_a_stream_the_projection_accepts() {
        use std::ffi::OsString;

        use plexmaton_agent::Input;
        use plexmaton_core::AgentId;
        use plexmaton_provider::{ModelRegistry, resolve_api_key};
        use plexmaton_runtime::{LiveRuntime, NativeToolCatalog};

        let fixture = FixtureWorkspace::new();
        std::fs::write(fixture.path().join("README.md"), "Plexmaton fixture\n")
            .unwrap_or_else(|error| panic!("write fixture file: {error}"));
        let (base_url, server) = fixture_http_server([
            include_str!("../../plexmaton-provider/tests/fixtures/chat_tool_call.sse"),
            include_str!("../../plexmaton-provider/tests/fixtures/chat_final_answer.sse"),
        ]);
        let config = ModelRegistry::parse(&format!(
            r#"active_model = {{ provider = "test", model = "fixture" }}

[providers.test]
base_url = "{base_url}"
api_key_env = "TEST_KEY"
[providers.test.models.fixture]
api = "openai_chat_completions"
id = "fixture"
reasoning_effort = "none"
context_window_tokens = 100000
max_output_tokens = 10000
output_reserve_tokens = 5000
"#
        ))
        .unwrap_or_else(|error| panic!("test config: {error}"));
        let model = config.active_model();
        let key = resolve_api_key(model, Some(OsString::from("fixture-only")))
            .unwrap_or_else(|error| panic!("test key: {error}"));
        let tools = NativeToolCatalog::open(
            fixture.path(),
            model.api_key_env(),
            "/bin/false",
            "/bin/false",
            Vec::new(),
        )
        .unwrap_or_else(|error| panic!("test native tools: {error}"));
        let agent_id =
            AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture agent: {error}"));
        let mut runtime =
            LiveRuntime::provider(agent_id.clone(), "Agent A", model.clone(), key, tools)
                .unwrap_or_else(|error| panic!("test runtime: {error}"));
        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 40))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        let mut streamed_deltas = 0_usize;
        let mut project = |workspace: &mut Workspace, event: SessionEventEnvelope| {
            let is_delta = matches!(&event.event, SessionEvent::TranscriptDelta { .. });
            workspace.emit(vec![event]);
            let work = workspace
                .draw(&mut terminal)
                .unwrap_or_else(|error| panic!("draw real-producer frame: {error}"));
            if is_delta {
                streamed_deltas = streamed_deltas.saturating_add(1);
                let work = work.unwrap_or_else(|| panic!("a real streamed delta changed no frame"));
                assert_eq!(
                    work.entries_wrapped, 1,
                    "a real streamed delta must re-wrap exactly its entry"
                );
            } else if let Some(work) = work {
                assert!(
                    work.entries_wrapped <= 1,
                    "one real producer event re-wrapped {} transcript entries",
                    work.entries_wrapped
                );
            }
        };
        while let Some(event) = runtime.try_next_event() {
            project(&mut workspace, event);
        }

        dispatch_live(
            &mut runtime,
            &mut workspace,
            AddressedInput {
                to: agent_id,
                input: Input::Submitted {
                    text: "Read the project name.".to_owned(),
                },
            },
        )
        .await
        .unwrap_or_else(|error| panic!("dispatch fixture request: {error}"));
        while runtime.has_active_work() {
            if let Some(event) = tokio::time::timeout(Duration::from_secs(5), runtime.next_event())
                .await
                .unwrap_or_else(|_| panic!("fixture runtime timed out"))
                .unwrap_or_else(|error| panic!("fixture runtime event: {error}"))
            {
                project(&mut workspace, event);
            }
        }
        while let Some(event) = runtime.try_next_event() {
            project(&mut workspace, event);
        }
        runtime
            .shutdown()
            .await
            .unwrap_or_else(|error| panic!("shutdown fixture runtime: {error}"));
        while let Some(event) = runtime.try_next_event() {
            project(&mut workspace, event);
        }

        let requests = server
            .join()
            .unwrap_or_else(|_| panic!("fixture HTTP server panicked"))
            .unwrap_or_else(|error| panic!("fixture HTTP server: {error}"));
        assert_eq!(requests.len(), 2);
        assert_native_tools_advertised(&requests);
        for request in &requests {
            let body: serde_json::Value = serde_json::from_slice(request)
                .unwrap_or_else(|error| panic!("fixture request JSON: {error}"));
            assert_eq!(body["model"], "fixture");
            assert_eq!(body["reasoning_effort"], "none");
            assert_eq!(body["max_completion_tokens"], 10_000);
            assert!(body.get("max_output_tokens").is_none());
        }
        let agent = workspace
            .state()
            .primary_agent()
            .unwrap_or_else(|| panic!("runtime never announced its agent"));
        assert!(agent.transcript().any(|item| {
            item.role == TranscriptRole::Assistant && item.source.contains("Plexmaton.")
        }));
        let read = agent
            .tools()
            .find(|tool| tool.label == "read_file")
            .unwrap_or_else(|| panic!("native read never entered the transcript"));
        assert_eq!(read.status, ToolCallStatus::Succeeded);
        assert!(matches!(
            &read.presentation.invocation,
            Some(ToolDetail::Text { source, omitted_bytes: 0 })
                if source.contains("README.md")
        ));
        assert!(matches!(
            &read.presentation.outcome,
            Some(ToolDetail::Text { source, omitted_bytes: 0 })
                if source.contains("Plexmaton fixture")
        ));
        assert!(agent.usage().is_some());
        assert!(
            streamed_deltas > 0,
            "the recorded provider emitted no transcript delta"
        );
        assert_eq!(
            workspace.state().notices().count(),
            0,
            "the production native-tool stream violated the projection contract"
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
        let before = workspace
            .state()
            .primary_agent()
            .map_or(0, |agent| agent.entries().count());

        dispatch_synthetic(
            &mut runtime,
            &mut workspace,
            route_interrupt(
                AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}")),
            ),
        )
        .unwrap_or_else(|error| panic!("the adapter must report its limitation: {error}"));

        let agent = workspace
            .state()
            .primary_agent()
            .unwrap_or_else(|| panic!("canonical scenario creates the primary agent"));
        assert_eq!(agent.entries().count(), before.saturating_add(1));
        assert!(matches!(
            agent.entries().last(),
            Some(TranscriptEntryView::Text(item))
                if item.kind == TranscriptTextKind::Warning
                    && item.source.contains("cannot interrupt")
        ));
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
