//! The transcript's own grammar: what one entry of a conversation looks like.
//!
//! Split from the rest of `content` because every other function there draws a list, a strip or a
//! placeholder from state the workspace owns, while these draw what a *producer* said. The rules
//! they answer to are `ui-ux.md` §transcript grammar, not the surface's.

use plexmaton_core::TranscriptRole;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::{
    TranscriptEntryView, TranscriptItemView, TranscriptTextKind,
    state::{EntryAppearance, wrap_line},
    surface::SurfaceId,
    theme::{Palette, Role},
};

use super::tool;
use crate::text_layout::Layout;

/// One transcript entry, as the logical lines a viewport measures and paints.
///
/// Per entry rather than per conversation, because both the height cache and the visible range are
/// expressed in entries: a frame that asks for one entry's rows must get exactly the rows that
/// entry contributes to the whole (TR-1). Tools stay one logical line in every lifecycle state;
/// wrapping that line at a narrow width loses no semantic content.
pub(crate) fn transcript_entry(
    entry: &TranscriptEntryView,
    palette: &Palette,
    appearance: EntryAppearance,
    width: u16,
) -> Vec<Line<'static>> {
    let mut layout = transcript_layout(entry, palette, appearance, width);
    if appearance.selected {
        for line in &mut layout.lines {
            if !line.spans.is_empty() {
                *line = Line::styled(line.to_string(), palette.style(Role::Selection));
            }
        }
    }
    layout.lines
}

pub(crate) fn transcript_layout(
    entry: &TranscriptEntryView,
    palette: &Palette,
    appearance: EntryAppearance,
    width: u16,
) -> Layout {
    let lines = match entry {
        TranscriptEntryView::Text(item) => {
            return transcript_text(item, palette, appearance.selected, width);
        }
        TranscriptEntryView::Tool(tool) => tool::entry(tool, palette, appearance),
        TranscriptEntryView::Artifact(artifact) => {
            let line = Line::from(vec![
                Span::styled("@ ", palette.style(Role::NewInformation)),
                Span::styled(artifact.label.clone(), palette.style(Role::Body)),
                Span::styled(
                    format!(" · {}", artifact.pointer),
                    palette.style(Role::Muted),
                ),
            ]);
            vec![select_line(line, palette, appearance.selected)]
        }
        TranscriptEntryView::Mail(mail) => {
            let line = Line::from(vec![
                Span::styled("-> ", palette.style(Role::NewInformation)),
                Span::styled(mail.to.to_string(), palette.style(Role::Body)),
                Span::styled(format!(" · {}", mail.summary), palette.style(Role::Muted)),
            ]);
            vec![select_line(line, palette, appearance.selected)]
        }
    };
    let mut layout = Layout::default();
    for line in lines {
        layout.logical(
            line,
            usize::from(width),
            false,
            "",
            palette.style(Role::Body),
        );
    }
    layout
        .text
        .truncate(layout.text.trim_end_matches('\n').len());
    layout
}

/// Which side of the conversation a message is on, said in the margin rather than in a word.
///
/// The two everyday roles carry no heading. `you` and `assistant` above every message is a label
/// on something the shape of the screen already says, and it cost two of the rows a short exchange
/// has. What separates them now is the margin: the user's turn wears a bar down its whole height,
/// the agent's turn sits on the plain ground, and the blank row between them is the gap. The
/// remaining kinds keep their word, because `reasoning`, `system`, `warning` and `error` are not
/// positions in a conversation — they are things the reader has to be told (ui-ux §transcript
/// grammar).
const GUTTER: &str = "▌";

/// TR-1: literal height consumes the same row breaks as drawing, without preparing hidden text.
pub(crate) fn literal_text_rows(item: &TranscriptItemView, width: u16) -> Option<usize> {
    if item.kind == TranscriptTextKind::Message
        && item.role == TranscriptRole::Assistant
        && crate::markdown::may_format(&item.source)
    {
        return None;
    }
    if width == 0 {
        return Some(0);
    }
    let (heading, _) = text_treatment(item);
    let gutter = item.kind == TranscriptTextKind::Message && item.role == TranscriptRole::User;
    let reserved = usize::from(width).saturating_sub(4 + usize::from(gutter));
    let body: usize = item
        .source
        .split('\n')
        .map(|line| crate::text_layout::wrap::count(line, reserved, false))
        .sum();
    let heading = heading.map_or(0, |(label, _)| {
        crate::text_layout::wrap::count(label, usize::from(width), false)
    });
    Some(body + heading + 1)
}

