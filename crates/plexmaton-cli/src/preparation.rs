//! One process owner for expensive presentation preparation; never a terminal-output owner.

use std::{future::Future, io, path::PathBuf, pin::Pin, process::Stdio, time::Duration};

use plexmaton_tui::preparation::{PreparedText, Request};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    process::{Child, ChildStdin, ChildStdout, Command},
};
use tokio_util::sync::CancellationToken;

mod projection;
#[path = "preparation/wire.rs"]
mod wire;
pub use projection::LivePreparation;
pub use wire::{Error as ProtocolError, Refusal, Ticket};

/// Private same-executable dispatch, recognized before configuration and terminal setup.
pub const DRIVER_ARGUMENT: &str = "--__plexmaton-prepare";
const OPERATION_DEADLINE: Duration = Duration::from_secs(2);
const CLEANUP_DEADLINE: Duration = Duration::from_millis(500);

/// Blocks on framed stdin/stdout in the isolated child; never call on the interaction loop.
pub fn run_driver() -> Result<(), ProtocolError> {
    wire::run(io::stdin().lock(), io::stdout().lock())
}

/// Process/protocol failures remain typed; diagnostics never include transcript payloads.
#[derive(Debug, thiserror::Error)]
pub enum Failure {
    /// Invalid, oversized or mismatched framed data.
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    /// A pipe or process could not be opened or advanced.
    #[error("preparation process I/O: {0}")]
    Io(#[from] io::Error),
    /// The persistent worker exited without an owned shutdown.
    #[error("preparation process exited unexpectedly with {0}")]
    Exited(std::process::ExitStatus),
    /// One request exceeded its processing and pipe deadline.
    #[error("preparation process exceeded its deadline")]
    TimedOut,
    /// Reaping failed; the owner keeps the child quarantined and refuses replacement.
    #[error("preparation process cleanup failed: {0}")]
    Cleanup(io::Error),
    /// The owner cannot accept new work after shutdown or uncertain cleanup.
    #[error("preparation owner is stopped or quarantined")]
    Unavailable,
    /// No identity can be reused within this owner's lifetime.
    #[error("preparation ticket space is exhausted")]
    Exhausted,
}

/// One observable completion, including idle-worker failure and cancelled superseded batches.
#[derive(Debug)]
pub enum Completion {
    /// Exact-ticket data, or an explicit batch-capacity refusal.
    Ready(Ticket, Result<Vec<PreparedText>, Refusal>),
    /// CPU work has ended and the cancelled child has been reaped.
    Cancelled(Ticket),
    /// A failed active ticket, or an unexpected idle-child failure with no ticket.
    Failed(Option<Ticket>, Failure),
}

struct Process {
    child: Child,
    input: ChildStdin,
    output: ChildStdout,
}

enum Prepared {
    Reply(wire::Reply),
    Cancelled,
}

type Operation = Pin<Box<dyn Future<Output = (Option<Process>, Result<Prepared, Failure>)>>>;

struct Active {
    ticket: Ticket,
    cancel: CancellationToken,
    operation: Operation,
    pending: Option<wire::Pending>,
}

enum State {
    Idle(Option<Process>),
    Active(Active),
    Quarantined(Option<Process>),
    Stopped,
}

/// One persistent child, one retained operation and at most one latest pending batch (PRE-2).
/// Call `shutdown` on all normal and error paths; kill-on-drop is only a final guard.
pub struct Preparation {
    state: State,
    sequence: u64,
    executable: PathBuf,
    deadline: Duration,
}

impl Preparation {
    /// Uses the current executable's absolute path, or a fixture executable at the process seam.
    pub fn new(executable: PathBuf) -> Self {
        Self {
            state: State::Idle(None),
            sequence: 0,
            executable,
            deadline: OPERATION_DEADLINE,
        }
    }

