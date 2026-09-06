//! Titles, borders, and the two regions that are nothing but chrome.
//!
//! Separated from the renderer because they answer a different question. That file decides what a
//! surface draws and how much of it a frame builds; this one decides what a region *says about
//! itself* — the name in its border, and the colour that name is allowed to carry.

use ratatui::{
    Frame,
    layout::Rect,
    style::Modifier,
    symbols::border,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use super::panel::Edges;
use crate::{
    ViewState, content,
    layout::{MIN_HEIGHT, MIN_WIDTH},
    state::{CurrentWork, StatusNote},
    surface::SurfaceId,
    theme::{Palette, Role},
};

/// A panel title: what the panel is, in the heading role, then what it says about itself, muted.
///
/// The name is the thing a user scans for; the status, the counts and the way out are secondary
/// and read that way. Section headings inside a panel share the heading role, so a panel's name
/// is never quieter than a section it contains.
pub(super) fn title(
    palette: &Palette,
    name: impl Into<String>,
    name_role: Role,
    rest: impl Into<String>,
) -> Line<'static> {
    title_with(palette, name, name_role, rest, Role::Muted)
}

/// A title whose detail carries a role of its own.
fn title_with(
    palette: &Palette,
    name: impl Into<String>,
    name_role: Role,
    rest: impl Into<String>,
    rest_role: Role,
) -> Line<'static> {
    Line::from(vec![
        Span::raw(" "),
        Span::styled(name.into(), palette.style(name_role)),
        Span::styled(rest.into(), palette.style(rest_role)),
        Span::raw(" "),
    ])
}

/// The rail names the rail. What is unanswered is the pill's, on the conversation the user is in.
///
/// Rejected: `Agents · !n`. It put the count on the panel furthest from where the user is reading,
/// beside a list whose own rows already carry each agent's badge, so the same fact was on screen
/// three times and the one place it mattered was not one of them.
pub(super) fn agents_title(palette: &Palette) -> Line<'static> {
    title(palette, "Agents", Role::SectionHeading, "")
}

/// The pill: `( !n )` at the far end of the conversation's top border while `n` are unanswered.
///
/// Chrome, not a surface — it takes no rows, no focus and no pointer target, which is what lets it
/// sit on a border at all (ATT-1). It counts what is still unanswered rather than what is queued: a
/// queue of five the user has already been to is not five things demanding them (ATT-3). The
/// brackets are the shape, `!` is the word, and the colour is third, so it survives monochrome.
///
/// The request open in the decision region counts too, even though it is on screen below. The pill
/// is the workspace's one answer to "is anything waiting on me", and a status indicator that goes
/// dark in the one case where the answer is loudest is not one. The band is the surface that
/// avoids drawing the same request twice; the pill is a number, and the number is still true.
///
/// Rejected: a full-width band above the workspace for the same fact. It cost four rows and the
/// top of the screen to say a number, pushed the conversation down whenever an agent asked
/// anything, and drew the request a second time beside the region already asking it.
pub(super) fn attention_pill(state: &ViewState, palette: &Palette) -> Option<Line<'static>> {
    let pending = state.attention_pending();
    if pending == 0 {
        return None;
    }
    // `DIM` is cleared rather than left alone: a title is patched over the border row it sits on,
    // and the border is drawn dim, so a role that only sets a colour inherits the dimming meant
    // for the frame. The same trap the section heading's explicit `Reset` foreground avoids.
    let lit = |role| palette.style(role).remove_modifier(Modifier::DIM);
    Some(Line::from(vec![
        Span::styled(" (", lit(Role::Muted)),
        Span::styled(format!(" !{pending} "), lit(Role::ActionRequired)),
        Span::styled(") ", lit(Role::Muted)),
    ]))
}

/// The band names both numbers, because it is the surface that can show the difference.
pub(super) fn attention_title(state: &ViewState, palette: &Palette) -> Line<'static> {
    let queued = state.attention_listed_count();
    let pending = state.attention_listed_pending();
    let rest = if pending == queued {
        format!(" · {queued}")
    } else {
        format!(" · {queued} · {pending} unanswered")
    };
    title(palette, "Attention", attention_role(state), rest)
}

