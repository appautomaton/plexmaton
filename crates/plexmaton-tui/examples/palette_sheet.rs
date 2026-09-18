//! Export the shipped palette as data, so a colourway can be read rather than transcribed.
use plexmaton_tui::{Palette, Role};
use std::io::{self, Write as _};

fn hex(style: ratatui::style::Style) -> String {
    match style.fg {
        Some(ratatui::style::Color::Rgb(red, green, blue)) => {
            format!("#{red:02x}{green:02x}{blue:02x}")
        }
        Some(other) => format!("{other:?}"),
        None => "-".into(),
    }
}

fn main() -> io::Result<()> {
    let mut out = io::BufWriter::new(io::stdout().lock());
    let palette = Palette::pastel();
    for role in Role::ALL {
        let style = palette.style(role);
        let mut notes = Vec::new();
        if style.add_modifier.contains(ratatui::style::Modifier::BOLD) {
            notes.push("bold");
        }
        if style
            .add_modifier
            .contains(ratatui::style::Modifier::ITALIC)
        {
            notes.push("italic");
        }
        if style
            .add_modifier
            .contains(ratatui::style::Modifier::REVERSED)
        {
            notes.push("reversed");
        }
        let background = match style.bg {
            Some(ratatui::style::Color::Rgb(red, green, blue)) => {
                format!("#{red:02x}{green:02x}{blue:02x}")
            }
            _ => "-".into(),
        };
        writeln!(
            out,
            "role\t{role:?}\t{}\t{background}\t{}",
            hex(style),
            notes.join(" ")
        )?;
    }
    for hue in [
        Role::SurfaceRoster,
        Role::SurfacePrimary,
        Role::SurfaceDelegate,
    ] {
        writeln!(
            out,
            "surface\t{hue:?}\t{}\t{}\t",
            hex(palette.surface_border(Some(hue), true)),
            hex(palette.surface_border(Some(hue), false))
        )?;
    }
    writeln!(
        out,
        "surface\tNone\t{}\t{}\t",
        hex(palette.surface_border(None, true)),
        hex(palette.surface_border(None, false))
    )?;
    out.flush()
}
