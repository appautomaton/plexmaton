//! The mark: a rounded-square frame around a round centre, drawn in braille (phase 04 stage 10).
//!
//! Each cell is a two-by-four dot matrix, so the mark is one small bitmap, the way Grok draws its
//! logo. Its width is chosen from the terminal's cell so the block comes out square, and every
//! dot is placed in the cell's own proportions, so a circle is round in any font. One moment of
//! it is a [`Moment`]; [`greeting`] is the timeline launch plays once. Presentation only.

use std::f32::consts::{FRAC_PI_4, PI, SQRT_2};

use ratatui::text::{Line, Span};

use crate::theme::Palette;

/// One terminal cell, in pixels, as the terminal reports it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CellSize {
    pub width: u16,
    pub height: u16,
}

/// A common monospace cell, 0.6 em by 1.32 em, for a terminal that reports none.
const MONOSPACE: CellSize = CellSize {
    width: 5,
    height: 11,
};

/// The mark's block for `rows` rows, made odd and at least three: the square width for `cell`
/// rounded to the nearest column and then up to an odd count, so the block is square or a little
/// wider and the mark, sized by its height, keeps a margin at each side. A common monospace cell
/// stands in when the terminal reports none.
#[must_use]
pub fn size(rows: u16, cell: Option<CellSize>) -> (u16, u16) {
    let rows = rows.max(3) | 1;
    let cell = known(cell);
    let tall = u32::from(rows) * u32::from(cell.height);
    let width = u32::from(cell.width);
    let columns = ((2 * tall + width) / (2 * width)) | 1;
    (u16::try_from(columns.clamp(3, 41)).unwrap_or(41), rows)
}

fn known(cell: Option<CellSize>) -> CellSize {
    cell.filter(|cell| cell.width > 0 && cell.height > 0)
        .unwrap_or(MONOSPACE)
}

/// One moment of the mark.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moment {
    /// The centre's size as a share of the block's half-side; zero draws no centre.
    pub core: f32,
    /// How square the centre is: 2 is a circle, 8 a square with softened corners.
    pub roundness: f32,
    /// How far the centre has turned, from upright (0) to a diamond (1).
    pub core_turn: f32,
    /// How far the frame has turned, likewise.
    pub frame_turn: f32,
    /// Where a passing sheen's band sits along the diagonal, bottom-left 0 to top-right 1.
    pub sheen: Option<f32>,
    /// How present the mark is: 0 gone, 1 full.
    pub level: f32,
}

impl Moment {
    /// The mark at rest: a round centre in an upright frame.
    pub const STILL: Self = Self {
        core: 0.32,
        roundness: 2.0,
        core_turn: 0.0,
        frame_turn: 0.0,
        sheen: None,
        level: 1.0,
    };
}

/// The motion clock's phases a second (MOT-1).
const PHASES_PER_SECOND: f32 = 15.0;
const FADE_IN: f32 = 0.25;
const PLAY: f32 = 2.0;
const FADE_OUT: f32 = 0.3;
/// Half the sheen's band, along the diagonal.
const BAND: f32 = 0.38;

fn ease(share: f32) -> f32 {
    0.5 - 0.5 * (PI * share.clamp(0.0, 1.0)).cos()
}

/// The greeting `phase` motion phases after it began, or `None` once it has left.
///
/// The frame fades in; the centre grows from a dot into a circle, becomes a rounded square and a
/// square, turns into a diamond while the frame turns with it, and shrinks away as the frame turns
/// back; a sheen passes once; frame and name fade out. About 2.5 s.
#[must_use]
pub fn greeting(phase: u16) -> Option<Moment> {
    let seconds = f32::from(phase) / PHASES_PER_SECOND;
    if seconds >= FADE_IN + PLAY + FADE_OUT {
        return None;
    }
    let level = if seconds < FADE_IN {
        seconds / FADE_IN
    } else if seconds > FADE_IN + PLAY {
        1.0 - (seconds - FADE_IN - PLAY) / FADE_OUT
    } else {
        1.0
    };
    let playing = (seconds - FADE_IN).clamp(0.0, PLAY);
    let share = playing / PLAY;
    let sheen_at = (playing - 0.6) / 0.8;
    let mut moment = centre_at(share);
    moment.sheen = (0.0..=1.0)
        .contains(&sheen_at)
        .then(|| -BAND + sheen_at * (1.0 + 2.0 * BAND));
    moment.level = level;
    if seconds >= FADE_IN + PLAY {
        moment.core = 0.0;
    }
    Some(moment)
}

