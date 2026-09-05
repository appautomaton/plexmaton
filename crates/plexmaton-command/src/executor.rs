use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;
use std::{future::Future, io};

use rustix::io::Errno;
use rustix::process::{Pid, Signal};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::admission::CanonicalArguments;
use crate::capture::{CapturedStream, DrainTracker, drain};
use crate::environment::CommandEnvironment;
use crate::process::{OsProcessOperations, ProcessOperations, try_wait_child, wait_for_child};
use crate::result::{CommandExecutionError, CommandOutput, ExitCause, OutputStream};

const SHELL: &str = "/bin/sh";
const TERMINATION_GRACE: Duration = Duration::from_secs(1);
const GROUP_SETTLE_DEADLINE: Duration = Duration::from_secs(1);
const GROUP_POLL_INTERVAL: Duration = Duration::from_millis(10);
const DRAIN_GRACE: Duration = Duration::from_secs(2);

pub(crate) async fn execute(
    workspace_root: &Path,
    arguments: CanonicalArguments,
    environment: &CommandEnvironment,
    cancellation: CancellationToken,
) -> Result<CommandOutput, CommandExecutionError> {
    execute_with_operations(
        workspace_root,
        arguments,
        environment,
        cancellation,
        &OsProcessOperations,
    )
    .await
}