/// An unanswered request must read as action required, not as ambient decoration.
pub(super) fn attention_role(state: &ViewState) -> Role {
    if state.attention_listed_pending() == 0 {
        Role::SectionHeading
    } else {
        Role::ActionRequired
    }
}

pub(super) fn transcript_title(state: &ViewState, palette: &Palette) -> Line<'static> {
    state.primary_agent().map_or_else(
        || title(palette, "Transcript", Role::SectionHeading, ""),
        |agent| {
            let rest = format!(
                " · {}{}{}",
                content::agent_status_label(agent.status),
                content::entry_counts(agent),
                selected_suffix(state, SurfaceId::Transcript)
            );
            title(palette, agent.label.clone(), Role::SectionHeading, rest)
        },
    )
}

/// What a surface says about the selection it is holding.
///
/// A copy leaves no trace of its own — OSC 52 is written and never answered — so the selection
/// staying visible, and counted, is the whole of the feedback the user gets (SEL-5).
fn selected_suffix(state: &ViewState, surface: SurfaceId) -> String {
    if let Some(note) = state.copy_note(surface) {
        use crate::state::CopyNote;
        return match note {
            CopyNote::Preparing => " · preparing copy",
            CopyNote::Unavailable => " · copy unavailable",
            CopyNote::Capacity => " · copy size limit",
            CopyNote::Changed => " · selected text changed",
        }
        .into();
    }
    state.selection().map_or_else(String::new, |selection| {
        if selection.surface == surface {
            if selection.is_text() {
                " · text selected".into()
            } else {
                format!(" · {} selected", selection.entries())
            }
        } else {
            String::new()
        }
    })
}

/// The title names the agent and the way out.
pub(super) fn inspector_title(state: &ViewState, palette: &Palette) -> Line<'static> {
    // The surface is registered only while an agent is open, so this is no title rather than a
    // word the user would otherwise never see (phase 01 §scope 1).
    let Some(open) = state.inspector() else {
        return title(palette, "", Role::SectionHeading, "");
    };
    let Some(agent) = state.agent(&open.agent) else {
        return title(
            palette,
            open.agent.to_string(),
            Role::SectionHeading,
            " · esc",
        );
    };
    let counts = content::entry_counts(agent);
    let selected = selected_suffix(state, SurfaceId::Inspector);
    title(
        palette,
        agent.label.clone(),
        Role::SectionHeading,
        format!("{counts}{selected} · esc"),
    )
}

pub(super) fn notices_title(state: &ViewState, palette: &Palette) -> Line<'static> {
    let retained = state.notices().count();
    let dropped = state.notices_dropped();
    let rest = if dropped == 0 {
        format!(" · {retained}")
    } else {
        format!(" · {retained} · {dropped} discarded")
    };
    title(palette, "Notices", Role::SectionHeading, rest)
}

/// The title names the target, which keeps the binding visible rather than remembered when the
/// selection is on a different agent (COM-4).
pub(super) fn composer_title(state: &ViewState, palette: &Palette) -> Line<'static> {
    if state.editing_retry() {
        return title_with(
            palette,
            "Editing previous message".to_owned(),
            Role::SectionHeading,
            " · Esc to cancel".to_owned(),
            Role::Muted,
        );
    }
    let name = state.primary_agent().map_or_else(
        || "Message".to_owned(),
        |agent| format!("Message {}", agent.label),
    );
    let (rest, role) = match state.current_work() {
        None => (String::new(), Role::Muted),
        Some(CurrentWork::Thinking) => (" · Thinking".to_owned(), Role::Ambient),
        Some(CurrentWork::Responding) => (" · Responding".to_owned(), Role::Ambient),
        Some(CurrentWork::RunningTool(tool)) => (format!(" · Running {tool}"), Role::Ambient),
        Some(CurrentWork::ApprovalRequired) => {
            (" · Approval required".to_owned(), Role::ActionRequired)
        }
    };
    title_with(palette, name, Role::SectionHeading, rest, role)
}

