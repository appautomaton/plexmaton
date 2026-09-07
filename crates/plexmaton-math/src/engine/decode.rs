use ratex_font::{FontId, get_char_metrics};
use ratex_types::{DisplayItem, DisplayList};
use unicode_width::UnicodeWidthStr as _;

use super::{Item, Kind, Scene, geometry, paint};
use crate::{FontStyle, MathError, TextScale, Unsupported};

use super::paths::{PathAdmissions, tall_parenthesis};

const EPSILON: f64 = 0.000_001;

pub(super) fn scene(
    list: &DisplayList,
    axis_height: f64,
    mut paths: PathAdmissions,
) -> Result<Scene, MathError> {
    let mut items = Vec::new();
    let mut cursor = 0;
    while let Some(item) = list.items.get(cursor) {
        match item {
            DisplayItem::GlyphPath { .. } => {
                let (item, merged) = glyph(item, list.items.get(cursor + 1), axis_height)?;
                items.push(item);
                cursor += usize::from(merged);
            }
            DisplayItem::Line {
                x,
                y,
                width,
                thickness,
                color,
                dashed,
            } => {
                if *dashed || *thickness > 0.15 || *thickness <= 0.0 || *width < 0.0 {
                    return Err(MathError::Unsupported(Unsupported::Construct));
                }
                items.push(Item {
                    x: geometry(*x)?,
                    y: geometry(*y)?,
                    width: geometry(*width)?,
                    top: geometry(y - thickness / 2.0)?,
                    bottom: geometry(y + thickness / 2.0)?,
                    kind: Kind::Horizontal,
                    paint: paint(*color)?,
                });
            }
            DisplayItem::Rect {
                x,
                y,
                width,
                height,
                color,
            } => {
                if *width < 0.0 || *height < 0.0 {
                    return Err(MathError::Unsupported(Unsupported::Construct));
                }
                let (kind, x, center) = if *height <= 0.15 && width > height {
                    (Kind::Horizontal, *x, y + height / 2.0)
                } else if *width <= 0.15 && height > width {
                    (Kind::Vertical, x + width / 2.0, *y)
                } else {
                    return Err(MathError::Unsupported(Unsupported::Paint));
                };
                items.push(Item {
                    x: geometry(x)?,
                    y: geometry(center)?,
                    width: geometry(*width)?,
                    top: geometry(*y)?,
                    bottom: geometry(y + height)?,
                    kind,
                    paint: paint(*color)?,
                });
            }
            DisplayItem::Path {
                x,
                y,
                commands,
                fill,
                color,
            } => items.push(tall_parenthesis(
                *x, *y, commands, *fill, *color, &mut paths,
            )?),
        }
        cursor += 1;
    }
    if items.is_empty() {
        return Err(MathError::Empty);
    }
    align_radical_roofs(&mut items);
    combine_accents(&mut items);
    let items = group_text(items);
    Ok(Scene {
        width: list.width,
        height: list.total_height(),
        axis: geometry(list.height - axis_height)?,
        items,
    })
}

fn align_radical_roofs(items: &mut [Item]) {
    for index in 0..items.len() {
        let item = &items[index];
        if !matches!(item.kind, Kind::Radical { .. }) {
            continue;
        }
        let roof = items
            .iter()
            .find(|other| {
                matches!(other.kind, Kind::Horizontal)
                    && other.paint == item.paint
                    && (other.x - item.x - item.width).abs() < 0.05
                    && (other.y - item.top).abs() < 0.2
            })
            .map(|other| other.y);
        if let Some(roof) = roof {
            items[index].top = roof;
        }
    }
}

fn native_scale(scale: f64) -> Result<TextScale, MathError> {
    if (scale - 1.0).abs() < EPSILON {
        Ok(TextScale::Full)
    } else if (scale - 0.7).abs() < EPSILON {
        Ok(TextScale::Script)
    } else if (scale - 0.5).abs() < EPSILON {
        Ok(TextScale::ScriptScript)
    } else {
        Err(MathError::Unsupported(Unsupported::Scale))
    }
}

fn font_style(font: FontId) -> Result<FontStyle, MathError> {
    match font {
        FontId::MainRegular
        | FontId::AmsRegular
        | FontId::TypewriterRegular
        | FontId::CjkRegular
        | FontId::Size1Regular
        | FontId::Size2Regular
        | FontId::Size3Regular
        | FontId::Size4Regular => Ok(FontStyle::Roman),
        FontId::MainItalic | FontId::MathItalic => Ok(FontStyle::Italic),
        FontId::MainBold => Ok(FontStyle::Bold),
        FontId::MainBoldItalic | FontId::MathBoldItalic => Ok(FontStyle::BoldItalic),
        _ => Err(MathError::Unsupported(Unsupported::Font)),
    }
}

