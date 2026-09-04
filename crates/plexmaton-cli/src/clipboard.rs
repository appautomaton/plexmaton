//! Where copied text goes once it leaves the workspace.
//!
//! The projection produces a [`CopyRequest`](plexmaton_tui::CopyRequest) and hands it back as a
//! value; nothing in `plexmaton-tui` knows a clipboard exists. This adapter owns the terminal and
//! tmux boundary without turning a remote host's native clipboard into the user's clipboard.

use std::{
    ffi::OsStr,
    io::{self, Write},
    process::Stdio,
    time::Duration,
};

use crossterm::{clipboard::CopyToClipboard, execute};
use tokio::{io::AsyncWriteExt as _, process::Command};

const TMUX_COPY_DEADLINE: Duration = Duration::from_millis(500);

/// How the process reaches the terminal that owns the user's clipboard.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClipboardRoute {
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
        Self::from_environment(std::env::var_os("TMUX").as_deref(), embedded_editor)
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
/// The operation is async because tmux is an external process. It remains directly owned and
/// bounded instead of blocking the event-loop thread or detaching a child.
pub(crate) trait ClipboardSink {
    async fn copy(&mut self, text: &str) -> io::Result<()>;
}

/// Sends semantic source to the terminal, with a second acknowledged tmux leg when applicable.
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
        ClipboardRoute::Direct
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
    let mut command = tmux_copy_command();
    let mut child = command.spawn()?;
    let operation = async {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("tmux clipboard stdin was not piped"))?;
        stdin.write_all(text.as_bytes()).await?;
        stdin.shutdown().await?;
        drop(stdin);
        child.wait().await
    };
    let result = tokio::time::timeout(TMUX_COPY_DEADLINE, operation).await;
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
                "tmux clipboard copy exceeded 500 ms",
            ));
        }
    };
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "tmux clipboard command exited with {status}"
        )))
    }
}

fn tmux_copy_command() -> Command {
    let mut command = Command::new("tmux");
    command
        .args(["load-buffer", "-w", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    command
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::{
        ClipboardRoute, ClipboardSink, TerminalClipboard, osc52_sequence, tmux_copy_command,
    };

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
