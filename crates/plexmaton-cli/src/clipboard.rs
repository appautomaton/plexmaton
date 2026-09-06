//! Where copied text goes once it leaves the workspace (SEL-5/SEL-8).
//!
//! The projection produces a [`CopyRequest`](plexmaton_tui::CopyRequest) and hands it back as a
//! value; nothing in `plexmaton-tui` knows a clipboard exists. This adapter owns the
//! local macOS and terminal boundary without using a remote host's native clipboard.

use std::{
    ffi::OsStr,
    future::Future,
    io::{self, Write},
    pin::Pin,
    process::Stdio,
    time::Duration,
};

use crossterm::{clipboard::CopyToClipboard, execute};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

mod helper;
#[cfg(test)]
mod owned_tests;
use helper::{Completion, Failure, copy_through_command};

const COPY_DEADLINE: Duration = Duration::from_millis(500);
// Count retained capacity, including an over-allocated short String, before accepting any effect.
const MAX_COPY_BYTES: usize = 8 * 1024 * 1024;

/// How the process reaches the terminal that owns the user's clipboard.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClipboardRoute {
    /// A local macOS session without a multiplexer or embedded terminal.
    LocalMacOs,
    /// The process is attached directly to the terminal emulator.
    Direct,
    /// tmux is the immediate terminal and must carry the copy to its outer client.
    Tmux {
        /// False inside an editor terminal, where tmux is not the immediate escape parser.
        dcs_passthrough: bool,
    },
}

impl ClipboardRoute {
    pub(crate) fn detect() -> Self {
        let embedded_editor = [
            "NVIM",
            "NVIM_LISTEN_ADDRESS",
            "VIM_TERMINAL",
            "INSIDE_EMACS",
        ]
        .into_iter()
        .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()));
        let remote_or_multiplexed = ["SSH_CONNECTION", "SSH_TTY", "SSH_CLIENT", "STY"]
            .into_iter()
            .any(|name| std::env::var_os(name).is_some());
        Self::for_host(
            cfg!(target_os = "macos"),
            remote_or_multiplexed,
            std::env::var_os("TMUX").as_deref(),
            embedded_editor,
        )
    }

    fn for_host(
        macos: bool,
        remote_or_multiplexed: bool,
        tmux: Option<&OsStr>,
        embedded_editor: bool,
    ) -> Self {
        let terminal = Self::from_environment(tmux, embedded_editor);
        if macos && !remote_or_multiplexed && tmux.is_none() && !embedded_editor {
            Self::LocalMacOs
        } else {
            terminal
        }
    }

    fn from_environment(tmux: Option<&OsStr>, embedded_editor: bool) -> Self {
        match tmux {
            Some(value) if !value.is_empty() => Self::Tmux {
                dcs_passthrough: !embedded_editor,
            },
            Some(_) | None => Self::Direct,
        }
    }
}

struct Active {
    // Retain the future across select iterations: cancelling write_all by dropping it could replay
    // an already-written prefix. This owner, not a detached task, holds the child and source.
    operation: Pin<Box<dyn Future<Output = Result<Completion, Failure>>>>,
    cancel: CancellationToken,
    terminal: Option<io::Result<()>>,
    pending: Option<String>,
}

enum Delivery {
    Idle,
    Active(Active),
    CleanupFailed,
}

/// Owned clipboard delivery, polled beside input; only the interaction loop writes terminal bytes.
pub(crate) struct TerminalClipboard<W> {
    writer: W,
    route: ClipboardRoute,
    // The two real process boundaries are pbcopy and tmux. Tests substitute only that executable,
    // never the delivery lifecycle or the terminal write path.
    helper: Box<dyn Fn() -> Command>,
    delivery: Delivery,
}

impl<W: Write> TerminalClipboard<W> {
    pub(crate) fn new(writer: W, route: ClipboardRoute) -> Self {
        Self {
            writer,
            route,
            helper: Box::new(if route == ClipboardRoute::LocalMacOs {
                pbcopy_command
            } else {
                tmux_copy_command
            }),
            delivery: Delivery::Idle,
        }
    }

    pub(crate) fn from_environment(writer: W) -> Self {
        Self::new(writer, ClipboardRoute::detect())
    }

    fn write_terminal(&mut self, text: &str) -> io::Result<()> {
        self.writer.write_all(&osc52_sequence(text, self.route)?)?;
        self.writer.flush()
    }

