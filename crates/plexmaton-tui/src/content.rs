//! What each surface has to say, before anything knows how much of it fits.
//!
//! These functions turn the projection into logical lines and nothing else: no rectangle, no
//! scroll offset, no widget. Keeping measurement and geometry out of them is what lets a viewport
//! ask "how tall is this" and a renderer ask "draw rows 12 to 20" without either duplicating the
//! other's work — and it is the seam used by the wrapping cache.

use plexmaton_core::{AgentId, AgentStatus};
use ratatui::{
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::{
    CleanupNotice, NoticeView, PersistenceNotice, SessionRestoration, SessionTailRepair,
    TranscriptEntryView, ViewState,
    theme::{Palette, Role, agent_role},
};

#[path = "content_approval.rs"]
mod approval_presentation;
mod tool;
#[path = "content_transcript.rs"]
mod transcript_presentation;

pub(crate) use approval_presentation::{approval, approval_option_label, attention, detail_rows};
pub(crate) use transcript_presentation::{
    conversation_placeholder, transcript_entry, transcript_layout,
};

/// The list of sub-agents: identity, lifecycle, and which one is being looked at.
///
/// The primary is not in it. Its conversation is the screen, and looking at it is looking at
/// nobody else (INS-1).
pub(crate) fn agents(state: &ViewState, palette: &Palette) -> Vec<Line<'static>> {
    let selected = state.selected_agent().map(|agent| agent.id.clone());
    if state.sub_agents().next().is_none() {
        return vec![Line::styled(
            "No sub-agents yet.",
            palette.style(Role::Muted),
        )];
    }
    state
        .sub_agents()
        .map(|agent| {
            let (marker, marker_role) = if selected.as_ref() == Some(&agent.id) {
                ("●", Role::Accent)
            } else {
                ("○", Role::Muted)
            };
            Line::from(vec![
                Span::styled(format!("{marker} "), palette.style(marker_role)),
                Span::styled(agent.label.clone(), palette.style(Role::Body)),
                Span::styled(
                    format!(
                        "  {}{}",
                        agent_status_label(agent.status),
                        agent_row_counts(agent)
                    ),
                    palette.style(agent_role(agent.status)),
                ),
            ])
        })
        .collect()
}

/// Which agent is painted on `row` of the list, counting rows the way the panel wraps them.
///
/// The pointer's way of looking at an agent. Rows are counted through the same lines the panel
/// paints, so a label that wrapped onto two rows hits on either; `row` is relative to the panel's
/// content and already past its scroll offset, which the caller knows and this function does not.
pub(crate) fn agent_at_row(
    state: &ViewState,
    palette: &Palette,
    width: u16,
    row: usize,
) -> Option<AgentId> {
    let target = row;
    let mut first = 0_usize;
    for (agent, line) in state.sub_agents().zip(agents(state, palette)) {
        let rows = Paragraph::new(line)
            .wrap(Wrap { trim: false })
            .line_count(width)
            .max(1);
        if (first..first.saturating_add(rows)).contains(&target) {
            return Some(agent.id.clone());
        }
        first = first.saturating_add(rows);
    }
    None
}

/// Counts of an agent's non-text entries. Empty when there is nothing to count, so a quiet agent's
/// row and conversation title stay short.
pub(crate) fn entry_counts(agent: &crate::AgentView) -> String {
    let (tools, artifacts, mail) = count_entries(agent);
    let mut parts = String::new();
    for (count, one, many) in [
        (tools, "tool", "tools"),
        (artifacts, "artifact", "artifacts"),
        (mail, "mail", "mail"),
    ] {
        if count > 0 {
            let noun = if count == 1 { one } else { many };
            parts.push_str(&format!(" · {count} {noun}"));
        }
    }
    parts
}

