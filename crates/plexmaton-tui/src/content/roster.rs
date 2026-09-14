//! The roster: who exists, what each one is doing, and which one needs the user.
//!
//! This panel is the product's index. Several agents run at once and only some of them want
//! anything, so the list's job is not to enumerate them but to order them: a row the user must act
//! on is read first, and ambient work is read last or not at all. Ordering, colour and the ruled
//! break between the two groups are the whole mechanism — there is no second surface announcing
//! requests, because a panel that already names every agent is where the user looks for one.

use plexmaton_core::{AgentId, AgentStatus, AttentionKind};
use ratatui::{
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};
use unicode_width::UnicodeWidthStr;

use super::{agent_status_label, count_entries};
use crate::{
    AgentView, ViewState,
    theme::{Palette, Role, agent_role},
};

/// Rows the panel paints, and which agent each one belongs to.
///
/// The owners travel with the lines instead of being re-derived from the roster, because the two
/// no longer line up: an agent spends more than one row, the groups are ruled apart, and the order
/// is attention's rather than arrival's. Recomputing that a second time for hit testing is how a
/// click lands on the agent above the one under the pointer.
pub(crate) struct RosterRows {
    pub(crate) lines: Vec<Line<'static>>,
    owners: Vec<Option<AgentId>>,
}

/// Where a row sits in the attention hierarchy, which is the order the panel reads in.
///
/// Failure outranks a request because a broken agent is not going to ask; a request outranks
/// everything else because it is the only class that is addressed to the user. Ambient work sorts
/// last and stays last: the whole point of the class is that the user was not asked to read it.
fn rank(agent: &AgentView, asking: bool) -> u8 {
    match () {
        () if agent.status == AgentStatus::Failed => 2,
        () if asking => 1,
        () => 0,
    }
}

/// The word at the row's right edge: what the agent wants, when it wants something.
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

/// The second row: why this agent is worth looking at.
///
/// An ask outranks a tally, so the field that survives is chosen by attention rather than by a
/// fixed schema. A working agent's counts say how much it has done; a waiting agent's counts are
/// beside the point next to the sentence it is blocked on.
fn detail(agent: &AgentView, summary: Option<&str>) -> String {
    if let Some(summary) = summary {
        return summary.to_owned();
    }
    let (tools, artifacts, mail) = count_entries(agent);
    let mut parts: Vec<String> = Vec::new();
    if tools > 0 {
        parts.push(format!("{tools} tool{}", if tools == 1 { "" } else { "s" }));
    }
    if artifacts > 0 {
        parts.push(format!("@{artifacts}"));
    }
    if mail > 0 {
        parts.push(format!("{mail} mail"));
    }
    parts.join(" ")
}

