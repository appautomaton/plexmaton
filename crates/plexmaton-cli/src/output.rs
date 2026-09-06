//! One physical writer for cells, native math and terminal clipboard effects.

use std::{
    cell::RefCell,
    io::{self, Write},
    rc::Rc,
    sync::Arc,
};

use crossterm::{
    event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture,
    },
    execute,
    terminal::{EndSynchronizedUpdate, EnterAlternateScreen},
};
use plexmaton_tui::math::{MathPresentation, MathUnavailable};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::clipboard::TerminalClipboard;

mod native;
pub(crate) use native::write_native;
#[cfg(test)]
mod tests;

/// Only these foreground consumers receive handles; no helper or preparation child owns one.
pub(crate) struct Writer<W>(Rc<RefCell<W>>);

impl<W> Clone for Writer<W> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<W: Write> Write for Writer<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .try_borrow_mut()
            .map_err(|_| io::Error::other("terminal output is already borrowed"))?
            .write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0
            .try_borrow_mut()
            .map_err(|_| io::Error::other("terminal output is already borrowed"))?
            .flush()
    }
}

pub(crate) struct TerminalOutput {
    pub(crate) terminal: Terminal<CrosstermBackend<Writer<io::Stdout>>>,
    pub(crate) clipboard: TerminalClipboard<Writer<io::Stdout>>,
    pub(crate) math: MathPresentation,
}

impl TerminalOutput {
    /// Startup probing finishes before EventStream exists. Crossterm's one reader retains any
    /// interleaved keyboard input for that subsequent stream; there is no parallel terminal read.
    pub(crate) fn acquire() -> io::Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        let mut writer = Writer(Rc::new(RefCell::new(io::stdout())));
        execute!(writer, EnterAlternateScreen, crossterm::cursor::Hide)?;
        let math = probe(&mut writer)?;
        execute!(
            writer,
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
            crossterm::cursor::MoveTo(0, 0),
            EnableFocusChange,
            EnableMouseCapture,
            EnableBracketedPaste
        )?;
        let terminal = Terminal::new(CrosstermBackend::new(writer.clone()))?;
        Ok(Self {
            terminal,
            clipboard: TerminalClipboard::from_environment(writer),
            math,
        })
    }
}

fn probe(writer: &mut impl Write) -> io::Result<MathPresentation> {
    if ["TMUX", "STY"]
        .into_iter()
        .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
    {
        return Ok(MathPresentation::Source(MathUnavailable::Multiplexer));
    }
    let (columns, rows) = crossterm::terminal::size()?;
    if columns < 8 || rows < 5 {
        return Ok(MathPresentation::default());
    }
    execute!(writer, crossterm::cursor::MoveTo(2, 2))?;
    let Ok(first) = crossterm::cursor::position() else {
        return Ok(MathPresentation::default());
    };
    writer.write_all(b"\x1b]66;w=2; \x07")?;
    writer.flush()?;
    let Ok(second) = crossterm::cursor::position() else {
        return Ok(MathPresentation::default());
    };
    writer.write_all(b"\x1b]66;s=2; \x07")?;
    writer.flush()?;
    let Ok(third) = crossterm::cursor::position() else {
        return Ok(MathPresentation::default());
    };
    Ok(classify([first, second, third]))
}

fn classify(positions: [(u16, u16); 3]) -> MathPresentation {
    match positions {
        [(2, 2), (4, 2), (6, 2)] => MathPresentation::Native,
        [(2, 2), (4, 2), (5, 2)] | [(2, 2), (2, 2), (2, 2)] => {
            MathPresentation::Source(MathUnavailable::Unsupported)
        }
        _ => MathPresentation::default(),
    }
}

type PanicHook = dyn Fn(&std::panic::PanicHookInfo<'_>) + Send + Sync + 'static;

/// Owns the process-local restoration hook and reporting modes from acquisition through exit.
pub(crate) struct RestoreTerminal {
    previous: Arc<PanicHook>,
}

impl RestoreTerminal {
    pub(crate) fn new() -> Self {
        let previous: Arc<PanicHook> = std::panic::take_hook().into();
        let hook = previous.clone();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            hook(info);
        }));
        Self { previous }
    }
}

impl Drop for RestoreTerminal {
    fn drop(&mut self) {
        restore();
        if !std::thread::panicking() {
            let previous = self.previous.clone();
            std::panic::set_hook(Box::new(move |info| previous(info)));
        }
    }
}

fn restore() {
    // Best effort on error and panic: release synchronized output and input modes before leaving
    // the alternate screen. No diagnostics are written until restoration has been attempted.
    let _ = execute!(
        io::stdout(),
        EndSynchronizedUpdate,
        crossterm::style::SetAttribute(crossterm::style::Attribute::Reset),
        DisableBracketedPaste,
        DisableFocusChange,
        DisableMouseCapture
    );
    let _ = ratatui::try_restore();
}