/// Compact counts for the narrow agent rail; `@` is the transcript's artifact marker.
fn agent_row_counts(agent: &crate::AgentView) -> String {
    let (tools, artifacts, mail) = count_entries(agent);
    let mut parts = String::new();
    if tools > 0 {
        parts.push_str(&format!(
            "{tools} tool{}",
            if tools == 1 { "" } else { "s" }
        ));
    }
    if artifacts > 0 {
        if !parts.is_empty() {
            parts.push(' ');
        }
        parts.push_str(&format!("@{artifacts}"));
    }
    if mail > 0 {
        if !parts.is_empty() {
            parts.push(' ');
        }
        parts.push_str(&format!("{mail} mail"));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" · {parts}")
    }
}

fn count_entries(agent: &crate::AgentView) -> (usize, usize, usize) {
    agent.entries().fold(
        (0_usize, 0_usize, 0_usize),
        |(tools, artifacts, mail), entry| match entry {
            TranscriptEntryView::Text(_) => (tools, artifacts, mail),
            TranscriptEntryView::Tool(_) => (tools.saturating_add(1), artifacts, mail),
            TranscriptEntryView::Artifact(_) => (tools, artifacts.saturating_add(1), mail),
            TranscriptEntryView::Mail(_) => (tools, artifacts, mail.saturating_add(1)),
        },
    )
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
            };
            Line::from(vec![
                Span::styled(marker, palette.style(role)),
                Span::styled(text, palette.style(Role::Body)),
            ])
        })
        .collect()
}

pub(crate) fn recovery_lines(
    recovery: &SessionRestoration,
    palette: &Palette,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some(tail) = recovery.tail {
        let text = match tail {
            SessionTailRepair::AddedFinalNewline => "completed final record repaired".to_owned(),
            SessionTailRepair::IsolatedFinalTail { bytes } => {
                format!("isolated {bytes}-byte incomplete tail")
            }
        };
        lines.push(Line::styled(text, palette.style(Role::ActionRequired)));
    }
    lines.push(Line::styled(
        "✓ Conversation restored.",
        palette.style(Role::NewInformation),
    ));
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
) -> Vec<Line<'static>> {
    let composer = state.composer();
    if composer.text().is_empty() && !focused {
        return vec![Line::styled(
            "Type a message · ⇥ to focus",
            palette.style(Role::Muted),
        )];
    }
    input_lines(composer, palette, width)
}

pub(crate) fn input_lines(
    input: &crate::state::TextInput,
    palette: &Palette,
    width: u16,
) -> Vec<Line<'static>> {
    input
        .visible_ranges(width)
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