/// Cut to a width, saying so. A silently shortened ask reads as a different ask.
fn clip(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_owned();
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

/// The list of sub-agents: identity, lifecycle, and which one is being looked at.
///
/// The primary is not in it. Its conversation is the screen, and looking at it is looking at
/// nobody else (INS-1).
pub(crate) fn roster(state: &ViewState, palette: &Palette, width: u16) -> RosterRows {
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

    let selected = state.selected_agent().map(|agent| agent.id.clone());
    let mut ordered: Vec<(&AgentView, Option<&crate::AttentionView>)> = state
        .sub_agents()
        .map(|agent| (agent, state.agent_request(&agent.id)))
        .collect();
    ordered.sort_by_key(|(agent, request)| std::cmp::Reverse(rank(agent, request.is_some())));
    let waiting = ordered
        .iter()
        .filter(|(agent, request)| rank(agent, request.is_some()) > 0)
        .count();

    let inner = usize::from(width);
    for (index, (agent, request)) in ordered.iter().enumerate() {
        if index > 0 {
            // The watershed, and then a blank row between agents. One separates what is addressed
            // to the user from what is not; the other keeps each agent's two rows reading as one
            // entry rather than as a wall of alternating lines.
            let ruled = waiting > 0 && index == waiting;
            rows.lines.push(if ruled {
                Line::styled("─".repeat(inner), palette.style(Role::Border))
            } else {
                Line::default()
            });
            rows.owners.push(None);
        }

        let role = if agent.status == AgentStatus::Failed {
            Role::Failure
        } else if request.is_some() {
            Role::ActionRequired
        } else {
            agent_role(agent.status)
        };
        // `●` open, `○` not. The glyph says which conversation is on screen and the colour says
        // what the agent's state is, so the row's first cell — the one a scan reaches first —
        // carries the fact the user is scanning for rather than the one they already know.
        let marker = if selected.as_ref() == Some(&agent.id) {
            "●"
        } else {
            "○"
        };
        let word = lifecycle(agent, request.map(|item| item.kind()));
        let label = clip(&agent.label, inner.saturating_sub(2));
        let mut spans = vec![
            Span::styled(format!("{marker} "), palette.style(role)),
            Span::styled(label.clone(), palette.style(Role::Body)),
        ];
        // The lifecycle word sits at the right edge when both fit on one line. When they do not,
        // it takes the second row and the detail yields: a column this narrow is the wrong size
        // for prose and the right size for a state word, and half an ask is worse than no ask.
        let gap = inner.saturating_sub(label.width() + word.width() + 2);
        let second = if label.width() + word.width() + 3 <= inner {
            spans.push(Span::styled(" ".repeat(gap), palette.style(Role::Body)));
            spans.push(Span::styled(word, palette.style(role)));
            let text = detail(agent, request.map(crate::AttentionView::summary));
            Span::styled(clip(&text, inner.saturating_sub(2)), palette.style(role))
        } else {
            Span::styled(word, palette.style(role))
        };
        rows.lines.push(Line::from(spans));
        rows.owners.push(Some(agent.id.clone()));
        if !second.content.is_empty() {
            rows.lines
                .push(Line::from(vec![Span::raw("  "), second.clone()]));
            rows.owners.push(Some(agent.id.clone()));
        }
    }
    rows
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
    let rows = roster(state, palette, width);
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
mod tests {
    use plexmaton_core::{AgentId, AttentionId, AttentionRequest, ConversationEvent};

    use super::{agent_at_row, roster};
    use crate::{
        test_support::Conversation,
        theme::{Palette, Role},
    };

    const RAIL: u16 = 24;

    fn agent(name: &str) -> AgentId {
        AgentId::new(name).unwrap_or_else(|error| panic!("invalid fixture: {error}"))
    }

    /// A roster with one of each class: failed, asking, working, and the canonical Agent B.
    fn crowded() -> Conversation {
        let mut conversation = Conversation::canonical();
        for (id, label, status) in [
            ("orion", "Orion", plexmaton_core::AgentStatus::Running),
            ("sol", "Sol", plexmaton_core::AgentStatus::Running),
            ("vega", "Vega", plexmaton_core::AgentStatus::Running),
        ] {
            conversation.emit(ConversationEvent::AgentCreated {
                agent_id: agent(id),
                label: label.to_owned(),
                status,
            });
        }
        conversation.emit(ConversationEvent::AgentStatusChanged {
            agent_id: agent("sol"),
            status: plexmaton_core::AgentStatus::Failed,
        });
        conversation.emit(ConversationEvent::AttentionRequested {
            agent_id: agent("vega"),
            attention_id: AttentionId::new("vega-1")
                .unwrap_or_else(|error| panic!("invalid fixture: {error}")),
            request: AttentionRequest::Clarification {
                summary: "Write the migration?".to_owned(),
            },
        });
        conversation
    }

    /// ui-ux §responsive layout classes: the roster is ordered by attention, so the row that needs
    /// the user is the first row read, and the two groups are ruled apart before any name is.
    #[test]
    fn a_roster_reads_failure_then_requests_then_work_and_rules_the_two_groups_apart() {
        let state = crowded().state;
        let rows = roster(&state, &Palette::pastel(), RAIL);
        let names: Vec<String> = rows
            .lines
            .iter()
            .map(|line| line.to_string())
            .filter(|text| text.starts_with('●') || text.starts_with('○'))
            .collect();
        assert_eq!(names.len(), 4, "{names:?}");
        assert!(names[0].contains("Sol"), "failure leads: {names:?}");
        assert!(names[0].contains("failed"), "{names:?}");
        for asking in &names[1..3] {
            assert!(
                asking.contains("Vega") || asking.contains("Agent B"),
                "requests come next: {names:?}"
            );
            assert!(asking.contains("ask"), "and say what they want: {names:?}");
        }
        assert!(
            names[3].contains("Orion"),
            "ambient work is last: {names:?}"
        );
        assert!(names[3].contains("running"), "{names:?}");

        let ruled = rows
            .lines
            .iter()
            .position(|line| line.to_string().starts_with('─'))
            .unwrap_or_else(|| panic!("the groups are ruled apart: {rows:?}", rows = names));
        let above = rows.lines[..ruled]
            .iter()
            .filter(|line| line.to_string().starts_with(['●', '○']))
            .count();
        assert_eq!(
            above, 3,
            "the rule falls after the three that want something"
        );
    }

    /// The colour is the instrument: the row's first cell and its lifecycle word both carry the
    /// agent's place in the attention hierarchy, so a scan down the column finds it.
    #[test]
    fn a_rows_marker_and_word_carry_its_attention_role_at_both_ends() {
        let state = crowded().state;
        let palette = Palette::pastel();
        let rows = roster(&state, &palette, RAIL);
        for (name, role) in [
            ("Sol", Role::Failure),
            ("Vega", Role::ActionRequired),
            ("Orion", Role::Ambient),
        ] {
            let line = rows
                .lines
                .iter()
                .find(|line| line.to_string().contains(name))
                .unwrap_or_else(|| panic!("{name} is in the roster"));
            let ends: Vec<_> = [line.spans.first(), line.spans.last()]
                .into_iter()
                .flatten()
                .map(|span| span.style)
                .collect();
            for style in ends {
                assert_eq!(style, palette.style(role), "{name} at {role:?}");
            }
        }
    }

    /// An ask outranks a tally: the detail row says what the agent wants when it wants something,
    /// and what it has done when it does not.
    #[test]
    fn the_detail_row_is_the_ask_when_there_is_one_and_the_counts_when_there_is_not() {
        let state = crowded().state;
        let rows = roster(&state, &Palette::pastel(), 40);
        let text: Vec<String> = rows.lines.iter().map(|line| line.to_string()).collect();
        let joined = text.join("\n");
        assert!(
            joined.contains("Write the migration?"),
            "the ask is the detail: {joined}"
        );
        let agent_b = text
            .iter()
            .position(|line| line.contains("Agent B"))
            .unwrap_or_else(|| panic!("canonical state includes Agent B: {joined}"));
        assert!(
            text[agent_b + 1].contains("overlap study"),
            "an asking agent shows its ask, not its counts: {:?}",
            text[agent_b + 1]
        );

        // The same agent once the request is answered: counts come back.
        let mut conversation = crowded();
        conversation.emit(ConversationEvent::AttentionResolved {
            agent_id: agent("agent-b"),
            attention_id: AttentionId::new("attention-b-1")
                .unwrap_or_else(|error| panic!("invalid fixture: {error}")),
        });
        let rows = roster(&conversation.state, &Palette::pastel(), 40);
        let text: Vec<String> = rows.lines.iter().map(|line| line.to_string()).collect();
        let agent_b = text
            .iter()
            .position(|line| line.contains("Agent B"))
            .unwrap_or_else(|| panic!("Agent B stays in the roster: {text:?}"));
        let detail = &text[agent_b + 1];
        assert!(detail.contains("tool"), "{detail:?}");
        assert!(detail.contains("@1"), "{detail:?}");
        assert!(detail.contains("mail"), "{detail:?}");
    }

    /// SURF-2: the pointer lands on the agent under it. Owners travel with the lines, so the ruled
    /// break and the blank spacers belong to nobody and a click on one selects nothing.
    #[test]
    fn every_painted_row_resolves_to_the_agent_it_belongs_to_and_chrome_resolves_to_none() {
        let state = crowded().state;
        let palette = Palette::pastel();
        let rows = roster(&state, &palette, RAIL);
        let mut seen = 0_usize;
        for (index, line) in rows.lines.iter().enumerate() {
            let text = line.to_string();
            let owner = agent_at_row(&state, &palette, RAIL, index);
            if text.trim().is_empty() || text.starts_with('─') {
                assert!(owner.is_none(), "row {index} is chrome: {text:?}");
            } else {
                assert!(owner.is_some(), "row {index} names an agent: {text:?}");
                seen = seen.saturating_add(1);
            }
        }
        // Agent B and Vega each have something to say and spend two rows; Sol and Orion have
        // neither an ask nor a tally, and an empty detail row is a row spent saying nothing.
        assert_eq!(seen, 6, "two agents with a detail, two without");
        assert!(agent_at_row(&state, &palette, RAIL, rows.lines.len() + 4).is_none());
    }
}
