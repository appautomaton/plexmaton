//! Typed bounded command completion and model presentation (CMD-3 and CMD-4).

use std::io;

use plexmaton_agent::bounded_tool_text;
use plexmaton_core::ToolDetail;
use thiserror::Error;
use tokio::task::JoinError;

use crate::capture::CapturedStream;

const MODEL_STREAM_TEXT_BYTES: usize = 30 * 1024;
const MODEL_TRUNCATION_MARKER: &str = "\n...[model text truncated]...\n";
/// Maximum UTF-8 bytes returned by [`CommandOutput::to_model_text`].
pub const MAX_MODEL_OUTPUT_BYTES: usize = 64 * 1024;

/// Typed reason the foreground command stopped.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExitCause {
    /// The root process called `_exit` or returned from `main`.
    Exited {
        /// Process exit code, including non-zero command failures.
        code: i32,
    },
    /// The root process was stopped by a Unix signal without executor intervention.
    Signaled {
        /// Platform signal number.
        signal: i32,
    },
    /// The admitted timeout elapsed, after which the process group was terminated.
    TimedOut,
    /// The owning turn cancelled the command, after which the process group was terminated.
    Cancelled,
}

/// Complete bounded result of one foreground command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandOutput {
    /// Typed root-process outcome or executor stop reason.
    pub cause: ExitCause,
    /// Independently drained, bounded standard output.
    pub stdout: CapturedStream,
    /// Independently drained, bounded standard error.
    pub stderr: CapturedStream,
    #[cfg(test)]
    pub(crate) owned_drains_at_return: usize,
    #[cfg(test)]
    pub(crate) sent_sigkill: bool,
}

impl CommandOutput {
    /// Formats one stable, typed, separately labelled result for the next model step.
    ///
    /// The final UTF-8 byte length is hard-bounded after replacement of invalid input bytes.
    #[must_use]
    pub fn to_model_text(&self) -> String {
        self.model_text_with_omissions().0
    }

    /// Returns the exact model text with every deliberate capture/format omission counted (ENT-4).
    #[must_use]
    pub fn to_transcript_detail(&self) -> ToolDetail {
        let (source, omitted_bytes) = self.model_text_with_omissions();
        bounded_tool_text(&source, omitted_bytes)
    }

    fn model_text_with_omissions(&self) -> (String, u64) {
        let mut rendered = String::new();
        match self.cause {
            ExitCause::Exited { code } => {
                rendered.push_str("status: exited\nexit_code: ");
                rendered.push_str(&code.to_string());
                rendered.push('\n');
            }
            ExitCause::Signaled { signal } => {
                rendered.push_str("status: signaled\nsignal: ");
                rendered.push_str(&signal.to_string());
                rendered.push('\n');
            }
            ExitCause::TimedOut => rendered.push_str("status: timed_out\n"),
            ExitCause::Cancelled => rendered.push_str("status: cancelled\n"),
        }
        let mut omitted_bytes = append_model_stream(&mut rendered, "stdout", &self.stdout);
        omitted_bytes = omitted_bytes.saturating_add(append_model_stream(
            &mut rendered,
            "stderr",
            &self.stderr,
        ));
        let (source, formatting_omitted) = truncate_utf8_middle(&rendered, MAX_MODEL_OUTPUT_BYTES);
        (source, omitted_bytes.saturating_add(formatting_omitted))
    }
}

/// One captured command pipe.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputStream {
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
}

impl std::fmt::Display for OutputStream {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stdout => formatter.write_str("stdout"),
            Self::Stderr => formatter.write_str("stderr"),
        }
    }
}

