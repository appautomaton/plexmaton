//! Review-only prose wrapping and inline composition; formula geometry comes solely from the crate.
use crate::corpus;
use plexmaton_math::{
    FontStyle, Formula, FormulaLayout, GlyphRun, Paint, TextScale, VerticalAlign,
};
use serde::Serialize;
use unicode_width::UnicodeWidthStr as _;

#[derive(Serialize)]
pub struct Document {
    pub source: String,
    pub width: u16,
    pub height: u16,
    pub runs: Vec<GlyphRun>,
    pub formulas: Vec<FormulaBounds>,
    pub pages: Vec<Page>,
}

#[derive(Serialize)]
pub struct FormulaBounds {
    pub start: usize,
    pub end: usize,
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[derive(Serialize)]
pub struct Page {
    pub start: u16,
    pub end: u16,
}

struct Line {
    x: u16,
    axis: u16,
    height: u16,
    runs: Vec<GlyphRun>,
    formulas: Vec<FormulaBounds>,
}

impl Default for Line {
    fn default() -> Self {
        Self {
            x: 0,
            axis: 0,
            height: 1,
            runs: Vec::new(),
            formulas: Vec::new(),
        }
    }
}

struct Flow {
    width: u16,
    y: u16,
    line: Line,
    runs: Vec<GlyphRun>,
    formulas: Vec<FormulaBounds>,
}

impl Flow {
    fn newline(&mut self) {
        let line = std::mem::take(&mut self.line);
        self.runs.extend(line.runs.into_iter().map(|mut run| {
            run.y += self.y;
            run
        }));
        self.formulas
            .extend(line.formulas.into_iter().map(|mut bounds| {
                bounds.y += self.y;
                bounds
            }));
        self.y += line.height;
    }

    fn reserve(&mut self, width: u16, axis: u16, height: u16) -> (u16, u16) {
        if self.line.x + width > self.width {
            self.newline();
        }
        if axis > self.line.axis {
            let shift = axis - self.line.axis;
            for run in &mut self.line.runs {
                run.y += shift;
            }
            for bounds in &mut self.line.formulas {
                bounds.y += shift;
            }
            self.line.height += shift;
            self.line.axis = axis;
        }
        let top = self.line.axis - axis;
        self.line.height = self.line.height.max(top + height);
        let x = self.line.x;
        self.line.x += width;
        (x, top)
    }

    fn text(&mut self, text: &str) -> Result<(), Box<dyn std::error::Error>> {
        // Retain Markdown markers in prose: this review does not implement a second Markdown UI.
        for token in text.split_inclusive(char::is_whitespace) {
            let word = token.trim_end_matches(char::is_whitespace);
            if !word.is_empty() {
                let width = u16::try_from(word.width())?;
                if width > self.width {
                    return Err("indivisible review prose exceeds width".into());
                }
                let (x, y) = self.reserve(width, 0, 1);
                self.line.runs.push(GlyphRun {
                    x,
                    y,
                    text: word.into(),
                    columns: width,
                    rows: 1,
                    scale: TextScale::Full,
                    align: VerticalAlign::Bottom,
                    style: FontStyle::Roman,
                    paint: Paint::Inherit,
                });
            }
            for space in token[word.len()..].chars() {
                if space == '\n' {
                    self.newline();
                } else if space != '\r' && self.line.x < self.width {
                    self.line.x += 1;
                }
            }
        }
        Ok(())
    }

    fn formula(&mut self, layout: &FormulaLayout, span: &corpus::Span) {
        if span.display && self.line.x != 0 {
            self.newline();
        }
        if span.display {
            self.line.x = (self.width - layout.width()) / 2;
        }
        let (x, y) = self.reserve(layout.width(), layout.axis(), layout.height());
        self.line
            .runs
            .extend(layout.runs().iter().cloned().map(|mut run| {
                run.x += x;
                run.y += y;
                run
            }));
        self.line.formulas.push(FormulaBounds {
            start: span.start,
            end: span.end,
            x,
            y,
            width: layout.width(),
            height: layout.height(),
        });
        // The following source newline ends a display formula, preserving the reply's spacing.
    }
}

pub fn document(
    fixture: &corpus::Reply,
    formulas: &[Formula],
    width: u16,
) -> Result<Document, Box<dyn std::error::Error>> {
    let mut flow = Flow {
        width: width - 4,
        y: 0,
        line: Line::default(),
        runs: Vec::new(),
        formulas: Vec::new(),
    };
    let mut cursor = 0;
    for (span, formula) in fixture.math.iter().zip(formulas) {
        flow.text(&fixture.text[cursor..span.start])?;
        flow.formula(&formula.layout(usize::from(width - 4))?, span);
        cursor = span.end;
    }
    flow.text(&fixture.text[cursor..])?;
    if flow.line.x != 0 {
        flow.newline();
    }
    let mut pages = Vec::new();
    let mut start = 0;
    while start < flow.y {
        let mut end = (start + 36).min(flow.y);
        // Page boundaries cannot bisect a formula or a shared multi-row inline band.
        loop {
            let revised = flow
                .formulas
                .iter()
                .filter(|bounds| bounds.y < end && bounds.y + bounds.height > end)
                .map(|bounds| bounds.y)
                .min()
                .unwrap_or(end);
            if revised == end {
                break;
            }
            end = revised;
        }
        if end <= start {
            return Err("one formula exceeds the review page".into());
        }
        pages.push(Page { start, end });
        start = end;
    }
    Ok(Document {
        source: fixture.text.clone(),
        width,
        height: flow.y,
        runs: flow.runs,
        formulas: flow.formulas,
        pages,
    })
}
