//! The roster: who exists, what each one is doing, and which one needs the user.
//!
//! This panel is the product's index. Several agents run at once and only some of them want
//! anything, so the list's job is not to enumerate them but to order them: a row the user must act
//! on is read first, and ambient work is read last or not at all. Ordering and colour are the whole
//! mechanism — there is no second surface announcing requests, because a panel that already names
//! every agent is where the user looks for one.
//!
//! One agent, one row, three columns that line up down the list (ui-ux §agents strip). The strip
//! above the user's conversation shows at most [`STRIP_AGENTS`] of them; the narrow navigator has
//! the whole region and shows everyone. Both ask for the same rows, so an agent reads the same way
//! in either place.

use plexmaton_core::{AgentId, AgentStatus, AttentionKind};
use ratatui::{
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};
use unicode_width::UnicodeWidthStr;

use super::agent_status_label;
use crate::{
    AgentView, ViewState,
    theme::{Palette, Role, agent_role},
};

/// The most agents the strip shows, however many are running.
///
/// The strip is a header, and a header that grows with the roster stops being one. At three, the
/// conversation moves by the same number of rows whether two delegates are running or twenty, so
/// starting one never costs the user their place. Nothing is hidden by the cap: the panel's title
/// says how many exist, `Ctrl-B` opens the full list, and the three that survive it are the three
/// the order already ranks first — which are the ones addressed to the user.
pub(crate) const STRIP_AGENTS: usize = 3;

/// Width of the state column: the longest word [`lifecycle`] can return.
///
/// Fixed rather than measured, so the third column starts in the same cell on every row and a scan
/// down the strip finds the tallies without reading the states beside them.
const STATE_COLUMN: usize = "cancelled".len();

/// Columns between one field and the next.
const GAP: usize = 2;

/// The column between the panel's edge and the first name.
///
/// Every other panel's content stands off its border; the roster did not, because in a
/// twenty-six-column rail the cell was worth more than the margin. At the conversation's width it
/// is not.
const GUTTER: usize = 1;

/// Rows the panel paints, and which agent each one belongs to.
///
/// The owners travel with the lines instead of being re-derived from the roster, because the two
/// no longer line up: the order is attention's rather than arrival's, and the strip shows a
/// bounded window onto it. Recomputing that a second time for hit testing is how a click lands on
/// the agent above the one under the pointer.
pub(crate) struct RosterRows {
    pub(crate) lines: Vec<Line<'static>>,
    owners: Vec<Option<AgentId>>,
}

/// Where a row sits in the attention hierarchy, which is the order the panel reads in.
///
/// Failure outranks a request because a broken agent is not going to ask; a request outranks
/// everything else because it is the only class that is addressed to the user. Ambient work sorts
/// last and stays last: the whole point of the class is that the user was not asked to read it.
/// Under a cap the ranking decides more than the order — it decides who is on screen at all.
fn rank(agent: &AgentView, request: Option<&crate::AttentionView>) -> u8 {
    match () {
        () if agent.status == AgentStatus::Failed => 2,
        // A request the user has already been to is still outstanding and still listed, but it has
        // stopped asking: they saw it, so it sorts with the work rather than above it (ATT-3).
        () if request.is_some_and(|item| !item.acknowledged) => 1,
        () => 0,
    }
}

/// The row's second column: what the agent wants, when it wants something.
///
/// `waiting` is true of an agent blocked on a tool and of one blocked on the user, and only the
/// second is the user's problem, so the request's own kind replaces the lifecycle word when there
/// is a request. The name of the thing to do is more use than the name of the state it induces.
fn lifecycle(agent: &AgentView, kind: Option<AttentionKind>) -> &'static str {
    match kind {
        Some(AttentionKind::Approval) => "approval",
        Some(AttentionKind::Clarification) => "ask",
        None => agent_status_label(agent.status),
    }
}

/// The row's third column: why this agent is worth looking at.
///
/// An ask outranks a tally, so the field that survives is chosen by attention rather than by a
/// fixed schema. A working agent's counts say how much it has done; a waiting agent's counts are
/// beside the point next to the sentence it is blocked on.
fn detail(agent: &AgentView, summary: Option<&str>) -> String {
    if let Some(summary) = summary {
        return summary.to_owned();
    }
    super::tally(agent).trim_start().to_owned()
}

/// Cut to a width, saying so. A silently shortened ask reads as a different ask.
fn clip(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    for grapheme in unicode_segmentation::UnicodeSegmentation::graphemes(text, true) {
        if out.width() + grapheme.width() + 1 > width {
            break;
        }
        out.push_str(grapheme);
    }
    out.push('…');
    out
}

/// The agents the panel has room for, in attention's order.
///
/// The cap is applied here rather than left to the panel to clip, because a clipped row is still a
/// row the pointer can be told about. The selected agent is kept on screen whatever its rank: the
/// row the next `Enter` acts on cannot be one the user cannot see, so it takes the last place
/// rather than the ordering's own occupant.
fn window<'a>(
    ordered: &[(&'a AgentView, Option<&'a crate::AttentionView>)],
    capacity: usize,
    cursor: Option<&AgentId>,
) -> Vec<(&'a AgentView, Option<&'a crate::AttentionView>)> {
    let mut kept: Vec<_> = ordered.iter().take(capacity).copied().collect();
    if kept.len() == ordered.len() {
        return kept;
    }
    if let Some(cursor) = cursor
        && !kept.iter().any(|(agent, _)| &agent.id == cursor)
        && let Some(entry) = ordered.iter().find(|(agent, _)| &agent.id == cursor)
        && let Some(last) = kept.last_mut()
    {
        *last = *entry;
    }
    kept
}

