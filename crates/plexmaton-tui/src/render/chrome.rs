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
    surface::SurfaceId,
    theme::{Palette, Role},
};

/// The rail counts what is still unanswered, because that is the number that means anything.
///
/// A queue of five the user has been to is not five things demanding them, so counting the total
/// here would keep the workspace shouting after they had done exactly what was asked (ATT-3).
pub(super) fn agents_title(state: &ViewState) -> String {
    format!(" Agents · attention {} ", state.attention_pending())
}

/// The band names both numbers, because it is the surface that can show the difference.
pub(super) fn attention_title(state: &ViewState) -> String {
    let queued = state.attention_count();
    let pending = state.attention_pending();
    if pending == queued {
        format!(" Attention · {queued} ")
    } else {
        format!(" Attention · {queued} · {pending} unanswered ")
    }
}

/// An unanswered request must read as action required, not as ambient decoration.
pub(super) fn attention_role(state: &ViewState) -> Role {
    if state.attention_pending() == 0 {
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
                " {} · {}{} ",
                agent.label,
                content::agent_status_label(agent.status),
                selected_suffix(state, SurfaceId::Transcript)
            )
        },
    )
}

/// What a surface says about the selection it is holding.
///
/// A copy leaves no trace of its own — OSC 52 is written and never answered — so the selection
/// staying visible, and counted, is the whole of the feedback the user gets (SEL-5).
fn selected_suffix(state: &ViewState, surface: SurfaceId) -> String {
    state.selection().map_or_else(String::new, |selection| {
        if selection.surface == surface {
            format!(" · {} selected", selection.entries())
        } else {
            String::new()
        }
    })
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
    let selected = selected_suffix(state, SurfaceId::Inspector);
    format!(" {label}{pin}{selected} · esc ")
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

/// One key hint, and how readily the strip gives it up when there is no room.
///
/// `sheds` is a rank, not a width: which hints survive a narrow terminal is an editorial decision
/// about what a user cannot work without, and reading order stays fixed whichever ones do.
struct Hint {
    key: &'static str,
    label: &'static str,
    sheds: u8,
}

/// Escape resolves the topmost layer and never quits, so no hint offers it as an exit. An arrow
/// means "move inside what holds focus" (INV-10), so the verb is the surface-independent one rather
/// than "select", which is only what it does in the rail.
const HINTS: [Hint; 6] = [
    Hint {
        key: "↑↓",
        label: "move",
        sheds: 3,
    },
    Hint {
        key: "⇥",
        label: "focus",
        sheds: 0,
    },
    Hint {
        key: "⏎",
        label: "open",
        sheds: 4,
    },
    Hint {
        key: "⇧↑↓",
        label: "select",
        sheds: 5,
    },
    Hint {
        key: "^y",
        label: "copy",
        sheds: 5,
    },
    Hint {
        key: "q",
        label: "quit",
        sheds: 1,
    },
];

/// Draws as many hints as fit, shedding the least essential first.
///
/// A clipped strip is worse than a shorter one: the hint that gets cut in half is always the last,
/// so a fixed list would silently lose `quit` on every narrow terminal — which is what it did until
/// the narrow render test caught it.
pub(super) fn render_footer(frame: &mut Frame<'_>, palette: &Palette, area: Rect) {
    let mut keep = HINTS.iter().map(|hint| hint.sheds).max().unwrap_or(0);
    while keep > 0 && hints(palette, keep).width() > usize::from(area.width) {
        keep = keep.saturating_sub(1);
    }
    frame.render_widget(Paragraph::new(hints(palette, keep)), area);
}

fn hints(palette: &Palette, keep: u8) -> Line<'static> {
    let mut spans = Vec::new();
    for hint in HINTS.iter().filter(|hint| hint.sheds <= keep) {
        if !spans.is_empty() {
            spans.push(Span::styled("  ·  ", palette.style(Role::Muted)));
        }
        spans.push(Span::styled(
            format!(" {} ", hint.key),
            palette.style(Role::KeyHint),
        ));
        spans.push(Span::styled(
            format!(" {}", hint.label),
            palette.style(Role::Muted),
        ));
    }
    Line::from(spans)
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
