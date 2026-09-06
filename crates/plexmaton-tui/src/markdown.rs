//! CommonMark is presentation only. No rendered text replaces the retained semantic source.
#[cfg(test)]
use crate::Palette;
use crate::Role;
use crate::text_layout::paint::{Line, MarkdownRole, Paint as Style, Span};
use crate::text_layout::{Layout, paint as wrap};
use crate::{math::MathPresentation, text_layout::math::Atom};
use pulldown_cmark::{CodeBlockKind, Event, Tag, TagEnd};
use ratatui::style::Modifier;
use unicode_width::UnicodeWidthStr;

mod syntax;
mod table;
#[cfg(test)]
mod tests;

pub(crate) const MAX_SOURCE_BYTES: usize = 128 * 1024;
pub(crate) const MAX_LINES: usize = 8192;
const MAX_EVENTS: usize = 32_768;
const MAX_DEPTH: usize = 32;
const MAX_RENDERED_BYTES: usize = 512 * 1024;

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
            c if c.is_control() => text.push('�'),
            c => text.push(c),
        }
    }
    text
}

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

#[cfg(test)]
pub(crate) fn render(
    source: &str,
    palette: &Palette,
    width: usize,
) -> Result<Vec<ratatui::text::Line<'static>>, PlainReason> {
    render_layout(source, width, MathPresentation::Native, Completion::Final)
        .map(|layout| layout.painted_entry(palette, crate::state::EntryAppearance::default()))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Completion {
    Streaming,
    Final,
}

pub(crate) fn render_layout(
    source: &str,
    width: usize,
    math: MathPresentation,
    completion: Completion,
) -> Result<Layout, PlainReason> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(PlainReason::Size);
    }
    if width == 0 {
        return Ok(Layout::default());
    }
    if width > 512 {
        return Err(PlainReason::Complexity);
    }
    let source = syntax::Source::new(source)?;
    let events: Vec<_> = source
        .events()
        .take(MAX_EVENTS + 1)
        .collect::<Result<_, _>>()?;
    if events.len() > MAX_EVENTS {
        return Err(PlainReason::Complexity);
    }
    render_events(events, width, math, completion)
}

fn render_events(
    events: Vec<Event<'_>>,
    width: usize,
    math: MathPresentation,
    completion: Completion,
) -> Result<Layout, PlainReason> {
    let mut out = Renderer::new(width, math, completion);
    let mut events = events.into_iter();
    while let Some(event) = events.next() {
        match event {
            Event::Start(Tag::Table(alignment)) => {
                out.flush(false)?;
                let mut body = Vec::new();
                for event in events.by_ref() {
                    if event == Event::End(TagEnd::Table) {
                        break;
                    }
                    body.push(event);
                }
                let prefix = out.prefix();
                let available = width
                    .checked_sub(prefix.width())
                    .filter(|w| *w > 0)
                    .ok_or(PlainReason::Complexity)?;
                let layout = table::render(body, alignment, available, math, completion)?;
                out.layout.append(layout, &prefix, out.prefix_style());
                out.check()?;
                out.blank()?;
            }
            Event::Start(tag) => out.start(tag)?,
            Event::End(tag) => out.end(tag)?,
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                out.text(&text, out.style.clone())?
            }
            Event::Code(text) => out.text(
                &text,
                out.style.clone().patch(MarkdownRole::InlineCode.into()),
            )?,
            Event::SoftBreak | Event::HardBreak => out.flush(true)?,
            Event::Rule => {
                out.flush(false)?;
                out.adornment(
                    &"─".repeat(width.saturating_sub(out.prefix().width()).min(48)),
                    MarkdownRole::Rule.into(),
                )?;
                out.blank()?;
            }
            Event::TaskListMarker(checked) => out.text(
                if checked { "[x] " } else { "[ ] " },
                MarkdownRole::TaskMarker.into(),
            )?,
            Event::InlineMath(source) => out.math(&source, false)?,
            Event::DisplayMath(source) => out.math(&source, true)?,
            Event::FootnoteReference(_) => {
                return Err(PlainReason::Complexity);
            }
        }
    }
    out.flush(false)?;
    out.layout.finish();
    Ok(out.layout)
}