/// The chosen cycle at `share` of its way through: grow, square, turn, shrink.
fn centre_at(share: f32) -> Moment {
    let (core, roundness, core_turn) = match share {
        p if p < 0.18 => (0.04 + 0.30 * ease(p / 0.18), 2.0, 0.0),
        p if p < 0.30 => (0.34, 2.0, 0.0),
        p if p < 0.48 => {
            let k = ease((p - 0.30) / 0.18);
            (0.34 - 0.04 * k, 2.0 + 6.0 * k, 0.0)
        }
        p if p < 0.60 => (0.30, 8.0, 0.0),
        p if p < 0.78 => {
            let k = ease((p - 0.60) / 0.18);
            (0.30 + 0.04 * k, 8.0, k)
        }
        p if p < 0.84 => (0.34, 8.0, 1.0),
        p => (0.34 - 0.30 * ease((p - 0.84) / 0.16), 8.0, 1.0),
    };
    let frame_turn = match share {
        p if p < 0.60 => 0.0,
        p if p < 0.78 => ease((p - 0.60) / 0.18),
        p if p < 0.84 => 1.0,
        p => 1.0 - ease((p - 0.84) / 0.16),
    };
    Moment {
        core,
        roundness,
        core_turn,
        frame_turn,
        sheen: None,
        level: 1.0,
    }
}

/// What one dot of the block belongs to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Dot {
    Empty,
    Frame,
    Core,
}

fn turned(x: f32, y: f32, turn: f32) -> (f32, f32) {
    let (sin, cos) = (turn * FRAC_PI_4).sin_cos();
    (x * cos + y * sin, -x * sin + y * cos)
}

fn rounded_box(x: f32, y: f32, half: f32, radius: f32) -> f32 {
    let (qx, qy) = (x.abs() - half + radius, y.abs() - half + radius);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius
}

/// The block as dots, `columns * 2` wide and `rows * 4` tall, in the cell's own proportions.
fn dots((columns, rows): (u16, u16), cell: Option<CellSize>, moment: Moment) -> Vec<Vec<Dot>> {
    let cell = known(cell);
    let (dot_width, dot_height) = (f32::from(cell.width) / 2.0, f32::from(cell.height) / 4.0);
    let (wide, tall) = (usize::from(columns) * 2, usize::from(rows) * 4);
    let half = (f32::from(columns) * f32::from(cell.width))
        .min(f32::from(rows) * f32::from(cell.height))
        / 2.0;
    let (centre_x, centre_y) = (
        wide as f32 * dot_width / 2.0,
        tall as f32 * dot_height / 2.0,
    );
    // A turned frame reaches further along its diagonal, so it shrinks to stay inside the block.
    let spin = (moment.frame_turn * 2.0 * FRAC_PI_4).sin();
    let frame_half = 0.9 / (1.0 + (SQRT_2 - 1.0) * spin * spin);
    (0..tall)
        .map(|row| {
            (0..wide)
                .map(|column| {
                    let x = ((column as f32 + 0.5) * dot_width - centre_x) / half;
                    let y = ((row as f32 + 0.5) * dot_height - centre_y) / half;
                    let (fx, fy) = turned(x, y, moment.frame_turn);
                    if rounded_box(fx, fy, frame_half, frame_half.min(0.36)).abs() < 0.075 {
                        return Dot::Frame;
                    }
                    let (cx, cy) = turned(x, y, moment.core_turn);
                    let reach = ((cx / moment.core).abs().powf(moment.roundness)
                        + (cy / moment.core).abs().powf(moment.roundness))
                    .powf(1.0 / moment.roundness);
                    if moment.core > 0.0 && reach < 1.0 {
                        Dot::Core
                    } else {
                        Dot::Empty
                    }
                })
                .collect()
        })
        .collect()
}

/// The braille bit of each dot in a cell, by row and then column.
const BRAILLE: [[u32; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];

/// The mark at `moment` as rows of braille cells, coloured by the palette.
#[must_use]
pub fn lines(
    block: (u16, u16),
    cell: Option<CellSize>,
    moment: Moment,
    palette: &Palette,
) -> Vec<Line<'static>> {
    let dots = dots(block, cell, moment);
    let (columns, rows) = (usize::from(block.0), usize::from(block.1));
    (0..rows)
        .map(|row| {
            let spans: Vec<Span<'static>> = (0..columns)
                .map(|column| {
                    let (mut bits, mut frame, mut core) = (0, 0, 0);
                    for (dy, row_bits) in BRAILLE.iter().enumerate() {
                        for (dx, bit) in row_bits.iter().enumerate() {
                            match dots[row * 4 + dy][column * 2 + dx] {
                                Dot::Empty => {}
                                Dot::Frame => {
                                    bits |= bit;
                                    frame += 1;
                                }
                                Dot::Core => {
                                    bits |= bit;
                                    core += 1;
                                }
                            }
                        }
                    }
                    if bits == 0 {
                        return Span::raw(" ");
                    }
                    let glyph = char::from_u32(0x2800 + bits).unwrap_or(' ');
                    let style = if frame >= core {
                        let diagonal = (column + rows - 1 - row) as f32 / (columns + rows) as f32;
                        palette.mark_frame_at(moment.level, shine(moment.sheen, diagonal))
                    } else {
                        palette.mark_core_at(moment.level)
                    };
                    Span::styled(glyph.to_string(), style)
                })
                .collect();
            Line::from(spans)
        })
        .collect()
}

