//! Logical lines for queued requests and the approval decision card.

use plexmaton_core::{ApprovalDecision, AttentionKind, ToolCapability};
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation as _;
use unicode_width::UnicodeWidthStr;

use crate::{
    ViewState,
    state::wrap_line,
    theme::{Palette, Role},
};

/// Queued background requests, oldest first, with the cursor on the one `Enter` would go to.
///
/// Approval and clarification are drawn apart because `ui-ux.md` §attention management refuses one
/// generic notification treatment: one is an agent that cannot proceed, the other is an agent that
/// can. Seen requests stay listed and stop shouting — acknowledging is not resolving (ATT-3).
pub(crate) fn attention(state: &ViewState, palette: &Palette) -> Vec<Line<'static>> {
    // Named rather than counted: the band lists a subset now, so an index into it is an index into
    // a different list than the one the cursor moves through.
    let cursor = state
        .attention()
        .nth(state.attention_cursor())
        .map(|item| item.id.clone());
    state
        .attention_listed()
        .map(|item| {
            let (marker, role) = match (item.acknowledged, item.kind()) {
                (true, _) => ("seen  ", Role::Muted),
                (false, AttentionKind::Approval) => ("block ", Role::ActionRequired),
                (false, AttentionKind::Clarification) => ("ask   ", Role::NewInformation),
            };
            let (caret, caret_role) = if cursor.as_ref() == Some(&item.id) {
                ("> ", Role::Accent)
            } else {
                ("  ", Role::Muted)
            };
            Line::from(vec![
                Span::styled(caret, palette.style(caret_role)),
                Span::styled(marker, palette.style(role)),
                Span::styled(format!("{} · ", item.agent_id), palette.style(Role::Muted)),
                Span::styled(item.summary().to_owned(), palette.style(Role::Body)),
            ])
        })
        .collect()
}

/// The decision region: a section of the asking conversation's box, above its composer.
///
/// Nothing here scrolls. Every row but the detail is clipped to `width`, and the two options are
/// the last rows the region has at either size, so the row carrying `Allow once` cannot be pushed,
/// wrapped, or scrolled out of the region that exists to show it. `Ctrl-O` grows the detail in
/// place, which is the same disclosure a tool entry uses (ui-ux §progressive disclosure).
///
/// Rejected: ordering the options first so that wrapping cannot reach them. It guarantees the row
/// at the cost of asking for a decision above the thing being decided. Rejected: scrolling the
/// region, which moved the options off it — a decision surface whose decision can leave the screen
/// is not one.
pub(crate) fn approval(state: &ViewState, palette: &Palette, width: u16) -> Vec<Line<'static>> {
    let Some(approval) = state.approval() else {
        return Vec::new();
    };
    let capabilities = approval
        .capabilities
        .iter()
        .map(|capability| match capability {
            ToolCapability::FileRead => "read files",
            ToolCapability::FileWrite => "change files",
            ToolCapability::ProcessSpawn => "run processes",
        })
        .collect::<Vec<_>>()
        .join(", ");

    let field = |label: &'static str, value: String, value_role: Role| {
        Line::from(vec![
            Span::styled(label, palette.style(Role::Muted)),
            Span::styled(value, palette.style(value_role)),
        ])
    };
    let option = |decision, label: &'static str| {
        let selected = approval.selected == decision;
        vec![
            Span::styled(
                // The same caret the Attention queue uses for the row `Enter` acts on. One cursor
                // glyph across the workspace, or the user learns two.
                if selected { "> " } else { "  " },
                palette.style(if selected { Role::Accent } else { Role::Muted }),
            ),
            Span::styled(
                label,
                palette.style(if selected {
                    Role::ActionRequired
                } else {
                    Role::Body
                }),
            ),
        ]
    };

    // Stacked, because the keys that move between them are `↑` and `↓`. Options read left to
    // right teach the hand the wrong gesture, and the arrow the eye expects then does nothing.
    // Each option carries one hint beside it, so the pair costs two rows rather than three.
    let choice = |decision, label: &'static str, hint: String| {
        let mut spans = option(decision, label);
        let used = label.chars().count().saturating_add(2);
        spans.push(Span::raw(
            " ".repeat(HINT_COLUMN.saturating_sub(used).max(2)),
        ));
        spans.push(Span::styled(hint, palette.style(Role::Muted)));
        Line::from(spans)
    };

    let mut lines = vec![clip(
        field("Access  ", capabilities, Role::ActionRequired),
        width,
    )];
    // No label: the producer's approval detail already names what it is (CMD-1 leads with the
    // command), and a second word in front of it would be the tool saying `Command` twice.
    lines.extend(
        detail_rows(approval.detail, width, approval.expanded)
            .into_iter()
            .map(|row| clip(Line::styled(row, palette.style(Role::Body)), width)),
    );
    lines.push(clip(
        choice(
            ApprovalDecision::AllowOnce,
            "Allow once",
            "↑↓ choose · Enter decide".to_owned(),
        ),
        width,
    ));
    lines.push(clip(
        choice(
            ApprovalDecision::Deny,
            "Deny",
            format!(
                "{} · Esc later",
                if approval.expanded {
                    "⌃O less"
                } else {
                    "⌃O more"
                }
            ),
        ),
        width,
    ));
    lines
}

/// The detail as the region will paint it: one clipped row, or every wrapped row up to the cap.
///
/// Public to the crate because the region's height is this many rows plus three, and a height
/// computed from a second wrap is a height for a detail nobody paints.
pub(crate) fn detail_rows(detail: &str, width: u16, expanded: bool) -> Vec<String> {
    if !expanded {
        return vec![detail.to_owned()];
    }
    let mut rows = wrap_line(detail, usize::from(width));
    if rows.len() > DETAIL_ROWS_MAX {
        rows.truncate(DETAIL_ROWS_MAX);
        // The tail is not lost: a tool entry retains its whole invocation and discloses it in the
        // conversation. This region shows enough to decide on, not everything there is to read.
        if let Some(last) = rows.last_mut() {
            last.push('…');
        }
    }
    rows
}

/// Column the decision hints start at, so the two option rows read as one aligned pair.
const HINT_COLUMN: usize = 18;

/// Rows the disclosed detail may take before the region stops growing into the conversation.
const DETAIL_ROWS_MAX: usize = 8;

/// Truncates one line to `width` display columns, marking a cut with an ellipsis.
///
/// Display width rather than characters, because a clip that counted `char`s would leave a wide
/// glyph straddling the border it was supposed to stay inside.
fn clip(line: Line<'static>, width: u16) -> Line<'static> {
    let budget = usize::from(width);
    if line.width() <= budget {
        return line;
    }
    let budget = budget.saturating_sub(1);
    let mut spans = Vec::new();
    let mut used = 0_usize;
    for span in line.spans {
        let room = budget.saturating_sub(used);
        if room == 0 {
            break;
        }
        let content_width = UnicodeWidthStr::width(span.content.as_ref());
        if content_width <= room {
            used = used.saturating_add(content_width);
            spans.push(span);
            continue;
        }
        let mut kept = String::new();
        let mut kept_width = 0_usize;
        for grapheme in span.content.graphemes(true) {
            let step = UnicodeWidthStr::width(grapheme);
            if kept_width.saturating_add(step) > room {
                break;
            }
            kept_width = kept_width.saturating_add(step);
            kept.push_str(grapheme);
        }
        let style = span.style;
        spans.push(Span::styled(kept, style));
        break;
    }
    spans.push(Span::raw("…"));
    Line::from(spans)
}
