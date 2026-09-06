//! Encode admitted native text only; no source parsing and no independent terminal writer.

use std::io::{self, Write};

use crossterm::{
    cursor::{MoveTo, RestorePosition, SavePosition},
    queue,
    style::{Attribute, Attributes, SetAttribute, SetAttributes},
    terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate},
};
use plexmaton_tui::math::{
    FontStyle, MathPaint, NativeStage, NativeText, TextScale, VerticalAlign,
};
use ratatui::style::{Color, Modifier};

const MAX_BYTES: usize = 256 * 1024;

pub(crate) fn write_native(writer: &mut impl Write, stage: NativeStage<'_>) -> io::Result<()> {
    match stage {
        NativeStage::Begin => queue!(writer, BeginSynchronizedUpdate),
        NativeStage::End { changed: text, .. } => {
            let bytes = encode(text)?;
            writer.write_all(&bytes)?;
            writer.flush()
        }
    }
}

fn encode(text: &[&NativeText]) -> io::Result<Vec<u8>> {
    if text.len() > 512 {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    queue!(bytes, SavePosition)?;
    for text in text {
        let glyph = &text.glyph;
        if glyph.text.is_empty()
            || glyph.text.chars().any(char::is_control)
            || glyph.columns == 0
            || glyph.x.checked_add(glyph.columns).is_none()
            || glyph.y.checked_add(glyph.rows).is_none()
            || glyph.text.len() > 4096
            || bytes.len() + glyph.text.len() + 256 > MAX_BYTES
        {
            return Err(invalid());
        }
        let foreground = match glyph.paint {
            MathPaint::Inherit => text.style.fg.unwrap_or(Color::Reset),
            MathPaint::Rgb { red, green, blue } => Color::Rgb(red, green, blue),
        };
        queue!(
            bytes,
            MoveTo(glyph.x, glyph.y),
            SetAttribute(Attribute::Reset)
        )?;
        color(&mut bytes, Plane::Foreground, foreground)?;
        color(
            &mut bytes,
            Plane::Background,
            text.style.bg.unwrap_or(Color::Reset),
        )?;
        color(
            &mut bytes,
            Plane::Underline,
            text.style.underline_color.unwrap_or(Color::Reset),
        )?;
        queue!(bytes, SetAttributes(attributes(text)))?;
        match glyph.scale {
            TextScale::Full if glyph.rows == 1 => bytes.extend_from_slice(glyph.text.as_bytes()),
            TextScale::Script | TextScale::ScriptScript
                if glyph.rows == 1 && glyph.columns <= 7 =>
            {
                let (n, d) = if glyph.scale == TextScale::Script {
                    (7, 10)
                } else {
                    (1, 2)
                };
                let alignment = match glyph.align {
                    VerticalAlign::Top => 0,
                    VerticalAlign::Bottom => 1,
                    VerticalAlign::Center => 2,
                };
                write!(
                    bytes,
                    "\x1b]66;s=1:n={n}:d={d}:v={alignment}:w={};{}\x07",
                    glyph.columns, glyph.text
                )?;
            }
            TextScale::Large
                if glyph.rows == 2 && glyph.columns <= 14 && glyph.columns.is_multiple_of(2) =>
            {
                write!(
                    bytes,
                    "\x1b]66;s=2:w={};{}\x07",
                    glyph.columns / 2,
                    glyph.text
                )?;
            }
            _ => return Err(invalid()),
        }
    }
    queue!(
        bytes,
        SetAttribute(Attribute::Reset),
        RestorePosition,
        EndSynchronizedUpdate
    )?;
    Ok(bytes)
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "native text exceeds admitted terminal output",
    )
}

fn attributes(text: &NativeText) -> Attributes {
    let mut modifiers = text.style.add_modifier;
    match text.glyph.style {
        FontStyle::Roman => {}
        FontStyle::Italic => modifiers |= Modifier::ITALIC,
        FontStyle::Bold => modifiers |= Modifier::BOLD,
        FontStyle::BoldItalic => modifiers |= Modifier::BOLD | Modifier::ITALIC,
    }
    let mut attributes = Attributes::default();
    for (modifier, attribute) in [
        (Modifier::BOLD, Attribute::Bold),
        (Modifier::DIM, Attribute::Dim),
        (Modifier::ITALIC, Attribute::Italic),
        (Modifier::UNDERLINED, Attribute::Underlined),
        (Modifier::REVERSED, Attribute::Reverse),
        (Modifier::CROSSED_OUT, Attribute::CrossedOut),
        (Modifier::SLOW_BLINK, Attribute::SlowBlink),
        (Modifier::RAPID_BLINK, Attribute::RapidBlink),
        (Modifier::HIDDEN, Attribute::Hidden),
    ] {
        if modifiers.contains(modifier) {
            attributes.set(attribute);
        }
    }
    attributes
}

enum Plane {
    Foreground,
    Background,
    Underline,
}

fn color(bytes: &mut impl Write, plane: Plane, color: Color) -> io::Result<()> {
    // Resolve the supplied palette and explicit math paint, never Crossterm's mutable global
    // color suppression. A requested RGB value cannot silently become inherited/reset paint.
    let code = match plane {
        Plane::Foreground => 38,
        Plane::Background => 48,
        Plane::Underline => 58,
    };
    let index = match color {
        Color::Reset => return write!(bytes, "\x1b[{}m", code + 1),
        Color::Rgb(r, g, b) => return write!(bytes, "\x1b[{code};2;{r};{g};{b}m"),
        Color::Black => 0,
        Color::Red => 1,
        Color::Green => 2,
        Color::Yellow => 3,
        Color::Blue => 4,
        Color::Magenta => 5,
        Color::Cyan => 6,
        Color::Gray => 7,
        Color::DarkGray => 8,
        Color::LightRed => 9,
        Color::LightGreen => 10,
        Color::LightYellow => 11,
        Color::LightBlue => 12,
        Color::LightMagenta => 13,
        Color::LightCyan => 14,
        Color::White => 15,
        Color::Indexed(index) => index,
    };
    write!(bytes, "\x1b[{code};5;{index}m")
}