    /// Accept the newest exact source without waiting for a helper. At most one child and one
    /// pending source exist; replacement reaps the old child before any newer terminal write.
    pub(crate) fn submit(&mut self, text: String) -> io::Result<()> {
        if text.capacity() > MAX_COPY_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "clipboard source exceeds the 8 MiB allocation limit",
            ));
        }
        match &mut self.delivery {
            Delivery::Idle => self.start(text),
            Delivery::Active(active) => {
                active.cancel.cancel();
                active.pending = Some(text);
                Ok(())
            }
            Delivery::CleanupFailed => Err(io::Error::other("clipboard cleanup failed")),
        }
    }

    fn start(&mut self, text: String) -> io::Result<()> {
        if self.route == ClipboardRoute::Direct {
            return self.write_terminal(&text);
        }
        let terminal =
            matches!(self.route, ClipboardRoute::Tmux { .. }).then(|| self.write_terminal(&text));
        let cancel = CancellationToken::new();
        self.delivery = Delivery::Active(Active {
            operation: Box::pin(copy_through_command(
                (self.helper)(),
                text,
                COPY_DEADLINE,
                cancel.clone(),
            )),
            cancel,
            terminal,
            pending: None,
        });
        Ok(())
    }

    /// Cancellation-safe: the owned helper future survives losing the outer select to input.
    /// Idle work has no wake; completion itself does not change the projection or request a frame.
    pub(crate) async fn next(&mut self) -> io::Result<()> {
        let result = match &mut self.delivery {
            Delivery::Active(active) => active.operation.as_mut().await,
            Delivery::Idle => std::future::pending().await,
            Delivery::CleanupFailed => return Err(io::Error::other("clipboard cleanup failed")),
        };
        let Delivery::Active(active) = std::mem::replace(&mut self.delivery, Delivery::Idle) else {
            unreachable!("only the retained active operation can complete")
        };
        if let Err(Failure::Cleanup(error)) = result {
            self.delivery = Delivery::CleanupFailed;
            return Err(error);
        }
        if let Some(text) = active.pending {
            return self.start(text);
        }
        match (active.terminal, result) {
            (_, Ok(Completion::Accepted | Completion::Cancelled)) | (Some(Ok(())), _) => Ok(()),
            (Some(Err(error)), _) => Err(error),
            (None, Err(error)) => Err(error.into()),
        }
    }

    /// Cancel requested work and reap it before the caller releases the terminal. No pending
    /// source may start another effect after shutdown begins; cleanup failure stays observable.
    pub(crate) async fn shutdown(&mut self) -> io::Result<()> {
        match &mut self.delivery {
            Delivery::Idle => return Ok(()),
            Delivery::CleanupFailed => return Err(io::Error::other("clipboard cleanup failed")),
            Delivery::Active(active) => {
                active.pending = None;
                active.cancel.cancel();
            }
        }
        self.next().await
    }
}

/// Builds direct OSC 52, or tmux's DCS passthrough envelope around that same sequence.
fn osc52_sequence(text: &str, route: ClipboardRoute) -> io::Result<Vec<u8>> {
    let mut direct = Vec::new();
    execute!(direct, CopyToClipboard::to_clipboard_from(text))?;
    let dcs_passthrough = match route {
        ClipboardRoute::LocalMacOs
        | ClipboardRoute::Direct
        | ClipboardRoute::Tmux {
            dcs_passthrough: false,
        } => false,
        ClipboardRoute::Tmux {
            dcs_passthrough: true,
        } => true,
    };
    if !dcs_passthrough {
        return Ok(direct);
    }

    // tmux passthrough doubles every ESC byte in the inner payload. The outer String Terminator
    // closes the DCS; the doubled one belongs to Crossterm's inner OSC 52 sequence.
    let mut wrapped = Vec::with_capacity(direct.len().saturating_mul(2).saturating_add(10));
    wrapped.extend_from_slice(b"\x1bPtmux;");
    for byte in direct {
        if byte == 0x1b {
            wrapped.push(0x1b);
        }
        wrapped.push(byte);
    }
    wrapped.extend_from_slice(b"\x1b\\");
    Ok(wrapped)
}

fn tmux_copy_command() -> Command {
    let mut command = Command::new("tmux");
    command.args(["load-buffer", "-w", "-"]);
    configure_copy_command(&mut command);
    command
}

fn pbcopy_command() -> Command {
    let mut command = Command::new("/usr/bin/pbcopy");
    // pbcopy chooses its encoding from the locale, independently of Rust's UTF-8 strings.
    command.env("LC_ALL", "en_US.UTF-8");
    configure_copy_command(&mut command);
    command
}

