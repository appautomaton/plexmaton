//! Semantic admission for the one pinned vector-path projection this adapter supports.

use ratex_types::{Color, PathCommand};

use super::{Item, Kind, geometry, paint};
use crate::{MathError, Unsupported};

const EPSILON: f64 = 0.000_001;

/// Semantic allowance for tall `\left(`/`\right)` delimiters before RaTeX flattens them.
#[derive(Default)]
pub(super) struct PathAdmissions {
    left_parentheses: usize,
    right_parentheses: usize,
}

impl PathAdmissions {
    pub(super) fn admit(&mut self, delimiter: &str) {
        match delimiter {
            "(" | "\\lparen" => self.left_parentheses += 1,
            ")" | "\\rparen" => self.right_parentheses += 1,
            _ => {}
        }
    }

    fn consume(&mut self, delimiter: char) -> Result<(), MathError> {
        let remaining = match delimiter {
            '(' => &mut self.left_parentheses,
            ')' => &mut self.right_parentheses,
            _ => return Err(MathError::Unsupported(Unsupported::Path)),
        };
        if *remaining == 0 {
            return Err(MathError::Unsupported(Unsupported::Path));
        }
        *remaining -= 1;
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum PathShape {
    Move(f64),
    Line(f64),
    Cubic(f64, f64, f64),
    Close,
}

// These x-coordinate signatures are the two tall-parenthesis paths emitted by RaTeX 0.1.14's
// stacked delimiter generator. The y coordinates vary with the requested delimiter height and
// are independently bounded below. Any generator change must refuse until this adapter is audited.
const LEFT_PARENTHESIS: &[PathShape] = &[
    PathShape::Move(0.863),
    PathShape::Cubic(0.863, 0.861, 0.857),
    PathShape::Cubic(0.857, 0.84, 0.84),
    PathShape::Cubic(0.8273, 0.8207, 0.82),
    PathShape::Cubic(0.8147, 0.8097, 0.805),
    PathShape::Cubic(0.5623, 0.4097, 0.347),
    PathShape::Cubic(0.3257, 0.3137, 0.311),
    PathShape::Line(0.311),
    PathShape::Cubic(0.3112, 0.311, 0.311),
    PathShape::Cubic(0.313, 0.321, 0.335),
    PathShape::Cubic(0.3883, 0.545, 0.805),
    PathShape::Cubic(0.8097, 0.8147, 0.82),
    PathShape::Cubic(0.8207, 0.827, 0.839),
    PathShape::Cubic(0.839, 0.857, 0.857),
    PathShape::Cubic(0.861, 0.863, 0.863),
    PathShape::Cubic(0.863, 0.8597, 0.853),
    PathShape::Cubic(0.7177, 0.6175, 0.5525),
    PathShape::Cubic(0.4875, 0.45, 0.44),
    PathShape::Cubic(0.438, 0.437, 0.437),
    PathShape::Line(0.437),
    PathShape::Cubic(0.437, 0.4427, 0.454),
    PathShape::Cubic(0.4747, 0.5177, 0.583),
    PathShape::Cubic(0.6483, 0.7383, 0.853),
    PathShape::Cubic(0.8597, 0.863, 0.863),
    PathShape::Close,
];

const RIGHT_PARENTHESIS: &[PathShape] = &[
    PathShape::Move(0.076),
    PathShape::Cubic(0.0593, 0.051, 0.051),
    PathShape::Cubic(0.051, 0.053, 0.057),
    PathShape::Cubic(0.0783, 0.0993, 0.12),
    PathShape::Cubic(0.2167, 0.2928, 0.3485),
    PathShape::Cubic(0.4042, 0.4413, 0.46),
    PathShape::Cubic(0.4713, 0.477, 0.477),
    PathShape::Cubic(0.477, 0.4787, 0.4803),
    PathShape::Line(0.4803),
    PathShape::Cubic(0.4773, 0.477, 0.477),
    PathShape::Cubic(0.477, 0.4713, 0.46),
    PathShape::Cubic(0.4413, 0.4042, 0.3485),
    PathShape::Cubic(0.2928, 0.2167, 0.12),
    PathShape::Cubic(0.0993, 0.0783, 0.057),
    PathShape::Cubic(0.055, 0.053, 0.051),
    PathShape::Cubic(0.051, 0.0567, 0.068),
    PathShape::Cubic(0.068, 0.079, 0.079),
    PathShape::Cubic(0.0883, 0.0933, 0.094),
    PathShape::Cubic(0.0993, 0.1043, 0.109),
    PathShape::Cubic(0.3517, 0.5043, 0.567),
    PathShape::Cubic(0.5883, 0.6003, 0.603),
    PathShape::Line(0.603),
    PathShape::Cubic(0.601, 0.593, 0.579),
    PathShape::Cubic(0.5257, 0.369, 0.109),
    PathShape::Cubic(0.1043, 0.0993, 0.094),
    PathShape::Cubic(0.0933, 0.0873, 0.076),
    PathShape::Close,
];

pub(super) fn tall_parenthesis(
    x: f64,
    y: f64,
    commands: &[PathCommand],
    fill: bool,
    color: Color,
    paths: &mut PathAdmissions,
) -> Result<Item, MathError> {
    if !fill {
        return Err(MathError::Unsupported(Unsupported::Path));
    }
    let delimiter = if matches_signature(commands, LEFT_PARENTHESIS) {
        '('
    } else if matches_signature(commands, RIGHT_PARENTHESIS) {
        ')'
    } else {
        return Err(MathError::Unsupported(Unsupported::Path));
    };
    paths.consume(delimiter)?;
    let (top, bottom) = path_vertical_bounds(commands)?;
    if bottom - top <= 0.0 {
        return Err(MathError::Unsupported(Unsupported::Path));
    }
    Ok(Item {
        x: geometry(x)?,
        y: geometry(y)?,
        // RaTeX's pinned tall-parenthesis box reserves 0.875 em even though its ink is inset.
        width: geometry(0.875)?,
        top: geometry(y + top)?,
        bottom: geometry(y + bottom)?,
        kind: Kind::Delimiter(delimiter),
        paint: paint(color)?,
    })
}

fn matches_signature(commands: &[PathCommand], signature: &[PathShape]) -> bool {
    commands.len() == signature.len()
        && commands
            .iter()
            .zip(signature)
            .all(|(command, expected)| match (command, expected) {
                (PathCommand::MoveTo { x, y }, PathShape::Move(expected))
                | (PathCommand::LineTo { x, y }, PathShape::Line(expected)) => {
                    y.is_finite() && approximately(*x, *expected)
                }
                (
                    PathCommand::CubicTo {
                        x1,
                        y1,
                        x2,
                        y2,
                        x,
                        y,
                    },
                    PathShape::Cubic(a, b, c),
                ) => {
                    [x1, y1, x2, y2, x, y].iter().all(|value| value.is_finite())
                        && approximately(*x1, *a)
                        && approximately(*x2, *b)
                        && approximately(*x, *c)
                }
                (PathCommand::Close, PathShape::Close) => true,
                _ => false,
            })
}

fn approximately(actual: f64, expected: f64) -> bool {
    (actual - expected).abs() <= EPSILON
}

fn path_vertical_bounds(commands: &[PathCommand]) -> Result<(f64, f64), MathError> {
    let mut top = f64::INFINITY;
    let mut bottom = f64::NEG_INFINITY;
    let mut visit = |value: f64| -> Result<(), MathError> {
        if !value.is_finite() {
            return Err(MathError::Unsupported(Unsupported::Path));
        }
        top = top.min(value);
        bottom = bottom.max(value);
        Ok(())
    };
    for command in commands {
        match command {
            PathCommand::MoveTo { y, .. } | PathCommand::LineTo { y, .. } => visit(*y)?,
            PathCommand::CubicTo { y1, y2, y, .. } => {
                visit(*y1)?;
                visit(*y2)?;
                visit(*y)?;
            }
            PathCommand::QuadTo { .. } => return Err(MathError::Unsupported(Unsupported::Path)),
            PathCommand::Close => {}
        }
    }
    if !top.is_finite() || !bottom.is_finite() {
        return Err(MathError::Unsupported(Unsupported::Path));
    }
    Ok((top, bottom))
}
