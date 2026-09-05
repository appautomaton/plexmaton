use crate::{Limit, MAX_SOURCE_BYTES, MathError, MathMode};
use std::ops::Range;

pub(super) struct FormulaSource {
    original: Box<str>,
    body: Range<usize>,
    mode: MathMode,
}

impl FormulaSource {
    pub(super) fn new(source: &str) -> Result<Self, MathError> {
        if source.len() > MAX_SOURCE_BYTES {
            return Err(MathError::Limited(Limit::SourceBytes));
        }
        if source
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        {
            return Err(MathError::Controls);
        }
        let (open, close, mode) = if source.starts_with("$$") {
            ("$$", "$$", MathMode::Display)
        } else if source.starts_with('$') {
            ("$", "$", MathMode::Inline)
        } else if source.starts_with(r"\[") {
            (r"\[", r"\]", MathMode::Display)
        } else if source.starts_with(r"\(") {
            (r"\(", r"\)", MathMode::Inline)
        } else {
            return Err(MathError::Delimiters);
        };
        if source.len() < open.len() + close.len() || !source.ends_with(close) {
            return Err(MathError::Delimiters);
        }
        let body = open.len()..source.len() - close.len();
        if source[body.clone()].trim().is_empty() {
            return Err(MathError::Empty);
        }
        Ok(Self {
            original: source.into(),
            body,
            mode,
        })
    }

    pub(super) fn body(&self) -> &str {
        &self.original[self.body.clone()]
    }
    pub(super) fn original(&self) -> &str {
        &self.original
    }
    pub(super) const fn mode(&self) -> MathMode {
        self.mode
    }
}
