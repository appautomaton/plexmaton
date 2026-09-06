//! Inline band composition, not formula layout: the engine's axis and reservations are unchanged.

use unicode_segmentation::UnicodeSegmentation as _;
use unicode_width::UnicodeWidthStr as _;

use super::*;

enum Piece {
    Text {
        span: Span,
        source: Range<usize>,
    },
    Formula {
        atom: Atom,
        source: Range<usize>,
        style: Paint,
    },
}

struct Placed {
    x: usize,
    y: usize,
    piece: Piece,
}

#[derive(Default)]
struct Band {
    x: usize,
    axis: usize,
    height: usize,
    pieces: Vec<Placed>,
}

struct Flow {
    width: usize,
    band: Band,
    layout: Layout,
}

pub(super) fn compose(line: Line, atoms: Vec<Atom>, width: usize) -> Result<Layout, PlainReason> {
    let mut flow = Flow {
        width,
        band: Band::default(),
        layout: Layout::default(),
    };
    let mut atoms = atoms.into_iter().peekable();
    for (index, span) in line.spans.into_iter().enumerate() {
        let start = flow.layout.text.len();
        flow.layout.text.push_str(&span.content);
        let source = start..flow.layout.text.len();
        if atoms.peek().is_some_and(|atom| atom.span == index) {
            let atom = atoms.next().expect("matched inline atom");
            let (x, y) = flow.reserve(atom.width, atom.axis, atom.height)?;
            flow.band.pieces.push(Placed {
                x,
                y,
                piece: Piece::Formula {
                    atom,
                    source,
                    style: span.style,
                },
            });
        } else {
            flow.text(span, start)?;
        }
    }
    flow.newline()?;
    flow.layout.text.push('\n');
    Ok(flow.layout)
}

impl Flow {
    fn reserve(
        &mut self,
        width: usize,
        axis: usize,
        height: usize,
    ) -> Result<(usize, usize), PlainReason> {
        if width > self.width {
            return Err(PlainReason::Complexity);
        }
        if self.band.x + width > self.width {
            self.newline()?;
        }
        if axis > self.band.axis {
            let shift = axis - self.band.axis;
            for placed in &mut self.band.pieces {
                placed.y += shift;
            }
            self.band.height += shift;
            self.band.axis = axis;
        }
        let top = self.band.axis - axis;
        self.band.height = self.band.height.max(top + height);
        let x = self.band.x;
        self.band.x += width;
        Ok((x, top))
    }

    fn text(&mut self, span: Span, start: usize) -> Result<(), PlainReason> {
        for (offset, word) in span.content.split_word_bound_indices() {
            let width = word.width();
            if word.chars().all(char::is_whitespace) && self.band.x + width > self.width {
                continue;
            }
            if width <= self.width {
                self.word(
                    word,
                    start + offset..start + offset + word.len(),
                    &span.style,
                )?;
            } else {
                for (inside, grapheme) in word.grapheme_indices(true) {
                    self.word(
                        if grapheme.width() > self.width {
                            "�"
                        } else {
                            grapheme
                        },
                        start + offset + inside..start + offset + inside + grapheme.len(),
                        &span.style,
                    )?;
                }
            }
        }
        Ok(())
    }

    fn word(&mut self, text: &str, source: Range<usize>, style: &Paint) -> Result<(), PlainReason> {
        let (x, y) = self.reserve(text.width(), 0, 1)?;
        self.band.pieces.push(Placed {
            x,
            y,
            piece: Piece::Text {
                span: Span::styled(text.to_owned(), style.clone()),
                source,
            },
        });
        Ok(())
    }

    fn newline(&mut self) -> Result<(), PlainReason> {
        let band = std::mem::take(&mut self.band);
        if band.pieces.is_empty() {
            return Ok(());
        }
        if self.layout.lines.len() + band.height > crate::markdown::MAX_LINES {
            return Err(PlainReason::Complexity);
        }
        let origin = self.layout.lines.len();
        let mut rows = vec![Vec::<(usize, Line)>::new(); band.height];
        let mut fragments = vec![Vec::new(); band.height];
        for placed in band.pieces {
            match placed.piece {
                Piece::Text { span, source } => {
                    rows[placed.y].push((placed.x, Line::from(vec![span])));
                    fragments[placed.y].push(Fragment {
                        column: placed.x,
                        text: source,
                        kind: FragmentKind::Text,
                    });
                }
                Piece::Formula {
                    atom,
                    source,
                    style,
                } => {
                    for row in placed.y..placed.y + atom.height {
                        fragments[row].push(Fragment {
                            column: placed.x,
                            text: source.clone(),
                            kind: FragmentKind::Atomic {
                                columns: atom.width,
                            },
                        });
                        let mut line =
                            atom.lines.get(row - placed.y).cloned().unwrap_or_else(|| {
                                Line::from(vec![Span::raw(" ".repeat(atom.width))])
                            });
                        if line.width() < atom.width {
                            line.spans
                                .push(Span::raw(" ".repeat(atom.width - line.width())));
                        }
                        rows[row].push((placed.x, line));
                    }
                    self.layout.formulas.push(PlacedFormula {
                        column: placed.x,
                        row: origin + placed.y,
                        width: atom.width,
                        height: atom.height,
                        text: source,
                        content: atom.content,
                        style,
                    });
                }
            }
        }
        for (mut row, mut fragments) in rows.into_iter().zip(fragments) {
            row.sort_by_key(|(x, _)| *x);
            fragments.sort_by_key(|fragment| fragment.column);
            let mut line = Line::default();
            let mut column = 0;
            for (x, mut piece) in row {
                if x > column {
                    line.spans.push(Span::raw(" ".repeat(x - column)));
                }
                column = x + piece.width();
                for span in &mut piece.spans {
                    span.style = piece.style.clone().patch(span.style.clone());
                }
                line.spans.extend(piece.spans);
            }
            self.layout.lines.push(line);
            self.layout.rows.push(fragments);
        }
        Ok(())
    }
}
