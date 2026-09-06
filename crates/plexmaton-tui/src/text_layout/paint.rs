//! Retained style intent. Palette resolution cannot parse text or change its geometry.
use std::borrow::Cow;

use ratatui::{style::Modifier, text};
use serde::{Deserialize, Serialize};
use unicode_width::UnicodeWidthStr;

use crate::{Palette, Role, state::EntryAppearance, theme::MarkdownStyles};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) enum MarkdownRole {
    Heading1,
    Heading2,
    Heading3,
    InlineCode,
    Code,
    Link,
    Quote,
    Marker,
    TaskMarker,
    Guide,
    Rule,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
enum Layer {
    Workspace(Role),
    Markdown(MarkdownRole),
    Add(#[serde(with = "modifier")] Modifier),
}

/// Ordered patches preserve nested modifiers, including arbitrary user role assignments.
/// A simple workspace/content role allocates nothing; only composed styles retain extra layers.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct Paint {
    base: Option<Layer>,
    patches: Vec<Layer>,
}

impl From<Role> for Paint {
    fn from(role: Role) -> Self {
        Self {
            base: Some(Layer::Workspace(role)),
            patches: Vec::new(),
        }
    }
}

impl From<MarkdownRole> for Paint {
    fn from(role: MarkdownRole) -> Self {
        Self {
            base: Some(Layer::Markdown(role)),
            patches: Vec::new(),
        }
    }
}

impl Paint {
    pub(crate) fn patch(mut self, other: Self) -> Self {
        if self.base.is_none() && self.patches.is_empty() {
            return other;
        }
        self.patches.extend(other.base);
        self.patches.extend(other.patches);
        self
    }

    pub(crate) fn add_modifier(mut self, modifier: Modifier) -> Self {
        if let Some(Layer::Add(previous)) = self.patches.last_mut() {
            *previous |= modifier;
        } else {
            self.patches.push(Layer::Add(modifier));
        }
        self
    }

    pub(crate) fn allocation_bytes(&self) -> usize {
        self.patches.capacity() * size_of::<Layer>()
    }

    pub(crate) fn is_bounded(&self) -> bool {
        // Above the parser's maximum composed nesting, but finite for decoded worker data.
        self.patches.len() <= 256
    }

    pub(crate) fn resolve(&self, colors: &Colors<'_>) -> ratatui::style::Style {
        self.base.iter().chain(&self.patches).fold(
            ratatui::style::Style::default(),
            |style, layer| match layer {
                Layer::Workspace(role) => style.patch(colors.palette.style(*role)),
                Layer::Markdown(role) => style.patch(colors.markdown(*role)),
                Layer::Add(modifier) => style.add_modifier(*modifier),
            },
        )
    }
}

/// One resolution table per paint, not one palette assignment per grapheme.
pub(crate) struct Colors<'a> {
    palette: &'a Palette,
    markdown: MarkdownStyles,
}

impl<'a> Colors<'a> {
    pub(crate) fn new(palette: &'a Palette) -> Self {
        Self {
            palette,
            markdown: palette.markdown_styles(),
        }
    }

    fn markdown(&self, role: MarkdownRole) -> ratatui::style::Style {
        match role {
            MarkdownRole::Heading1 => self.markdown.headings[0],
            MarkdownRole::Heading2 => self.markdown.headings[1],
            MarkdownRole::Heading3 => self.markdown.headings[2],
            MarkdownRole::InlineCode => self.markdown.inline_code,
            MarkdownRole::Code => self.markdown.code,
            MarkdownRole::Link => self.markdown.link,
            MarkdownRole::Quote => self.markdown.quote,
            MarkdownRole::Marker => self.markdown.marker,
            MarkdownRole::TaskMarker => self.markdown.task_marker,
            MarkdownRole::Guide => self.markdown.guide,
            MarkdownRole::Rule => self.markdown.rule,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct Span {
    pub content: Cow<'static, str>,
    pub style: Paint,
}

impl Span {
    pub(crate) fn styled(content: impl Into<Cow<'static, str>>, style: impl Into<Paint>) -> Self {
        Self {
            content: content.into(),
            style: style.into(),
        }
    }

    pub(crate) fn raw(content: impl Into<Cow<'static, str>>) -> Self {
        Self::styled(content, Paint::default())
    }
}

/// Interaction treatment is retained geometry/meaning, not a selected/hovered state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) enum Treatment {
    #[default]
    Content,
    SelectionWidth(usize),
    ToolHeading,
    Diff,
}

/// Prepared rows are not widgets: only text, style intent and a conversion at the paint boundary.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct Line {
    pub spans: Vec<Span>,
    pub style: Paint,
    pub treatment: Treatment,
}

impl From<Vec<Span>> for Line {
    fn from(spans: Vec<Span>) -> Self {
        Self {
            spans,
            style: Paint::default(),
            treatment: Treatment::default(),
        }
    }
}

impl std::fmt::Display for Line {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for span in &self.spans {
            f.write_str(&span.content)?;
        }
        Ok(())
    }
}

