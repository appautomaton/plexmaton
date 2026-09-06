//! Project already positioned engine output onto non-overlapping native cell reservations.
use crate::{
    FontStyle, GlyphRun, Limit, MAX_CELLS, MAX_DIMENSION, MathError, Paint, TextScale,
    VerticalAlign,
    engine::{Item, Kind, Scene},
};
use unicode_width::UnicodeWidthStr as _;

mod axis;
mod paint;
use axis::Axis;

pub(super) struct Layout {
    pub width: u16,
    pub height: u16,
    pub axis: u16,
    pub runs: Vec<GlyphRun>,
}

struct Placement {
    x: usize,
    columns: usize,
    rows: usize,
    y_index: usize,
}

pub(super) fn project(scene: &Scene, available: usize) -> Result<Layout, MathError> {
    if available == 0 || available > MAX_DIMENSION {
        return Err(MathError::Limited(Limit::Geometry));
    }
    let mut xp = vec![0.0, scene.width];
    let mut yp = vec![0.0, scene.height, scene.axis];
    for item in &scene.items {
        xp.extend([item.x, item.x + item.width]);
        yp.extend([item.y, item.top, item.bottom]);
    }
    let mut x = Axis::new(&xp, 1.7)?;
    let mut y = Axis::new(&yp, 0.65)?;
    for item in &scene.items {
        if item.width > 0.0 {
            x.require(item.x, item.x + item.width, natural_size(item).0)?;
        }
        if matches!(
            item.kind,
            Kind::Vertical | Kind::Delimiter(_) | Kind::Radical
        ) {
            y.require(
                item.top,
                item.bottom,
                if matches!(item.kind, Kind::Radical) {
                    2
                } else {
                    1
                },
            )?;
        }
    }
    preserve_horizontal_gaps(scene, &mut x)?;
    x.solve()?;
    let placements = scene
        .items
        .iter()
        .map(|item| {
            let (columns, rows) = natural_size(item);
            Ok(Placement {
                x: x.at(item.x)?,
                columns,
                rows,
                y_index: y.position(item.y)?,
            })
        })
        .collect::<Result<Vec<_>, MathError>>()?;
    place_rows(scene, &x, &mut y, &placements)?;
    let mut runs = Vec::new();
    let mut rules = Vec::new();
    let mut width = 0;
    let mut height = 0;
    for (item, at) in scene.items.iter().zip(&placements) {
        match &item.kind {
            Kind::Glyph {
                text,
                style,
                scale,
                baseline,
            } => {
                if text.len() > crate::MAX_RUN_BYTES {
                    return Err(MathError::Limited(Limit::NativeTextBytes));
                }
                if *scale != TextScale::Full && at.columns > 7 * at.rows {
                    return Err(MathError::Unsupported(crate::Unsupported::Scale));
                }
                let row = y
                    .at(item.y)?
                    .checked_sub(at.rows / 2)
                    .ok_or(MathError::Overlap)?;
                let align = alignment(scene, &y, item, *baseline, *scale)?;
                width = width.max(at.x + at.columns);
                height = height.max(row + at.rows);
                runs.push(GlyphRun {
                    x: bounded(at.x)?,
                    y: bounded(row)?,
                    columns: bounded(at.columns)?,
                    rows: bounded(at.rows)?,
                    text: text.clone(),
                    style: *style,
                    scale: *scale,
                    paint: item.paint,
                    align,
                });
            }
            Kind::Horizontal => {
                let end = x.at(item.x + item.width)?.max(at.x + 1);
                let row = y.at(item.y)?;
                width = width.max(end);
                height = height.max(row + 1);
                rules.push(paint::Rule {
                    x: at.x,
                    y: row,
                    length: end - at.x,
                    horizontal: true,
                    paint: item.paint,
                });
            }
            Kind::Vertical => {
                let top = y.at(item.top)?;
                let end = y.at(item.bottom)?.max(top + 1);
                width = width.max(at.x + 1);
                height = height.max(end);
                rules.push(paint::Rule {
                    x: at.x,
                    y: top,
                    length: end - top,
                    horizontal: false,
                    paint: item.paint,
                });
            }
            Kind::Delimiter(_) | Kind::Radical => {
                let top = y.at(item.top)?;
                let minimum = if matches!(item.kind, Kind::Radical) {
                    2
                } else {
                    1
                };
                let end = y.at(item.bottom)?.max(top + minimum);
                width = width.max(at.x + 1);
                height = height.max(end);
                if runs.len() + end - top > MAX_CELLS {
                    return Err(MathError::Limited(Limit::Cells));
                }
                for row in top..end {
                    runs.push(plain_run(
                        at.x,
                        row,
                        vertical_character(&item.kind, row - top, end - top),
                        item.paint,
                    )?);
                }
            }
        }
    }
    let axis = y.at(scene.axis)?;
    height = height.max(axis + 1);
    check_bounds(width, height, available)?;
    paint::finish(width, height, &mut runs, &rules)?;
    runs.sort_by_key(|run| (run.y, run.x));
    Ok(Layout {
        width: bounded(width)?,
        height: bounded(height)?,
        axis: bounded(axis)?,
        runs,
    })
}