/// Failure to start, supervise, drain, or reap an admitted command.
#[derive(Debug, Error)]
pub enum CommandExecutionError {
    /// The supplied value was not emitted by this exact trusted definition.
    #[error("call was not admitted by this command definition")]
    InvalidAdmittedCall,
    /// The fixed shell could not be started in the fixed workspace root.
    #[error("cannot spawn foreground command: {0}")]
    Spawn(#[source] io::Error),
    /// The canonical workspace identity changed after admission.
    #[error("command workspace changed after admission")]
    WorkspaceChanged,
    /// The platform did not expose the spawned root process identity.
    #[error("spawned command has no process id")]
    MissingProcessId,
    /// A process-group lifecycle check failed.
    #[error("cannot inspect command process group: {0}")]
    InspectGroup(#[source] io::Error),
    /// A phase of two-step group termination failed.
    #[error("cannot send {signal} to command process group: {source}")]
    SignalGroup {
        /// Phase that failed.
        signal: &'static str,
        /// Platform signal error.
        #[source]
        source: io::Error,
    },
    /// The root process could not be reaped.
    #[error("cannot reap foreground command: {0}")]
    Wait(#[source] io::Error),
    /// One pipe failed before EOF.
    #[error("cannot drain command {stream}: {source}")]
    StreamRead {
        /// Pipe that failed.
        stream: OutputStream,
        /// Asynchronous read failure.
        #[source]
        source: io::Error,
    },
    /// An owned pipe-drain task failed before it could be joined.
    #[error("command {stream} drain task failed: {source}")]
    DrainTask {
        /// Pipe whose owned task failed.
        stream: OutputStream,
        /// Runtime task failure.
        #[source]
        source: JoinError,
    },
    /// Unix returned neither an exit code nor a terminating signal.
    #[error("foreground command returned an unclassified exit status")]
    UnclassifiedExit,
    /// The owned process group remained observable after bounded `SIGKILL` cleanup.
    #[error("command process group still exists after SIGKILL cleanup deadline")]
    ProcessGroupSurvived,
}

fn append_model_stream(rendered: &mut String, label: &str, stream: &CapturedStream) -> u64 {
    rendered.push_str(label);
    rendered.push_str("_bytes: ");
    rendered.push_str(&stream.total_bytes().to_string());
    rendered.push('\n');
    rendered.push_str(label);
    rendered.push_str("_complete: ");
    rendered.push_str(if stream.is_complete() {
        "true"
    } else {
        "false"
    });
    rendered.push('\n');
    rendered.push_str(label);
    rendered.push_str(":\n");
    if stream.total_bytes() == 0 {
        rendered.push_str("[empty]\n");
        0
    } else {
        let text = stream.to_lossy_utf8();
        let (bounded, formatting_omitted) = truncate_utf8_middle(&text, MODEL_STREAM_TEXT_BYTES);
        rendered.push_str(&bounded);
        rendered.push('\n');
        stream.omitted_bytes().saturating_add(formatting_omitted)
    }
}

fn truncate_utf8_middle(text: &str, limit: usize) -> (String, u64) {
    if text.len() <= limit {
        return (text.to_owned(), 0);
    }
    if limit <= MODEL_TRUNCATION_MARKER.len() {
        return (
            MODEL_TRUNCATION_MARKER[..limit].to_owned(),
            text.len() as u64,
        );
    }
    let retained = limit - MODEL_TRUNCATION_MARKER.len();
    let mut head_end = retained / 2;
    while !text.is_char_boundary(head_end) {
        head_end -= 1;
    }
    let mut tail_start = text.len() - (retained - head_end);
    while !text.is_char_boundary(tail_start) {
        tail_start += 1;
    }
    let mut bounded = String::with_capacity(limit);
    bounded.push_str(&text[..head_end]);
    bounded.push_str(MODEL_TRUNCATION_MARKER);
    bounded.push_str(&text[tail_start..]);
    let omitted = text
        .len()
        .saturating_sub(head_end)
        .saturating_sub(text.len().saturating_sub(tail_start));
    (bounded, omitted as u64)
}

#[cfg(test)]
mod tests {
    use plexmaton_core::ToolDetail;

    use super::{CommandOutput, ExitCause, MAX_MODEL_OUTPUT_BYTES};
    use crate::capture::{CapturedStream, MAX_RETAINED_STREAM_BYTES};

    #[test]
    fn cmd_3_and_cmd_4_model_formatter_is_typed_and_hard_bounded_after_lossy_utf8() {
        let invalid = vec![0xff; MAX_RETAINED_STREAM_BYTES];
        for (cause, expected) in [
            (
                ExitCause::Exited { code: 23 },
                "status: exited\nexit_code: 23",
            ),
            (
                ExitCause::Signaled { signal: 9 },
                "status: signaled\nsignal: 9",
            ),
            (ExitCause::TimedOut, "status: timed_out"),
            (ExitCause::Cancelled, "status: cancelled"),
        ] {
            let output = CommandOutput {
                cause,
                stdout: CapturedStream::from_bytes(&invalid),
                stderr: CapturedStream::from_bytes(b"distinct stderr"),
                owned_drains_at_return: 0,
                sent_sigkill: false,
            };
            let model = output.to_model_text();
            let ToolDetail::Text {
                source,
                omitted_bytes,
            } = output.to_transcript_detail()
            else {
                panic!("command output presenter changed detail kind");
            };
            assert_eq!(source, model, "presentation preserves exact model bytes");
            assert!(omitted_bytes > 0, "bounded invalid stream was truncated");
            assert!(model.starts_with(expected));
            assert!(model.contains("stdout:\n"));
            assert!(model.contains("stderr:\ndistinct stderr"));
            assert!(model.contains('\u{fffd}'));
            assert!(model.len() <= MAX_MODEL_OUTPUT_BYTES);
        }

        let empty = CommandOutput {
            cause: ExitCause::Exited { code: 0 },
            stdout: CapturedStream::empty(),
            stderr: CapturedStream::empty(),
            owned_drains_at_return: 0,
            sent_sigkill: false,
        }
        .to_model_text();
        assert_eq!(empty.matches("[empty]").count(), 2);
    }
}
