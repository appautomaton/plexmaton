//! External parser/layout boundary. Admission normalizes paint but never owns TeX geometry.
use ratex_layout::{LayoutOptions, layout, to_display_list};
use ratex_parser::ParseNode;
use ratex_types::{Color, MathStyle};

use crate::{
    FontStyle, Limit, MAX_ITEMS, MAX_NODES, MathError, MathMode, Paint, TextScale, Unsupported,
};

mod decode;

// Transparent colors are refused at admission, reserving this marker for inherited paint.
const INHERIT: Color = Color::new(0.0, 0.0, 0.0, 0.0);
const MAX_EM: f64 = 512.0;

pub(super) struct Scene {
    pub width: f64,
    pub height: f64,
    pub axis: f64,
    pub items: Vec<Item>,
}

#[derive(Clone)]
pub(super) struct Item {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub top: f64,
    pub bottom: f64,
    pub kind: Kind,
    pub paint: Paint,
}

#[derive(Clone)]
pub(super) enum Kind {
    Glyph {
        text: String,
        style: FontStyle,
        scale: TextScale,
        baseline: f64,
    },
    Horizontal,
    Vertical,
    Delimiter(char),
    Radical,
}

pub(super) fn prepare(source: &str, mode: MathMode) -> Result<Scene, MathError> {
    let mut tree = ratex_parser::parse(source).map_err(|_| MathError::ParseRejected)?;
    admit(&mut tree)?;
    let options = LayoutOptions {
        style: match mode {
            MathMode::Inline => MathStyle::Text,
            MathMode::Display => MathStyle::Display,
        },
        color: INHERIT,
        ..LayoutOptions::default()
    };
    let boxes = layout(&tree, &options);
    let list = to_display_list(&boxes);
    if list.items.len() > MAX_ITEMS {
        return Err(MathError::Limited(Limit::Items));
    }
    for value in [list.width, list.height, list.depth, list.total_height()] {
        geometry(value)?;
    }
    if list.width < 0.0 || list.total_height() <= 0.0 {
        return Err(MathError::Empty);
    }
    decode::scene(&list, options.metrics().axis_height)
}

fn geometry(value: f64) -> Result<f64, MathError> {
    if value.is_finite() && value.abs() <= MAX_EM {
        Ok(value)
    } else {
        Err(MathError::Limited(Limit::Geometry))
    }
}

fn paint(color: Color) -> Result<Paint, MathError> {
    if color == INHERIT {
        return Ok(Paint::Inherit);
    }
    if color.a != 1.0
        || [color.r, color.g, color.b]
            .iter()
            .any(|c| !c.is_finite() || !(0.0..=1.0).contains(c))
    {
        return Err(MathError::Unsupported(Unsupported::Paint));
    }
    Ok(Paint::Rgb {
        red: (color.r * 255.0).round() as u8,
        green: (color.g * 255.0).round() as u8,
        blue: (color.b * 255.0).round() as u8,
    })
}

fn source_color(value: &str) -> Result<Color, MathError> {
    let color = Color::parse(value).ok_or(MathError::Unsupported(Unsupported::Paint))?;
    if color == INHERIT {
        return Err(MathError::Unsupported(Unsupported::Paint));
    }
    paint(color).map(|_| color)
}

