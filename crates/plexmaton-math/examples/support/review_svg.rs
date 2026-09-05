//! Pixel review of native reservations; this export is never a formula transport.
use crate::reply::{Document, Page};
use plexmaton_math::{FontStyle, Paint, TextScale, VerticalAlign};
use std::fmt::Write as _;
use unicode_width::UnicodeWidthStr as _;

fn escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn svg(document: &Document, page: &Page) -> String {
    let width = u32::from(document.width) * 10;
    // A square canvas also avoids Quick Look cropping non-square SVG thumbnails on macOS.
    let side = width.max(880);
    let mut svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{side}" height="{side}" viewBox="0 0 {side} {side}"><rect width="100%" height="100%" fill="#0c0e15"/><rect width="{width}" height="880" fill="#11131c"/><g font-family="Menlo,monospace" font-size="16" fill="#dce1ea">"##
    );
    writeln!(
        svg,
        r##"<text x="20" y="24" fill="#9cbcff">Plexmaton / native math / {} columns</text>"##,
        document.width
    )
    .expect("String write");
    for run in &document.runs {
        if run.y < page.start || run.y + run.rows > page.end {
            continue;
        }
        let scale: f64 = match run.scale {
            TextScale::Full => 1.0,
            TextScale::Script => 0.7,
            TextScale::ScriptScript => 0.5,
            TextScale::Large => 2.0,
        };
        let height = f64::from(run.rows) * 22.0;
        let extra = height - 22.0 * scale;
        let align = match run.align {
            VerticalAlign::Top => 0.0,
            VerticalAlign::Bottom => extra,
            VerticalAlign::Center => extra / 2.0,
        };
        let x = (u32::from(run.x) + 2) * 10;
        let y = f64::from(run.y - page.start + 2) * 22.0 + align + 17.0 * scale;
        let italic = if matches!(run.style, FontStyle::Italic | FontStyle::BoldItalic) {
            "italic"
        } else {
            "normal"
        };
        let weight = if matches!(run.style, FontStyle::Bold | FontStyle::BoldItalic) {
            "bold"
        } else {
            "normal"
        };
        let paint = match run.paint {
            Paint::Inherit => "#dce1ea".into(),
            Paint::Rgb { red, green, blue } => format!("#{red:02x}{green:02x}{blue:02x}"),
        };
        let advance = run.text.width() as f64 * 10.0 * scale;
        writeln!(svg, r#"<text x="{x}" y="{y:.2}" font-size="{}" font-style="{italic}" font-weight="{weight}" fill="{paint}" textLength="{advance:.2}" lengthAdjust="spacingAndGlyphs">{}</text>"#, scale * 16.0, escaped(&run.text)).expect("String write");
    }
    writeln!(svg, r##"<text x="20" y="863" fill="#9099ad">RaTeX → native runs / review only / retained rows {}–{}</text></g></svg>"##, page.start, page.end).expect("String write");
    svg
}
