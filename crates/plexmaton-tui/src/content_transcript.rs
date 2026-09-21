//! The transcript's own grammar: what one entry of a conversation looks like.
//!
//! Split from the rest of `content` because every other function there draws a list, a strip or a
//! placeholder from state the workspace owns, while these draw what a *producer* said. The rules
//! they answer to are `ui-ux.md` §transcript grammar, not the surface's.

use crate::text_layout::paint::{Line, Span, Treatment};
use plexmaton_core::TranscriptRole;
use unicode_width::{UnicodeWidthChar as _, UnicodeWidthStr as _};

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
) -> Vec<ratatui::text::Line<'static>> {
    transcript_layout(
        entry,
        appearance,
        width,
        crate::math::MathPresentation::default(),
    )
    .painted_entry(palette, appearance)
}

pub(crate) fn transcript_layout(
    entry: &TranscriptEntryView,
    appearance: EntryAppearance,
    width: u16,
    math: crate::math::MathPresentation,
) -> Layout {
    transcript_layout_with_prefix(entry, appearance, width, math, None).0
}

pub(crate) fn transcript_layout_with_prefix(
    entry: &TranscriptEntryView,
    appearance: EntryAppearance,
    width: u16,
    math: crate::math::MathPresentation,
    prefix: Option<&crate::markdown::PrefixHint>,
) -> (Layout, Option<crate::markdown::PrefixCheckpoint>, bool) {
    // An addressed item is two things with different authors: the envelope, which belongs to the
    // conversation holding it, and the body, which is prose whoever wrote it chose. The heading is
    // built here with the rest of the row grammar; the body is held aside and disclosed by the
    // transcript's own grammar below, so a letter is read the way a message is read.
    let mut disclosed = None;
    let rows = match entry {
        TranscriptEntryView::Text(item) => {
            return transcript_text_with_prefix(item, width, math, prefix);
        }
        TranscriptEntryView::Tool(tool) => tool::prepared_entry(tool, appearance),
        TranscriptEntryView::ServerTool(view) => tool::prepared_server_tool(view, appearance),
        TranscriptEntryView::Artifact(artifact) => {
            let line = Line::from(vec![
                Span::styled("@ ", Role::NewInformation),
                Span::styled(artifact.label.clone(), Role::Body),
                Span::styled(format!(" · {}", artifact.pointer), Role::Muted),
            ]);
            vec![Row::heading(line)]
        }
        TranscriptEntryView::Mail(mail) => {
            let heading = if mail.owner == mail.to {
                "received from "
            } else {
                "sent to "
            };
            disclosed = appearance.open.then_some(mail.summary.as_str());
            addressed_heading(heading, &mail.counterpart, &mail.summary, width)
        }
        // The same shape, because it is the same kind of fact: one session addressed another.
        TranscriptEntryView::Task(task) => {
            let heading = if task.owner == task.to {
                "assigned by "
            } else {
                "assigned to "
            };
            disclosed = appearance.open.then_some(task.task.as_str());
            addressed_heading(heading, &task.counterpart, &task.task, width)
        }
        TranscriptEntryView::Handoff(_) => {
            let line = Line::from(vec![
                Span::styled("handoff", Role::NewInformation),
                Span::styled(" · Controller: User", Role::Muted),
            ]);
            vec![Row::heading(line)]
        }
    };
    let mut layout = Layout::default();
    for row in rows {
        let reserved = usize::from(width).saturating_sub(row.gutter.width());
        layout.logical(row.line, reserved, false, row.gutter, Role::Muted);
    }
    if let Some(body) = disclosed {
        append_prose(&mut layout, body, width, math);
    }
    layout
        .text
        .truncate(layout.text.trim_end_matches('\n').len());
    (layout, None, false)
}