/// The command list's body: the filter, the matching commands, and how to work them.
///
/// The chosen row is marked and coloured exactly as the Attention list and the approval card mark
/// theirs (`Accent` against `Muted`), because it is the same interaction: `↑↓` to choose, `Enter` to
/// act. `Selection` is deliberately not used — that role means content selected for copying.
///
/// The keys are a muted last row rather than a badge on the title: they are a sentence, and the
/// badge is where a short status goes.
pub(crate) fn command_palette(
    state: &ViewState,
    palette: &Palette,
    width: u16,
    height: u16,
) -> Vec<Line<'static>> {
    let Some(commands) = state.command_palette() else {
        return Vec::new();
    };
    let mut filter = vec![Span::styled(" ", palette.style(Role::Muted))];
    if commands.filter().text().is_empty() {
        filter.push(Span::styled("Type to filter", palette.style(Role::Muted)));
    } else {
        filter.extend(input_spans(
            commands.filter(),
            palette,
            commands.filter_range(width),
        ));
    }
    let mut lines = vec![Line::from(filter)];
    if let Some(sessions) = &commands.sessions {
        let matches = sessions.matches(commands.filter().text());
        let window = commands.choice_window(height);
        for (index, entry) in matches.iter().enumerate().skip(window.start).take(
            if commands.session_rows_visible(height) {
                window.len()
            } else {
                0
            },
        ) {
            let role = if index == commands.chosen_index() {
                Role::Accent
            } else {
                Role::Muted
            };
            let marker = if index == commands.chosen_index() {
                "> "
            } else {
                "  "
            };
            let label = format!("{marker}{}", entry.title);
            lines.push(Line::styled(
                command_summary(&label, usize::from(width)),
                palette.style(role),
            ));
        }
        if matches.is_empty() && height >= 6 {
            lines.push(Line::default());
        }
        let note = if sessions.status == crate::SessionPickerStatus::Ready && !matches.is_empty() {
            if sessions.limited {
                "Recent sessions only · older files remain on disk"
            } else {
                matches
                    .get(commands.chosen_index())
                    .map_or("", |entry| entry.id.as_str())
            }
        } else {
            sessions.status.message()
        };
        if height >= 6 || matches.is_empty() || !commands.session_rows_visible(height) {
            let role = if matches!(
                sessions.status,
                crate::SessionPickerStatus::OpenFailed | crate::SessionPickerStatus::ListFailed
            ) {
                Role::Failure
            } else {
                Role::Muted
            };
            lines.push(Line::styled(
                command_summary(note, usize::from(width)),
                palette.style(role),
            ));
        }
        lines.push(Line::styled(
            " ↑↓ choose · Enter resume · Esc close",
            palette.style(Role::Muted),
        ));
        return lines;
    }
    let matches = commands.matches();
    if matches.is_empty() {
        lines.push(Line::styled(
            " No command matches".to_owned(),
            palette.style(Role::Muted),
        ));
    }
    let window = commands.choice_window(height);
    for (index, command) in matches
        .into_iter()
        .enumerate()
        .skip(window.start)
        .take(window.len())
    {
        let chosen = index == commands.chosen_index();
        let (marker, role) = if chosen {
            ("> ", Role::Accent)
        } else {
            ("  ", Role::Muted)
        };
        let name = format!("{}  ", command.name());
        let remaining = usize::from(width).saturating_sub(2 + name.width());
        lines.push(Line::from(vec![
            Span::styled(marker, palette.style(role)),
            Span::styled(name, palette.style(role)),
            Span::styled(
                command_summary(command.summary(), remaining),
                palette.style(Role::Muted),
            ),
        ]));
    }
    lines.push(Line::styled(
        " ↑↓ choose · Enter run · Esc close".to_owned(),
        palette.style(Role::Muted),
    ));
    lines
}

/// A command occupies one row so its description cannot push the controls out of the panel.
fn command_summary(source: &str, width: usize) -> String {
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
    use ratatui::widgets::{Paragraph, Wrap};

    use super::agents;
    use crate::{test_support::canonical_state, theme::Palette};

    #[test]
    fn restoration_confirmation_is_green_and_tail_repair_is_separate() {
        // JRN-5 / ui-ux §responsive interaction: success and repair have distinct named cues.
        let palette = Palette::ansi();
        for tail in [
            None,
            Some(crate::SessionTailRepair::IsolatedFinalTail { bytes: 37 }),
        ] {
            let lines = super::recovery_lines(&crate::SessionRestoration { tail }, &palette);
            let confirmation = &lines[lines.len() - 2];
            assert_eq!(confirmation.to_string(), "✓ Conversation restored.");
            assert_eq!(confirmation.style.fg, Some(ratatui::style::Color::Green));
            assert_eq!(lines.len(), if tail.is_some() { 3 } else { 2 });
            if tail.is_some() {
                assert_eq!(lines[0].style.fg, Some(ratatui::style::Color::Yellow));
            }
        }
    }

    /// Phase 01 stage 3 slice 3: retiring the detail panel keeps its counts on each agent row.
    #[test]
    fn an_agent_row_carries_its_tool_artifact_and_mail_counts() {
        let state = canonical_state();
        let rows = agents(&state, &Palette::monochrome());
        let agent_b = rows
            .iter()
            .find(|line| line.to_string().contains("Agent B"))
            .unwrap_or_else(|| panic!("canonical state includes Agent B"))
            .to_string();

        assert!(agent_b.contains("1 tool"), "{agent_b:?}");
        assert!(agent_b.contains("@1"), "{agent_b:?}");
        assert!(agent_b.contains("1 mail"), "{agent_b:?}");
        for width in [26, 24] {
            let physical_rows = Paragraph::new(agent_b.clone())
                .wrap(Wrap { trim: false })
                .line_count(width);
            assert_eq!(physical_rows, 2, "{width} cells: {agent_b:?}");
        }
    }
}
