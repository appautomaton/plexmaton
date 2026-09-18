//! What may be formatted at all, and what a refusal is called when it may not.
//!
//! Admission runs before any parser and decides nothing about layout. A refusal named here is the
//! entry's whole presentation falling back to literal source, so the reasons stay few and each one
//! names a bound the reader can act on. Budgets that cost only part of an entry — a formula's
//! geometry, a code block's highlighting — are not refusals and do not belong in [`PlainReason`].

pub(crate) const MAX_SOURCE_BYTES: usize = 128 * 1024;

/// Conservative admission only, never Markdown parsing: any possible syntax takes the parser.
pub(crate) fn may_format(source: &str) -> bool {
    source.len() > MAX_SOURCE_BYTES
        || source.chars().next().is_some_and(|c| {
            c.is_whitespace() || c.is_ascii_digit() || matches!(c, '-' | '+' | '=')
        })
        || source.chars().any(|c| {
            c.is_control()
                || matches!(
                    c,
                    '*' | '_' | '`' | '[' | ']' | '<' | '>' | '\\' | '&' | '#' | '|' | '~' | '$'
                )
        })
}

pub(crate) fn inert(source: &str) -> String {
    let mut text = String::with_capacity(source.len());
    for c in source.chars() {
        match c {
            '\t' => text.push_str("    "),
            '\n' => text.push('\n'),
            c if c.is_control() => text.push('\u{fffd}'),
            c => text.push(c),
        }
    }
    text
}

/// Why an entry is shown as literal source instead of as a formatted document.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlainReason {
    Size,
    Complexity,
}

impl PlainReason {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Size => "Markdown size limit · showing source",
            Self::Complexity => "Markdown layout limit · showing source",
        }
    }
}
