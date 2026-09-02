//! Titles, borders, and the two regions that are nothing but chrome.
//!
//! Separated from the renderer because they answer a different question. That file decides what a
//! surface draws and how much of it a frame builds; this one decides what a region *says about
//! itself* — the name in its border, and the colour that name is allowed to carry.

use ratatui::{
    Frame,
    layout::Rect,
    symbols::border,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use super::panel::Edges;
use crate::{
    ViewState, content,
    layout::{MIN_HEIGHT, MIN_WIDTH},
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

/// A title whose detail carries a role of its own, for the one detail that is not muted.
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

/// The rail carries a badge, `!n`, only while `n` requests are unanswered, and nothing otherwise.
///
/// The word is the band's: it sits under the notice strip whenever the queue has something to
/// say, so the rail repeating `attention` would say it twice and the number would drown in it.
/// The badge is the number that means anything, what is still unanswered; a queue of five the
/// user has been to is not five things demanding them, so counting the total here would keep the
/// workspace shouting after they had done exactly what was asked (ATT-3). The panel's name keeps
/// the heading role, so the colour lands on the badge and not on the word beside it.
pub(super) fn agents_title(state: &ViewState, palette: &Palette) -> Line<'static> {
    title_with(
        palette,
        "Agents",
        Role::SectionHeading,
        attention_badge(state),
        Role::ActionRequired,
    )
}

/// `" · !n"` while `n` requests are unanswered; empty otherwise, so the title says nothing.
pub(super) fn attention_badge(state: &ViewState) -> String {
    match state.attention_pending() {
        0 => String::new(),
        pending => format!(" · !{pending}"),
    }
}

/// The band names both numbers, because it is the surface that can show the difference.
pub(super) fn attention_title(state: &ViewState, palette: &Palette) -> Line<'static> {
    let queued = state.attention_count();
    let pending = state.attention_pending();
    let rest = if pending == queued {
        format!(" · {queued}")
    } else {
        format!(" · {queued} · {pending} unanswered")
    };
    title(palette, "Attention", attention_role(state), rest)
}

/// An unanswered request must read as action required, not as ambient decoration.
pub(super) fn attention_role(state: &ViewState) -> Role {
    if state.attention_pending() == 0 {
        Role::SectionHeading
    } else {
        Role::ActionRequired
    }
}

pub(super) fn transcript_title(
    state: &ViewState,
    palette: &Palette,
    with_counts: bool,
) -> Line<'static> {
    state.primary_agent().map_or_else(
        || title(palette, "Transcript", Role::SectionHeading, ""),
        |agent| {
            let rest = format!(
                " · {}{}{}",
                content::agent_status_label(agent.status),
                if with_counts {
                    content::activity_counts(agent)
                } else {
                    String::new()
                },
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
    state.selection().map_or_else(String::new, |selection| {
        if selection.surface == surface {
            format!(" · {} selected", selection.entries())
        } else {
            String::new()
        }
    })
}

/// The title names the agent and the way out.
pub(super) fn inspector_title(
    state: &ViewState,
    palette: &Palette,
    with_counts: bool,
) -> Line<'static> {
    let Some(open) = state.inspector() else {
        return title(palette, "Inspector", Role::SectionHeading, "");
    };
    let Some(agent) = state.agent(&open.agent) else {
        return title(
            palette,
            open.agent.to_string(),
            Role::SectionHeading,
            " · esc",
        );
    };
    let counts = if with_counts {
        content::activity_counts(agent)
    } else {
        String::new()
    };
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
    let name = state.primary_agent().map_or_else(
        || "Message".to_owned(),
        |agent| format!("Message {}", agent.label),
    );
    title(palette, name, Role::SectionHeading, "")
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

/// Escape resolves the topmost layer and never quits, so no hint offers it as an exit, and the
/// quit that is offered is the chord: a bare `q` used to be here and ended sessions from the wrong
/// focus (INV-7). An arrow means "move inside what holds focus" (INV-10), so the verb is the
/// surface-independent one rather than "select", which is only what it does in the rail.
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
        key: "^c",
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
        Edges::None => (Borders::NONE, border::PLAIN),
        Edges::All => (Borders::ALL, border::PLAIN),
        Edges::Upper => (Borders::TOP | Borders::LEFT | Borders::RIGHT, border::PLAIN),
        Edges::Closing => (
            Borders::LEFT | Borders::RIGHT | Borders::BOTTOM,
            border::PLAIN,
        ),
        // The divider joins the sides it sits between, so the two sections read as one box.
        Edges::Lower => (
            Borders::ALL,
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