/// The greeting at `moment` for a conversation `rows` tall and `width` wide: the mark centred
/// with the product's name beneath, seven rows where there is room and five or three where there
/// is less, or nothing where not even three fit.
pub(crate) fn greeting_lines(
    moment: Moment,
    cell: Option<CellSize>,
    rows: u16,
    width: u16,
    palette: &Palette,
) -> Vec<Line<'static>> {
    let Some(block) = [7, 5, 3]
        .into_iter()
        .map(|tall| size(tall, cell))
        .find(|&(columns, tall)| tall + 2 <= rows && columns <= width)
    else {
        return Vec::new();
    };
    let mut drawn = vec![Line::default(); usize::from((rows - block.1 - 2) / 2)];
    drawn.extend(
        lines(block, cell, moment, palette)
            .into_iter()
            .map(Line::centered),
    );
    drawn.push(Line::default());
    drawn.push(Line::styled("Plexmaton", palette.mark_name_at(moment.level)).centered());
    drawn
}

/// How much a passing sheen at `band` lights a cell at `diagonal`: a raised cosine, as Grok's.
fn shine(band: Option<f32>, diagonal: f32) -> f32 {
    band.map_or(0.0, |band| {
        let distance = (diagonal - band).abs();
        if distance < BAND {
            0.55 * 0.5 * (1.0 + (PI * distance / BAND).cos())
        } else {
            0.0
        }
    })
}

#[cfg(test)]
mod tests {
    use unicode_width::UnicodeWidthStr;

    use super::{CellSize, Dot, Moment, dots, greeting, lines, size};
    use crate::theme::Palette;

    /// The block is square for the cell the terminal reports, as measured in the spike: a
    /// Sarasa Term cell of 8 by 23 pixels gives 9×3 and 15×5.
    #[test]
    fn the_block_is_nearest_square_for_the_reported_cell() {
        let sarasa = Some(CellSize {
            width: 8,
            height: 23,
        });
        assert_eq!(size(3, sarasa), (9, 3));
        assert_eq!(size(5, sarasa), (15, 5));
    }

    /// With no report, a common monospace cell stands in: 7×3, 11×5 and 15×7.
    #[test]
    fn a_terminal_that_reports_no_cell_gets_monospace_proportions() {
        assert_eq!(size(3, None), (7, 3));
        assert_eq!(size(5, None), (11, 5));
        assert_eq!(size(7, None), (15, 7));
        let nothing = Some(CellSize {
            width: 0,
            height: 0,
        });
        assert_eq!(size(5, nothing), (11, 5), "a zero report is no report");
    }

    /// At rest the mark is symmetric about its centre on both axes, so it sits where it is put.
    #[test]
    fn the_still_mark_is_symmetric_about_its_centre() {
        let cell = Some(CellSize {
            width: 9,
            height: 20,
        });
        for rows in [5, 7] {
            let grid = dots(size(rows, cell), cell, Moment::STILL);
            let (tall, wide) = (grid.len(), grid[0].len());
            for row in 0..tall {
                for column in 0..wide {
                    assert_eq!(grid[row][column], grid[row][wide - 1 - column]);
                    assert_eq!(grid[row][column], grid[tall - 1 - row][column]);
                }
            }
            let flat = grid.iter().flatten();
            assert!(flat.clone().any(|dot| *dot == Dot::Frame));
            assert!(flat.clone().any(|dot| *dot == Dot::Core));
        }
    }

    /// Every cell is a braille pattern or a space, one cell wide (MOT-2), and every row is the
    /// block's width.
    #[test]
    fn the_mark_is_braille_one_cell_a_glyph() {
        let palette = Palette::default();
        let block = size(7, None);
        for phase in 0..40 {
            let Some(moment) = greeting(phase) else {
                continue;
            };
            for line in lines(block, None, moment, &palette) {
                assert_eq!(line.width(), usize::from(block.0));
                for span in &line.spans {
                    assert_eq!(span.content.width(), 1);
                    let glyph = span.content.chars().next().unwrap_or(' ');
                    assert!(glyph == ' ' || ('\u{2800}'..='\u{28FF}').contains(&glyph));
                }
            }
        }
    }

    /// The greeting fades in from nothing, starts its centre as a dot, reaches the whole circle,
    /// turns, ends with no centre, and leaves after about two and a half seconds.
    #[test]
    fn the_greeting_grows_from_a_dot_and_leaves() {
        let first = greeting(0).unwrap_or(Moment::STILL);
        assert!(first.level < 0.01, "it fades in from nothing");
        let dot = greeting(4).unwrap_or(Moment::STILL);
        assert!(dot.core < 0.1, "the centre starts as a dot: {dot:?}");
        let moments: Vec<Moment> = (0..=38).filter_map(greeting).collect();
        assert_eq!(moments.len(), 39, "every phase of the 2.55 s has a moment");
        assert!(moments.iter().any(|moment| moment.core > 0.33));
        assert!(moments.iter().any(|moment| moment.frame_turn > 0.99));
        assert!(moments.iter().any(|moment| moment.sheen.is_some()));
        assert_eq!(moments.last().map(|moment| moment.core), Some(0.0));
        assert_eq!(greeting(39), None, "then it has left");
        assert_eq!(greeting(u16::MAX), None);
    }
}
