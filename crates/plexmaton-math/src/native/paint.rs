use super::plain_run;
use crate::{GlyphRun, MathError, Paint};

pub(super) struct Rule {
    pub x: usize,
    pub y: usize,
    pub length: usize,
    pub horizontal: bool,
    pub paint: Paint,
}

#[derive(Clone, Copy)]
enum Cell {
    Empty,
    Text,
    Rule {
        horizontal: bool,
        vertical: bool,
        paint: Paint,
    },
}

pub(super) fn finish(
    width: usize,
    height: usize,
    runs: &mut Vec<GlyphRun>,
    rules: &[Rule],
) -> Result<(), MathError> {
    let mut cells = vec![Cell::Empty; width * height];
    for run in runs.iter() {
        for y in usize::from(run.y)..usize::from(run.y) + usize::from(run.rows) {
            for x in usize::from(run.x)..usize::from(run.x) + usize::from(run.columns) {
                if x >= width || y >= height || !matches!(cells[y * width + x], Cell::Empty) {
                    return Err(MathError::Overlap);
                }
                cells[y * width + x] = Cell::Text;
            }
        }
    }
    for rule in rules {
        for offset in 0..rule.length {
            let x = rule.x + if rule.horizontal { offset } else { 0 };
            let y = rule.y + if rule.horizontal { 0 } else { offset };
            if x >= width || y >= height {
                return Err(MathError::Overlap);
            }
            cells[y * width + x] = match cells[y * width + x] {
                Cell::Empty => Cell::Rule {
                    horizontal: rule.horizontal,
                    vertical: !rule.horizontal,
                    paint: rule.paint,
                },
                Cell::Rule {
                    horizontal,
                    vertical,
                    paint,
                } if paint == rule.paint => Cell::Rule {
                    horizontal: horizontal || rule.horizontal,
                    vertical: vertical || !rule.horizontal,
                    paint,
                },
                _ => return Err(MathError::Overlap),
            };
        }
    }
    for y in 0..height {
        for x in 0..width {
            let Cell::Rule {
                horizontal,
                vertical,
                paint,
            } = cells[y * width + x]
            else {
                continue;
            };
            let neighbor = |x: usize, y: usize| {
                if x >= width || y >= height {
                    return None;
                }
                match cells[y * width + x] {
                    Cell::Rule {
                        horizontal,
                        vertical,
                        paint: other,
                    } if other == paint => Some((horizontal, vertical)),
                    _ => None,
                }
            };
            let left = x
                .checked_sub(1)
                .and_then(|x| neighbor(x, y))
                .is_some_and(|(h, _)| h || horizontal);
            let right = neighbor(x + 1, y).is_some_and(|(h, _)| h || horizontal);
            let up = y
                .checked_sub(1)
                .and_then(|y| neighbor(x, y))
                .is_some_and(|(_, v)| v || vertical);
            let down = neighbor(x, y + 1).is_some_and(|(_, v)| v || vertical);
            let bits =
                u8::from(left) | (u8::from(right) * 2) | (u8::from(up) * 4) | (u8::from(down) * 8);
            let character = match bits {
                10 => '┌',
                9 => '┐',
                6 => '└',
                5 => '┘',
                11 => '┬',
                7 => '┴',
                14 => '├',
                13 => '┤',
                15 => '┼',
                4 | 8 | 12 => '│',
                _ if horizontal => '─',
                _ => '│',
            };
            runs.push(plain_run(x, y, character, paint)?);
        }
    }
    Ok(())
}

pub(super) fn delimiter(character: char, row: usize, height: usize) -> char {
    if height == 1 {
        return character;
    }
    let (top, middle, bottom) = match character {
        '(' => ('⎛', '⎜', '⎝'),
        ')' => ('⎞', '⎟', '⎠'),
        '[' => ('⎡', '⎢', '⎣'),
        ']' => ('⎤', '⎥', '⎦'),
        '{' => ('⎧', '⎪', '⎩'),
        '}' => ('⎫', '⎪', '⎭'),
        '|' => ('│', '│', '│'),
        '‖' => ('‖', '‖', '‖'),
        '⟨' => ('╱', '│', '╲'),
        '⟩' => ('╲', '│', '╱'),
        other => return other,
    };
    if row == 0 {
        top
    } else if row + 1 == height {
        bottom
    } else if row == height / 2 && character == '{' {
        '⎨'
    } else if row == height / 2 && character == '}' {
        '⎬'
    } else {
        middle
    }
}