enum Prefix {
    Quote,
    Item(String),
    Indent(usize),
    Code,
}
struct Renderer {
    width: usize,
    layout: Layout,
    current: Vec<Span>,
    atoms: Vec<Atom>,
    math: MathPresentation,
    completion: Completion,
    math_bytes: usize,
    style: Style,
    styles: Vec<Style>,
    prefixes: Vec<Prefix>,
    lists: Vec<Option<u64>>,
    links: Vec<String>,
    code_depth: usize,
    checked_rows: usize,
    bytes: usize,
}

impl Renderer {
    fn new(width: usize, math: MathPresentation, completion: Completion) -> Self {
        Self {
            width,
            layout: Layout::default(),
            current: Vec::new(),
            atoms: Vec::new(),
            math,
            completion,
            math_bytes: 0,
            style: Role::Body.into(),
            styles: Vec::new(),
            prefixes: Vec::new(),
            lists: Vec::new(),
            links: Vec::new(),
            code_depth: 0,
            bytes: 0,
            checked_rows: 0,
        }
    }
    fn prefix(&self) -> String {
        self.prefixes
            .iter()
            .map(|p| match p {
                Prefix::Quote => "│ ".into(),
                Prefix::Item(marker) => marker.clone(),
                Prefix::Indent(n) => " ".repeat(*n),
                Prefix::Code => "│ ".into(),
            })
            .collect()
    }
    fn prefix_style(&self) -> Style {
        if self
            .prefixes
            .iter()
            .any(|prefix| matches!(prefix, Prefix::Item(_) | Prefix::Indent(_)))
        {
            MarkdownRole::Marker.into()
        } else {
            MarkdownRole::Guide.into()
        }
    }
    fn row(&mut self, line: Line) -> Result<(), PlainReason> {
        self.layout.decoration(line);
        self.check()
    }
    fn check(&mut self) -> Result<(), PlainReason> {
        self.bytes += self.layout.lines[self.checked_rows..]
            .iter()
            .flat_map(|line| &line.spans)
            .map(|span| span.content.len())
            .sum::<usize>();
        self.checked_rows = self.layout.lines.len();
        if self.bytes > MAX_RENDERED_BYTES
            || self.layout.lines.len() > MAX_LINES
            || self.layout.text.len() > MAX_RENDERED_BYTES
            || self.layout.formulas.len() > crate::text_layout::math::MAX_FORMULAS
        {
            return Err(PlainReason::Complexity);
        }
        Ok(())
    }
    fn blank(&mut self) -> Result<(), PlainReason> {
        self.layout.blank();
        self.check()
    }
    fn adornment(&mut self, text: &str, style: Style) -> Result<(), PlainReason> {
        self.flush(false)?;
        let prefix = self.prefix();
        let width = self
            .width
            .checked_sub(prefix.width())
            .filter(|w| *w > 0)
            .ok_or(PlainReason::Complexity)?;
        for mut row in wrap::wrap(Line::styled(inert(text), style), width, true) {
            if !prefix.is_empty() {
                row.spans
                    .insert(0, Span::styled(prefix.clone(), self.prefix_style()));
            }
            self.row(row)?;
        }
        Ok(())
    }
    fn flush(&mut self, empty: bool) -> Result<(), PlainReason> {
        if self.current.is_empty() && !empty {
            return Ok(());
        }
        let prefix = self.prefix();
        let width = self
            .width
            .checked_sub(prefix.width())
            .filter(|w| *w > 0)
            .ok_or(PlainReason::Complexity)?;
        let line = Line::from(std::mem::take(&mut self.current));
        let start = self.layout.lines.len();
        if self.atoms.is_empty() {
            self.layout.logical(
                line,
                width,
                self.code_depth > 0,
                &prefix,
                self.prefix_style(),
            );
        } else {
            self.layout.math_logical(
                line,
                std::mem::take(&mut self.atoms),
                width,
                &prefix,
                self.prefix_style(),
            )?;
        }
        // A list marker belongs only to its first visual row. Continuations keep its width.
        let mut continuation = false;
        for p in &mut self.prefixes {
            if let Prefix::Item(marker) = p {
                *p = Prefix::Indent(marker.width());
                continuation = true;
            }
        }
        if continuation {
            let prefix = self.prefix();
            for row in self.layout.lines.iter_mut().skip(start + 1) {
                if let Some(span) = row.spans.first_mut() {
                    span.content = prefix.clone().into();
                }
            }
        }
        self.check()
    }
    fn text(&mut self, text: &str, style: Style) -> Result<(), PlainReason> {
        for part in text.split_inclusive('\n') {
            let text = inert(part.trim_end_matches('\n'));
            if !text.is_empty() {
                self.current.push(Span::styled(text, style.clone()));
            }
            if part.ends_with('\n') {
                self.flush(true)?;
            }
        }
        Ok(())
    }

