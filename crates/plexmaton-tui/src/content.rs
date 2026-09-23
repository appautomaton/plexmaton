//! What each surface has to say, before anything knows how much of it fits.
//!
//! These functions turn the projection into logical lines and nothing else: no rectangle, no
//! scroll offset, no widget. Keeping measurement and geometry out of them is what lets a viewport
//! ask "how tall is this" and a renderer ask "draw rows 12 to 20" without either duplicating the
//! other's work — and it is the seam used by the wrapping cache.

use plexmaton_core::AgentStatus;
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::{
    CleanupNotice, ConversationTailRepair, NoticeView, PersistenceNotice, TranscriptEntryView,
    ViewState,
    theme::{Palette, Role},
};

#[path = "content_approval.rs"]
mod approval_presentation;
mod command;
mod composer_menu;
pub(crate) use composer_menu::composer_menu;
mod drawer;
mod roster;
pub(crate) use command::{command_display_source, command_transcript_source};
pub(crate) use roster::{
    STRIP_AGENTS, agent_at_row, capacity as roster_capacity, population, roster,
};
mod tool;
#[path = "content_transcript.rs"]
mod transcript_presentation;

pub(crate) use approval_presentation::{approval, approval_choice_rows, approval_scope_fits};
pub(crate) use drawer::drawer;
pub(crate) use transcript_presentation::{
    conversation_placeholder, discloses, literal_text_rows, transcript_entry,
    transcript_layout_with_prefix,
};

/// What has arrived for an agent, as glyphs a scan recognises without reading.
///
/// Nerd Font Private Use codepoints, the dependency the transcript's copy affordance already
/// takes; a terminal font without them draws a box, so they are named once here and never spelled
/// at a call site. Empty when there is nothing to count, so a quiet agent's row and title stay
/// short. One formatter for every surface that shows these facts: a roster row, a conversation
/// title, an inspected child's title. Rejected: a second, noun-spelling formatter beside this one,
/// which is how `1 tool` and its glyph came to disagree about whether a sent letter counts.
pub(crate) fn tally(agent: &crate::AgentView) -> String {
    const TOOLS: char = '\u{f1323}'; // md-hammer_wrench
    const TASKS: char = '\u{f0756}'; // md-format_list_checks
    const MAIL: char = '\u{f01ee}'; // md-email
    const ARTIFACTS: char = '\u{f03e2}'; // md-paperclip

    let counts = count_entries(agent);
    let mut parts = String::new();
    for (glyph, count) in [
        (TOOLS, counts.tools),
        (TASKS, counts.tasks),
        (MAIL, counts.mail),
        (ARTIFACTS, counts.artifacts),
    ] {
        if count > 0 {
            parts.push_str(&format!("  {glyph} {count}"));
        }
    }
    parts
}

/// What a roster row and a conversation title count, one field per entry kind.
///
/// A tuple was fine for three and became unreadable at four; a named field is also what makes
/// adding the next kind a compiler error at every reader rather than a silent zero.
#[derive(Clone, Copy, Default)]
pub(crate) struct EntryCounts {
    pub(crate) tools: usize,
    pub(crate) artifacts: usize,
    pub(crate) mail: usize,
    pub(crate) tasks: usize,
}

fn count_entries(agent: &crate::AgentView) -> EntryCounts {
    agent
        .entries()
        .fold(EntryCounts::default(), |counts, entry| match entry {
            TranscriptEntryView::Text(_) | TranscriptEntryView::Handoff(_) => counts,
            // A call the provider ran is still a tool the model reached for; the row's colour,
            // not the tally, says where it ran.
            TranscriptEntryView::Tool(_) | TranscriptEntryView::ServerTool(_) => EntryCounts {
                tools: counts.tools.saturating_add(1),
                ..counts
            },
            TranscriptEntryView::Artifact(_) => EntryCounts {
                artifacts: counts.artifacts.saturating_add(1),
                ..counts
            },
            // Addressed entries land in both conversations, so counting them all told an agent
            // how many letters it had handled. A roster says where the user's work is, and a
            // letter this agent sent is not work waiting in it: only what arrived is counted.
            TranscriptEntryView::Mail(mail) if mail.to == mail.owner => EntryCounts {
                mail: counts.mail.saturating_add(1),
                ..counts
            },
            TranscriptEntryView::Task(task) if task.to == task.owner => EntryCounts {
                tasks: counts.tasks.saturating_add(1),
                ..counts
            },
            TranscriptEntryView::Mail(_) | TranscriptEntryView::Task(_) => counts,
        })
}

/// The one row the primary composer keeps while a sub-agent's input is active (INS-5).
///
/// It does not disappear. A composer that vanishes costs the user the affordance and jumps the tail
/// of the transcript they are reading by three rows; one row of jump is what this accepts, and the
/// row stays clickable and stays a focus stop.
pub(crate) fn composer_collapsed(state: &ViewState, palette: &Palette) -> Vec<Line<'static>> {
    let target = state.primary_agent().map_or_else(
        || "the primary agent".to_owned(),
        |agent| agent.label.clone(),
    );
    vec![Line::from(vec![
        Span::styled(format!("Message {target}"), palette.style(Role::Muted)),
        Span::styled("  ·  ", palette.style(Role::Muted)),
        Span::styled(" ⇥ ", palette.style(Role::KeyHint)),
        Span::styled(" to return", palette.style(Role::Muted)),
    ])]
}