/// Decoded script rows or the cwd baseline, with system hints on the final terminal row (STL-4).
pub(super) fn render_status(
    frame: &mut Frame<'_>,
    state: &ViewState,
    palette: &Palette,
    area: Rect,
) {
    let status = state.status();
    match status.footer() {
        crate::state::Footer::Script { text, .. } => {
            let clipped = text.lines().len() > usize::from(area.height)
                || text
                    .lines()
                    .iter()
                    .take(usize::from(area.height))
                    .any(|line| line.width() > usize::from(area.width));
            for (index, line) in text
                .lines()
                .iter()
                .take(usize::from(area.height))
                .enumerate()
            {
                let last = index + 1 == usize::from(area.height);
                let width = area.width.saturating_sub(u16::from(clipped && last));
                frame.render_widget(
                    Paragraph::new(line.clone()).style(palette.style(Role::Muted)),
                    Rect::new(area.x, area.y + index as u16, width, 1),
                );
            }
            if clipped && area.width > 0 && area.height > 0 {
                frame.render_widget(
                    Paragraph::new("…").style(palette.style(Role::Muted)),
                    Rect::new(area.right() - 1, area.bottom() - 1, 1, 1),
                );
            }
        }
        crate::state::Footer::Failed(error) => {
            frame.render_widget(
                Paragraph::new(error.as_str()).style(palette.style(Role::Failure)),
                area,
            );
        }
        crate::state::Footer::Default => {}
    }
    if status.note() == StatusNote::Quiet
        && !matches!(status.footer(), crate::state::Footer::Default)
    {
        return;
    }
    let area = Rect::new(
        area.x,
        area.bottom().saturating_sub(1),
        area.width,
        area.height.min(1),
    );
    frame.render_widget(ratatui::widgets::Clear, area);
    let (text, role) = match status.note() {
        StatusNote::QuitArmed { .. } => (
            "press Ctrl-D again to quit".to_owned(),
            Role::ActionRequired,
        ),
        StatusNote::CommandHint { .. } => (
            "press Ctrl-P for the command palette".to_owned(),
            Role::NewInformation,
        ),
        StatusNote::Quiet => (
            status.working_directory().unwrap_or_default().to_owned(),
            Role::Muted,
        ),
    };
    let line = Line::from(vec![
        Span::raw(" "),
        Span::styled(text, palette.style(role)),
    ]);
    frame.render_widget(Paragraph::new(line), area);
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
/// Adds a right-aligned status to a block's top border row, when there is one to add.
pub(super) trait Badged {
    fn badge(self, badge: Option<Line<'static>>) -> Self;
}

impl Badged for Block<'static> {
    fn badge(self, badge: Option<Line<'static>>) -> Self {
        match badge {
            // The top border is the only row a pill can sit on without costing the conversation
            // one, so a section drawn without that edge simply has nowhere to put it.
            Some(badge) => self.title_top(badge.right_aligned()),
            None => self,
        }
    }
}

pub(super) fn block(
    palette: &Palette,
    title: Line<'static>,
    focused: bool,
    edges: Edges,
) -> Block<'static> {
    let border = if focused {
        Role::BorderFocused
    } else {
        Role::Border
    };
    let (borders, set) = match edges {
        Edges::All => (Borders::ALL, border::PLAIN),
        Edges::Upper => (Borders::TOP | Borders::LEFT | Borders::RIGHT, border::PLAIN),
        Edges::Closing => (
            Borders::LEFT | Borders::RIGHT | Borders::BOTTOM,
            border::PLAIN,
        ),
        // The divider joins the sides it sits between, so the sections read as one box.
        Edges::Lower => (
            Borders::ALL,
            border::Set {
                top_left: "├",
                top_right: "┤",
                ..border::PLAIN
            },
        ),
        Edges::Middle => (
            Borders::TOP | Borders::LEFT | Borders::RIGHT,
            border::Set {
                top_left: "├",
                top_right: "┤",
                ..border::PLAIN
            },
        ),
    };
    let block = Block::default()
        .borders(borders)
        .border_set(set)
        .border_style(palette.style(border));
    // An empty title is no title. Ratatui still reserves the top row for one when the block has
    // no top edge, which would leave a one-row region with nowhere to paint its row.
    if title.spans.iter().all(|span| span.content.is_empty()) {
        block
    } else {
        block.title(title)
    }
}