fn check_bounds(width: usize, height: usize, available: usize) -> Result<(), MathError> {
    if width > available {
        return Err(MathError::TooWide {
            required: width,
            available,
        });
    }
    if width == 0 || height == 0 {
        return Err(MathError::Empty);
    }
    if width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(MathError::Limited(Limit::Geometry));
    }
    if width
        .checked_mul(height)
        .is_none_or(|cells| cells > MAX_CELLS)
    {
        return Err(MathError::Limited(Limit::Cells));
    }
    Ok(())
}

fn place_rows(
    scene: &Scene,
    x: &Axis,
    y: &mut Axis,
    placements: &[Placement],
) -> Result<(), MathError> {
    enclose_baselines(scene, y, placements)?;
    separate_rows(scene, x, y, placements)
}

fn vertical_character(kind: &Kind, row: usize, height: usize) -> char {
    match kind {
        Kind::Delimiter(character) => paint::delimiter(*character, row, height),
        Kind::Radical if row + 1 == height => '√',
        Kind::Radical if row == 0 => '┌',
        Kind::Radical => '│',
        _ => unreachable!("vertical text branch"),
    }
}

fn enclose_baselines(
    scene: &Scene,
    y: &mut Axis,
    placements: &[Placement],
) -> Result<(), MathError> {
    // Tall borders and delimiters must include interior baselines after quantization.
    for item in &scene.items {
        if !matches!(
            item.kind,
            Kind::Vertical | Kind::Delimiter(_) | Kind::Radical
        ) {
            continue;
        }
        for (other, at) in scene.items.iter().zip(placements) {
            if matches!(item.kind, Kind::Delimiter(_))
                && matches!(other.kind, Kind::Horizontal)
                && item.x >= other.x
                && item.x < other.x + other.width
            {
                if other.y < item.top {
                    y.require(other.y, item.top, 1)?;
                }
                if other.y >= item.bottom {
                    y.require(item.bottom, other.y, 0)?;
                }
            }
            if matches!(other.kind, Kind::Glyph { .. } | Kind::Horizontal)
                && other.y >= item.top
                && other.y <= item.bottom
            {
                y.require(item.top, other.y, at.rows / 2)?;
                y.require(other.y, item.bottom, at.rows - at.rows / 2)?;
            }
        }
    }
    Ok(())
}

