//! Bounded, palette-neutral code preparation. The owned preparation process ends CPU work.
use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, TagEnd};
use tree_sitter_highlight::{HighlightEvent, Highlighter};

use super::{PlainReason, Prefix, Renderer};
use crate::text_layout::paint::{MarkdownRole, Treatment};
use crate::theme::code::CodeRole;

mod languages;
#[cfg(test)]
mod tests;

pub(super) const MAX_CODE_BYTES: usize = 32 * 1024;
const MAX_CODE_EVENTS: usize = 32_768;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PlainCode {
    Unspecified,
    UnknownLanguage,
    Limit,
    Unavailable,
}

impl PlainCode {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Unspecified | Self::UnknownLanguage => "",
            Self::Limit => " · syntax limit; plain text",
            Self::Unavailable => " · syntax unavailable; plain text",
        }
    }
}

pub(super) struct Token {
    pub(super) bytes: Range<usize>,
    pub(super) role: CodeRole,
}

/// One renderer owns its parser; the compiled grammars it reads are process-wide constants.
#[derive(Default)]
pub(super) struct CodeHighlighter {
    pub(super) preparations: usize,
    engine: Option<Highlighter>,
}

impl CodeHighlighter {
    pub(super) fn highlight(
        &mut self,
        language: &str,
        source: &str,
    ) -> Result<Vec<Token>, PlainCode> {
        let language = languages::Language::from_info(language)?;
        if source.len() > MAX_CODE_BYTES {
            return Err(PlainCode::Limit);
        }
        let config = language.configuration()?;
        let engine = self.engine.get_or_insert_with(Highlighter::new);
        // Injections are deliberately disabled: a fence names one bundled grammar. PRE-2's
        // process owner bounds parser/query CPU and can kill computation, not merely its future.
        self.preparations += 1;
        let events = engine
            .highlight(config, source.as_bytes(), None, |_| None)
            .map_err(|_| PlainCode::Unavailable)?;
        collect_tokens(source, events)
    }
}

fn collect_tokens(
    source: &str,
    events: impl Iterator<Item = Result<HighlightEvent, tree_sitter_highlight::Error>>,
) -> Result<Vec<Token>, PlainCode> {
    let mut stack = Vec::new();
    let mut tokens: Vec<Token> = Vec::new();
    let mut consumed = 0;
    for (count, event) in events.enumerate() {
        if count >= MAX_CODE_EVENTS || stack.len() > 128 {
            return Err(PlainCode::Limit);
        }
        match event.map_err(|_| PlainCode::Unavailable)? {
            HighlightEvent::HighlightStart(highlight) => {
                stack.push(
                    languages::CAPTURES
                        .get(highlight.0)
                        .ok_or(PlainCode::Unavailable)?
                        .1,
                );
            }
            HighlightEvent::HighlightEnd => {
                stack.pop().ok_or(PlainCode::Unavailable)?;
            }
            HighlightEvent::Source { start, end } => {
                if start != consumed || end < start || source.get(start..end).is_none() {
                    return Err(PlainCode::Unavailable);
                }
                consumed = end;
                if start == end {
                    continue;
                }
                let role = stack.last().copied().unwrap_or(CodeRole::Text);
                if let Some(previous) = tokens.last_mut()
                    && previous.role == role
                {
                    previous.bytes.end = end;
                } else {
                    tokens.push(Token {
                        bytes: start..end,
                        role,
                    });
                }
            }
        }
    }
    if consumed != source.len() || !stack.is_empty() {
        return Err(PlainCode::Unavailable);
    }
    Ok(tokens)
}

impl Renderer {
    pub(super) fn code_block<'a>(
        &mut self,
        kind: CodeBlockKind<'a>,
        events: &mut impl Iterator<Item = (Event<'a>, Range<usize>)>,
    ) -> Result<usize, PlainReason> {
        self.flush(false)?;
        let language = match kind {
            CodeBlockKind::Fenced(info) => info
                .split_whitespace()
                .next()
                .unwrap_or("")
                .chars()
                .take(32)
                .collect::<String>(),
            CodeBlockKind::Indented => String::new(),
        };
        let mut source = String::new();
        let mut event_end = 0;
        for (event, range) in events.by_ref() {
            match event {
                Event::Text(text) => source.push_str(&text),
                Event::End(TagEnd::CodeBlock) => {
                    event_end = range.end;
                    break;
                }
                _ => return Err(PlainReason::Complexity),
            }
            if source.len() > super::MAX_SOURCE_BYTES {
                return Err(PlainReason::Size);
            }
        }
        let tokens = self.code_highlighter.highlight(&language, &source);
        let label = tokens.as_ref().err().map_or("", |reason| reason.label());
        self.adornment(&format!("┌ {language}{label}"), MarkdownRole::Guide.into())?;
        self.prefixes.push(Prefix::Code);
        self.code_depth += 1;
        let first_row = self.layout.lines.len();
        match tokens {
            Ok(tokens) => {
                for token in tokens {
                    self.text(&source[token.bytes], token.role.into())?;
                }
            }
            Err(_) => self.text(&source, MarkdownRole::Code.into())?,
        }
        self.flush(false)?;
        for row in &mut self.layout.lines[first_row..] {
            row.treatment = Treatment::MarkdownSelectionWidth(0);
        }
        self.prefixes.pop();
        self.code_depth = self.code_depth.saturating_sub(1);
        self.adornment("└", MarkdownRole::Guide.into())?;
        self.blank()?;
        Ok(event_end)
    }
}