/// One addressed item's envelope — who the other end is, which way it went, and a preview.
///
/// The summary is whatever another session chose to write: one sentence in the fixtures, a page in
/// practice. `Ctrl-O` reveals the rest (ENT-4), and copy carries the whole letter either way, so
/// the row itself only has to stay a row. Rejected: putting the entire summary in the heading,
/// which read correctly for as long as the simulator was the only thing producing mail; the first
/// real letter filled the conversation it arrived in and pushed its own heading off the top.
fn addressed_heading(heading: &'static str, counterpart: &str, body: &str, width: u16) -> Vec<Row> {
    // Two facts, and they answer to different owners. Who the other end is belongs to the item, so
    // both conversations agree on it. What the row *is* belongs to the conversation holding it, and
    // it is said in a word: `ui-ux.md`'s grammar keeps a name for whatever is not a position in a
    // conversation but something the reader has to be told, and which way this went is exactly
    // that. Rejected: a bare arrow relative to the reader naming only the other end, which the
    // person who asked for the feature read backwards on both sides; and then dropping direction
    // altogether, which left the outbox and the inbox drawn identically.
    let spent = heading.width() + counterpart.width() + " · ".width();
    // What the letter says, rather than how it was written: a heading marker or a fence in the
    // preview is the markup leaking into the one row that was supposed to summarize past it.
    let preview = crate::markdown::preview(body).unwrap_or_else(|| body.to_owned());
    let mut compact = Line::from(vec![
        Span::styled(heading, Role::Muted),
        Span::styled(counterpart.to_owned(), Role::NewInformation),
        Span::styled(
            format!(
                " · {}",
                opening(&preview, usize::from(width).saturating_sub(spent))
            ),
            Role::Muted,
        ),
    ]);
    compact.treatment = Treatment::EntryHeading;
    vec![Row::heading(compact)]
}

/// One disclosed body, drawn by the transcript's own grammar under the item's gutter.
///
/// A letter is prose a producer wrote, so Markdown and native math reach it exactly as they reach
/// an assistant message (MD-1, MTH-1). The roadmap's bound on mail is a size limit, not a demotion
/// to metadata: a letter that arrives as a document has to read as one, and the first real producer
/// wrote headings, emphasis and display formulas. `Layout::append` re-bases the body's copy ranges,
/// fragment columns and formula geometry onto the gutter, so selection and atomic formula copy
/// survive the indent. Rejected: a second parser beside `addressed_heading`, which would have
/// drifted from the one the transcript already owns, and would have had to be remembered by every
/// later change to the grammar.
fn append_prose(
    layout: &mut Layout,
    source: &str,
    width: u16,
    math: crate::math::MathPresentation,
) {
    let reserved = usize::from(width).saturating_sub(BODY.width());
    if crate::markdown::may_format(source)
        && let Ok(rendered) = crate::markdown::render_layout_with_prefix(
            source,
            reserved,
            math,
            // A letter is complete when it is accepted into the ledger; nothing later appends to it.
            crate::markdown::Completion::Final,
            None,
        )
    {
        let mut body = rendered.layout;
        for line in &mut body.lines {
            if !line.spans.is_empty() {
                line.treatment = Treatment::MarkdownSelectionWidth(reserved);
            }
        }
        layout.append(body, BODY, Role::Muted);
        return;
    }
    // Exact source, under the same gutter, whenever the body is not admissible Markdown or its
    // layout is refused: the letter stays readable and copyable either way (MD-4).
    for line in source.split('\n') {
        let mut line = Line::from(vec![Span::styled(line.to_owned(), Role::Body)]);
        line.treatment = Treatment::Content;
        layout.logical(line, reserved, false, BODY, Role::Muted);
    }
}

/// As much of the letter as this row holds, and an ellipsis when that is not all of it.
///
/// The bound is the width actually being drawn rather than a constant: a heading is a preview, and
/// a preview that leaves two thirds of its row empty has thrown away the only thing it had. The
/// ellipsis is the one signal that `Ctrl-O` has more, so it also appears for a letter whose words
/// all fit but whose line breaks did not — the body is where those survive.
fn opening(summary: &str, available: usize) -> String {
    let flowed = flow(summary);
    if !summary.contains('\n') && flowed.width() <= available {
        return flowed;
    }
    let budget = available.saturating_sub("…".width());
    let mut kept = String::new();
    let mut used = 0;
    for character in flowed.chars() {
        let step = character.width().unwrap_or(0);
        if used + step > budget {
            break;
        }
        used += step;
        kept.push(character);
    }
    kept.push('…');
    kept
}

