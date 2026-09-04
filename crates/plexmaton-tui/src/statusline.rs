//! The narrow styled-text boundary for user status commands; never a terminal emulator.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

#[cfg(test)]
mod tests;

const MAX_BYTES: usize = 16 * 1024;
const MAX_ROWS: usize = 64;
const MAX_SGR_BYTES: usize = 128;
const MAX_PARAMETERS: usize = 32;

/// Fully validated script output. Geometry and process ownership live outside this value.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StatusLineText {
    lines: Vec<Line<'static>>,
}

/// Output is rejected as a whole; diagnostics never echo script content or escape sequences.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum StatusLineTextError {
    #[error("status-line output exceeds its byte limit")]
    TooManyBytes,
    #[error("status-line output is not UTF-8")]
    InvalidUtf8,
    #[error("status-line output exceeds its row limit")]
    TooManyRows,
    #[error("status-line output contains an unsupported control")]
    UnsupportedControl,
    #[error("status-line output contains a malformed or unsupported style")]
    InvalidStyle,
}

impl StatusLineText {
    /// Decode one complete stdout result under STL-1. One trailing line ending terminates the
    /// last row; interior empty rows and leading/trailing spaces are intentional presentation.
    pub fn parse(output: &[u8]) -> Result<Self, StatusLineTextError> {
        if output.len() > MAX_BYTES {
            return Err(StatusLineTextError::TooManyBytes);
        }
        let source = std::str::from_utf8(output).map_err(|_| StatusLineTextError::InvalidUtf8)?;
        let mut lines = Vec::new();
        let mut style = Style::default();
        for row in source.split_inclusive('\n') {
            if lines.len() == MAX_ROWS {
                return Err(StatusLineTextError::TooManyRows);
            }
            let row = match row.strip_suffix('\n') {
                Some(row) => row.strip_suffix('\r').unwrap_or(row),
                None => row,
            };
            lines.push(parse_row(row, &mut style)?);
        }
        Ok(Self { lines })
    }

    /// Styled rows with no terminal controls. The renderer chooses a cell-correct viewport.
    #[must_use]
    pub fn lines(&self) -> &[Line<'static>] {
        &self.lines
    }
}

fn parse_row(mut source: &str, style: &mut Style) -> Result<Line<'static>, StatusLineTextError> {
    let mut spans = Vec::new();
    while !source.is_empty() {
        let end = source.find('\x1b').unwrap_or(source.len());
        let text = &source[..end];
        if text.chars().any(char::is_control) {
            return Err(StatusLineTextError::UnsupportedControl);
        }
        if !text.is_empty() {
            spans.push(Span::styled(text.to_owned(), *style));
        }
        source = &source[end..];
        if source.is_empty() {
            break;
        }
        source = source
            .strip_prefix("\x1b[")
            .ok_or(StatusLineTextError::UnsupportedControl)?;
        let end = source
            .bytes()
            .take(MAX_SGR_BYTES)
            .position(|byte| byte == b'm')
            .ok_or(StatusLineTextError::InvalidStyle)?;
        apply_sgr(&source[..end], style)?;
        source = &source[end + 1..];
    }
    Ok(Line::from(spans))
}

fn apply_sgr(source: &str, style: &mut Style) -> Result<(), StatusLineTextError> {
    let mut parameters = Vec::new();
    for value in source.split(';') {
        if parameters.len() == MAX_PARAMETERS || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(StatusLineTextError::InvalidStyle);
        }
        parameters.push(if value.is_empty() {
            0
        } else {
            value
                .parse::<u16>()
                .map_err(|_| StatusLineTextError::InvalidStyle)?
        });
    }
    let mut parameters = parameters.into_iter();
    while let Some(parameter) = parameters.next() {
        *style = match parameter {
            0 => Style::default(),
            1 => style.add_modifier(Modifier::BOLD),
            2 => style.add_modifier(Modifier::DIM),
            3 => style.add_modifier(Modifier::ITALIC),
            4 => style.add_modifier(Modifier::UNDERLINED),
            7 => style.add_modifier(Modifier::REVERSED),
            9 => style.add_modifier(Modifier::CROSSED_OUT),
            22 => style.remove_modifier(Modifier::BOLD | Modifier::DIM),
            23 => style.remove_modifier(Modifier::ITALIC),
            24 => style.remove_modifier(Modifier::UNDERLINED),
            27 => style.remove_modifier(Modifier::REVERSED),
            29 => style.remove_modifier(Modifier::CROSSED_OUT),
            30..=37 => style.fg(ansi_color(parameter - 30)),
            40..=47 => style.bg(ansi_color(parameter - 40)),
            90..=97 => style.fg(ansi_color(parameter - 90 + 8)),
            100..=107 => style.bg(ansi_color(parameter - 100 + 8)),
            38 => style.fg(extended_color(&mut parameters)?),
            48 => style.bg(extended_color(&mut parameters)?),
            39 => Style { fg: None, ..*style },
            49 => Style { bg: None, ..*style },
            _ => return Err(StatusLineTextError::InvalidStyle),
        };
    }
    Ok(())
}

fn extended_color(
    parameters: &mut impl Iterator<Item = u16>,
) -> Result<Color, StatusLineTextError> {
    let mode = parameters.next();
    let mut channel = || {
        parameters
            .next()
            .and_then(|n| u8::try_from(n).ok())
            .ok_or(StatusLineTextError::InvalidStyle)
    };
    match mode {
        Some(5) => Ok(Color::Indexed(channel()?)),
        Some(2) => Ok(Color::Rgb(channel()?, channel()?, channel()?)),
        _ => Err(StatusLineTextError::InvalidStyle),
    }
}

fn ansi_color(index: u16) -> Color {
    const COLORS: [Color; 16] = [
        Color::Black,
        Color::Red,
        Color::Green,
        Color::Yellow,
        Color::Blue,
        Color::Magenta,
        Color::Cyan,
        Color::Gray,
        Color::DarkGray,
        Color::LightRed,
        Color::LightGreen,
        Color::LightYellow,
        Color::LightBlue,
        Color::LightMagenta,
        Color::LightCyan,
        Color::White,
    ];
    COLORS[usize::from(index)]
}