fn configure_copy_command(command: &mut Command) {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsStr, time::Duration};

    use tokio::process::Command;

    use super::{
        CancellationToken, ClipboardRoute, Failure, TerminalClipboard, configure_copy_command,
        copy_through_command, osc52_sequence, pbcopy_command, tmux_copy_command,
    };

    #[test]
    fn native_copy_requires_an_unambiguous_local_macos_terminal() {
        // SEL-5: a remote client must never receive the server's native clipboard.
        assert_eq!(
            ClipboardRoute::for_host(true, false, None, false),
            ClipboardRoute::LocalMacOs
        );
        for (macos, remote, editor) in [
            (false, false, false),
            (true, true, false),
            (true, false, true),
        ] {
            assert_eq!(
                ClipboardRoute::for_host(macos, remote, None, editor),
                ClipboardRoute::Direct
            );
        }
        for remote in [false, true] {
            assert_eq!(
                ClipboardRoute::for_host(true, remote, Some(OsStr::new("tmux")), false),
                ClipboardRoute::Tmux {
                    dcs_passthrough: true
                }
            );
        }
    }

    #[test]
    fn native_copy_uses_the_system_helper_with_utf8() {
        // SEL-5: this inspects the command without modifying the host clipboard.
        let command = pbcopy_command();
        assert_eq!(command.as_std().get_program(), "/usr/bin/pbcopy");
        assert!(
            command.as_std().get_envs().any(|(key, value)| {
                key == "LC_ALL" && value == Some(OsStr::new("en_US.UTF-8"))
            })
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn clipboard_helper_receives_exact_unicode_source_and_eof() {
        // SEL-2, SEL-5: the real pipe preserves newlines and Unicode without a host clipboard.
        let source = "中文 e\u{301} 👩‍💻\nsecond line\n\n";
        let mut command = Command::new("/bin/sh");
        command.args([
            "-c",
            "value=$(/bin/cat; printf '.'); test \"$value\" = \"$1.\"",
            "clipboard-fixture",
            source,
        ]);
        configure_copy_command(&mut command);
        assert!(
            copy_through_command(
                command,
                source.into(),
                Duration::from_secs(2),
                CancellationToken::new()
            )
            .await
            .is_ok()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn clipboard_helper_rejection_is_not_reported_as_delivery() {
        // SEL-5: successful stdin writes cannot conceal a rejected copy.
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "/bin/cat > /dev/null; exit 7"]);
        configure_copy_command(&mut command);
        let result = copy_through_command(
            command,
            "source".into(),
            Duration::from_secs(2),
            CancellationToken::new(),
        )
        .await;
        assert!(matches!(result, Err(Failure::Rejected(status)) if status.code() == Some(7)));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn clipboard_deadline_bounds_a_blocked_stdin_pipe() {
        // SEL-5: a helper that never reads cannot hold the application on a full pipe.
        let mut command = Command::new("/bin/sleep");
        command.arg("30");
        configure_copy_command(&mut command);
        let result = copy_through_command(
            command,
            "x".repeat(1_048_576),
            Duration::from_millis(50),
            CancellationToken::new(),
        )
        .await;
        assert!(matches!(result, Err(Failure::TimedOut)));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn clipboard_deadline_also_bounds_waiting_after_eof() {
        // SEL-5: consuming stdin does not permit an unbounded wait for acknowledgement.
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "/bin/cat > /dev/null; exec /bin/sleep 30"]);
        configure_copy_command(&mut command);
        let result = copy_through_command(
            command,
            "source".into(),
            Duration::from_millis(50),
            CancellationToken::new(),
        )
        .await;
        assert!(matches!(result, Err(Failure::TimedOut)));
    }

    #[test]
    fn direct_copy_writes_the_exact_terminated_osc_52_sequence() {
        let mut sink = TerminalClipboard::new(Vec::new(), ClipboardRoute::Direct);

        sink.submit("plexmaton".into())
            .unwrap_or_else(|error| panic!("writing to a vector cannot fail: {error}"));

        assert_eq!(sink.writer, b"\x1b]52;c;cGxleG1hdG9u\x1b\\");
    }

    #[test]
    fn tmux_copy_escapes_the_inner_sequence_inside_one_dcs_envelope() {
        let sequence = osc52_sequence(
            "δ 汉字",
            ClipboardRoute::Tmux {
                dcs_passthrough: true,
            },
        )
        .unwrap_or_else(|error| panic!("encode copy: {error}"));

        assert_eq!(
            sequence,
            b"\x1bPtmux;\x1b\x1b]52;c;zrQg5rGJ5a2X\x1b\x1b\\\x1b\\"
        );
    }

    #[test]
    fn route_detection_requires_a_non_empty_tmux_identity() {
        assert_eq!(
            ClipboardRoute::from_environment(Some(OsStr::new("/tmp/tmux,1,0")), false),
            ClipboardRoute::Tmux {
                dcs_passthrough: true
            }
        );
        assert_eq!(
            ClipboardRoute::from_environment(Some(OsStr::new("")), false),
            ClipboardRoute::Direct
        );
        assert_eq!(
            ClipboardRoute::from_environment(None, false),
            ClipboardRoute::Direct
        );
    }

    #[test]
    fn an_editor_terminal_keeps_tmux_delivery_but_receives_plain_osc_52() {
        let route = ClipboardRoute::from_environment(Some(OsStr::new("/tmp/tmux,1,0")), true);

        assert!(matches!(route, ClipboardRoute::Tmux { .. }));
        assert_eq!(
            osc52_sequence("plexmaton", route)
                .unwrap_or_else(|error| panic!("encode copy: {error}")),
            b"\x1b]52;c;cGxleG1hdG9u\x1b\\"
        );
    }

    #[test]
    fn tmux_delivery_names_the_outer_clipboard_flag_and_stdin() {
        let command = tmux_copy_command();
        let command = command.as_std();

        assert_eq!(command.get_program(), "tmux");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["load-buffer", "-w", "-"]
        );
    }
}