fn admit(nodes: &mut [ParseNode]) -> Result<(), MathError> {
    if nodes.len() > MAX_NODES {
        return Err(MathError::Limited(Limit::Nodes));
    }
    let mut pending: Vec<_> = nodes
        .iter_mut()
        .map(|node| (node, 0_usize, INHERIT, false))
        .collect();
    let mut count = 0_usize;
    while let Some((node, depth, inherited, explicit_font)) = pending.pop() {
        count += 1;
        // The upstream parser separately limits logical input nesting to 32.
        if depth > 64 {
            return Err(MathError::Limited(Limit::Depth));
        }
        let mut push = |node, color, explicit_font| {
            if count + pending.len() == MAX_NODES {
                return Err(MathError::Limited(Limit::Nodes));
            }
            pending.push((node, depth + 1, color, explicit_font));
            Ok(())
        };
        match node {
            ParseNode::Atom { text, mode, .. }
            | ParseNode::MathOrd { text, mode, .. }
            | ParseNode::TextOrd { text, mode, .. } => {
                if resolved_symbol_is_accent_marker(text, *mode) {
                    return Err(MathError::Unsupported(Unsupported::Construct));
                }
                if explicit_font && text.chars().any(is_supported_cjk) {
                    return Err(MathError::Unsupported(Unsupported::Font));
                }
            }
            ParseNode::OpToken { .. }
            | ParseNode::AccentToken { .. }
            | ParseNode::SpacingNode { .. }
            | ParseNode::Kern { .. }
            | ParseNode::DelimSizing { .. }
            | ParseNode::Middle { .. }
            | ParseNode::Internal { .. } => {}
            ParseNode::OrdGroup { body, .. }
            | ParseNode::OperatorName { body, .. }
            | ParseNode::Styling { body, .. }
            | ParseNode::MClass { body, .. }
            | ParseNode::HBox { body, .. } => {
                for node in body {
                    push(node, inherited, explicit_font)?;
                }
            }
            ParseNode::SupSub { base, sup, sub, .. } => {
                for node in [base, sup, sub]
                    .into_iter()
                    .filter_map(|node| node.as_deref_mut())
                {
                    push(node, inherited, explicit_font)?;
                }
            }
            ParseNode::GenFrac { numer, denom, .. } => {
                push(numer.as_mut(), inherited, explicit_font)?;
                push(denom.as_mut(), inherited, explicit_font)?;
            }
            ParseNode::Sqrt { body, index, .. } => {
                push(body.as_mut(), inherited, explicit_font)?;
                if let Some(index) = index {
                    push(index.as_mut(), inherited, explicit_font)?;
                }
            }
            ParseNode::Accent { label, base, .. } => {
                if !matches!(label.as_str(), "\\hat" | "\\bar") || !single_accent_base(base) {
                    return Err(MathError::Unsupported(Unsupported::Construct));
                }
                push(base.as_mut(), inherited, explicit_font)?;
            }
            ParseNode::Font { body: base, .. } => {
                push(base.as_mut(), inherited, true)?;
            }
            ParseNode::Overline { body: base, .. }
            | ParseNode::Underline { body: base, .. }
            | ParseNode::Lap { body: base, .. } => {
                push(base.as_mut(), inherited, explicit_font)?;
            }
            ParseNode::Op { body, .. } => {
                if let Some(body) = body {
                    for node in body {
                        push(node, inherited, explicit_font)?;
                    }
                }
            }
            ParseNode::Text { body, font, .. } => {
                for node in body {
                    push(
                        node,
                        inherited,
                        explicit_font || text_font_changes_cjk(font.as_deref()),
                    )?;
                }
            }
            ParseNode::Color { color, body, .. } => {
                let color = source_color(color)?;
                for node in body {
                    push(node, color, explicit_font)?;
                }
            }
            ParseNode::LeftRight {
                body, right_color, ..
            } => {
                if let Some(color) = right_color {
                    source_color(color)?;
                }
                for node in body {
                    push(node, inherited, explicit_font)?;
                }
            }
            ParseNode::Enclose {
                body,
                background_color: None,
                border_color,
                ..
            } => {
                if let Some(color) = border_color {
                    source_color(color)?;
                } else {
                    // The pinned engine defaults uncolored frames to BLACK, even inside Color.
                    // Resolve the missing paint before layout, without conflating explicit black.
                    *border_color = Some(match paint(inherited)? {
                        Paint::Inherit => "#0000".into(),
                        Paint::Rgb { red, green, blue } => {
                            format!("#{red:02x}{green:02x}{blue:02x}")
                        }
                    });
                }
                push(body.as_mut(), inherited, explicit_font)?;
            }
            ParseNode::Array {
                body, tags: None, ..
            } => {
                for node in body.iter_mut().flatten() {
                    push(node, inherited, explicit_font)?;
                }
            }
            ParseNode::HtmlMathMl { html, mathml, .. } => {
                for node in html.iter_mut().chain(mathml) {
                    push(node, inherited, explicit_font)?;
                }
            }
            _ => return Err(MathError::Unsupported(Unsupported::Construct)),
        }
    }
    Ok(())
}

fn single_accent_base(node: &ParseNode) -> bool {
    match node {
        ParseNode::Atom { .. } | ParseNode::MathOrd { .. } | ParseNode::TextOrd { .. } => true,
        ParseNode::OrdGroup { body, .. }
        | ParseNode::Text { body, .. }
        | ParseNode::Styling { body, .. }
        | ParseNode::MClass { body, .. }
        | ParseNode::HBox { body, .. }
        | ParseNode::Color { body, .. } => {
            matches!(body.as_slice(), [base] if single_accent_base(base))
        }
        ParseNode::Font { body, .. } => single_accent_base(body),
        _ => false,
    }
}

fn is_supported_cjk(ch: char) -> bool {
    matches!(
        u32::from(ch),
        0x3040..=0x30ff
            | 0x31f0..=0x31ff
            | 0x3400..=0x4dbf
            | 0x4e00..=0x9fff
            | 0xf900..=0xfaff
            | 0xac00..=0xd7af
            | 0xff01..=0xff60
            | 0xffe0..=0xffee
    )
}

fn text_font_changes_cjk(font: Option<&str>) -> bool {
    !matches!(
        font,
        None | Some("\\text" | "\\textrm" | "\\textnormal" | "\\textup" | "\\textmd")
    )
}

fn resolved_symbol_is_accent_marker(text: &str, mode: ratex_parser::Mode) -> bool {
    let mode = match mode {
        ratex_parser::Mode::Math => ratex_font::Mode::Math,
        ratex_parser::Mode::Text => ratex_font::Mode::Text,
    };
    ratex_font::get_symbol(text, mode)
        .and_then(|symbol| symbol.codepoint)
        .or_else(|| text.chars().next())
        .is_some_and(is_accent_marker)
}

fn is_accent_marker(ch: char) -> bool {
    matches!(ch, '^' | '\u{02c9}')
}
