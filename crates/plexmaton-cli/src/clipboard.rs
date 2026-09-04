//! Where copied text goes once it leaves the workspace.
//!
//! The projection produces a [`CopyRequest`](plexmaton_tui::CopyRequest) and hands it back as a
//! value; nothing in `plexmaton-tui` knows a clipboard exists. This adapter owns the
//! local macOS and terminal boundary without using a remote host's native clipboard.

use std::{
    ffi::OsStr,
    io::{self, Write},
    process::Stdio,
    time::Duration,
};

use crossterm::{clipboard::CopyToClipboard, execute};
use tokio::{io::AsyncWriteExt as _, process::Command};

const COPY_DEADLINE: Duration = Duration::from_millis(500);

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

    const fn uses_tmux(self) -> bool {
        matches!(self, Self::Tmux { .. })
    }
}

/// One place copied text can be delivered.
///
/// The operation is async because clipboard helpers are external processes. Each remains owned and
/// bounded instead of blocking the event-loop thread or detaching a child.
pub(crate) trait ClipboardSink {
    async fn copy(&mut self, text: &str) -> io::Result<()>;
}

/// Delivers semantic source through the route belonging to the user's terminal.
pub(crate) struct TerminalClipboard<W> {
    writer: W,
    route: ClipboardRoute,
}

impl<W: Write> TerminalClipboard<W> {
    pub(crate) const fn new(writer: W, route: ClipboardRoute) -> Self {
        Self { writer, route }
    }

    pub(crate) fn from_environment(writer: W) -> Self {
        Self::new(writer, ClipboardRoute::detect())
    }

    fn write_terminal(&mut self, text: &str) -> io::Result<()> {
        self.writer.write_all(&osc52_sequence(text, self.route)?)?;
        self.writer.flush()
    }
}

impl<W: Write> ClipboardSink for TerminalClipboard<W> {
    async fn copy(&mut self, text: &str) -> io::Result<()> {
        if self.route == ClipboardRoute::LocalMacOs {
            return copy_through_command(pbcopy_command(), text, COPY_DEADLINE).await;
        }
        // Try every route before inspecting either result. A disconnected terminal must not stop
        // tmux from delivering, and a wedged tmux must not suppress the escape sequence.
        let terminal = self.write_terminal(text);
        let tmux = self.route.uses_tmux().then(|| copy_through_tmux(text));
        let tmux = match tmux {
            Some(copy) => Some(copy.await),
            None => None,
        };
        match (terminal, tmux) {
            (Ok(()), _) | (_, Some(Ok(()))) => Ok(()),
            (Err(error), None | Some(Err(_))) => Err(error),
        }
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

/// Asks tmux to retain the text and send it to the outer client's clipboard.
async fn copy_through_tmux(text: &str) -> io::Result<()> {
    copy_through_command(tmux_copy_command(), text, COPY_DEADLINE).await
}

/// The deadline covers both a blocked stdin pipe and waiting for acknowledgement.
async fn copy_through_command(
    mut command: Command,
    text: &str,
    deadline: Duration,
) -> io::Result<()> {
    let mut child = command.spawn()?;
    let operation = async {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("clipboard helper stdin was not piped"))?;
        stdin.write_all(text.as_bytes()).await?;
        stdin.shutdown().await?;
        drop(stdin);
        child.wait().await
    };
    let result = tokio::time::timeout(deadline, operation).await;
    let status = match result {
        Ok(Ok(status)) => status,
        Ok(Err(error)) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Err(error);
        }
        Err(_) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "clipboard helper exceeded its deadline",
            ));
        }
    };
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "clipboard helper exited with {status}"
        )))
    }
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
        ClipboardRoute, ClipboardSink, TerminalClipboard, configure_copy_command,
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
            copy_through_command(command, source, Duration::from_secs(2))
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
        let result = copy_through_command(command, "source", Duration::from_secs(2)).await;
        assert!(matches!(result, Err(error) if error.kind() == std::io::ErrorKind::Other));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn clipboard_deadline_bounds_a_blocked_stdin_pipe() {
        // SEL-5: a helper that never reads cannot hold the application on a full pipe.
        let mut command = Command::new("/bin/sleep");
        command.arg("30");
        configure_copy_command(&mut command);
        let result =
            copy_through_command(command, &"x".repeat(1_048_576), Duration::from_millis(50)).await;
        assert!(matches!(result, Err(error) if error.kind() == std::io::ErrorKind::TimedOut));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn clipboard_deadline_also_bounds_waiting_after_eof() {
        // SEL-5: consuming stdin does not permit an unbounded wait for acknowledgement.
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "/bin/cat > /dev/null; exec /bin/sleep 30"]);
        configure_copy_command(&mut command);
        let result = copy_through_command(command, "source", Duration::from_millis(50)).await;
        assert!(matches!(result, Err(error) if error.kind() == std::io::ErrorKind::TimedOut));
    }

    #[tokio::test]
    async fn direct_copy_writes_the_exact_terminated_osc_52_sequence() {
        let mut sink = TerminalClipboard::new(Vec::new(), ClipboardRoute::Direct);

        sink.copy("plexmaton")
            .await
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

        assert!(route.uses_tmux());
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