async fn execute_with_operations<O: ProcessOperations>(
    workspace_root: &Path,
    arguments: CanonicalArguments,
    environment: &CommandEnvironment,
    cancellation: CancellationToken,
    operations: &O,
) -> Result<CommandOutput, CommandExecutionError> {
    if cancellation.is_cancelled() {
        return Ok(CommandOutput {
            cause: ExitCause::Cancelled,
            stdout: CapturedStream::empty(),
            stderr: CapturedStream::empty(),
            #[cfg(test)]
            owned_drains_at_return: 0,
            #[cfg(test)]
            sent_sigkill: false,
        });
    }
    let mut command = Command::new(SHELL);
    command
        .arg("-c")
        .arg(arguments.cmd)
        .current_dir(workspace_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    environment.install(&mut command, workspace_root);
    let mut child = command.spawn().map_err(CommandExecutionError::Spawn)?;
    let Some(process_group) = child.id().and_then(|id| Pid::from_raw(id.cast_signed())) else {
        kill_root_and_wait(&mut child).await;
        return Err(CommandExecutionError::MissingProcessId);
    };
    operations.record_spawn(process_group);
    let Some(stdout) = child.stdout.take() else {
        let primary =
            CommandExecutionError::Spawn(io::Error::other("spawned command has no stdout pipe"));
        best_effort_cleanup(process_group, &mut child, operations).await;
        return Err(primary);
    };
    let Some(stderr) = child.stderr.take() else {
        drop(stdout);
        let primary =
            CommandExecutionError::Spawn(io::Error::other("spawned command has no stderr pipe"));
        best_effort_cleanup(process_group, &mut child, operations).await;
        return Err(primary);
    };
    let drain_tracker = DrainTracker::default();
    let drain_seal = CancellationToken::new();
    let stdout_drain = tokio::spawn(drain(
        stdout,
        OutputStream::Stdout,
        drain_tracker.clone(),
        drain_seal.clone(),
    ));
    let stderr_drain = tokio::spawn(drain(
        stderr,
        OutputStream::Stderr,
        drain_tracker.clone(),
        drain_seal.clone(),
    ));

    let timeout = tokio::time::sleep(Duration::from_millis(arguments.timeout_ms));
    let trigger = wait_for_trigger(
        &cancellation,
        timeout,
        wait_for_child(&mut child, operations),
    )
    .await;

    let supervision = match trigger {
        Trigger::Cancelled => terminate_and_reap(process_group, &mut child, operations)
            .await
            .map(|report| (ExitCause::Cancelled, report)),
        Trigger::TimedOut => terminate_and_reap(process_group, &mut child, operations)
            .await
            .map(|report| (ExitCause::TimedOut, report)),
        Trigger::Completed(Ok(status)) => {
            match terminate_remaining_descendants(process_group, operations).await {
                Ok(report) => classify(status).map(|cause| (cause, report)),
                Err(error) => Err(error),
            }
        }
        Trigger::Completed(Err(wait_error)) => Err(CommandExecutionError::Wait(wait_error)),
    };
    if supervision.is_err() {
        // Preserve the first typed supervision failure, but never let it become an early-return
        // path that drops a live root or process group. Cleanup is deliberately best-effort: its
        // failures cannot replace the error that caused this transition (CMD-5 and CMD-6).
        best_effort_cleanup(process_group, &mut child, operations).await;
    }
    // Both handles are awaited even when process supervision failed. Once a child has started,
    // no error path may turn either owned drain into a detached task (CMD-5 and CMD-6).
    let drains = join_drains(
        stdout_drain,
        stderr_drain,
        &drain_seal,
        tokio::time::sleep(DRAIN_GRACE),
    )
    .await;
    let (cause, termination) = supervision?;
    #[cfg(not(test))]
    let _ = termination;
    let (stdout, stderr) = drains?;
    Ok(CommandOutput {
        cause,
        stdout,
        stderr,
        #[cfg(test)]
        owned_drains_at_return: drain_tracker.active(),
        #[cfg(test)]
        sent_sigkill: termination.sent_sigkill,
    })
}

async fn wait_for_trigger<D, P>(
    cancellation: &CancellationToken,
    deadline: D,
    process: P,
) -> Trigger
where
    D: Future<Output = ()>,
    P: Future<Output = io::Result<ExitStatus>>,
{
    tokio::pin!(deadline);
    tokio::pin!(process);
    tokio::select! {
        biased;
        () = cancellation.cancelled() => Trigger::Cancelled,
        () = &mut deadline => {
            if cancellation.is_cancelled() {
                Trigger::Cancelled
            } else {
                Trigger::TimedOut
            }
        }
        status = &mut process => Trigger::Completed(status),
    }
}

enum Trigger {
    Completed(Result<ExitStatus, io::Error>),
    TimedOut,
    Cancelled,
}

async fn terminate_and_reap(
    process_group: Pid,
    child: &mut Child,
    operations: &impl ProcessOperations,
) -> Result<TerminationReport, CommandExecutionError> {
    signal_group(operations, process_group, Signal::TERM, "SIGTERM")?;
    let deadline = tokio::time::Instant::now() + TERMINATION_GRACE;
    let mut root_reaped = false;
    loop {
        if !root_reaped {
            root_reaped = try_wait_child(child, operations)
                .map_err(CommandExecutionError::Wait)?
                .is_some();
        }
        if root_reaped && !process_group_exists(operations, process_group)? {
            return Ok(TerminationReport::default());
        }
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(GROUP_POLL_INTERVAL).await;
    }

    let mut sent_sigkill = kill_group_if_present(operations, process_group)?;
    if !root_reaped {
        // The root may itself have escaped the process group. It remains an owned child even when
        // descendants outside that group are beyond this executor's containment claim.
        child.start_kill().map_err(CommandExecutionError::Wait)?;
        sent_sigkill = true;
        wait_for_child(child, operations)
            .await
            .map_err(CommandExecutionError::Wait)?;
    }
    await_group_disappearance(operations, process_group).await?;
    Ok(TerminationReport { sent_sigkill })
}

async fn terminate_remaining_descendants(
    process_group: Pid,
    operations: &impl ProcessOperations,
) -> Result<TerminationReport, CommandExecutionError> {
    if process_group_exists(operations, process_group)? {
        return terminate_group(process_group, operations).await;
    }
    Ok(TerminationReport::default())
}

async fn terminate_group(
    process_group: Pid,
    operations: &impl ProcessOperations,
) -> Result<TerminationReport, CommandExecutionError> {
    signal_group(operations, process_group, Signal::TERM, "SIGTERM")?;
    let deadline = tokio::time::Instant::now() + TERMINATION_GRACE;
    loop {
        if !process_group_exists(operations, process_group)? {
            return Ok(TerminationReport::default());
        }
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(GROUP_POLL_INTERVAL).await;
    }
    let sent_sigkill = kill_group_if_present(operations, process_group)?;
    await_group_disappearance(operations, process_group).await?;
    Ok(TerminationReport { sent_sigkill })
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct TerminationReport {
    sent_sigkill: bool,
}

fn kill_group_if_present(
    operations: &impl ProcessOperations,
    process_group: Pid,
) -> Result<bool, CommandExecutionError> {
    match operations.kill_process_group(process_group, Signal::KILL) {
        Ok(()) => Ok(true),
        Err(Errno::SRCH) => Ok(false),
        Err(error) => Err(CommandExecutionError::SignalGroup {
            signal: "SIGKILL",
            source: io::Error::from(error),
        }),
    }
}

async fn await_group_disappearance(
    operations: &impl ProcessOperations,
    process_group: Pid,
) -> Result<(), CommandExecutionError> {
    let deadline = tokio::time::Instant::now() + GROUP_SETTLE_DEADLINE;
    loop {
        if !process_group_exists(operations, process_group)? {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(CommandExecutionError::ProcessGroupSurvived);
        }
        tokio::time::sleep(GROUP_POLL_INTERVAL).await;
    }
}

async fn best_effort_cleanup(
    process_group: Pid,
    child: &mut Child,
    operations: &impl ProcessOperations,
) {
    // This is the error-recovery path after a root has been spawned. Try every ownership release
    // in order even if an earlier one fails; the caller retains the primary typed error.
    let _ = signal_group(operations, process_group, Signal::KILL, "SIGKILL");
    let _ = child.start_kill();
    let _ = wait_for_child(child, operations).await;
    best_effort_observe_group_disappearance(operations, process_group).await;
}

async fn best_effort_observe_group_disappearance(
    operations: &impl ProcessOperations,
    process_group: Pid,
) {
    let deadline = tokio::time::Instant::now() + GROUP_SETTLE_DEADLINE;
    loop {
        if matches!(process_group_exists(operations, process_group), Ok(false)) {
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            return;
        }
        tokio::time::sleep(GROUP_POLL_INTERVAL).await;
    }
}

async fn kill_root_and_wait(child: &mut Child) {
    let _ = child.start_kill();
    let _ = child.wait().await;
}

fn signal_group(
    operations: &impl ProcessOperations,
    process_group: Pid,
    signal: Signal,
    label: &'static str,
) -> Result<(), CommandExecutionError> {
    match operations.kill_process_group(process_group, signal) {
        Ok(()) | Err(Errno::SRCH) => Ok(()),
        Err(error) => Err(CommandExecutionError::SignalGroup {
            signal: label,
            source: io::Error::from(error),
        }),
    }
}

fn process_group_exists(
    operations: &impl ProcessOperations,
    process_group: Pid,
) -> Result<bool, CommandExecutionError> {
    match operations.test_kill_process_group(process_group) {
        Ok(()) => Ok(true),
        Err(Errno::SRCH) => Ok(false),
        // Like process probes, signal 0 uses EPERM to report an existing target for which the
        // caller lacks signal permission. It is existence evidence, not an inspection failure.
        Err(Errno::PERM) => Ok(true),
        Err(error) => Err(CommandExecutionError::InspectGroup(io::Error::from(error))),
    }
}

async fn join_drains<D>(
    mut stdout: JoinHandle<Result<CapturedStream, CommandExecutionError>>,
    mut stderr: JoinHandle<Result<CapturedStream, CommandExecutionError>>,
    seal: &CancellationToken,
    deadline: D,
) -> Result<(CapturedStream, CapturedStream), CommandExecutionError>
where
    D: Future<Output = ()>,
{
    tokio::pin!(deadline);
    let mut stdout_result = None;
    let mut stderr_result = None;
    let mut sealed = false;
    while stdout_result.is_none() || stderr_result.is_none() {
        tokio::select! {
            biased;
            result = &mut stdout, if stdout_result.is_none() => stdout_result = Some(result),
            result = &mut stderr, if stderr_result.is_none() => stderr_result = Some(result),
            () = &mut deadline, if !sealed => {
                // An escaped group may retain inherited writers indefinitely. Sealing makes both
                // readers return partial evidence; completed handles remain consumed exactly once.
                sealed = true;
                seal.cancel();
            }
        }
    }
    let (Some(stdout), Some(stderr)) = (stdout_result, stderr_result) else {
        unreachable!("the drain loop exits only after retaining both join results")
    };
    let stdout = stdout.map_err(|source| CommandExecutionError::DrainTask {
        stream: OutputStream::Stdout,
        source,
    })??;
    let stderr = stderr.map_err(|source| CommandExecutionError::DrainTask {
        stream: OutputStream::Stderr,
        source,
    })??;
    Ok((stdout, stderr))
}

#[cfg(unix)]
fn classify(status: ExitStatus) -> Result<ExitCause, CommandExecutionError> {
    use std::os::unix::process::ExitStatusExt;

    if let Some(code) = status.code() {
        Ok(ExitCause::Exited { code })
    } else if let Some(signal) = status.signal() {
        Ok(ExitCause::Signaled { signal })
    } else {
        Err(CommandExecutionError::UnclassifiedExit)
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::{OsStr, OsString};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
    use std::time::Duration;

    use plexmaton_agent::{
        AdmissionOutcome, AdmissionRequest, Agent, Effect, Input, ModelEvent, ModelOutputPosition,
        StopReason, ToolCall,
    };
    use plexmaton_core::{AgentId, ToolCallId};
    use rustix::io::Errno;
    use rustix::process::{
        Pid, Signal, kill_process, kill_process_group, test_kill_process, test_kill_process_group,
    };
    use tokio_util::sync::CancellationToken;

    use super::{
        CapturedStream, CommandEnvironment, ExitCause, ProcessOperations, execute,
        execute_with_operations, join_drains,
    };
    use crate::admission::{COMMAND_TOOL_NAME, CommandTool};
    use crate::capture::MAX_RETAINED_STREAM_BYTES;

    static NEXT_DIR: AtomicU64 = AtomicU64::new(1);

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum InjectedFailure {
        Signal,
        KillSignal,
        Inspect,
        Wait,
        TryWait,
    }

    struct InjectingProcessOperations {
        failure: InjectedFailure,
        fired: AtomicBool,
        spawned_group: AtomicI32,
    }

    struct MissingGroupOperations {
        kill_called: AtomicBool,
    }

    struct PermissionDeniedProbeOperations;

    impl ProcessOperations for MissingGroupOperations {
        fn kill_process_group(&self, _process_group: Pid, _signal: Signal) -> Result<(), Errno> {
            self.kill_called.store(true, Ordering::SeqCst);
            Err(Errno::SRCH)
        }

        fn test_kill_process_group(&self, _process_group: Pid) -> Result<(), Errno> {
            Err(Errno::SRCH)
        }
    }

    impl ProcessOperations for PermissionDeniedProbeOperations {
        fn kill_process_group(&self, _process_group: Pid, _signal: Signal) -> Result<(), Errno> {
            Ok(())
        }

        fn test_kill_process_group(&self, _process_group: Pid) -> Result<(), Errno> {
            Err(Errno::PERM)
        }
    }

    impl InjectingProcessOperations {
        fn new(failure: InjectedFailure) -> Self {
            Self {
                failure,
                fired: AtomicBool::new(false),
                spawned_group: AtomicI32::new(0),
            }
        }

        fn fail_once(&self, point: InjectedFailure) -> bool {
            self.failure == point && !self.fired.swap(true, Ordering::SeqCst)
        }

        fn spawned_group(&self) -> Pid {
            let raw = self.spawned_group.load(Ordering::SeqCst);
            Pid::from_raw(raw).unwrap_or_else(|| panic!("fixture did not observe spawned group"))
        }
    }

    impl ProcessOperations for InjectingProcessOperations {
        fn record_spawn(&self, process_group: Pid) {
            self.spawned_group
                .store(process_group.as_raw_pid(), Ordering::SeqCst);
        }

        fn kill_process_group(&self, process_group: Pid, signal: Signal) -> Result<(), Errno> {
            if signal == Signal::TERM && self.fail_once(InjectedFailure::Signal) {
                return Err(Errno::PERM);
            }
            if signal == Signal::KILL && self.fail_once(InjectedFailure::KillSignal) {
                return Err(Errno::PERM);
            }
            kill_process_group(process_group, signal)
        }

        fn test_kill_process_group(&self, process_group: Pid) -> Result<(), Errno> {
            if self.fail_once(InjectedFailure::Inspect) {
                return Err(Errno::IO);
            }
            test_kill_process_group(process_group)
        }

        fn before_child_wait(&self) -> std::io::Result<()> {
            if self.fail_once(InjectedFailure::Wait) {
                return Err(std::io::Error::other("injected child wait failure"));
            }
            Ok(())
        }

        fn before_child_try_wait(&self) -> std::io::Result<()> {
            if self.fail_once(InjectedFailure::TryWait) {
                return Err(std::io::Error::other("injected child try_wait failure"));
            }
            Ok(())
        }
    }

    struct TestWorkspace(PathBuf);

    impl TestWorkspace {
        fn new() -> Self {
            let serial = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "plexmaton-command-executor-{}-{serial}",
                std::process::id()
            ));
            std::fs::create_dir(&path)
                .unwrap_or_else(|error| panic!("create test workspace {path:?}: {error}"));
            Self(path)
        }

        fn tool(&self) -> CommandTool {
            CommandTool::new(&self.0, "TEST_KEY")
                .unwrap_or_else(|error| panic!("command tool fixture: {error}"))
        }
    }

    impl Drop for TestWorkspace {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0)
                .unwrap_or_else(|error| panic!("remove test workspace {:?}: {error}", self.0));
        }
    }

    fn admitted(
        tool: &CommandTool,
        command: &str,
        timeout_ms: u64,
    ) -> plexmaton_agent::AdmittedToolCall {
        let request = admission_request(command, timeout_ms);
        let AdmissionOutcome::Admitted(call) = tool.admit(request) else {
            panic!("fixture command must be admitted");
        };
        call
    }

    fn admission_request(command: &str, timeout_ms: u64) -> AdmissionRequest {
        let mut agent = Agent::new(
            AgentId::new("command-executor-fixture")
                .unwrap_or_else(|error| panic!("fixture agent id: {error}")),
        );
        let _submitted = agent.handle_at(
            Input::Submitted {
                text: "exercise command tool".to_owned(),
            },
            plexmaton_agent::UnixMillis::EPOCH,
        );
        let step_id = agent
            .active_model_step()
            .unwrap_or_else(|| panic!("fixture model step did not open"));
        let call = ToolCall {
            call_id: ToolCallId::new("command-1")
                .unwrap_or_else(|error| panic!("fixture call id: {error}")),
            name: COMMAND_TOOL_NAME.to_owned(),
            arguments: serde_json::json!({"cmd": command, "timeout_ms": timeout_ms}).to_string(),
        };
        let called = agent.handle_at(
            Input::Streamed {
                step_id: step_id.clone(),
                event: ModelEvent::Called {
                    position: ModelOutputPosition::new(0, 0),
                    call,
                },
            },
            plexmaton_agent::UnixMillis::EPOCH,
        );
        assert!(called.effects.is_empty());
        let stopped = agent.handle_at(
            Input::Streamed {
                step_id,
                event: ModelEvent::Stopped(StopReason::ToolCalls),
            },
            plexmaton_agent::UnixMillis::EPOCH,
        );
        let mut effects = stopped.effects.into_iter();
        let Some(Effect::AdmitTool(request)) = effects.next() else {
            panic!("fixture did not emit one admission request");
        };
        assert!(effects.next().is_none());
        request
    }

    async fn wait_for_file(path: &Path) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while std::fs::metadata(path).map_or(true, |metadata| metadata.len() == 0) {
                // Poll the explicit marker at a bounded cadence; elapsed time is not readiness.
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("command did not reach readiness barrier {path:?}"));
    }

    fn read_pid(path: &Path) -> Pid {
        let raw = std::fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("read command pid {path:?}: {error}"));
        let number: i32 = raw
            .trim()
            .parse()
            .unwrap_or_else(|error| panic!("parse command pid {raw:?}: {error}"));
        Pid::from_raw(number).unwrap_or_else(|| panic!("command pid must be positive"))
    }

    fn assert_group_gone(process_group: Pid) {
        assert_eq!(test_kill_process_group(process_group), Err(Errno::SRCH));
    }

    fn assert_process_gone(process: Pid) {
        assert_eq!(test_kill_process(process), Err(Errno::SRCH));
    }

    async fn execute_with_injected_failure(
        failure: InjectedFailure,
    ) -> (super::CommandExecutionError, Pid) {
        let workspace = TestWorkspace::new();
        let operations = InjectingProcessOperations::new(failure);
        let arguments = crate::admission::CanonicalArguments {
            cmd: "trap '' TERM; exec /bin/sleep 30".to_owned(),
            timeout_ms: 25,
            workspace_root: workspace.0.to_string_lossy().into_owned(),
        };
        let error = execute_with_operations(
            &workspace.0,
            arguments,
            &CommandEnvironment::from_pairs([]),
            CancellationToken::new(),
            &operations,
        )
        .await
        .expect_err("injected supervision operation must fail");
        assert!(operations.fired.load(Ordering::SeqCst));
        let process_group = operations.spawned_group();
        (error, process_group)
    }

    struct ProcessGroupCleanup(Pid);

    impl Drop for ProcessGroupCleanup {
        fn drop(&mut self) {
            let _ = kill_process_group(self.0, Signal::KILL);
            let _ = kill_process(self.0, Signal::KILL);
        }
    }

    #[tokio::test]
    async fn cmd_5_signal_failure_retains_primary_error_and_reaps_the_owned_group() {
        let (error, process_group) = execute_with_injected_failure(InjectedFailure::Signal).await;
        let _cleanup = ProcessGroupCleanup(process_group);
        assert!(matches!(
            error,
            super::CommandExecutionError::SignalGroup {
                signal: "SIGTERM",
                ..
            }
        ));
        assert_group_gone(process_group);
        assert_process_gone(process_group);
    }

    #[tokio::test]
    async fn cmd_5_kill_failure_is_retried_while_retaining_the_primary_error() {
        let (error, process_group) =
            execute_with_injected_failure(InjectedFailure::KillSignal).await;
        let _cleanup = ProcessGroupCleanup(process_group);
        assert!(matches!(
            error,
            super::CommandExecutionError::SignalGroup {
                signal: "SIGKILL",
                ..
            }
        ));
        assert_group_gone(process_group);
        assert_process_gone(process_group);
    }

    #[tokio::test]
    async fn cmd_5_inspect_failure_retains_primary_error_and_reaps_the_owned_group() {
        let (error, process_group) = execute_with_injected_failure(InjectedFailure::Inspect).await;
        let _cleanup = ProcessGroupCleanup(process_group);
        assert!(matches!(
            error,
            super::CommandExecutionError::InspectGroup(_)
        ));
        assert_group_gone(process_group);
        assert_process_gone(process_group);
    }

    #[tokio::test]
    async fn cmd_5_wait_failure_retains_primary_error_and_reaps_the_owned_group() {
        let (error, process_group) = execute_with_injected_failure(InjectedFailure::Wait).await;
        let _cleanup = ProcessGroupCleanup(process_group);
        assert!(matches!(error, super::CommandExecutionError::Wait(_)));
        assert_group_gone(process_group);
        assert_process_gone(process_group);
    }

    #[tokio::test]
    async fn cmd_5_try_wait_failure_retains_primary_error_and_reaps_the_owned_group() {
        let (error, process_group) = execute_with_injected_failure(InjectedFailure::TryWait).await;
        let _cleanup = ProcessGroupCleanup(process_group);
        assert!(matches!(error, super::CommandExecutionError::Wait(_)));
        assert_group_gone(process_group);
        assert_process_gone(process_group);
    }

    #[tokio::test]
    async fn cmd_1_executor_refuses_a_call_pinned_to_another_workspace() {
        let first_workspace = TestWorkspace::new();
        let second_workspace = TestWorkspace::new();
        let first_tool = first_workspace.tool();
        let second_tool = second_workspace.tool();
        let call = admitted(&first_tool, "printf ran > must-not-exist", 5_000);
        assert!(matches!(
            second_tool.execute(&call, CancellationToken::new()).await,
            Err(super::CommandExecutionError::InvalidAdmittedCall)
        ));
        assert!(!second_workspace.0.join("must-not-exist").exists());
    }

    #[tokio::test]
    async fn cmd_2_and_cmd_4_use_fixed_noninteractive_context_and_typed_exit() {
        let workspace = TestWorkspace::new();
        let tool = workspace.tool();
        let command = "exit 7";
        let output = tool
            .execute(&admitted(&tool, command, 5_000), CancellationToken::new())
            .await
            .unwrap_or_else(|error| panic!("execute fixture command: {error}"));
        assert_eq!(output.cause, ExitCause::Exited { code: 7 });
        assert_eq!(output.stdout.omitted_bytes(), 0);
        assert_eq!(output.stderr.total_bytes(), 0);

        let null_stdin = tool
            .execute(
                &admitted(&tool, "if read line; then exit 99; else exit 0; fi", 5_000),
                CancellationToken::new(),
            )
            .await
            .unwrap_or_else(|error| panic!("execute null-stdin fixture: {error}"));
        assert_eq!(null_stdin.cause, ExitCause::Exited { code: 0 });

        let signaled = tool
            .execute(
                &admitted(&tool, "kill -TERM $$", 5_000),
                CancellationToken::new(),
            )
            .await
            .unwrap_or_else(|error| panic!("execute signaled fixture: {error}"));
        assert_eq!(signaled.cause, ExitCause::Signaled { signal: 15 });
    }

    #[tokio::test]
    async fn cmd_2_snapshot_preserves_path_and_home_but_scrubs_private_authority() {
        use std::os::unix::fs::PermissionsExt;

        let workspace = TestWorkspace::new();
        let bin = workspace.0.join("bin");
        std::fs::create_dir(&bin)
            .unwrap_or_else(|error| panic!("create helper bin directory: {error}"));
        let helper = bin.join("plexmaton-env-helper");
        std::fs::write(&helper, "#!/bin/sh\nprintf helper")
            .unwrap_or_else(|error| panic!("write PATH helper: {error}"));
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|error| panic!("make PATH helper executable: {error}"));
        let environment = CommandEnvironment::from_pairs([
            (OsString::from("PATH"), bin.into_os_string()),
            (
                OsString::from("HOME"),
                OsString::from("/real/developer/home"),
            ),
            (
                OsString::from("PLEXMATON_LOCAL_API_KEY"),
                OsString::from("must-not-leak"),
            ),
            (
                OsString::from("OPENAI_API_KEY"),
                OsString::from("must-not-leak-either"),
            ),
        ]);
        let arguments = crate::admission::CanonicalArguments {
            cmd: r#"plexmaton-env-helper; printf '|%s|%s|%s|%s' "$HOME" "${PLEXMATON_LOCAL_API_KEY-unset}" "${OPENAI_API_KEY-unset}" "$PWD""#.to_owned(),
            timeout_ms: 5_000,
            workspace_root: workspace.0.to_string_lossy().into_owned(),
        };
        let output = execute(
            &workspace.0,
            arguments,
            &environment,
            CancellationToken::new(),
        )
        .await
        .unwrap_or_else(|error| panic!("execute inherited environment fixture: {error}"));
        assert_eq!(output.cause, ExitCause::Exited { code: 0 });
        assert_eq!(
            output.stdout.head(),
            format!(
                "helper|/real/developer/home|unset|unset|{}",
                workspace.0.display()
            )
            .as_bytes()
        );
    }

    #[tokio::test]
    async fn cmd_2_selected_api_key_environment_is_removed_even_without_credential_shape() {
        let workspace = TestWorkspace::new();
        let environment = CommandEnvironment::from_pairs_excluding(
            [
                (
                    OsString::from("MODEL_AUTH"),
                    OsString::from("must-not-leak"),
                ),
                (OsString::from("MODEL_REGION"), OsString::from("local")),
            ],
            OsStr::new("MODEL_AUTH"),
        );
        let arguments = crate::admission::CanonicalArguments {
            cmd: r#"printf '%s|%s' "${MODEL_AUTH-unset}" "$MODEL_REGION""#.to_owned(),
            timeout_ms: 5_000,
            workspace_root: workspace.0.to_string_lossy().into_owned(),
        };

        let output = execute(
            &workspace.0,
            arguments,
            &environment,
            CancellationToken::new(),
        )
        .await
        .unwrap_or_else(|error| panic!("execute selected credential fixture: {error}"));

        assert_eq!(output.cause, ExitCause::Exited { code: 0 });
        assert_eq!(output.stdout.head(), b"unset|local");
    }

    #[tokio::test]
    async fn cmd_6_pre_cancelled_command_never_spawns() {
        let workspace = TestWorkspace::new();
        let tool = workspace.tool();
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let output = tool
            .execute(
                &admitted(&tool, "printf spawned > must-not-exist", 5_000),
                cancellation,
            )
            .await
            .unwrap_or_else(|error| panic!("execute pre-cancelled fixture: {error}"));
        assert_eq!(output.cause, ExitCause::Cancelled);
        assert!(!workspace.0.join("must-not-exist").exists());
        assert!(output.stdout.is_complete());
        assert!(output.stderr.is_complete());
    }

    #[tokio::test]
    async fn cmd_3_drains_one_mibibyte_from_each_pipe_after_retention_fills() {
        let workspace = TestWorkspace::new();
        let tool = workspace.tool();
        let command = "head -c 1048576 /dev/zero; head -c 1048576 /dev/zero >&2";
        let output = tool
            .execute(&admitted(&tool, command, 5_000), CancellationToken::new())
            .await
            .unwrap_or_else(|error| panic!("execute large-output fixture: {error}"));
        assert_eq!(output.cause, ExitCause::Exited { code: 0 });
        for stream in [&output.stdout, &output.stderr] {
            assert_eq!(stream.total_bytes(), 1024 * 1024);
            assert_eq!(
                stream.head().len() + stream.tail().len(),
                MAX_RETAINED_STREAM_BYTES
            );
            assert_eq!(
                stream.omitted_bytes(),
                (1024 * 1024 - MAX_RETAINED_STREAM_BYTES) as u64
            );
        }
        assert_eq!(output.owned_drains_at_return, 0);
    }

    #[tokio::test]
    async fn cmd_3_completed_drain_is_not_polled_again_when_the_sibling_is_sealed() {
        let seal = CancellationToken::new();
        let stdout = tokio::spawn(async {
            Ok::<_, super::CommandExecutionError>(CapturedStream::from_bytes(b"stdout"))
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            while !stdout.is_finished() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("stdout fixture did not reach its explicit ready state"));
        assert!(stdout.is_finished());
        let stderr_seal = seal.clone();
        let stderr = tokio::spawn(async move {
            stderr_seal.cancelled().await;
            Ok::<_, super::CommandExecutionError>(CapturedStream::from_bytes(b"stderr"))
        });
        let mut first_poll = true;
        let deadline = std::future::poll_fn(move |context| {
            if std::mem::take(&mut first_poll) {
                context.waker().wake_by_ref();
                std::task::Poll::Pending
            } else {
                std::task::Poll::Ready(())
            }
        });

        let (stdout, stderr) = join_drains(stdout, stderr, &seal, deadline)
            .await
            .unwrap_or_else(|error| panic!("join drains: {error}"));

        assert_eq!(stdout.head(), b"stdout");
        assert_eq!(stderr.head(), b"stderr");
    }

    #[tokio::test]
    async fn cmd_3_preserves_invalid_utf8_as_raw_bytes_and_bounds_its_text_view() {
        let workspace = TestWorkspace::new();
        let tool = workspace.tool();
        let output = tool
            .execute(
                &admitted(&tool, r#"printf '\377\376A'"#, 5_000),
                CancellationToken::new(),
            )
            .await
            .unwrap_or_else(|error| panic!("execute invalid UTF-8 fixture: {error}"));
        assert_eq!(output.stdout.head(), &[0xff, 0xfe, b'A']);
        assert_eq!(output.stdout.to_lossy_utf8(), "��A");
    }

    #[tokio::test]
    async fn cmd_5_cancellation_gracefully_terms_reaps_and_joins_drains() {
        let workspace = TestWorkspace::new();
        let tool = workspace.tool();
        let ready = workspace.0.join("ready.pid");
        let term_seen = workspace.0.join("term-seen");
        // Shell wait is interruptible: the trap must run before the grace deadline (CMD-5).
        let command = "trap 'printf term > term-seen; exit 0' TERM; /bin/sleep 30 & printf '%s' $$ > ready.pid; wait";
        let cancellation = CancellationToken::new();
        let runner = {
            let tool = tool.clone();
            let call = admitted(&tool, command, 5_000);
            let cancellation = cancellation.clone();
            tokio::spawn(async move { tool.execute(&call, cancellation).await })
        };
        wait_for_file(&ready).await;
        let process_group = read_pid(&ready);
        cancellation.cancel();
        let output = runner
            .await
            .unwrap_or_else(|error| panic!("join command owner: {error}"))
            .unwrap_or_else(|error| panic!("cancel fixture command: {error}"));
        assert_eq!(output.cause, ExitCause::Cancelled);
        assert_eq!(
            std::fs::read_to_string(term_seen)
                .unwrap_or_else(|error| panic!("SIGTERM trap did not run: {error}")),
            "term"
        );
        assert_group_gone(process_group);
        assert_eq!(output.owned_drains_at_return, 0);
        assert!(!output.sent_sigkill, "graceful SIGTERM must return early");
    }

    #[tokio::test]
    async fn cmd_5_timeout_has_a_typed_cause_and_leaves_no_process_group() {
        let workspace = TestWorkspace::new();
        let tool = workspace.tool();
        let ready = workspace.0.join("timeout.pid");
        let call = admitted(
            &tool,
            "trap '' TERM; printf '%s' $$ > timeout.pid; exec /bin/sleep 30",
            100,
        );
        let output = tool
            .execute(&call, CancellationToken::new())
            .await
            .unwrap_or_else(|error| panic!("timeout fixture command: {error}"));
        let process_group = read_pid(&ready);
        assert_eq!(output.cause, ExitCause::TimedOut);
        assert_group_gone(process_group);
        assert_eq!(output.owned_drains_at_return, 0);
        assert!(
            output.sent_sigkill,
            "SIGTERM-ignoring group requires SIGKILL"
        );
    }

    #[tokio::test]
    async fn cmd_5_interrupt_wins_when_timeout_is_simultaneously_ready() {
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let trigger = super::wait_for_trigger(
            &cancellation,
            std::future::ready(()),
            std::future::pending::<std::io::Result<std::process::ExitStatus>>(),
        )
        .await;
        assert!(matches!(trigger, super::Trigger::Cancelled));
    }

    #[tokio::test]
    async fn cmd_5_deadline_wins_when_completion_is_already_ready() {
        use std::os::unix::process::ExitStatusExt;

        let trigger = super::wait_for_trigger(
            &CancellationToken::new(),
            std::future::ready(()),
            std::future::ready(Ok(std::process::ExitStatus::from_raw(0))),
        )
        .await;
        assert!(matches!(trigger, super::Trigger::TimedOut));
    }

    #[test]
    fn cmd_5_group_disappearing_at_sigkill_does_not_report_it_sent() {
        let operations = MissingGroupOperations {
            kill_called: AtomicBool::new(false),
        };
        let process_group = Pid::from_raw(1).unwrap_or_else(|| panic!("fixture process group"));

        let sent = super::kill_group_if_present(&operations, process_group)
            .unwrap_or_else(|error| panic!("inspect missing group: {error}"));

        assert!(!sent);
        assert!(operations.kill_called.load(Ordering::SeqCst));
    }

    #[test]
    fn cmd_5_permission_denied_probe_still_reports_an_existing_group() {
        let process_group = Pid::from_raw(1).unwrap_or_else(|| panic!("fixture process group"));

        assert!(matches!(
            super::process_group_exists(&PermissionDeniedProbeOperations, process_group),
            Ok(true)
        ));
    }

    #[tokio::test]
    async fn cmd_5_root_exit_terminates_a_descendant_holding_an_inherited_pipe() {
        let workspace = TestWorkspace::new();
        let tool = workspace.tool();
        let group = workspace.0.join("group.pid");
        let descendant = workspace.0.join("descendant.pid");
        // exec preserves the descendant PID and ignored SIGTERM without creating another child.
        let command = r#"/bin/sh -c 'trap "" TERM; printf "%s" $$ > descendant.pid; exec /bin/sleep 30' & printf '%s' $$ > group.pid; while [ ! -s descendant.pid ]; do /bin/sleep 0.01; done; exit 0"#;
        let output = tool
            .execute(&admitted(&tool, command, 5_000), CancellationToken::new())
            .await
            .unwrap_or_else(|error| panic!("descendant fixture command: {error}"));
        let process_group = read_pid(&group);
        let descendant = read_pid(&descendant);
        assert_eq!(output.cause, ExitCause::Exited { code: 0 });
        assert_group_gone(process_group);
        assert_process_gone(descendant);
        assert_eq!(output.owned_drains_at_return, 0);
        assert!(output.sent_sigkill);
    }

    #[tokio::test]
    async fn cmd_3_escaped_pipe_holder_is_sealed_and_joined_with_partial_evidence() {
        let workspace = TestWorkspace::new();
        let tool = workspace.tool();
        let command = r#"/usr/bin/perl -MPOSIX -e 'pipe(R,W) or die; my $pid=fork(); die unless defined $pid; if ($pid) { close W; <R>; exit 0; } close R; POSIX::setsid(); open(F, ">escaped.pid") or die; print F "$$\n"; close F; syswrite(STDOUT, "before-escape\n"); print W "ready\n"; close W; sleep 30;'"#;
        let output = tool
            .execute(&admitted(&tool, command, 5_000), CancellationToken::new())
            .await
            .unwrap_or_else(|error| panic!("escaped pipe fixture: {error}"));
        let escaped = read_pid(&workspace.0.join("escaped.pid"));
        let _cleanup = EscapedProcess(escaped);
        assert_eq!(output.cause, ExitCause::Exited { code: 0 });
        assert_eq!(output.stdout.head(), b"before-escape\n");
        assert!(!output.stdout.is_complete());
        assert!(!output.stderr.is_complete());
        assert_eq!(output.owned_drains_at_return, 0);
        assert!(output.to_model_text().contains("stdout_complete: false"));
    }

    struct EscapedProcess(Pid);

    impl Drop for EscapedProcess {
        fn drop(&mut self) {
            let _ = kill_process(self.0, Signal::KILL);
        }
    }
}
