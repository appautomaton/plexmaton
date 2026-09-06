//! Formula rectangles compose with prose, but always map to complete delimited source.

use std::ops::Range;

use plexmaton_math::{Formula, MathError, NativeLayout};
use serde::{Deserialize, Serialize};

use super::{
    Fragment, FragmentKind, Layout,
    paint::{Line, Paint, Span},
};
use crate::{
    Role,
    markdown::PlainReason,
    math::{MathPresentation, MathUnavailable},
};

mod flow;
#[cfg(test)]
mod tests;

pub(crate) const MAX_FORMULAS: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) enum SourceReason {
    Terminal(MathUnavailable),
    Incomplete,
    Syntax,
    Unsupported,
    Capacity,
    Width { required: usize, available: usize },
}

impl SourceReason {
    fn label(&self) -> String {
        match self {
            Self::Terminal(reason) => reason.label().into(),
            Self::Incomplete => "Incomplete math · showing source".into(),
            Self::Syntax => "Math syntax refused · showing source".into(),
            Self::Unsupported => "Native math unavailable · showing source".into(),
            Self::Capacity => "Math preparation limit · showing source".into(),
            Self::Width {
                required,
                available,
            } => format!("Math needs {required} columns; {available} available · showing source"),
        }
    }
}

impl From<MathError> for SourceReason {
    fn from(error: MathError) -> Self {
        match error {
            MathError::Delimiters => Self::Incomplete,
            MathError::ParseRejected | MathError::Empty => Self::Syntax,
            MathError::Limited(_) => Self::Capacity,
            MathError::TooWide {
                required,
                available,
            } => Self::Width {
                required,
                available,
            },
            MathError::Controls | MathError::Unsupported(_) | MathError::Overlap => {
                Self::Unsupported
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) enum FormulaContent {
    Native(NativeLayout),
    Source(SourceReason),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct PlacedFormula {
    pub column: usize,
    pub row: usize,
    pub width: usize,
    pub height: usize,
    pub text: Range<usize>,
    pub content: FormulaContent,
    pub style: Paint,
}

impl PlacedFormula {
    pub(super) fn allocation_bytes(&self) -> usize {
        self.style.allocation_bytes()
            + match &self.content {
                FormulaContent::Native(layout) => layout.allocation_bytes(),
                FormulaContent::Source(_) => 0,
            }
    }
}

/// Transient inline composition keeps source in its original span; this is not retained state.
pub(crate) struct Atom {
    pub span: usize,
    width: usize,
    height: usize,
    axis: usize,
    content: FormulaContent,
    lines: Vec<Line>,
}

impl Atom {
    pub(crate) fn allocation_bytes(&self) -> usize {
        self.lines.capacity() * size_of::<Line>()
            + self.lines.iter().map(Line::allocation_bytes).sum::<usize>()
            + match &self.content {
                FormulaContent::Native(native) => native.allocation_bytes(),
                FormulaContent::Source(_) => 0,
            }
    }

    pub(crate) fn prepare(
        source: &str,
        span: usize,
        width: usize,
        math: MathPresentation,
    ) -> Result<Self, PlainReason> {
        let prepared = match math {
            MathPresentation::Native => Formula::parse(source)
                .and_then(|formula| formula.layout(width))
                .map(plexmaton_math::FormulaLayout::into_native)
                .map_err(SourceReason::from),
            MathPresentation::Source(reason) => Err(SourceReason::Terminal(reason)),
        };
        match prepared {
            Ok(native) => Ok(Self {
                span,
                width: usize::from(native.width()),
                height: usize::from(native.height()),
                axis: usize::from(native.axis()),
                content: FormulaContent::Native(native),
                lines: Vec::new(),
            }),
            Err(reason) => {
                let mut lines = Vec::new();
                let source = crate::markdown::inert(source);
                for (source, role) in [(reason.label(), Role::Muted), (source, Role::Body)] {
                    for line in source.split('\n') {
                        lines.extend(
                            super::paint::ranges(Line::styled(line.to_owned(), role), width, true)
                                .into_iter()
                                .map(|(line, _)| line),
                        );
                        if lines.len() > crate::markdown::MAX_LINES {
                            return Err(PlainReason::Complexity);
                        }
                    }
                }
                Ok(Self {
                    span,
                    width: lines.iter().map(Line::width).max().unwrap_or(1).max(1),
                    height: lines.len(),
                    axis: 0,
                    content: FormulaContent::Source(reason),
                    lines,
                })
            }
        }
    }
}

impl Layout {
    pub(crate) fn formulas_validate(&self, width: usize) -> bool {
        if self.formulas.len() > MAX_FORMULAS {
            return false;
        }
        let mut expected = 0;
        for formula in &self.formulas {
            if formula.width == 0
                || formula.height == 0
                || formula
                    .column
                    .checked_add(formula.width)
                    .is_none_or(|end| end > width)
                || formula
                    .row
                    .checked_add(formula.height)
                    .is_none_or(|end| end > self.rows.len())
                || formula.text.is_empty()
                || self.text.get(formula.text.clone()).is_none()
                || !formula.style.is_bounded()
            {
                return false;
            }
            if let FormulaContent::Native(native) = &formula.content
                && (usize::from(native.width()) != formula.width
                    || usize::from(native.height()) != formula.height)
            {
                return false;
            }
            for row in formula.row..formula.row + formula.height {
                if !self.rows[row].iter().any(|fragment| {
                    fragment.column == formula.column
                        && fragment.text == formula.text
                        && fragment.kind
                            == (FragmentKind::Atomic {
                                columns: formula.width,
                            })
                }) {
                    return false;
                }
            }
            expected += formula.height;
        }
        let mut actual = 0;
        for row in &self.rows {
            let mut right = 0;
            for fragment in row {
                if let FragmentKind::Atomic { columns } = fragment.kind {
                    if columns == 0
                        || fragment.column < right
                        || fragment
                            .column
                            .checked_add(columns)
                            .is_none_or(|end| end > width)
                    {
                        return false;
                    }
                    right = fragment.column + columns;
                    actual += 1;
                }
            }
        }
        actual == expected
    }

    pub(crate) fn math_logical(
        &mut self,
        line: Line,
        atoms: Vec<Atom>,
        width: usize,
        prefix: &str,
        style: Paint,
    ) -> Result<(), PlainReason> {
        let layout = flow::compose(line, atoms, width)?;
        self.append(layout, prefix, style);
        Ok(())
    }
}
