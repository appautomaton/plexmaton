//! Cell-buffer projection for visual review. ANSI slots use an explicitly modeled dark terminal.
use ratatui::{
    buffer::Buffer,
    style::{Color, Modifier},
};
use std::fmt::Write as _;
use unicode_width::UnicodeWidthStr as _;

const BACKGROUND: &str = "#11131c";
const FOREGROUND: &str = "#dce1ea";

fn color(value: Color, fallback: &str) -> String {
    match value {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Black => "#171922".into(),
        Color::Red => "#d97880".into(),
        Color::Green => "#99bd87".into(),
        Color::Yellow => "#d8bc83".into(),
        Color::Blue => "#829fc4".into(),
        Color::Magenta => "#b494c7".into(),
        Color::Cyan => "#87b9bc".into(),
        Color::Gray => "#c0c5cf".into(),
        Color::DarkGray => "#737c90".into(),
        Color::LightRed => "#eda1a7".into(),
        Color::LightGreen => "#b2d6a0".into(),
        Color::LightYellow => "#ebd49d".into(),
        Color::LightBlue => "#a0bfe6".into(),
        Color::LightMagenta => "#cdb0df".into(),
        Color::LightCyan => "#a1d4d5".into(),
        Color::White => FOREGROUND.into(),
        Color::Reset => fallback.into(),
        Color::Indexed(index) => panic!("preview fixture uses an unmodeled indexed color {index}"),
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn svg(buffer: &Buffer) -> String {
    let width = buffer.area.width * 9;
    let height = buffer.area.height * 20;
    let mut out = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="100%" height="100%" viewBox="0 0 {width} {height}" preserveAspectRatio="xMinYMin meet"><rect width="100%" height="100%" fill="{BACKGROUND}"/><g font-family="Menlo,'Agave Nerd Font Mono',monospace" font-size="14">"##
    );
    for y in 0..buffer.area.height {
        let mut x = 0;
        while x < buffer.area.width {
            let cell = &buffer[(x, y)];
            let mut foreground = color(cell.fg, FOREGROUND);
            let mut background = color(cell.bg, BACKGROUND);
            if cell.modifier.contains(Modifier::REVERSED) {
                std::mem::swap(&mut foreground, &mut background);
            }
            let start = x;
            let mut text = String::new();
            while x < buffer.area.width && buffer[(x, y)].style() == cell.style() {
                let symbol = buffer[(x, y)].symbol();
                if symbol == "\u{e0b0}" {
                    break;
                }
                text.push_str(symbol);
                x += symbol.width().max(1) as u16;
            }
            if x == start {
                x += 1;
            }
            let cells = x - start;
            if background != BACKGROUND {
                let _ = write!(
                    out,
                    r#"<rect x="{}" y="{}" width="{}" height="20" fill="{background}"/>"#,
                    start * 9,
                    y * 20,
                    cells * 9
                );
            }
            if text.is_empty() {
                let _ = write!(
                    out,
                    r#"<polygon points="{},{} {},{} {},{}" fill="{foreground}"/>"#,
                    start * 9,
                    y * 20,
                    x * 9,
                    y * 20 + 10,
                    start * 9,
                    y * 20 + 20
                );
            } else if !text.trim().is_empty() {
                let bold = if cell.modifier.contains(Modifier::BOLD) {
                    "bold"
                } else {
                    "normal"
                };
                let italic = if cell.modifier.contains(Modifier::ITALIC) {
                    "italic"
                } else {
                    "normal"
                };
                let opacity = if cell.modifier.contains(Modifier::DIM) {
                    "0.65"
                } else {
                    "1"
                };
                let decoration = match (
                    cell.modifier.contains(Modifier::UNDERLINED),
                    cell.modifier.contains(Modifier::CROSSED_OUT),
                ) {
                    (true, true) => "underline line-through",
                    (true, false) => "underline",
                    (false, true) => "line-through",
                    (false, false) => "none",
                };
                let _ = write!(
                    out,
                    r#"<text x="{}" y="{}" fill="{foreground}" font-weight="{bold}" font-style="{italic}" opacity="{opacity}" text-decoration="{decoration}" xml:space="preserve" textLength="{}" lengthAdjust="spacingAndGlyphs">{}</text>"#,
                    start * 9,
                    y * 20 + 15,
                    cells * 9,
                    escape(&text)
                );
            }
        }
    }
    out.push_str("</g></svg>");
    out
}

#[test]
fn review_export_preserves_modifiers_wide_cells_and_inert_text() {
    use ratatui::{layout::Rect, style::Style};
    let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 1));
    buffer.set_string(
        0,
        0,
        "中<&",
        Style::new()
            .fg(Color::Rgb(1, 2, 3))
            .add_modifier(Modifier::BOLD | Modifier::ITALIC | Modifier::UNDERLINED),
    );
    let output = svg(&buffer);
    assert!(output.contains("中&lt;&amp;"));
    assert!(output.contains("#010203"));
    for modifier in [
        "font-weight=\"bold\"",
        "font-style=\"italic\"",
        "text-decoration=\"underline\"",
    ] {
        assert!(output.contains(modifier), "{modifier}");
    }
}
