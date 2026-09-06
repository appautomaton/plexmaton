//! Logical lines for queued requests and the approval decision card.

use plexmaton_core::AttentionKind;
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
    let cursor = state.listed_attention_cursor();
    state
        .attention_listed()
        .map(|item| {
            let (marker, role) = match (item.acknowledged, item.kind()) {
                (true, _) => ("seen  ", Role::Muted),
                (false, AttentionKind::Approval) => ("block ", Role::ActionRequired),
                (false, AttentionKind::Clarification) => ("ask   ", Role::NewInformation),
            };
            let (caret, caret_role) = if cursor == Some(&item.id) {
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

/// Builds the exact visible card rows. On short terminals secondary copy yields before actions.
struct ApprovalContent {
    lines: Vec<Line<'static>>,
    choices: Vec<(usize, crate::ApprovalChoice)>,
}

pub(crate) fn approval(
    state: &ViewState,
    palette: &Palette,
    width: u16,
    height: u16,
) -> Vec<Line<'static>> {
    approval_content(state, palette, width, height).lines
}

fn approval_content(
    state: &ViewState,
    palette: &Palette,
    width: u16,
    height: u16,
) -> ApprovalContent {
    use crate::{ApprovalChoice, ApprovalStage};
    let Some(view) = state.approval() else {
        return ApprovalContent {
            lines: Vec::new(),
            choices: Vec::new(),
        };
    };
    let scope_fits = approval_scope_fits(state, width, height);
    let budget = usize::from(height);
    let choices = view.choices();
    let choice_rows = choices.len().min(budget);
    let mut heading = detail_rows(view.detail, width, view.expanded);
    match view.stage {
        ApprovalStage::Review => heading.extend(wrap_line(
            match view.reason {
                plexmaton_core::ApprovalReason::PermissionRequired => {
                    "Approval is required for this operation."
                }
                plexmaton_core::ApprovalReason::ExplicitAsk => {
                    "An explicit Ask rule requires an individual decision."
                }
                plexmaton_core::ApprovalReason::NativeFileChange => {
                    "No current permission allows this file change."
                }
                plexmaton_core::ApprovalReason::CommandExecution => {
                    "No current permission allows this command."
                }
            },
            usize::from(width),
        )),
        ApprovalStage::Remember => {
            if let Some(offer) = view.remember {
                let scope = wrap_line(&format!("Scope: {}", offer.label), usize::from(width));
                let available = budget.saturating_sub(choice_rows);
                // PER-10: repeated operation detail yields before the scope being confirmed.
                heading.truncate(available.saturating_sub(scope.len()));
                heading.extend(scope);
                if let Some(note) = &offer.note {
                    heading.extend(wrap_line(note, usize::from(width)));
                }
            }
        }
        ApprovalStage::Submitting => {
            heading.push("Applying decision… Waiting for confirmation.".to_owned())
        }
    }
    if let Some(feedback) = view.feedback {
        heading.push(feedback.message().to_owned());
    }
    if !scope_fits {
        heading = vec!["More space needed to review scope.".to_owned()];
    }
    let hint = match view.stage {
        ApprovalStage::Review => "↑↓ choose · Enter decide · Ctrl-O details",
        ApprovalStage::Remember => "↑↓ choose · Enter confirm · Esc back",
        ApprovalStage::Submitting => "Esc input · your draft stays usable",
    };
    let description = match view.selected {
        ApprovalChoice::ThisSession => "Until Plexmaton exits; kept across /new and resume.",
        ApprovalChoice::ThisProject => "Saved for this checkout across restarts.",
        ApprovalChoice::Back => "Return without granting permission.",
        _ if state.approval_in_primary() => "Esc input · Tab returns to this card",
        _ => "Esc returns to the conversation",
    };
    let extras = if budget >= heading.len() + choice_rows + 4 {
        4
    } else if budget >= heading.len() + choice_rows + 2 {
        2
    } else {
        0
    };
    heading.truncate(budget.saturating_sub(choice_rows + extras));
    let mut lines: Vec<_> = heading
        .into_iter()
        .enumerate()
        .map(|(index, text)| {
            clip(
                Line::styled(
                    text,
                    palette.style(if index == 0 { Role::Body } else { Role::Muted }),
                ),
                width,
            )
        })
        .collect();
    if extras > 0 {
        lines.push(Line::default());
    }
    let mut choice_positions = Vec::with_capacity(choice_rows);
    for choice in choices.iter().take(choice_rows) {
        let enabled = scope_fits || *choice == ApprovalChoice::Back;
        if enabled {
            choice_positions.push((lines.len(), *choice));
        }
        let selected = *choice == view.selected;
        lines.push(clip(
            Line::from(vec![
                Span::styled(
                    if selected { "> " } else { "  " },
                    palette.style(if selected { Role::Accent } else { Role::Muted }),
                ),
                Span::styled(
                    if enabled {
                        choice.label().to_owned()
                    } else {
                        format!("{} (resize)", choice.label())
                    },
                    palette.style(if !enabled {
                        Role::Muted
                    } else if selected {
                        Role::ActionRequired
                    } else {
                        Role::Body
                    }),
                ),
            ]),
            width,
        ));
    }
    if extras > 0 {
        if extras == 4 {
            lines.push(Line::default());
        }
        lines.push(clip(Line::styled(hint, palette.style(Role::Muted)), width));
        if extras == 4 {
            lines.push(clip(
                Line::styled(description, palette.style(Role::Muted)),
                width,
            ));
        }
    }
    ApprovalContent {
        lines,
        choices: choice_positions,
    }
}

/// Both input routes use this same full-scope constraint; clipped confirmation never grants.
pub(crate) fn approval_scope_fits(state: &ViewState, width: u16, height: u16) -> bool {
    let Some(view) = state.approval() else {
        return true;
    };
    if view.stage != crate::ApprovalStage::Remember {
        return true;
    }
    let Some(offer) = view.remember else {
        return false;
    };
    wrap_line(&format!("Scope: {}", offer.label), usize::from(width)).len() + view.choices().len()
        <= usize::from(height)
}

/// Geometry of the actually drawn options, shared with pointer routing.
pub(crate) fn approval_choice_rows(
    state: &ViewState,
    width: u16,
    height: u16,
) -> Vec<(usize, crate::ApprovalChoice)> {
    approval_content(state, &Palette::default(), width, height).choices
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
