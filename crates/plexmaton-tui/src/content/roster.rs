//! The roster: who exists, what each one is doing, and which one needs the user.
//!
//! This panel is the product's index. Several agents run at once and only some of them want
//! anything, so the list's job is not to enumerate them but to order them: a row the user must act
//! on is read first, and ambient work is read last or not at all. Ordering, colour and the ruled
//! break between the two groups are the whole mechanism — there is no second surface announcing
//! requests, because a panel that already names every agent is where the user looks for one.

use plexmaton_core::{AgentId, AgentStatus, AttentionKind};
use ratatui::{
    style::Modifier,
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
fn rank(agent: &AgentView, request: Option<&crate::AttentionView>) -> u8 {
    match () {
        () if agent.status == AgentStatus::Failed => 2,
        // A request the user has already been to is still outstanding and still listed, but it has
        // stopped asking: they saw it, so it sorts with the work rather than above it (ATT-3).
        () if request.is_some_and(|item| !item.acknowledged) => 1,
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
    let counts = count_entries(agent);
    // A glyph a scan recognises without reading, so a 26-column row can carry the state word and
    // the tally together. These are Nerd Font Private Use codepoints, the same dependency the
    // transcript's copy affordance already takes; a terminal font without them draws a box, so
    // they are named here once and never spelled inline.
    const TOOLS: char = '\u{f1323}'; // md-hammer_wrench
    const TASKS: char = '\u{f0756}'; // md-format_list_checks
    const MAIL: char = '\u{f01ee}'; // md-email
    let mut parts: Vec<String> = Vec::new();
    for (glyph, count) in [
        (TOOLS, counts.tools),
        (TASKS, counts.tasks),
        (MAIL, counts.mail),
    ] {
        if count > 0 {
            parts.push(format!("{glyph} {count}"));
        }
    }
    if counts.artifacts > 0 {
        parts.push(format!("@{}", counts.artifacts));
    }
    parts.join("  ")
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

    let selected = state.roster_cursor_agent().map(|agent| agent.id.clone());
    let mut ordered: Vec<(&AgentView, Option<&crate::AttentionView>)> = state
        .sub_agents()
        .map(|agent| (agent, state.agent_request(&agent.id)))
        .collect();
    ordered.sort_by_key(|(agent, request)| std::cmp::Reverse(rank(agent, *request)));
    let waiting = ordered
        .iter()
        .filter(|(agent, request)| rank(agent, *request) > 0)
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

        let role = match request {
            _ if agent.status == AgentStatus::Failed => Role::Failure,
            Some(item) if !item.acknowledged => Role::ActionRequired,
            // Seen, still outstanding: the row keeps saying what the agent wants and stops
            // competing for the attention it has already had.
            Some(_) => Role::Muted,
            None => agent_role(agent.status),
        };
        // The name owns the first row outright: bold, in the colour of the state it is in, with
        // the whole column to be long in. The row the cursor is on takes `Chosen`'s ground under
        // it and keeps its own colour on top, so one row answers both questions without either
        // channel having to give way. Rejected: a `●`/`○` marker in the first cell, which spent
        // the two columns a name wants and said in a private glyph what the workspace already
        // says with `Chosen` everywhere else.
        let cursor = selected.as_ref() == Some(&agent.id);
        let mut name = palette.style(role).add_modifier(Modifier::BOLD);
        if cursor {
            name.bg = palette.style(Role::Chosen).bg;
        }
        let label = clip(&agent.label, inner);
        rows.lines
            .push(Line::from(Span::styled(format!("{label:<inner$}"), name)));
        rows.owners.push(Some(agent.id.clone()));

        // The second row is the state and what is waiting in it. An ask outranks a tally, so a
        // request replaces the counts rather than crowding in beside them.
        let word = lifecycle(agent, request.map(|item| item.kind()));
        let rest = detail(agent, request.map(crate::AttentionView::summary));
        let second = if rest.is_empty() {
            word.to_owned()
        } else {
            format!("{word}  {rest}")
        };
        rows.lines.push(Line::from(Span::styled(
            clip(&second, inner),
            palette.style(role),
        )));
        rows.owners.push(Some(agent.id.clone()));
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

    use super::{Modifier, agent_at_row, roster};
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
        // An agent's first row is its name and the one under it is its state, so the pair is
        // taken where the owner changes rather than by how the row is painted: two of the four
        // attention roles are bold in their own right, and weight cannot tell a name from them.
        let names: Vec<(String, String)> = rows
            .owners
            .iter()
            .enumerate()
            .filter(|(index, owner)| {
                owner.is_some() && (*index == 0 || rows.owners[index - 1] != **owner)
            })
            .map(|(index, _)| {
                (
                    rows.lines[index].to_string().trim_end().to_owned(),
                    rows.lines[index + 1].to_string(),
                )
            })
            .collect();
        assert_eq!(names.len(), 4, "{names:?}");
        assert!(names[0].0.contains("Sol"), "failure leads: {names:?}");
        assert!(names[0].1.contains("failed"), "{names:?}");
        for (name, state) in &names[1..3] {
            assert!(
                name.contains("Vega") || name.contains("Agent B"),
                "requests come next: {names:?}"
            );
            assert!(state.contains("ask"), "and say what they want: {names:?}");
        }
        assert!(
            names[3].0.contains("Orion"),
            "ambient work is last: {names:?}"
        );
        assert!(names[3].1.contains("running"), "{names:?}");

        let ruled = rows
            .lines
            .iter()
            .position(|line| line.to_string().starts_with('─'))
            .unwrap_or_else(|| panic!("the groups are ruled apart: {rows:?}", rows = names));
        // Count agents above the rule, not rows: each one spends two.
        let above = rows.owners[..ruled]
            .iter()
            .enumerate()
            .filter(|(index, owner)| {
                owner.is_some() && (*index == 0 || rows.owners[index - 1] != **owner)
            })
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
            let index = rows
                .lines
                .iter()
                .position(|candidate| candidate.to_string().contains(name))
                .unwrap_or_else(|| panic!("{name} is in the roster"));
            let named = line.spans.first().map(|span| span.style);
            assert_eq!(
                named,
                Some(palette.style(role).add_modifier(Modifier::BOLD)),
                "the name carries its own state, with weight: {name} at {role:?}"
            );
            let state = rows.lines[index + 1].spans.first().map(|span| span.style);
            assert_eq!(
                state,
                Some(palette.style(role)),
                "and the state row carries it without: {name} at {role:?}"
            );
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
        assert!(detail.contains('\u{f1323}'), "tools: {detail:?}");
        assert!(detail.contains("@1"), "{detail:?}");
        // Agent B wrote that letter rather than receiving it. A roster says where the user's work
        // is waiting, and an agent's own outbound letter is not work waiting in it.
        assert!(
            !detail.contains('\u{f01ee}'),
            "a sent letter is counted in the conversation it arrived in, not the one it left: \
             {detail:?}"
        );
    }

    /// A roster counts what arrived for an agent, never what the agent sent.
    ///
    /// One letter lands in both conversations, so counting every addressed entry told each agent
    /// how many letters it had *handled* — a number that answers no question the roster is asked.
    /// The roster says where the user's work is waiting, and an agent's own outbound letter is not
    /// work waiting in it. The two directions are exercised in one fixture so neither can be made
    /// to pass by a rule that simply counts less.
    #[test]
    fn a_roster_counts_the_letters_that_arrived_and_not_the_ones_that_left() {
        let mut conversation = Conversation::canonical();
        // The canonical letter already runs Agent B → Agent A, filed into Agent B's side. File the
        // reply into the same conversation, so it holds one letter each way and nothing but the
        // direction can explain why they count differently.
        conversation.emit(ConversationEvent::MailDelivered {
            agent_id: agent("agent-b"),
            item_id: plexmaton_core::TranscriptItemId::new("reply-to-b")
                .unwrap_or_else(|error| panic!("invalid fixture: {error}")),
            mail_id: plexmaton_core::MailId::new("reply-to-b")
                .unwrap_or_else(|error| panic!("invalid fixture: {error}")),
            from: agent("agent-a"),
            to: agent("agent-b"),
            summary: "Acknowledged; carry on.".to_owned(),
        });
        conversation.emit(ConversationEvent::AttentionResolved {
            agent_id: agent("agent-b"),
            attention_id: AttentionId::new("attention-b-1")
                .unwrap_or_else(|error| panic!("invalid fixture: {error}")),
        });

        let counts = crate::content::entry_counts(
            conversation
                .state
                .agent(&agent("agent-b"))
                .unwrap_or_else(|| panic!("the canonical timeline creates Agent B")),
        );
        assert!(
            counts.contains("1 mail"),
            "Agent B received exactly one of the two letters: {counts:?}"
        );

        // Both letters are in Agent B's conversation, so a rule that counted entries rather than
        // arrivals would say two here. That is the number this test exists to refuse.
        assert!(
            !counts.contains("2 mail"),
            "the letter Agent B wrote is counted where it arrived, not where it left: {counts:?}"
        );
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
        // Every agent spends exactly two rows: its name, and the state row under it. The state
        // row is never empty, because an agent always has a lifecycle even when it has no tally.
        assert_eq!(seen, 8, "four agents, two rows each");
        assert!(agent_at_row(&state, &palette, RAIL, rows.lines.len() + 4).is_none());
    }
}
