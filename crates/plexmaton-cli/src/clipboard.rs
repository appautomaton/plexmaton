//! Where copied text goes once it leaves the workspace.
//!
//! The projection produces a [`CopyRequest`](plexmaton_tui::CopyRequest) and hands it back as a
//! value; nothing in `plexmaton-tui` knows a clipboard exists. This is the other side of that seam,
//! and it lives in the composition root for the same reason the terminal does.
//!
//! The transport is OSC 52 — an escape sequence the terminal itself acts on — rather than a native
//! clipboard crate. That is the deliberate choice: a native clipboard reaches the desktop the
//! *process* is on, which over SSH or inside tmux is the wrong machine, and the phase file already
//! records that a local clipboard cannot be the only path. OSC 52 reaches the terminal the *user* is
//! at, which is the one holding their clipboard.
//!
//! Its cost is that it is unacknowledged. The terminal never replies, and many terminals and
//! multiplexers decline OSC 52 unless configured to allow it. So a successful write here means the
//! sequence was sent, and nothing stronger. That is why the workspace's own feedback is the
//! selection staying visible rather than a message claiming the copy landed (SEL-5).

use std::io::{self, Write};

use crossterm::{clipboard::CopyToClipboard, execute};

/// One place copied text can be delivered.
///
/// A trait with one production implementation, because the alternative is a `#[cfg(test)]` branch
/// inside the composition root, and a code path the binary never runs is not evidence.
pub trait ClipboardSink {
    /// Offers `text` to the user's clipboard.
    ///
    /// An `Ok` means the request was delivered to the terminal, not that the terminal accepted it.
    fn copy(&mut self, text: &str) -> io::Result<()>;
}

/// Sends OSC 52 to whatever terminal is on the other end of a writer.
pub struct TerminalClipboard<W> {
    writer: W,
}

impl<W: Write> TerminalClipboard<W> {
    /// Wraps the stream the terminal is reading, normally the process's own stdout.
    pub const fn new(writer: W) -> Self {
        Self { writer }
    }
}

impl<W: Write> ClipboardSink for TerminalClipboard<W> {
    fn copy(&mut self, text: &str) -> io::Result<()> {
        // Queued and flushed by `execute!`, so the sequence is not left sitting in a buffer until
        // the next frame happens to push it out.
        execute!(self.writer, CopyToClipboard::to_clipboard_from(text))
    }
}

#[cfg(test)]
mod tests {
    use super::{ClipboardSink, TerminalClipboard};

    /// The adapter's whole job is the bytes it emits, and those are checkable without a terminal.
    ///
    /// Pinned literally rather than by re-encoding: computing the expectation the same way the code
    /// does would pass whatever the code produced, including an unterminated sequence or the wrong
    /// selection character, and the whole point of a wire format is that the other side agrees.
    #[test]
    fn copying_writes_a_terminated_osc_52_sequence_carrying_the_encoded_text() {
        let mut sink = TerminalClipboard::new(Vec::new());

        sink.copy("plexmaton")
            .unwrap_or_else(|error| panic!("writing to a vector cannot fail: {error}"));

        let written = String::from_utf8(sink.writer)
            .unwrap_or_else(|error| panic!("the sequence must be text: {error}"));
        // Terminated by String Terminator, which is what Crossterm emits; the BEL form is the
        // older alternative and is not what this project sends.
        assert_eq!(written, "\u{1b}]52;c;cGxleG1hdG9u\u{1b}\\");
    }

    /// Multi-byte text is where a naive encoder truncates, so it is worth its own case.
    #[test]
    fn multi_byte_text_survives_the_encoding() {
        let mut sink = TerminalClipboard::new(Vec::new());

        sink.copy("δ 汉字")
            .unwrap_or_else(|error| panic!("writing to a vector cannot fail: {error}"));

        let written = String::from_utf8(sink.writer)
            .unwrap_or_else(|error| panic!("the sequence must be text: {error}"));
        assert_eq!(written, "\u{1b}]52;c;zrQg5rGJ5a2X\u{1b}\\");
    }
}