fn transcript_text(
    item: &TranscriptItemView,
    palette: &Palette,
    selected: bool,
    width: u16,
) -> Layout {
    let (heading, default_body) = text_treatment(item);
    let body = if selected {
        Role::Selection
    } else {
        default_body
    };
    let gutter = matches!(
        (item.kind, item.role),
        (TranscriptTextKind::Message, TranscriptRole::User)
    );
    let reserved = usize::from(width).saturating_sub(4 + usize::from(gutter));

    let markdown = matches!(
        (item.kind, item.role),
        (TranscriptTextKind::Message, TranscriptRole::Assistant)
    );
    let mut fallback = None;
    if markdown && crate::markdown::may_format(&item.source) {
        match crate::markdown::render_layout(&item.source, palette, reserved) {
            Ok(mut layout) => {
                if selected {
                    for line in &mut layout.lines {
                        *line = Line::styled(
                            pad(line.to_string(), reserved),
                            palette.style(Role::Selection),
                        );
                    }
                }
                layout.decoration(Line::default());
                return layout;
            }
            Err(reason) => fallback = Some(reason),
        }
    }

    // Wrapped here rather than by the paragraph, because a margin painted on a logical line only
    // reaches the first row it wraps onto, and a selection painted on one only reaches as far as
    // the text does. Both have to run the full height and the full width of what they mark.
    // A fixed right gutter holds the first-row action; hovering adds no row or reflow.
    let mut layout = Layout::default();
    if let Some(reason) = fallback {
        for row in wrap_line(reason.label(), reserved) {
            layout.decoration(Line::styled(row, palette.style(Role::Muted)));
        }
    }
    if let Some((word, role)) = heading {
        layout.decoration(Line::styled(
            word,
            palette.style(if selected { Role::Selection } else { role }),
        ));
    }
    let literal;
    let source = if fallback.is_some() {
        literal = crate::markdown::inert(&item.source);
        literal.as_str()
    } else {
        &item.source
    };
    for line in source.split('\n') {
        layout.logical(
            Line::styled(line.to_owned(), palette.style(body)),
            reserved,
            false,
            if gutter { GUTTER } else { "" },
            palette.style(Role::Accent),
        );
    }
    // split preserves explicit trailing line breaks; discard only the builder's final separator.
    layout.text.pop();
    if selected {
        for line in &mut layout.lines {
            *line = Line::styled(
                pad(line.to_string(), reserved + usize::from(gutter)),
                palette.style(Role::Selection),
            );
        }
    }
    layout.decoration(Line::default());
    layout
}

/// Pads a row out to `width` display columns so a selection is a rectangle, not a ragged edge.
fn pad(row: String, width: usize) -> String {
    let used = UnicodeWidthStr::width(row.as_str());
    let mut padded = row;
    padded.push_str(&" ".repeat(width.saturating_sub(used)));
    padded
}