fn native_character(font: FontId, code: u32) -> Result<char, MathError> {
    let ch = char::from_u32(code).ok_or(MathError::Unsupported(Unsupported::Glyph))?;
    if ch.is_control()
        || matches!(code, 0xe000..=0xf8ff | 0xf0000..=0xffffd | 0x100000..=0x10fffd)
        || ch.to_string().width() == 0
    {
        return Err(MathError::Unsupported(Unsupported::Glyph));
    }
    if font != FontId::AmsRegular || !ch.is_ascii_alphanumeric() {
        return Ok(ch);
    }
    // AMS Latin letters/digits are double-struck glyphs, not plain ASCII in a terminal font.
    let mapped = match ch {
        'C' => u32::from('ℂ'),
        'H' => u32::from('ℍ'),
        'N' => u32::from('ℕ'),
        'P' => u32::from('ℙ'),
        'Q' => u32::from('ℚ'),
        'R' => u32::from('ℝ'),
        'Z' => u32::from('ℤ'),
        'A'..='Z' => 0x1d538 + code - u32::from('A'),
        'a'..='z' => 0x1d552 + code - u32::from('a'),
        '0'..='9' => 0x1d7d8 + code - u32::from('0'),
        _ => return Err(MathError::Unsupported(Unsupported::Font)),
    };
    char::from_u32(mapped).ok_or(MathError::Unsupported(Unsupported::Glyph))
}

fn glyph_metrics(
    font: FontId,
    code: u32,
    scale: f64,
) -> Result<ratex_font::CharMetrics, MathError> {
    if let Some(metrics) = get_char_metrics(font, code) {
        return Ok(metrics);
    }
    let ch = native_character(font, code)?;
    if font != FontId::CjkRegular || !super::is_supported_cjk(ch) {
        return Err(MathError::Unsupported(Unsupported::Glyph));
    }
    // The display list omits the metrics of system-font glyphs. Ask the pinned engine for
    // the same text glyph box instead of copying its fallback-width/height rules (MTH-2).
    let style = match native_scale(scale)? {
        TextScale::Full => ratex_types::MathStyle::Text,
        TextScale::Script => ratex_types::MathStyle::Script,
        TextScale::ScriptScript => ratex_types::MathStyle::ScriptScript,
        TextScale::Large => return Err(MathError::Unsupported(Unsupported::Scale)),
    };
    let node = ratex_parser::ParseNode::TextOrd {
        mode: ratex_parser::Mode::Text,
        text: ch.to_string(),
        loc: None,
    };
    let glyph = ratex_layout::layout(
        &[node],
        &ratex_layout::LayoutOptions {
            style,
            ..ratex_layout::LayoutOptions::default()
        },
    );
    Ok(ratex_font::CharMetrics {
        width: geometry(glyph.width)?,
        height: geometry(glyph.height)?,
        depth: geometry(glyph.depth)?,
        italic: 0.0,
        skew: 0.0,
    })
}