impl Line {
    pub(crate) fn allocation_bytes(&self) -> usize {
        self.style.allocation_bytes()
            + self.spans.capacity() * size_of::<Span>()
            + self
                .spans
                .iter()
                .map(|span| {
                    span.style.allocation_bytes()
                        + match &span.content {
                            Cow::Owned(text) => text.capacity(),
                            Cow::Borrowed(_) => 0,
                        }
                })
                .sum::<usize>()
    }

    pub(crate) fn styled(content: impl Into<Cow<'static, str>>, style: impl Into<Paint>) -> Self {
        Self {
            spans: vec![Span::raw(content)],
            style: style.into(),
            treatment: Treatment::default(),
        }
    }

    pub(crate) fn width(&self) -> usize {
        self.spans.iter().map(|span| span.content.width()).sum()
    }

    pub(crate) fn is_bounded(&self, width: usize) -> bool {
        self.width() <= width
            && self.style.is_bounded()
            && self.spans.iter().all(|span| span.style.is_bounded())
            && !matches!(self.treatment, Treatment::SelectionWidth(reserved) if reserved > width)
    }

    pub(crate) fn paint_ref(&self, colors: &Colors<'_>) -> text::Line<'static> {
        text::Line::from(
            self.spans
                .iter()
                .map(|span| text::Span::styled(span.content.clone(), span.style.resolve(colors)))
                .collect::<Vec<_>>(),
        )
        .style(self.style.resolve(colors))
    }

    pub(crate) fn paint_entry(
        &self,
        colors: &Colors<'_>,
        appearance: EntryAppearance,
    ) -> text::Line<'static> {
        if appearance.selected {
            let selection = colors.palette.style(Role::Selection);
            if self.treatment == Treatment::Diff {
                let mut line = self.paint_ref(colors);
                for span in &mut line.spans {
                    span.style = line.style.patch(span.style).patch(selection);
                }
                line.style = ratatui::style::Style::default();
                return line;
            }
            if self.spans.is_empty() && self.treatment == Treatment::Content {
                return text::Line::default();
            }
            let mut text = self.to_string();
            if let Treatment::SelectionWidth(width) = self.treatment {
                text.push_str(&" ".repeat(width.saturating_sub(text.width())));
            }
            text::Line::styled(text, selection)
        } else if appearance.hovered && self.treatment == Treatment::ToolHeading {
            text::Line::styled(self.to_string(), colors.palette.style(Role::Accent))
        } else {
            self.paint_ref(colors)
        }
    }
}

pub(crate) fn ranges(
    line: Line,
    width: usize,
    literal: bool,
) -> Vec<(Line, std::ops::Range<usize>)> {
    let mut text = String::new();
    let mut styles = Vec::new();
    for span in line.spans {
        text.push_str(&span.content);
        styles.push((text.len(), line.style.clone().patch(span.style)));
    }
    super::wrap::styled_ranges(&text, &styles, width, literal)
        .into_iter()
        .map(|(run, range)| {
            let mut row = match run {
                super::wrap::Runs::Text(spans) => Line::from(
                    spans
                        .into_iter()
                        .map(|(text, style)| Span::styled(text, style))
                        .collect::<Vec<_>>(),
                ),
                super::wrap::Runs::Replacement(style) => Line::styled("�", style),
            };
            row.treatment = line.treatment;
            (row, range)
        })
        .collect()
}

pub(crate) fn wrap(line: Line, width: usize, literal: bool) -> Vec<Line> {
    ranges(line, width, literal)
        .into_iter()
        .map(|(line, _)| line)
        .collect()
}

// The wire carries only admitted modifiers, not Ratatui Style or a dependency-wide serde feature.
mod modifier {
    use super::*;

    pub(super) fn serialize<S: serde::Serializer>(
        value: &Modifier,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        s.serialize_u16(value.bits())
    }

    pub(super) fn deserialize<'de, D: serde::Deserializer<'de>>(
        d: D,
    ) -> Result<Modifier, D::Error> {
        let bits = u16::deserialize(d)?;
        Modifier::from_bits(bits).ok_or_else(|| serde::de::Error::custom("unknown paint modifier"))
    }
}