/// The word above a message, when it needs one, and the role its body is drawn in.
const fn text_treatment(item: &TranscriptItemView) -> (Option<(&'static str, Role)>, Role) {
    match item.kind {
        TranscriptTextKind::Message => match item.role {
            TranscriptRole::User | TranscriptRole::Assistant => (None, Role::Body),
            TranscriptRole::Reasoning => (Some(("reasoning", Role::Ambient)), Role::Muted),
            TranscriptRole::System => (Some(("system", Role::Muted)), Role::Muted),
        },
        TranscriptTextKind::Warning => (Some(("warning", Role::ActionRequired)), Role::Body),
        TranscriptTextKind::Error => (Some(("error", Role::Failure)), Role::Body),
    }
}

pub(super) fn select_line(line: Line<'static>, palette: &Palette, selected: bool) -> Line<'static> {
    if selected {
        Line::styled(line.to_string(), palette.style(Role::Selection))
    } else {
        line
    }
}

/// What a conversation says when it has no items to show.
///
/// An empty panel, a panel waiting for its first event, and a panel whose agent has gone all look
/// the same and mean different things, so none of them is left to be inferred from blank rows.
pub(crate) fn conversation_placeholder(
    palette: &Palette,
    surface: SurfaceId,
    has_agent: bool,
) -> Vec<Line<'static>> {
    let message = match (surface, has_agent) {
        (_, true) => "Agent is active; no transcript item has started yet.",
        (SurfaceId::Inspector, false) => "That agent is no longer in the roster.",
        (_, false) => "Waiting for the first semantic event…",
    };
    vec![Line::styled(message, palette.style(Role::Muted))]
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{TranscriptItemId, TranscriptRole};

    use super::{literal_text_rows, transcript_text};
    use crate::{
        TranscriptItemView, TranscriptTextKind,
        theme::{Palette, Role},
    };

    /// TR-1/MD-2: measurement-only geometry agrees with the actual paragraph at every small width.
    #[test]
    fn literal_height_without_presentation_matches_the_drawn_paragraph() {
        use ratatui::widgets::{Paragraph, Wrap};
        for source in [
            "",
            "short text",
            "long unbroken abcdefghijklmnopqrstuvwxyz",
            "中🙂e\u{301}",
            "line one\n\nline three\n",
            " tab\t and carriage\r",
            "**literal marks**",
        ] {
            for (role, kind) in [
                (TranscriptRole::User, TranscriptTextKind::Message),
                (TranscriptRole::Assistant, TranscriptTextKind::Message),
                (TranscriptRole::Reasoning, TranscriptTextKind::Message),
                (TranscriptRole::System, TranscriptTextKind::Warning),
                (TranscriptRole::System, TranscriptTextKind::Error),
            ] {
                let item = TranscriptItemView {
                    id: TranscriptItemId::new("height").expect("id"),
                    source: source.into(),
                    role,
                    kind,
                    revision: 0,
                    finalized: true,
                };
                if role == TranscriptRole::Assistant && crate::markdown::may_format(source) {
                    assert_eq!(literal_text_rows(&item, 60), None);
                    continue;
                }
                for palette in [Palette::ansi(), Palette::pastel(), Palette::monochrome()] {
                    for width in [0, 1, 4, 5, 6, 12, 58, 86, 118] {
                        let lines = transcript_text(&item, &palette, false, width).lines;
                        let expected = Paragraph::new(lines)
                            .wrap(Wrap { trim: false })
                            .line_count(width);
                        assert_eq!(
                            literal_text_rows(&item, width),
                            Some(expected),
                            "{source:?} / {role:?} / {kind:?} at {width}"
                        );
                    }
                }
            }
        }
    }

    /// ENT-1: explicit plaintext reasoning, runtime system text, warnings, and errors each keep a
    /// named monochrome treatment and a semantic palette role.
    #[test]
    fn non_chat_text_roles_have_distinct_named_treatments() {
        let cases = [
            (
                TranscriptRole::Reasoning,
                TranscriptTextKind::Message,
                "reasoning",
                Role::Ambient,
                Role::Muted,
            ),
            (
                TranscriptRole::System,
                TranscriptTextKind::Message,
                "system",
                Role::Muted,
                Role::Muted,
            ),
            (
                TranscriptRole::System,
                TranscriptTextKind::Warning,
                "warning",
                Role::ActionRequired,
                Role::Body,
            ),
            (
                TranscriptRole::System,
                TranscriptTextKind::Error,
                "error",
                Role::Failure,
                Role::Body,
            ),
        ];
        let palette = Palette::pastel();
        for (role, kind, label, heading, body) in cases {
            let item = TranscriptItemView {
                id: TranscriptItemId::new(label).unwrap_or_else(|error| panic!("fixture: {error}")),
                role,
                kind,
                source: format!("{label} source"),
                revision: 0,
                finalized: true,
            };
            let lines = transcript_text(&item, &palette, false, 40);
            assert_eq!(lines.lines[0].to_string(), label);
            assert_eq!(lines.lines[0].style, palette.style(heading));
            assert_eq!(lines.lines[1].spans[0].style, palette.style(body));

            let monochrome = transcript_text(&item, &Palette::monochrome(), false, 40);
            assert_eq!(monochrome.lines[0].to_string(), label);
            assert_eq!(monochrome.lines[1].to_string(), format!("{label} source"));
        }
    }
}
