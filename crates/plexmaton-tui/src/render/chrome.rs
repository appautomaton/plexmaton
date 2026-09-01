//! Titles, borders, and the two regions that are nothing but chrome.
//!
//! Separated from the renderer because they answer a different question. That file decides what a
//! surface draws and how much of it a frame builds; this one decides what a region *says about
//! itself* — the name in its border, and the colour that name is allowed to carry.

use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::{
    ViewState, content,
    layout::{MIN_HEIGHT, MIN_WIDTH},
    theme::{Palette, Role},
};

pub(super) fn agents_title(state: &ViewState) -> String {
    format!(" Agents · attention {} ", state.attention_count())
}

/// An unanswered request must read as action required, not as ambient decoration.
pub(super) fn attention_role(state: &ViewState) -> Role {
    if state.attention_count() == 0 {
        Role::Muted
    } else {
        Role::ActionRequired
    }
}

pub(super) fn transcript_title(state: &ViewState) -> String {
    state.selected_agent().map_or_else(
        || " Transcript ".to_owned(),
        |agent| {
            format!(
                " {} · {} ",
                agent.label,
                content::agent_status_label(agent.status)
            )
        },
    )
}

/// The title carries the presentation state, because pin and maximize have no other signal.
pub(super) fn inspector_title(state: &ViewState) -> String {
    let Some(open) = state.inspector() else {
        return " Inspector ".to_owned();
    };
    let label = state
        .agent(&open.agent)
        .map_or_else(|| open.agent.to_string(), |agent| agent.label.clone());
    let pin = if open.pinned { " · pinned" } else { "" };
    format!(" {label}{pin} · esc ")
}

pub(super) fn notices_title(state: &ViewState) -> String {
    let retained = state.notices().count();
    let dropped = state.notices_dropped();
    if dropped == 0 {
        format!(" Notices · {retained} ")
    } else {
        format!(" Notices · {retained} · {dropped} discarded ")
    }
}

/// The title names the target, which keeps the binding visible rather than remembered when the
/// selection is on a different agent (COM-4).
pub(super) fn composer_title(state: &ViewState) -> String {
    state.primary_agent().map_or_else(
        || " Message ".to_owned(),
        |agent| format!(" Message {} ", agent.label),
    )
}

pub(super) fn render_footer(frame: &mut Frame<'_>, palette: &Palette, area: Rect) {
    // Escape resolves the topmost layer and never quits, so the hint must not offer it as an exit.
    let footer = Line::from(vec![
        Span::styled(" ↑↓ ", palette.style(Role::KeyHint)),
        Span::styled(" select  ·  ", palette.style(Role::Muted)),
        Span::styled(" ⇥ ", palette.style(Role::KeyHint)),
        Span::styled(" focus  ·  ", palette.style(Role::Muted)),
        Span::styled(" q ", palette.style(Role::KeyHint)),
        Span::styled(" quit", palette.style(Role::Muted)),
    ]);
    frame.render_widget(Paragraph::new(footer), area);
}

pub(super) fn render_too_small(frame: &mut Frame<'_>, palette: &Palette, area: Rect) {
    // One honest notice. Clipping the workspace instead would show a layout that misrepresents
    // both the agents and the controls.
    let lines = vec![
        Line::styled("Terminal too small", palette.style(Role::ActionRequired)),
        Line::raw(""),
        Line::styled(
            format!("Need at least {MIN_WIDTH} x {MIN_HEIGHT}."),
            palette.style(Role::Body),
        ),
        Line::styled(
            format!("This one is {} x {}.", area.width, area.height),
            palette.style(Role::Muted),
        ),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

/// A bordered region.
///
/// The border carries focus and the title carries attention, so the two never compete for the same
/// pixels and a focused panel with a pending request still reads as both.
pub(super) fn block(
    palette: &Palette,
    title: String,
    title_role: Role,
    focused: bool,
) -> Block<'static> {
    let border = if focused {
        Role::BorderFocused
    } else {
        Role::Border
    };
    Block::default()
        .borders(Borders::ALL)
        .border_style(palette.style(border))
        .title(Span::styled(title, palette.style(title_role)))
}
