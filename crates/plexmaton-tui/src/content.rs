//! What each surface has to say, before anything knows how much of it fits.
//!
//! These functions turn the projection into logical lines and nothing else: no rectangle, no
//! scroll offset, no widget. Keeping measurement and geometry out of them is what lets a viewport
//! ask "how tall is this" and a renderer ask "draw rows 12 to 20" without either duplicating the
//! other's work — and it is the seam the wrapping cache attaches to in delivery step 5.

use plexmaton_core::{AgentId, AgentStatus, AttentionKind, ToolCallStatus, TranscriptRole};
use ratatui::{
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

use crate::{
    NoticeView, TranscriptItemView, ViewState,
    state::Selected,
    surface::SurfaceId,
    theme::{Palette, Role, agent_role, tool_role},
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
                    format!("  {}", agent_status_label(agent.status)),
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
    row: u16,
) -> Option<AgentId> {
    let target = usize::from(row);
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

/// One transcript item, as the logical lines a viewport measures and paints.
///
/// Per item rather than per conversation, because both the height cache and the visible range are
/// expressed in items: a frame that asks for one item's rows must get exactly the rows that item
/// contributes to the whole (TR-1).
pub(crate) fn transcript_item(
    item: &TranscriptItemView,
    palette: &Palette,
    selected: bool,
) -> Vec<Line<'static>> {
    let author = match item.role {
        TranscriptRole::User => "you",
        TranscriptRole::Assistant => "assistant",
        TranscriptRole::System => "system",
    };
    // Selection replaces the body role rather than adding to it: what is selected has to be
    // legible as one block, and a message whose text kept its own colour while the heading did not
    // reads as two things.
    let (heading, body) = if selected {
        (Role::Selection, Role::Selection)
    } else {
        (Role::SectionHeading, Role::Body)
    };
    vec![
        Line::styled(author, palette.style(heading)),
        Line::styled(item.source.clone(), palette.style(body)),
        Line::raw(""),
    ]
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

/// Counts of an agent's tools, artifacts and mail, for a title that has no activity column beside
/// it. Empty when there is nothing to count, so a quiet agent's title stays short.
pub(crate) fn activity_counts(agent: &crate::AgentView) -> String {
    let mut parts = String::new();
    for (count, one, many) in [
        (agent.tool_activity().count(), "tool", "tools"),
        (agent.artifacts().count(), "artifact", "artifacts"),
        (agent.inbox().count(), "mail", "mail"),
    ] {
        if count > 0 {
            let noun = if count == 1 { one } else { many };
            parts.push_str(&format!(" · {count} {noun}"));
        }
    }
    parts
}

/// Tools, artifacts, and mail belonging to the agent being looked at, else the primary.
pub(crate) fn activity(state: &ViewState, palette: &Palette) -> Vec<Line<'static>> {
    let Some(agent) = state.activity_agent() else {
        return vec![Line::styled("No agents yet.", palette.style(Role::Muted))];
    };
    detail(
        agent,
        palette,
        state.selected_in(SurfaceId::Activity, &agent.id),
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

/// Tools, then artifacts, then mail — the order a selection index means.
///
/// `selected` is consulted against a running entry counter, and that counter walks the three groups
/// in exactly the order [`ViewState::sources`](crate::ViewState) builds them. Two orders here would
/// select one thing and copy another, which is the failure the shared order exists to prevent.
fn detail(agent: &crate::AgentView, palette: &Palette, selected: Selected) -> Vec<Line<'static>> {
    let mut entry = 0_usize;
    let mut next = |lines: &mut Vec<Line<'static>>, rows: Vec<Line<'static>>| {
        let mark = selected.contains(entry);
        entry = entry.saturating_add(1);
        for row in rows {
            lines.push(if mark {
                Line::styled(row.to_string(), palette.style(Role::Selection))
            } else {
                row
            });
        }
    };

    let mut lines = vec![Line::styled("Tools", palette.style(Role::SectionHeading))];
    let mut tools = 0_usize;
    for tool in agent.tool_activity() {
        tools = tools.saturating_add(1);
        next(
            &mut lines,
            vec![Line::from(vec![
                Span::styled(
                    format!("{} ", tool_marker(tool.status)),
                    palette.style(tool_role(tool.status)),
                ),
                Span::styled(tool.label.clone(), palette.style(Role::Body)),
            ])],
        );
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
        next(
            &mut lines,
            vec![
                Line::from(vec![
                    Span::styled("@ ", palette.style(Role::NewInformation)),
                    Span::styled(artifact.label.clone(), palette.style(Role::Body)),
                ]),
                Line::styled(
                    format!("  {}", artifact.pointer),
                    palette.style(Role::Muted),
                ),
            ],
        );
    }
    if artifacts == 0 {
        lines.push(Line::styled("  none", palette.style(Role::Muted)));
    }

    lines.push(Line::styled("Mail", palette.style(Role::SectionHeading)));
    let mut mail = 0_usize;
    for item in agent.inbox() {
        mail = mail.saturating_add(1);
        next(
            &mut lines,
            vec![
                Line::from(vec![
                    Span::styled("<- ", palette.style(Role::NewInformation)),
                    Span::styled(item.from.to_string(), palette.style(Role::Body)),
                ]),
                Line::styled(format!("  {}", item.summary), palette.style(Role::Muted)),
            ],
        );
    }
    if mail == 0 {
        lines.push(Line::styled("  none", palette.style(Role::Muted)));
    }
    lines
}

/// Queued background requests, oldest first, with the cursor on the one `Enter` would go to.
///
/// Approval and clarification are drawn apart because `ui-ux.md` §attention management refuses one
/// generic notification treatment: one is an agent that cannot proceed, the other is an agent that
/// can. Seen requests stay listed and stop shouting — acknowledging is not resolving (ATT-3).
pub(crate) fn attention(state: &ViewState, palette: &Palette) -> Vec<Line<'static>> {
    let cursor = state.attention_cursor();
    state
        .attention()
        .enumerate()
        .map(|(index, item)| {
            let (marker, role) = match (item.acknowledged, item.kind) {
                (true, _) => ("seen  ", Role::Muted),
                (false, AttentionKind::Approval) => ("block ", Role::ActionRequired),
                (false, AttentionKind::Clarification) => ("ask   ", Role::NewInformation),
            };
            let (caret, caret_role) = if index == cursor {
                ("> ", Role::Accent)
            } else {
                ("  ", Role::Muted)
            };
            Line::from(vec![
                Span::styled(caret, palette.style(caret_role)),
                Span::styled(marker, palette.style(role)),
                Span::styled(format!("{} · ", item.agent_id), palette.style(Role::Muted)),
                Span::styled(item.summary.clone(), palette.style(Role::Body)),
            ])
        })
        .collect()
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

/// Tool markers stay legible without colour so monochrome terminals keep the same status grammar.
const fn tool_marker(status: ToolCallStatus) -> &'static str {
    match status {
        ToolCallStatus::Queued => "[ ]",
        ToolCallStatus::Running => "[~]",
        ToolCallStatus::Succeeded => "[+]",
        ToolCallStatus::Failed => "[!]",
        ToolCallStatus::Cancelled => "[-]",
    }
}
