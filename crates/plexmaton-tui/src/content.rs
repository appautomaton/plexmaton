//! What each surface has to say, before anything knows how much of it fits.
//!
//! These functions turn the projection into logical lines and nothing else: no rectangle, no
//! scroll offset, no widget. Keeping measurement and geometry out of them is what lets a viewport
//! ask "how tall is this" and a renderer ask "draw rows 12 to 20" without either duplicating the
//! other's work — and it is the seam the wrapping cache attaches to in delivery step 5.

use plexmaton_core::{AgentStatus, ToolActivityStatus, TranscriptRole};
use ratatui::text::{Line, Span};

use crate::{
    NoticeView, ViewState,
    theme::{Palette, Role, agent_role, tool_role},
};

/// The agent rail: identity, lifecycle, and which one is selected.
pub(crate) fn agents(state: &ViewState, palette: &Palette) -> Vec<Line<'static>> {
    let selected = state.selected_agent().map(|agent| agent.id.clone());
    state
        .agents()
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
                    format!("  {}", agent_status_label(agent.status)),
                    palette.style(agent_role(agent.status)),
                ),
            ])
        })
        .collect()
}

/// The selected agent's conversation.
pub(crate) fn transcript(state: &ViewState, palette: &Palette) -> Vec<Line<'static>> {
    let Some(agent) = state.selected_agent() else {
        return vec![Line::styled(
            "Waiting for the first semantic event…",
            palette.style(Role::Muted),
        )];
    };

    let mut lines = Vec::new();
    for item in agent.transcript() {
        let author = match item.role {
            TranscriptRole::User => "you",
            TranscriptRole::Assistant => "assistant",
            TranscriptRole::System => "system",
        };
        lines.push(Line::styled(author, palette.style(Role::SectionHeading)));
        lines.push(Line::styled(item.source.clone(), palette.style(Role::Body)));
        lines.push(Line::raw(""));
    }

    if lines.is_empty() {
        lines.push(Line::styled(
            "Agent is active; no transcript item has started yet.",
            palette.style(Role::Muted),
        ));
    }
    lines
}

/// Tools, artifacts, and mail belonging to the selected agent.
pub(crate) fn activity(state: &ViewState, palette: &Palette) -> Vec<Line<'static>> {
    let Some(agent) = state.selected_agent() else {
        return vec![Line::styled(
            "No agent selected.",
            palette.style(Role::Muted),
        )];
    };

    let mut lines = vec![Line::styled("Tools", palette.style(Role::SectionHeading))];
    let mut tools = 0_usize;
    for tool in agent.tool_activity() {
        tools = tools.saturating_add(1);
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} ", tool_marker(tool.status)),
                palette.style(tool_role(tool.status)),
            ),
            Span::styled(tool.label.clone(), palette.style(Role::Body)),
        ]));
    }
    if tools == 0 {
        lines.push(Line::styled("  none", palette.style(Role::Muted)));
    }

    lines.push(Line::styled(
        "Artifacts",
        palette.style(Role::SectionHeading),
    ));
    let mut artifacts = 0_usize;
    for artifact in agent.artifacts() {
        artifacts = artifacts.saturating_add(1);
        lines.push(Line::from(vec![
            Span::styled("@ ", palette.style(Role::NewInformation)),
            Span::styled(artifact.label.clone(), palette.style(Role::Body)),
        ]));
        lines.push(Line::styled(
            format!("  {}", artifact.pointer),
            palette.style(Role::Muted),
        ));
    }
    if artifacts == 0 {
        lines.push(Line::styled("  none", palette.style(Role::Muted)));
    }

    lines.push(Line::styled("Mail", palette.style(Role::SectionHeading)));
    let mut mail = 0_usize;
    for item in agent.inbox() {
        mail = mail.saturating_add(1);
        lines.push(Line::from(vec![
            Span::styled("<- ", palette.style(Role::NewInformation)),
            Span::styled(item.from.to_string(), palette.style(Role::Body)),
        ]));
        lines.push(Line::styled(
            format!("  {}", item.summary),
            palette.style(Role::Muted),
        ));
    }
    if mail == 0 {
        lines.push(Line::styled("  none", palette.style(Role::Muted)));
    }
    lines
}

/// Producer defects, oldest first. The strip opens at its newest entry.
pub(crate) fn notices(state: &ViewState, palette: &Palette) -> Vec<Line<'static>> {
    state
        .notices()
        .map(|notice| {
            let (marker, role, text) = match notice {
                NoticeView::RuntimeWarning { message } => {
                    ("[warn] ", Role::ActionRequired, message.clone())
                }
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
            };
            Line::from(vec![
                Span::styled(marker, palette.style(role)),
                Span::styled(text, palette.style(Role::Body)),
            ])
        })
        .collect()
}

/// The draft, or the hint that stands in for it when nobody is typing.
pub(crate) fn composer(state: &ViewState, palette: &Palette, focused: bool) -> Vec<Line<'static>> {
    let composer = state.composer();
    if composer.draft().is_empty() && !focused {
        return vec![Line::styled(
            "Type a message · ⇥ to focus",
            palette.style(Role::Muted),
        )];
    }
    composer
        .visible_lines()
        .map(|line| Line::styled(line.to_owned(), palette.style(Role::Body)))
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

/// Tool markers stay legible without colour so monochrome terminals keep the same status grammar.
const fn tool_marker(status: ToolActivityStatus) -> &'static str {
    match status {
        ToolActivityStatus::Queued => "[ ]",
        ToolActivityStatus::Running => "[~]",
        ToolActivityStatus::Succeeded => "[+]",
        ToolActivityStatus::Failed => "[!]",
        ToolActivityStatus::Cancelled => "[-]",
    }
}