fn combine_accents(items: &mut Vec<Item>) {
    let mut index = 0;
    while index < items.len() {
        let accent = &items[index];
        let Kind::Glyph {
            text,
            scale,
            baseline,
            ..
        } = &accent.kind
        else {
            index += 1;
            continue;
        };
        let combining = match text.as_str() {
            "ˉ" => '\u{0304}',
            "^" => '\u{0302}',
            _ => {
                index += 1;
                continue;
            }
        };
        let center = accent.x + accent.width / 2.0;
        let target = items[..index]
            .iter()
            .enumerate()
            .filter_map(|(candidate, item)| {
                let Kind::Glyph {
                    text,
                    scale: other_scale,
                    baseline: other_baseline,
                    ..
                } = &item.kind
                else {
                    return None;
                };
                if scale != other_scale
                    || accent.paint != item.paint
                    || text.chars().count() != 1
                    || other_baseline + EPSILON < *baseline
                    || other_baseline - baseline > 1.5
                    || accent.top >= item.top
                    || center < item.x - 0.1
                    || center > item.x + item.width + 0.1
                {
                    return None;
                }
                Some((candidate, (center - item.x - item.width / 2.0).abs()))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|value| value.0);
        if let Some(target) = target {
            let top = accent.top;
            if let Kind::Glyph { text, .. } = &mut items[target].kind {
                text.push(combining);
            }
            items[target].top = items[target].top.min(top);
            items.remove(index);
        } else {
            index += 1;
        }
    }
}

fn group_text(items: Vec<Item>) -> Vec<Item> {
    let mut groups: Vec<Item> = Vec::new();
    for item in items {
        let merge = groups
            .last()
            .is_some_and(|previous| match (&previous.kind, &item.kind) {
                (
                    Kind::Glyph {
                        style: a,
                        scale: sa,
                        baseline: ya,
                        ..
                    },
                    Kind::Glyph {
                        style: b,
                        scale: sb,
                        baseline: yb,
                        ..
                    },
                ) => {
                    a == b
                        && sa == sb
                        && previous.paint == item.paint
                        && (ya - yb).abs() < EPSILON
                        && item.x >= previous.x
                        && (item.x - previous.x - previous.width).abs() <= 0.025
                }
                _ => false,
            });
        if merge {
            let previous = groups
                .last_mut()
                .expect("merge was checked against the last item");
            if let (Kind::Glyph { text: a, .. }, Kind::Glyph { text: b, .. }) =
                (&mut previous.kind, &item.kind)
            {
                a.push_str(b);
            }
            previous.width = item.x + item.width - previous.x;
            previous.top = previous.top.min(item.top);
            previous.bottom = previous.bottom.max(item.bottom);
        } else {
            groups.push(item);
        }
    }
    groups
}

fn glyph(
    item: &DisplayItem,
    next: Option<&DisplayItem>,
    axis_height: f64,
) -> Result<(Item, bool), MathError> {
    let DisplayItem::GlyphPath {
        x,
        y,
        scale,
        font,
        char_code,
        color,
    } = item
    else {
        unreachable!("glyph branch");
    };
    let mut merged = false;
    let id = FontId::parse(font).ok_or(MathError::Unsupported(Unsupported::Font))?;
    let mut value = *char_code;
    let mut glyph_metrics = glyph_metrics(id, value, *scale)?;
    // KaTeX's private-use negation overlay and equals sign share one exact origin.
    // Replace only this verified pair; never emit private-use font codes as Unicode.
    if value == 0xe020 {
        let Some(DisplayItem::GlyphPath {
            x: other_x,
            y: other_y,
            scale: other_scale,
            font: other_font,
            char_code: 61,
            color: other_color,
        }) = next
        else {
            return Err(MathError::Unsupported(Unsupported::Glyph));
        };
        if id != FontId::MainRegular
            || font != other_font
            || color != other_color
            || (x - other_x).abs() > EPSILON
            || (y - other_y).abs() > EPSILON
            || (scale - other_scale).abs() > EPSILON
        {
            return Err(MathError::Unsupported(Unsupported::Glyph));
        }
        let equal = get_char_metrics(id, 61).ok_or(MathError::Unsupported(Unsupported::Glyph))?;
        glyph_metrics.width = equal.width;
        glyph_metrics.height = glyph_metrics.height.max(equal.height);
        glyph_metrics.depth = glyph_metrics.depth.max(equal.depth);
        value = u32::from('≠');
        merged = true;
    }
    let ch = native_character(id, value)?;
    let style = font_style(id)?;
    let mut text_scale = native_scale(*scale)?;
    let top = geometry(y - glyph_metrics.height * scale)?;
    let bottom = geometry(y + glyph_metrics.depth * scale)?;
    let sized = matches!(
        id,
        FontId::Size1Regular | FontId::Size2Regular | FontId::Size3Regular | FontId::Size4Regular
    );
    let kind = if ch == '√' {
        Kind::Radical { scale: text_scale }
    } else if sized
        && matches!(
            ch,
            '(' | ')' | '[' | ']' | '{' | '}' | '|' | '‖' | '⟨' | '⟩'
        )
    {
        Kind::Delimiter(ch)
    } else {
        if sized && *scale == 1.0 && matches!(ch, '∑' | '∏' | '∐' | '∫' | '∮') {
            text_scale = TextScale::Large;
        }
        Kind::Glyph {
            text: ch.to_string(),
            style,
            scale: text_scale,
            baseline: *y,
        }
    };
    Ok((
        Item {
            x: geometry(*x)?,
            y: geometry(y - axis_height * scale)?,
            width: geometry(glyph_metrics.width * scale)?,
            top,
            bottom,
            kind,
            paint: paint(*color)?,
        },
        merged,
    ))
}