/// Columns the name may take before it starts crowding out the ask beside it.
///
/// The longest name on screen, so the states line up, but never more than a third of the panel: a
/// delegate labelled with a sentence would otherwise push the one field addressed to the user off
/// the right-hand edge.
fn name_column(shown: &[(&AgentView, Option<&crate::AttentionView>)], width: usize) -> usize {
    shown
        .iter()
        .map(|(agent, _)| agent.label.width())
        .max()
        .unwrap_or(1)
        .clamp(1, (width / 3).max(1))
}

/// The list of sub-agents: identity, lifecycle, and which one is being looked at.
///
/// The primary is not in it. Its conversation is the screen, and looking at it is looking at
/// nobody else (INS-1). `capacity` is how many rows the caller has to give: [`STRIP_AGENTS`] in
/// the strip, the whole roster in the narrow navigator.
pub(crate) fn roster(
    state: &ViewState,
    palette: &Palette,
    width: u16,
    capacity: usize,
) -> RosterRows {
    let mut rows = RosterRows {
        lines: Vec::new(),
        owners: Vec::new(),
    };
    if state.sub_agents().next().is_none() {
        rows.lines.push(Line::styled(
            "No sub-agents yet.",
            palette.style(Role::Muted),
        ));
        rows.owners.push(None);
        return rows;
    }

    let selected = state.roster_cursor_agent().map(|agent| agent.id.clone());
    let mut ordered: Vec<(&AgentView, Option<&crate::AttentionView>)> = state
        .sub_agents()
        .map(|agent| (agent, state.agent_request(&agent.id)))
        .collect();
    ordered.sort_by_key(|(agent, request)| std::cmp::Reverse(rank(agent, *request)));
    let shown = window(&ordered, capacity, selected.as_ref());

    let inner = usize::from(width);
    let usable = inner.saturating_sub(GUTTER);
    let name_col = name_column(&shown, usable);
    let detail_col = usable.saturating_sub(name_col + STATE_COLUMN + GAP * 2);
    for (agent, request) in shown {
        let role = match request {
            _ if agent.status == AgentStatus::Failed => Role::Failure,
            Some(item) if !item.acknowledged => Role::ActionRequired,
            // Seen, still outstanding: the row keeps saying what the agent wants and stops
            // competing for the attention it has already had.
            Some(_) => Role::Muted,
            None => agent_role(agent.status),
        };
        // The name owns the first column outright: bold, in the colour of the state it is in. The
        // row the cursor is on takes `Chosen`'s ground under the whole line and keeps its own
        // colour on top, so one row answers both questions without either channel giving way.
        // Rejected: a `●`/`○` marker in the first cell, which spent the two columns a name wants
        // and said in a private glyph what the workspace already says with `Chosen` everywhere
        // else.
        let mut style = palette.style(role);
        if selected.as_ref() == Some(&agent.id) {
            style.bg = palette.style(Role::Chosen).bg;
        }
        let name = clip(&agent.label, name_col);
        // An ask outranks a tally, so a request replaces the counts rather than crowding in beside
        // them. The tail is padded to the panel's width so the cursor's ground runs the whole row
        // rather than stopping where the text does.
        let word = lifecycle(agent, request.map(|item| item.kind()));
        let rest = clip(
            &detail(agent, request.map(crate::AttentionView::summary)),
            detail_col,
        );
        let gap = " ".repeat(GAP);
        let tail = format!("{gap}{word:<STATE_COLUMN$}{gap}{rest}");
        // The gutter rides on the name's span rather than standing outside the row, so the
        // cursor's ground starts at the panel's edge instead of one cell in.
        rows.lines.push(Line::from(vec![
            Span::styled(
                format!("{:gutter$}{name:<name_col$}", "", gutter = GUTTER),
                style.add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "{tail:<rest_col$}",
                    rest_col = usable.saturating_sub(name_col)
                ),
                style,
            ),
        ]));
        rows.owners.push(Some(agent.id.clone()));
    }
    rows
}

/// How many sub-agents exist, for a panel that is showing only some of them.
#[must_use]
pub(crate) fn population(state: &ViewState) -> usize {
    state.sub_agents().count()
}

/// Rows a panel this tall gives the list: its interior, inside the box.
///
/// One answer for the renderer and for the pointer, because a row that was painted and a row that
/// is hit-tested have to be the same row. The strip's own cap reaches the list this way rather
/// than as a second rule here: layout sized the rectangle to [`STRIP_AGENTS`], and the list fills
/// what it was given.
#[must_use]
pub(crate) fn capacity(bounds: Rect) -> usize {
    usize::from(bounds.height.saturating_sub(2))
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
    capacity: usize,
    row: usize,
) -> Option<AgentId> {
    let rows = roster(state, palette, width, capacity);
    let mut first = 0_usize;
    for (line, owner) in rows.lines.iter().zip(&rows.owners) {
        let painted = Paragraph::new(line.clone())
            .wrap(Wrap { trim: false })
            .line_count(width)
            .max(1);
        if (first..first.saturating_add(painted)).contains(&row) {
            return owner.clone();
        }
        first = first.saturating_add(painted);
    }
    None
}

#[cfg(test)]
mod tests;