/// Producer defects, oldest first. The strip opens at its newest entry.
pub(crate) fn notices(state: &ViewState, palette: &Palette) -> Vec<Line<'static>> {
    state
        .notices()
        .map(|notice| {
            let (marker, role, text) = match notice {
                NoticeView::SequenceGap { expected, received } => (
                    "[gap]  ",
                    Role::ActionRequired,
                    format!("resynchronized from {expected} to {received}"),
                ),
                NoticeView::Rejected { sequence, error } => (
                    "[drop] ",
                    Role::Failure,
                    format!("sequence {}: {error}", sequence.get()),
                ),
                NoticeView::PersistenceFailed(PersistenceNotice::NotWritten) => (
                    "[save] ",
                    Role::Failure,
                    "message was not saved; draft restored".to_owned(),
                ),
                NoticeView::PersistenceFailed(PersistenceNotice::OutcomeUnknown) => (
                    "[save] ",
                    Role::Failure,
                    "write outcome unknown; reopen before retrying".to_owned(),
                ),
                NoticeView::CleanupFailed(CleanupNotice::Provider) => (
                    "[stop] ",
                    Role::Failure,
                    "provider cleanup failed".to_owned(),
                ),
                NoticeView::CleanupFailed(CleanupNotice::Tools) => {
                    ("[stop] ", Role::Failure, "tool cleanup failed".to_owned())
                }
                NoticeView::CleanupFailed(CleanupNotice::JournalWriter) => (
                    "[save] ",
                    Role::Failure,
                    "journal writer cleanup failed".to_owned(),
                ),
                NoticeView::SkillDiagnostic { message } => (
                    "[skill] ",
                    Role::Failure,
                    format!("Skills · {}", inert_inline(message)),
                ),
                NoticeView::DispatchRefused { message } => (
                    "[send] ",
                    Role::Failure,
                    format!("Not sent · {}", inert_inline(message)),
                ),
                // Muted, not Failure: the switch worked. This says what it cost.
                NoticeView::DegradedHistory => (
                    "[model] ",
                    Role::Muted,
                    "Earlier replies came from another model — their reasoning is now plain text."
                        .to_owned(),
                ),
            };
            Line::from(vec![
                Span::styled(marker, palette.style(role)),
                Span::styled(text, palette.style(Role::Body)),
            ])
        })
        .collect()
}

fn inert_inline(source: &str) -> String {
    crate::markdown::inert(source).replace('\n', " ")
}

/// One note after the last entry: restoration (JRN-5) or a requested compaction's end (CPL-9).
pub(crate) fn note_lines(
    note: &crate::state::ConversationNote,
    palette: &Palette,
) -> Vec<Line<'static>> {
    use crate::state::ConversationNote;
    let mut lines = Vec::new();
    match note {
        ConversationNote::Restored(recovery) => {
            if let Some(tail) = recovery.tail {
                let text = match tail {
                    ConversationTailRepair::AddedFinalNewline => {
                        "completed final record repaired".to_owned()
                    }
                    ConversationTailRepair::IsolatedFinalTail { bytes } => {
                        format!("isolated {bytes}-byte incomplete tail")
                    }
                };
                lines.push(Line::styled(text, palette.style(Role::ActionRequired)));
            }
            lines.push(Line::styled(
                "✓ Conversation restored.",
                palette.style(Role::NewInformation),
            ));
        }
        ConversationNote::CompactionRefused(refusal) => {
            let role = if refusal.offers_an_action() {
                Role::ActionRequired
            } else {
                Role::Muted
            };
            lines.push(Line::styled(refusal.message(), palette.style(role)));
        }
        ConversationNote::CompactionFailed { reason } => lines.push(Line::styled(
            format!("Could not compact: {reason}."),
            palette.style(Role::Failure),
        )),
        ConversationNote::SwitchRefused(refusal) => {
            let role = if refusal.is_failure() {
                Role::Failure
            } else {
                Role::Muted
            };
            lines.push(Line::styled(refusal.message(), palette.style(role)));
        }
    }
    lines.push(Line::default());
    lines
}

/// The draft, or the hint that stands in for it when nobody is typing.
///
/// `width` is the panel's inner width, because the rows returned are the wrapped ones the caret is
/// placed against (COM-1).
pub(crate) fn composer(
    state: &ViewState,
    palette: &Palette,
    focused: bool,
    width: u16,
    window: u16,
) -> Vec<Line<'static>> {
    let composer = state.composer();
    if composer.text().is_empty() && !focused {
        return vec![Line::styled(
            "Type a message · ⇥ to focus",
            palette.style(Role::Muted),
        )];
    }
    input_lines(composer, palette, width, window)
}

