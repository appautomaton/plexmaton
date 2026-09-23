//! The launch greeting in the terminal it is run in, for review (phase 04 stage 10).
//! cargo run -p plexmaton-tui --example mark_greeting            plays it; Enter replays, q quits
//! cargo run -p plexmaton-tui --example mark_greeting -- 0 12 30  prints those phases as text
//!
//! Drawn with the product's own geometry, timeline and colours, so what is reviewed is what ships.

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use plexmaton_tui::{
    Palette,
    mark::{CellSize, greeting, lines, size},
};
use ratatui::{
    crossterm::terminal::{disable_raw_mode, enable_raw_mode, window_size},
    style::{Color, Style},
    text::Line,
};

fn cell() -> Option<CellSize> {
    window_size().ok().and_then(|window| {
        (window.columns > 0 && window.rows > 0 && window.width > 0).then(|| CellSize {
            width: window.width / window.columns,
            height: window.height / window.rows,
        })
    })
}

fn ansi(style: Style) -> String {
    match style.fg {
        Some(Color::Rgb(r, g, b)) => format!("\x1b[38;2;{r};{g};{b}m"),
        _ => String::new(),
    }
}

fn paint(line: &Line<'_>, colour: bool) -> String {
    line.spans
        .iter()
        .map(|span| {
            if colour {
                format!("{}{}\x1b[0m", ansi(span.style), span.content)
            } else {
                span.content.to_string()
            }
        })
        .collect()
}

fn main() -> std::io::Result<()> {
    let palette = Palette::pastel();
    let mut out = std::io::stdout();
    let phases: Vec<u16> = std::env::args()
        .skip(1)
        .filter_map(|arg| arg.parse().ok())
        .collect();
    if !phases.is_empty() {
        let block = size(
            7,
            Some(CellSize {
                width: 9,
                height: 20,
            }),
        );
        for phase in phases {
            writeln!(out, "-- phase {phase}")?;
            match greeting(phase) {
                Some(moment) => {
                    for line in lines(
                        block,
                        Some(CellSize {
                            width: 9,
                            height: 20,
                        }),
                        moment,
                        &palette,
                    ) {
                        writeln!(out, "   {}", paint(&line, false))?;
                    }
                }
                None => writeln!(out, "   gone")?,
            }
        }
        return Ok(());
    }
    let cell = cell();
    let block = size(7, cell);
    enable_raw_mode()?;
    write!(out, "\x1b[?1049h\x1b[?25l")?;
    let mut input = std::io::stdin();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut byte = [0u8; 1];
        while input.read(&mut byte).is_ok_and(|read| read == 1) {
            if tx.send(byte[0]).is_err() {
                break;
            }
        }
    });
    let mut start = Instant::now();
    loop {
        let phase = u16::try_from(start.elapsed().as_millis() * 15 / 1000).unwrap_or(u16::MAX);
        write!(out, "\x1b[H\x1b[2J")?;
        if let Some(moment) = greeting(phase) {
            for (row, line) in lines(block, cell, moment, &palette).iter().enumerate() {
                write!(out, "\x1b[{};4H{}", row + 3, paint(line, true))?;
            }
            let name = palette.mark_name_at(moment.level);
            let column = 4 + usize::from(block.0).saturating_sub(9) / 2;
            write!(
                out,
                "\x1b[{};{column}H{}Plexmaton\x1b[0m",
                usize::from(block.1) + 4,
                ansi(name)
            )?;
        }
        write!(
            out,
            "\x1b[{};4H\x1b[38;2;142;162;196mEnter replays · q quits\x1b[0m",
            usize::from(block.1) + 7
        )?;
        out.flush()?;
        match rx.recv_timeout(Duration::from_millis(33)) {
            Ok(b'q') => break,
            Ok(b'\r' | b'\n') => start = Instant::now(),
            _ => {}
        }
    }
    write!(out, "\x1b[?25h\x1b[?1049l")?;
    out.flush()?;
    disable_raw_mode()
}