    fn math(&mut self, source: &str, display: bool) -> Result<(), PlainReason> {
        if self.layout.formulas.len() + self.atoms.len() >= crate::text_layout::math::MAX_FORMULAS {
            return Err(PlainReason::Complexity);
        }
        if display {
            self.flush(false)?;
        }
        let width = self
            .width
            .checked_sub(self.prefix().width())
            .filter(|width| *width > 0)
            .ok_or(PlainReason::Complexity)?;
        let atom = Atom::prepare(
            source,
            self.current.len(),
            width,
            self.math,
            self.completion,
        )?;
        self.math_bytes += atom.allocation_bytes();
        if self.math_bytes > crate::preparation::MAX_PREPARED_BYTES {
            return Err(PlainReason::Complexity);
        }
        self.atoms.push(atom);
        self.current
            .push(Span::styled(source.to_owned(), self.style.clone()));
        if display {
            self.flush(false)?;
        }
        Ok(())
    }
    fn start(&mut self, tag: Tag<'_>) -> Result<(), PlainReason> {
        if self.styles.len() >= MAX_DEPTH {
            return Err(PlainReason::Complexity);
        }
        self.styles.push(self.style.clone());
        match tag {
            Tag::Paragraph | Tag::HtmlBlock => self.flush(false)?,
            Tag::Heading { level, .. } => {
                self.flush(false)?;
                self.style = match level {
                    pulldown_cmark::HeadingLevel::H1 => MarkdownRole::Heading1,
                    pulldown_cmark::HeadingLevel::H2 => MarkdownRole::Heading2,
                    _ => MarkdownRole::Heading3,
                }
                .into();
            }
            Tag::Emphasis => self.style = self.style.clone().add_modifier(Modifier::ITALIC),
            Tag::Strong => self.style = self.style.clone().add_modifier(Modifier::BOLD),
            Tag::Strikethrough => {
                self.style = self.style.clone().add_modifier(Modifier::CROSSED_OUT)
            }
            Tag::BlockQuote(_) => {
                self.flush(false)?;
                self.prefixes.push(Prefix::Quote);
                self.style = MarkdownRole::Quote.into();
            }
            Tag::List(start) => {
                self.flush(false)?;
                self.lists.push(start);
            }
            Tag::Item => {
                self.flush(false)?;
                let marker = match self.lists.last_mut() {
                    Some(Some(n)) => {
                        let label = format!("{n}. ");
                        *n = n.saturating_add(1);
                        label
                    }
                    _ => "• ".into(),
                };
                self.prefixes.push(Prefix::Item(marker));
            }
            Tag::CodeBlock(kind) => {
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
                self.adornment(&format!("┌ {language}"), MarkdownRole::Guide.into())?;
                self.prefixes.push(Prefix::Code);
                self.code_depth += 1;
                self.style = MarkdownRole::Code.into();
            }
            Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                self.links.push(dest_url.into_string());
                self.style = self.style.clone().patch(MarkdownRole::Link.into());
            }
            _ => return Err(PlainReason::Complexity),
        }
        Ok(())
    }
    fn end(&mut self, tag: TagEnd) -> Result<(), PlainReason> {
        match tag {
            TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::HtmlBlock => {
                self.flush(false)?;
                if self.lists.is_empty() {
                    self.blank()?;
                }
            }
            TagEnd::Item => {
                self.flush(false)?;
                self.prefixes.pop();
            }
            TagEnd::List(_) => {
                self.flush(false)?;
                self.lists.pop();
                if self.lists.is_empty() {
                    self.blank()?;
                }
            }
            TagEnd::BlockQuote(_) => {
                self.flush(false)?;
                self.prefixes.pop();
                self.blank()?;
            }
            TagEnd::CodeBlock => {
                self.flush(false)?;
                self.prefixes.pop();
                self.code_depth = self.code_depth.saturating_sub(1);
                self.adornment("└", MarkdownRole::Guide.into())?;
                self.blank()?;
            }
            TagEnd::Link | TagEnd::Image => {
                if let Some(url) = self.links.pop() {
                    self.text(&format!(" ({url})"), MarkdownRole::Guide.into())?;
                }
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {}
            _ => return Err(PlainReason::Complexity),
        }
        self.style = self.styles.pop().unwrap_or_else(|| Role::Body.into());
        Ok(())
    }
}