fn separate_rows(
    scene: &Scene,
    x: &Axis,
    y: &mut Axis,
    placements: &[Placement],
) -> Result<(), MathError> {
    for index in 0..y.len() {
        y.advance(index)?;
        for (position, (item, at)) in scene.items.iter().zip(placements).enumerate() {
            if at.y_index != index || !matches!(item.kind, Kind::Glyph { .. } | Kind::Horizontal) {
                continue;
            }
            y.raise(index, at.rows / 2)?;
            let end = if matches!(item.kind, Kind::Horizontal) {
                x.at(item.x + item.width)?.max(at.x + 1)
            } else {
                at.x + at.columns
            };
            for (other_position, (other, prior)) in scene.items.iter().zip(placements).enumerate() {
                if other_position == position
                    || !matches!(other.kind, Kind::Glyph { .. } | Kind::Horizontal)
                {
                    continue;
                }
                let prior_end = if matches!(other.kind, Kind::Horizontal) {
                    x.at(other.x + other.width)?.max(prior.x + 1)
                } else {
                    prior.x + prior.columns
                };
                if at.x >= prior_end || prior.x >= end {
                    continue;
                }
                if prior.y_index == index {
                    // Equal-position rules may form one stroke; independent glyphs cannot overprint.
                    if matches!(
                        (&item.kind, &other.kind),
                        (Kind::Horizontal, Kind::Horizontal)
                    ) {
                        continue;
                    }
                    return Err(MathError::Overlap);
                }
                if prior.y_index < index {
                    let minimum = y.at(other.y)? + prior.rows - prior.rows / 2 + at.rows / 2;
                    y.raise(index, minimum)?;
                }
            }
        }
    }
    Ok(())
}

fn preserve_horizontal_gaps(scene: &Scene, x: &mut Axis) -> Result<(), MathError> {
    let mut glyphs: Vec<_> = scene
        .items
        .iter()
        .filter_map(|item| {
            let Kind::Glyph { baseline, .. } = item.kind else {
                return None;
            };
            Some((baseline, item))
        })
        .collect();
    glyphs.sort_by(|(a, left), (b, right)| a.total_cmp(b).then(left.x.total_cmp(&right.x)));
    for pair in glyphs.windows(2) {
        let [(a, left), (b, right)] = pair else {
            unreachable!("windows of two");
        };
        let end = left.x + left.width;
        if (a - b).abs() < 0.000_001 && right.x - end >= 0.15 {
            x.require(end, right.x, 1)?;
        }
    }
    Ok(())
}

fn natural_size(item: &Item) -> (usize, usize) {
    match &item.kind {
        Kind::Glyph { text, scale, .. } => {
            let columns = text.width();
            match scale {
                TextScale::Full => (columns, 1),
                TextScale::Script => ((columns * 7).div_ceil(10), 1),
                TextScale::ScriptScript => (columns.div_ceil(2), 1),
                TextScale::Large => (columns.div_ceil(1) * 2, 2),
            }
        }
        _ => (1, 1),
    }
}

fn alignment(
    scene: &Scene,
    y: &Axis,
    item: &Item,
    baseline: f64,
    scale: TextScale,
) -> Result<VerticalAlign, MathError> {
    if matches!(scale, TextScale::Full | TextScale::Large) {
        return Ok(VerticalAlign::Bottom);
    }
    let row = y.at(item.y)?;
    let base = scene
        .items
        .iter()
        .filter_map(|other| {
            let Kind::Glyph {
                scale: TextScale::Full,
                baseline: other_baseline,
                ..
            } = other.kind
            else {
                return None;
            };
            if y.at(other.y).ok()? != row {
                return None;
            }
            Some(((item.x - other.x).abs(), other_baseline))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    Ok(match base {
        Some((_, other_baseline)) if baseline < other_baseline => VerticalAlign::Top,
        Some(_) => VerticalAlign::Bottom,
        None if item.y < scene.axis => VerticalAlign::Bottom,
        None => VerticalAlign::Top,
    })
}

fn bounded(value: usize) -> Result<u16, MathError> {
    if value > MAX_DIMENSION {
        return Err(MathError::Limited(Limit::Geometry));
    }
    u16::try_from(value).map_err(|_| MathError::Limited(Limit::Geometry))
}

fn plain_run(x: usize, y: usize, character: char, paint: Paint) -> Result<GlyphRun, MathError> {
    Ok(GlyphRun {
        x: bounded(x)?,
        y: bounded(y)?,
        text: character.to_string(),
        columns: 1,
        rows: 1,
        scale: TextScale::Full,
        align: VerticalAlign::Bottom,
        style: FontStyle::Roman,
        paint,
    })
}
