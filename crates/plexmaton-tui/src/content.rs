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

use crate::{
    CleanupNotice, NoticeView, PersistenceNotice, SessionRecoveryNotice, TailRecoveryNotice,
    TranscriptEntryView, ViewState,
    theme::{Palette, Role, agent_role},
};

#[path = "content_approval.rs"]
mod approval_presentation;
mod tool;
#[path = "content_transcript.rs"]
mod transcript_presentation;

pub(crate) use approval_presentation::{approval, attention, detail_rows};
pub(crate) use transcript_presentation::{conversation_placeholder, transcript_entry};

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
                NoticeView::SessionRecovered(SessionRecoveryNotice {
                    tail: TailRecoveryNotice::AddedFinalNewline,
                }) => (
                    "[resume] ",
                    Role::NewInformation,
                    "completed final record repaired".to_owned(),
                ),
                NoticeView::SessionRecovered(SessionRecoveryNotice {
                    tail: TailRecoveryNotice::IsolatedFinalTail { bytes },
                }) => (
                    "[resume] ",
                    Role::NewInformation,
                    format!("isolated {bytes}-byte incomplete tail"),
                ),
            };
            Line::from(vec![
                Span::styled(marker, palette.style(role)),
                Span::styled(text, palette.style(Role::Body)),
            ])
        })
        .collect()
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
    if composer.draft().is_empty() && !focused {
        return vec![Line::styled(
            "Type a message · ⇥ to focus",
            palette.style(Role::Muted),
        )];
    }
    composer
        .visible_rows(width)
        .into_iter()
        .map(|row| Line::styled(row, palette.style(Role::Body)))
        .collect()
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

#[cfg(test)]
mod tests {

    use ratatui::widgets::{Paragraph, Wrap};

    use super::agents;

    use crate::{test_support::canonical_state, theme::Palette};

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