/// One row cannot hold a line break, so a heading spends them as spaces.
fn flow(summary: &str) -> String {
    summary
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether an entry retains a body, and so answers `Ctrl-O` (ENT-4).
///
/// A letter always does. Its heading is a preview by construction — flowed onto one row and cut to
/// the width being drawn — so the body is the only place the letter exists as it was written, and
/// whether the preview happened to fit is a property of this frame rather than of the entry. The
/// key is pressed against no particular column and must not have to guess one. Rejected: asking
/// whether the entry is a tool, which was true while tools were the only kind with a body and made
/// `Ctrl-O` inert for the first letter that needed it; and a constant floor below which a letter
/// was deemed to always fit, which promised disclosure with an ellipsis in a panel narrower than
/// the floor and then refused it.
pub(crate) fn discloses(entry: &TranscriptEntryView) -> bool {
    match entry {
        TranscriptEntryView::Tool(tool) => {
            tool.presentation.invocation.is_some() || tool.presentation.outcome.is_some()
        }
        TranscriptEntryView::ServerTool(view) => view.action_source().is_some(),
        TranscriptEntryView::Mail(mail) => !mail.summary.is_empty(),
        TranscriptEntryView::Task(task) => !task.task.is_empty(),
        TranscriptEntryView::Text(_)
        | TranscriptEntryView::Artifact(_)
        | TranscriptEntryView::Handoff(_) => false,
    }
}

/// One prepared row, and the gutter every row it wraps onto repeats.
///
/// The gutter belongs to the wrapper rather than to the row's own spans: a retained line is source
/// text of any length, and prose from another agent is the first kind that reliably exceeds a
/// panel. Written into the spans it marks only the first row, and the remainder of a wrapped line
/// escapes to column zero, reading as if it belonged to the conversation rather than the letter.
pub(crate) struct Row {
    line: Line,
    gutter: &'static str,
}

impl Row {
    /// A row that owns the full width, such as an entry's compact heading.
    pub(crate) const fn heading(line: Line) -> Self {
        Self { line, gutter: "" }
    }

    /// The row with its gutter written in, for a caller that paints without laying out a viewport.
    #[cfg(test)]
    pub(crate) fn flattened(mut self) -> Line {
        if !self.gutter.is_empty() {
            self.line
                .spans
                .insert(0, Span::styled(self.gutter, Role::Muted));
        }
        self.line
    }
}

/// The gutter a disclosed body hangs from.
const BODY: &str = "  │ ";

/// One retained body under its gutter, shared by every entry that discloses one.
pub(crate) fn append_source(
    rows: &mut Vec<Row>,
    source: &str,
    treatment: Treatment,
    role: impl Fn(&str) -> Role,
) {
    rows.extend(source.split('\n').map(|row| {
        let mut line = Line::from(vec![Span::styled(row.to_owned(), role(row))]);
        line.treatment = treatment;
        Row { line, gutter: BODY }
    }));
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

/// ENT-1: terminal reasoning newlines are source, not additional inter-entry spacing.
fn literal_display_source(item: &TranscriptItemView) -> &str {
    if item.kind == TranscriptTextKind::Message && item.role == TranscriptRole::Reasoning {
        item.source.trim_end_matches('\n')
    } else {
        &item.source
    }
}

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
    let body: usize = literal_display_source(item)
        .split('\n')
        .map(|line| crate::text_layout::wrap::count(line, reserved, false))
        .sum();
    let heading = heading.map_or(0, |(label, _)| {
        crate::text_layout::wrap::count(label, usize::from(width), false)
    });
    Some(body + heading)
}

#[cfg(test)]
fn transcript_text(
    item: &TranscriptItemView,
    width: u16,
    math: crate::math::MathPresentation,
) -> Layout {
    transcript_text_with_prefix(item, width, math, None).0
}

fn transcript_text_with_prefix(
    item: &TranscriptItemView,
    width: u16,
    math: crate::math::MathPresentation,
    prefix: Option<&crate::markdown::PrefixHint>,
) -> (Layout, Option<crate::markdown::PrefixCheckpoint>, bool) {
    let (heading, body) = text_treatment(item);
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
        let completion = if item.finalized {
            crate::markdown::Completion::Final
        } else {
            crate::markdown::Completion::Streaming
        };
        match crate::markdown::render_layout_with_prefix(
            &item.source,
            reserved,
            math,
            completion,
            // Finalization reveals every incomplete construct and establishes a canonical
            // retained result; a streaming hint is valid only while the source may still grow.
            if item.finalized { None } else { prefix },
        ) {
            Ok(rendered) => {
                let mut layout = rendered.layout;
                for line in &mut layout.lines {
                    if !line.spans.is_empty() {
                        line.treatment = Treatment::MarkdownSelectionWidth(reserved);
                    }
                }
                return (layout, rendered.checkpoint, rendered.reused_prefix);
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
            layout.decoration(Line::styled(row, Role::Muted));
        }
    }
    if let Some((word, role)) = heading {
        layout.decoration(Line::styled(word, role));
    }
    let literal;
    let source = if fallback.is_some() {
        literal = crate::markdown::inert(&item.source);
        literal.as_str()
    } else {
        literal_display_source(item)
    };
    for line in source.split('\n') {
        layout.logical(
            Line::styled(line.to_owned(), body),
            reserved,
            false,
            if gutter { GUTTER } else { "" },
            Role::Accent,
        );
    }
    // split preserves explicit trailing line breaks; discard only the builder's final separator.
    layout.text.pop();
    for line in &mut layout.lines {
        line.treatment = Treatment::SelectionWidth(reserved + usize::from(gutter));
    }
    (layout, None, false)
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

/// What a conversation says when it has no items to show.
///
/// An empty panel, a panel waiting for its first event, and a panel whose agent has gone all look
/// the same and mean different things, so none of them is left to be inferred from blank rows.
pub(crate) fn conversation_placeholder(
    palette: &Palette,
    surface: SurfaceId,
    has_agent: bool,
) -> Vec<ratatui::text::Line<'static>> {
    let message = match (surface, has_agent) {
        (_, true) => "Agent is active; no transcript item has started yet.",
        (SurfaceId::Inspector, false) => "That agent is no longer in the roster.",
        (_, false) => "Waiting for the first semantic event…",
    };
    vec![ratatui::text::Line::styled(
        message,
        palette.style(Role::Muted),
    )]
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{AgentId, TranscriptItemId, TranscriptRole};

    use super::{discloses, literal_text_rows, transcript_entry, transcript_text};
    use crate::{
        HandoffView, TranscriptEntryView, TranscriptItemView, TranscriptTextKind,
        state::EntryAppearance,
        theme::{Palette, Role},
    };

    /// CCV-3: acknowledged control is a stable semantic row at every accepted width.
    #[test]
    fn ccv_3_handoff_entry_is_distinct_at_three_widths() {
        let child = AgentId::new("delegated-1").expect("child");
        let entry = TranscriptEntryView::Handoff(HandoffView {
            entry_id: TranscriptItemId::new("handoff-in").expect("item"),
            owner: child.clone(),
            child,
            revision: 0,
        });
        for width in [120, 95, 60] {
            let lines = transcript_entry(
                &entry,
                &Palette::pastel(),
                EntryAppearance::default(),
                width,
            );
            assert_eq!(lines.len(), 1, "{width} columns");
            assert_eq!(lines[0].to_string(), "handoff · Controller: User");
        }
        assert!(!discloses(&entry));
    }

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
                for palette in [Palette::pastel(), Palette::pastel(), Palette::pastel()] {
                    for width in [0, 1, 4, 5, 6, 12, 58, 86, 118] {
                        let lines =
                            transcript_text(&item, width, crate::math::MathPresentation::default())
                                .painted_lines(&palette);
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

    /// ENT-1/TR-1: terminal newlines add no reasoning rows; internal paragraphs keep their maps.
    #[test]
    fn reasoning_terminal_newlines_share_measurement_and_paint() {
        for width in [60, 88, 120] {
            for finalized in [false, true] {
                for body in ["", "A short thought.", "\nFirst paragraph.\n\n再检查一次。"] {
                    let mut item = TranscriptItemView {
                        id: TranscriptItemId::new("reasoning-gap").expect("id"),
                        role: TranscriptRole::Reasoning,
                        kind: TranscriptTextKind::Message,
                        source: body.into(),
                        revision: 0,
                        finalized,
                    };
                    let baseline = transcript_text(&item, width, Default::default());
                    for ending in ["\n", "\n\n", "\n\n\n"] {
                        item.source = format!("{body}{ending}");
                        let layout = transcript_text(&item, width, Default::default());
                        assert_eq!(layout.lines, baseline.lines);
                        assert_eq!(layout.rows, baseline.rows);
                        assert_eq!(
                            layout.text, baseline.text,
                            "pointer copy follows visible text"
                        );
                        assert_eq!(literal_text_rows(&item, width), Some(layout.lines.len()));
                    }
                }
            }
        }
    }

    /// ENT-1: explicit plaintext reasoning, runtime system text, warnings, and errors each keep a
    /// named textual treatment and a semantic palette role.
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
            let prepared = transcript_text(&item, 40, crate::math::MathPresentation::default());
            let lines = prepared.painted_lines(&palette);
            assert_eq!(lines[0].to_string(), label);
            assert_eq!(lines[0].style, palette.style(heading));
            assert_eq!(lines[1].spans[0].style, palette.style(body));

            let painted = prepared.painted_lines(&Palette::pastel());
            assert_eq!(painted[0].to_string(), label);
            assert_eq!(painted[1].to_string(), format!("{label} source"));
        }
    }
}

/// ENT-4: a heading is measured in columns, because a letter is written in whatever script its
/// author uses and half of them are twice as wide as the count of their characters suggests.
#[cfg(test)]
mod mail_heading_tests {
    use unicode_width::UnicodeWidthStr as _;

    use plexmaton_core::{AgentId, MailId, TranscriptItemId};

    use super::{TranscriptEntryView, opening};

    #[test]
    fn a_heading_spends_columns_and_never_more_than_it_has() {
        for available in [12_usize, 24, 48, 96] {
            for summary in [
                "只读检查结果:\n1) .git/HEAD 指向 refs/heads/main,当前分支为 main。\n2) 未提交改动无法列出。",
                "Read-only check complete.\nThe branch is main.\nNothing was modified.",
                "短",
            ] {
                let heading = opening(summary, available);
                assert!(
                    heading.width() <= available,
                    "{available} columns: {heading:?} is {} wide",
                    heading.width()
                );
                assert!(!heading.contains('\n'), "a heading is one row: {heading:?}");
            }
        }
    }

    /// The ellipsis promises `Ctrl-O` answers. A panel narrow enough abridges any letter, so the
    /// promise only holds if disclosure never consults the width the promise was made at.
    #[test]
    fn every_ellipsis_is_a_promise_disclosure_keeps() {
        for summary in [
            "只读检查结果:\n1) 当前分支为 main。",
            "Read-only check complete.\nNothing was modified.",
            "One short line.",
            "短",
        ] {
            let entry = TranscriptEntryView::Mail(crate::MailView {
                entry_id: TranscriptItemId::new("letter").unwrap_or_else(|error| panic!("{error}")),
                id: MailId::new("letter").unwrap_or_else(|error| panic!("{error}")),
                owner: AgentId::new("agent-b").unwrap_or_else(|error| panic!("{error}")),
                from: AgentId::new("agent-b").unwrap_or_else(|error| panic!("{error}")),
                to: AgentId::new("agent-a").unwrap_or_else(|error| panic!("{error}")),
                counterpart: "Agent A".to_owned(),
                summary: summary.to_owned(),
                revision: 0,
            });
            for available in [8_usize, 12, 24, 48, 96, 240] {
                if opening(summary, available).ends_with('…') {
                    assert!(
                        super::discloses(&entry),
                        "{summary:?} abridged at {available} columns"
                    );
                }
            }
        }
    }
}

/// ENT-1: one item, two conversations, and each says which side it is looking at.
#[cfg(test)]
mod addressed_entry_tests {
    use plexmaton_core::{AgentId, MailId, TranscriptItemId};

    use super::{TranscriptEntryView, transcript_layout};
    use crate::state::EntryAppearance;

    fn agent(id: &str) -> AgentId {
        AgentId::new(id).unwrap_or_else(|error| panic!("fixture: {error}"))
    }

    fn letter(owner: &str) -> TranscriptEntryView {
        TranscriptEntryView::Mail(crate::MailView {
            entry_id: TranscriptItemId::new(format!("letter-{owner}"))
                .unwrap_or_else(|error| panic!("fixture: {error}")),
            id: MailId::new("letter").unwrap_or_else(|error| panic!("fixture: {error}")),
            owner: agent(owner),
            from: agent("delegated-1"),
            to: agent("agent-primary"),
            counterpart: if owner == "agent-primary" {
                "Delegated 1"
            } else {
                "Plexmaton"
            }
            .to_owned(),
            summary: "the answer".to_owned(),
            revision: 0,
        })
    }

    fn task(owner: &str) -> TranscriptEntryView {
        TranscriptEntryView::Task(crate::TaskView {
            entry_id: TranscriptItemId::new(format!("task-{owner}"))
                .unwrap_or_else(|error| panic!("fixture: {error}")),
            owner: agent(owner),
            from: agent("agent-primary"),
            to: agent("delegated-1"),
            counterpart: if owner == "agent-primary" {
                "Delegated 1"
            } else {
                "Plexmaton"
            }
            .to_owned(),
            task: "the ask".to_owned(),
            revision: 0,
        })
    }

    fn drawn(entry: &TranscriptEntryView) -> String {
        transcript_layout(
            entry,
            EntryAppearance::compact(false),
            120,
            crate::math::MathPresentation::default(),
        )
        .lines
        .iter()
        .map(ToString::to_string)
        .collect()
    }

    /// Each side names the other end by its display label while retaining routing identities.
    #[test]
    fn each_side_of_one_item_names_the_other_end() {
        for (sent, received, sent_heading, received_heading) in [
            (
                letter("delegated-1"),
                letter("agent-primary"),
                "sent to Plexmaton",
                "received from Delegated 1",
            ),
            (
                task("agent-primary"),
                task("delegated-1"),
                "assigned to Delegated 1",
                "assigned by Plexmaton",
            ),
        ] {
            let sent = drawn(&sent);
            let received = drawn(&received);
            assert_ne!(
                sent, received,
                "the outbox and the inbox must not read alike"
            );
            assert!(sent.contains(sent_heading), "{sent:?}");
            assert!(received.contains(received_heading), "{received:?}");
            for internal in ["agent-primary", "delegated-1"] {
                assert!(!sent.contains(internal), "{sent:?}");
                assert!(!received.contains(internal), "{received:?}");
            }
        }
    }

    /// MD-1/MTH-1: a disclosed letter is prose, so the transcript's own grammar draws it.
    ///
    /// The first real producer wrote headings, emphasis and display formulas into its mail. Drawn
    /// as an envelope's retained source, every one of those reached the terminal as the characters
    /// the author typed. This asserts the three that a reader notices — a heading is a heading, a
    /// display formula is laid-out geometry rather than its delimiters, and the exact source is
    /// still what copy carries.
    #[test]
    fn a_disclosed_letter_is_drawn_as_markdown_and_native_math() {
        let mut entry = letter("agent-primary");
        let TranscriptEntryView::Mail(mail) = &mut entry else {
            panic!("fixture: the letter is mail");
        };
        mail.summary = "## Follow-up B\n\nSet \\(h = e + p/\\rho\\) first.\n\n\\[\n\\rho c \\frac{\\partial T}{\\partial t} = \\nabla\\cdot(k\\nabla T) + \\dot{q}\n\\]\n".to_owned();
        let layout = transcript_layout(
            &entry,
            EntryAppearance {
                open: true,
                ..EntryAppearance::compact(false)
            },
            80,
            crate::math::MathPresentation::Native,
        );
        let painted = layout
            .lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !painted.contains("## "),
            "a heading marker reached the terminal: {painted}"
        );
        assert!(
            !painted.contains("\\rho c"),
            "display source reached the terminal: {painted}"
        );
        assert!(
            !layout.formulas.is_empty(),
            "no formula geometry was placed: {painted}"
        );
        assert!(
            layout.text.contains("\\rho c \\frac"),
            "copy must carry the letter exactly as it was written"
        );
        assert!(
            painted.contains("received from Delegated 1"),
            "the envelope keeps its heading: {painted}"
        );
    }
}