pub(crate) fn input_lines(
    input: &crate::state::TextInput,
    palette: &Palette,
    width: u16,
    window: u16,
) -> Vec<Line<'static>> {
    input
        .visible_ranges(width, window)
        .into_iter()
        .map(|range| Line::from(input_spans(input, palette, range)))
        .collect()
}

fn input_spans(
    input: &crate::state::TextInput,
    palette: &Palette,
    range: std::ops::Range<usize>,
) -> Vec<Span<'static>> {
    let selected = input
        .selected_range()
        .map(|selected| selected.start.max(range.start)..selected.end.min(range.end))
        .filter(|selected| !selected.is_empty());
    let Some(selected) = selected else {
        return vec![Span::styled(
            input.text()[range].to_owned(),
            palette.style(Role::Body),
        )];
    };
    vec![
        Span::styled(
            input.text()[range.start..selected.start].to_owned(),
            palette.style(Role::Body),
        ),
        Span::styled(
            input.text()[selected.clone()].to_owned(),
            palette
                .style(Role::Body)
                .patch(palette.style(Role::Selection)),
        ),
        Span::styled(
            input.text()[selected.end..range.end].to_owned(),
            palette.style(Role::Body),
        ),
    ]
}

/// The active configuration is a display projection; values come from the composition root.
pub(crate) fn configuration(state: &ViewState, palette: &Palette) -> Vec<Line<'static>> {
    let Some(summary) = state.configuration() else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    for (label, value) in summary.fields() {
        lines.push(Line::styled(label.to_owned(), palette.style(Role::Muted)));
        lines.push(Line::styled(value.to_owned(), palette.style(Role::Body)));
        lines.push(Line::default());
    }
    lines.push(Line::styled(
        "Edit config.toml and restart to change.",
        palette.style(Role::Muted),
    ));
    lines
}

pub(crate) const fn agent_status_label(status: AgentStatus) -> &'static str {
    match status {
        AgentStatus::Idle => "idle",
        AgentStatus::Running => "running",
        AgentStatus::Waiting => "waiting",
        AgentStatus::Completed => "done",
        AgentStatus::Failed => "failed",
        AgentStatus::Cancelled => "cancelled",
    }
}

/// The row `Enter` acts on, wherever it is: every span sits on `Chosen`, a bar with weight and a
/// hue, and the remainder of the row is padded so the bar does not stop where the text does. A
/// span's own colour survives on the bar, so a marker keeps saying what it said.
pub(crate) fn chosen_row(
    spans: Vec<Span<'static>>,
    palette: &Palette,
    width: u16,
) -> Line<'static> {
    let chosen = palette.style(Role::Chosen);
    let used: usize = spans.iter().map(|span| span.content.width()).sum();
    let mut spans: Vec<Span<'static>> = spans
        .into_iter()
        .map(|span| Span::styled(span.content, chosen.patch(span.style)))
        .collect();
    let padding = usize::from(width).saturating_sub(used);
    if padding > 0 {
        spans.push(Span::styled(" ".repeat(padding), chosen));
    }
    Line::from(spans)
}

/// A row occupies one line so its text cannot push the controls out of the panel.
pub(crate) fn command_summary(source: &str, width: usize) -> String {
    if source.width() <= width {
        return source.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut visible = String::new();
    let mut used = 0;
    for cluster in source.graphemes(true) {
        used += cluster.width();
        if used >= width {
            break;
        }
        visible.push_str(cluster);
    }
    visible.push('…');
    visible
}

#[cfg(test)]
mod tests {
    use super::notices;
    use crate::{ViewState, theme::Palette};

    #[test]
    fn skill_diagnostic_names_the_feature_and_returned_input() {
        let mut state = ViewState::default();
        state.report_skill_diagnostic(
            "Skill unavailable\ninput\treturned\u{1b} to the composer".to_owned(),
        );

        let lines = notices(&state, &Palette::pastel());
        assert_eq!(
            lines[0].to_string(),
            "[skill] Skills · Skill unavailable input    returned� to the composer"
        );
    }

    #[test]
    fn restoration_confirmation_is_green_and_tail_repair_is_separate() {
        // JRN-5 / ui-ux §responsive interaction: success and repair have distinct named cues.
        let palette = Palette::pastel();
        for tail in [
            None,
            Some(crate::ConversationTailRepair::IsolatedFinalTail { bytes: 37 }),
        ] {
            let lines = super::note_lines(
                &crate::state::ConversationNote::Restored(crate::ConversationRestoration { tail }),
                &palette,
            );
            let confirmation = &lines[lines.len() - 2];
            assert_eq!(confirmation.to_string(), "✓ Conversation restored.");
            assert_eq!(
                confirmation.style,
                palette.style(crate::theme::Role::NewInformation)
            );
            assert_eq!(lines.len(), if tail.is_some() { 3 } else { 2 });
            if tail.is_some() {
                assert_eq!(
                    lines[0].style,
                    palette.style(crate::theme::Role::ActionRequired)
                );
            }
        }
    }
}