    /// Admits bounded wire data before cancelling prior work; replacement keeps only the latest.
    pub fn submit(&mut self, requests: Vec<Request>) -> Result<Ticket, Failure> {
        if !self.executable.is_absolute() {
            return Err(Failure::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "preparation executable must be absolute",
            )));
        }
        if matches!(self.state, State::Quarantined(_) | State::Stopped) {
            return Err(Failure::Unavailable);
        }
        let ticket = Ticket(self.sequence.checked_add(1).ok_or(Failure::Exhausted)?);
        let pending = wire::Pending::new(ticket, requests)?;
        self.sequence = ticket.0;
        match &mut self.state {
            State::Active(active) => {
                active.cancel.cancel();
                active.pending = Some(pending);
            }
            State::Idle(process) => {
                let process = process.take();
                self.start(pending, process);
            }
            State::Stopped | State::Quarantined(_) => unreachable!("admission checked state"),
        }
        Ok(ticket)
    }

    fn start(&mut self, pending: wire::Pending, process: Option<Process>) {
        let cancel = CancellationToken::new();
        let mut command = Command::new(&self.executable);
        command.arg(DRIVER_ARGUMENT);
        self.state = State::Active(Active {
            ticket: pending.ticket,
            operation: Box::pin(operate(
                process,
                command,
                pending,
                self.deadline,
                cancel.clone(),
            )),
            cancel,
            pending: None,
        });
    }

    /// Retains the operation itself: losing a select branch must not replay a partial write/read.
    pub async fn next(&mut self) -> Completion {
        match &mut self.state {
            State::Active(active) => {
                let (process, result) = active.operation.as_mut().await;
                let ticket = active.ticket;
                let pending = active.pending.take();
                let quarantined = matches!(result, Err(Failure::Cleanup(_)));
                self.state = if quarantined {
                    State::Quarantined(process)
                } else {
                    State::Idle(process)
                };
                let completion = match result {
                    Err(error) => Completion::Failed(Some(ticket), error),
                    Ok(Prepared::Cancelled) => Completion::Cancelled(ticket),
                    Ok(Prepared::Reply(reply)) => Completion::Ready(ticket, reply.result),
                };
                if !quarantined && let Some(pending) = pending {
                    // operate has already reaped the cancelled child. No overlapping parser.
                    self.start(pending, None);
                }
                completion
            }
            State::Idle(Some(process)) => {
                let status = process.child.wait().await;
                let failure = match status {
                    Ok(status) => Failure::Exited(status),
                    Err(error) => {
                        if let Err(cleanup) = reap(&mut process.child).await {
                            let State::Idle(process) =
                                std::mem::replace(&mut self.state, State::Stopped)
                            else {
                                unreachable!("idle child is still owned");
                            };
                            self.state = State::Quarantined(process);
                            return Completion::Failed(None, Failure::Cleanup(cleanup));
                        }
                        Failure::Io(error)
                    }
                };
                self.state = State::Idle(None);
                Completion::Failed(None, failure)
            }
            State::Idle(None) | State::Quarantined(_) | State::Stopped => {
                std::future::pending().await
            }
        }
    }

    /// Cancels active work and discards the pending tail; `next` observes actual reaping.
    pub fn cancel(&mut self) {
        if let State::Active(active) = &mut self.state {
            active.cancel.cancel();
            active.pending = None;
        }
    }

    /// Every normal/error exit explicitly awaits reaping before restoring terminal ownership.
    pub async fn shutdown(&mut self) -> Result<(), Failure> {
        let state = std::mem::replace(&mut self.state, State::Stopped);
        match state {
            State::Active(mut active) => {
                active.cancel.cancel();
                active.pending = None;
                let (process, result) = active.operation.await;
                if let Some(process) = process {
                    self.stop(process).await?;
                }
                result.map(|_| ())
            }
            State::Idle(Some(process)) | State::Quarantined(Some(process)) => {
                self.stop(process).await
            }
            State::Quarantined(None) => Err(Failure::Unavailable),
            State::Idle(None) | State::Stopped => Ok(()),
        }
    }

    async fn stop(&mut self, mut process: Process) -> Result<(), Failure> {
        match reap(&mut process.child).await {
            Ok(()) => Ok(()),
            Err(error) => {
                self.state = State::Quarantined(Some(process));
                Err(Failure::Cleanup(error))
            }
        }
    }
}

async fn operate(
    process: Option<Process>,
    mut command: Command,
    pending: wire::Pending,
    deadline: Duration,
    cancel: CancellationToken,
) -> (Option<Process>, Result<Prepared, Failure>) {
    if cancel.is_cancelled() {
        if let Some(mut process) = process {
            return match reap(&mut process.child).await {
                Ok(()) => (None, Ok(Prepared::Cancelled)),
                Err(error) => (Some(process), Err(Failure::Cleanup(error))),
            };
        }
        return (None, Ok(Prepared::Cancelled));
    }
    let mut process = match process {
        Some(process) => process,
        None => {
            command
                .env_clear()
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true);
            let mut child = match command.spawn() {
                Ok(child) => child,
                Err(error) => return (None, Err(Failure::Io(error))),
            };
            let input = child.stdin.take().expect("spawned with Stdio::piped stdin");
            let output = child
                .stdout
                .take()
                .expect("spawned with Stdio::piped stdout");
            Process {
                child,
                input,
                output,
            }
        }
    };
    let operation = exchange(&mut process.input, &mut process.output, &pending);
    let result = tokio::select! {
        biased;
        () = cancel.cancelled() => Ok(Prepared::Cancelled),
        result = tokio::time::timeout(deadline, operation) => match result {
            Ok(result) => result.map(Prepared::Reply),
            Err(_) => Err(Failure::TimedOut),
        },
        status = process.child.wait() => match status {
            Ok(status) => Err(Failure::Exited(status)),
            Err(error) => Err(Failure::Io(error)),
        },
    };
    if cancel.is_cancelled() || result.is_err() {
        return match reap(&mut process.child).await {
            Ok(()) => (None, result),
            Err(error) => (Some(process), Err(Failure::Cleanup(error))),
        };
    }
    (Some(process), result)
}

async fn exchange(
    input: &mut ChildStdin,
    output: &mut ChildStdout,
    pending: &wire::Pending,
) -> Result<wire::Reply, Failure> {
    input
        .write_all(&(pending.bytes.len() as u32).to_be_bytes())
        .await?;
    input.write_all(&pending.bytes).await?;
    input.flush().await?;
    let mut header = [0; 4];
    output.read_exact(&mut header).await?;
    let size = wire::length(header, wire::MAX_REPLY_BYTES)?;
    let mut bytes = vec![0; size];
    output.read_exact(&mut bytes).await?;
    Ok(pending.decode_reply(&bytes)?)
}

async fn reap(child: &mut Child) -> io::Result<()> {
    let _ = child.start_kill();
    match tokio::time::timeout(CLEANUP_DEADLINE, child.wait()).await {
        Ok(result) => result.map(|_| ()),
        Err(_) => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "preparation child could not be reaped",
        )),
    }
}
