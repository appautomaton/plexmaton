use plexmaton_core::{AgentId, AttentionId, AttentionRequest, ConversationEvent};

use super::{GAP, GUTTER, Modifier, STATE_COLUMN, STRIP_AGENTS, agent_at_row, roster};
use crate::{
    test_support::Conversation,
    theme::{Palette, Role},
};

/// The strip above a 120-column conversation.
const STRIP: u16 = 118;

/// Everyone, as the narrow navigator asks for them.
const ALL: usize = usize::MAX;

fn agent(name: &str) -> AgentId {
    AgentId::new(name).unwrap_or_else(|error| panic!("invalid fixture: {error}"))
}

/// A roster with one of each class: failed, asking, working, and the canonical Agent B.
///
/// Sorted, this is `Sol`, `Agent B`, `Vega`, `Orion` — three that want the user and one that does
/// not, which is exactly one more than the strip has room for.
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

/// The rows as plain text, past the panel's gutter — which is where every name starts.
fn text(conversation: &Conversation, width: u16, capacity: usize) -> Vec<String> {
    roster(&conversation.state, &Palette::pastel(), width, capacity)
        .lines
        .iter()
        .map(|line| line.to_string().split_off(GUTTER))
        .collect()
}

/// ui-ux §agents strip: one agent, one row. Attention sets the order, so the row that needs the
/// user is the first row read and ambient work is last.
#[test]
fn a_roster_spends_one_row_an_agent_and_reads_failure_then_requests_then_work() {
    let rows = text(&crowded(), STRIP, ALL);
    assert_eq!(rows.len(), 4, "four agents, four rows: {rows:?}");
    assert!(rows[0].starts_with("Sol"), "failure leads: {rows:?}");
    assert!(rows[0].contains("failed"), "{rows:?}");
    for row in &rows[1..3] {
        assert!(
            row.starts_with("Vega") || row.starts_with("Agent B"),
            "requests come next: {rows:?}"
        );
        assert!(row.contains("ask"), "and say what they want: {rows:?}");
    }
    assert!(
        rows[3].starts_with("Orion"),
        "ambient work is last: {rows:?}"
    );
    assert!(rows[3].contains("running"), "{rows:?}");
}

/// The strip's height is a constant, so starting a fourth delegate never moves the conversation.
#[test]
fn the_strip_stops_at_three_rows_however_many_agents_are_running() {
    let mut conversation = crowded();
    for index in 0..12 {
        conversation.emit(ConversationEvent::AgentCreated {
            agent_id: agent(&format!("extra-{index}")),
            label: format!("Extra {index}"),
            status: plexmaton_core::AgentStatus::Running,
        });
    }
    assert_eq!(conversation.state.sub_agents().count(), 16);
    let rows = text(&conversation, STRIP, STRIP_AGENTS);
    assert_eq!(rows.len(), STRIP_AGENTS, "{rows:?}");
    // The cap keeps the top of the order, which is the part addressed to the user. A cap that
    // took the first three to arrive would have shown three running agents and hidden the failure.
    assert!(rows[0].starts_with("Sol"), "{rows:?}");
    assert!(
        rows.iter().all(|row| !row.starts_with("Extra")),
        "ambient work is what the cap drops: {rows:?}"
    );
    // The navigator has the whole region, so it is bound by nothing but the roster.
    assert_eq!(text(&conversation, STRIP, ALL).len(), 16);
}

/// The row the next `Enter` acts on is on screen, whatever the ordering thinks of it.
#[test]
fn the_selected_agent_keeps_a_row_even_when_the_order_puts_it_past_the_cap() {
    let mut conversation = crowded();
    // Orion is ambient work and sorts last of four, so the cap would drop it.
    let unranked = text(&conversation, STRIP, STRIP_AGENTS);
    assert!(
        unranked.iter().all(|row| !row.starts_with("Orion")),
        "the fixture is only interesting if Orion is off screen: {unranked:?}"
    );

    conversation
        .state
        .select_agent(&agent("orion"))
        .unwrap_or_else(|error| panic!("Orion is in the roster: {error}"));
    let rows = text(&conversation, STRIP, STRIP_AGENTS);
    assert_eq!(rows.len(), STRIP_AGENTS, "{rows:?}");
    assert!(
        rows[STRIP_AGENTS - 1].starts_with("Orion"),
        "the cursor takes the last row rather than the order's own occupant: {rows:?}"
    );
    assert!(
        rows[0].starts_with("Sol"),
        "and takes it from the bottom of the order, not the top: {rows:?}"
    );
}

/// The three columns start in the same cell on every row, which is what a scan down the strip
/// reads. A row whose name is longer than the column is cut, not allowed to push the rest along.
#[test]
fn every_row_starts_its_state_and_detail_in_the_same_cell() {
    let mut conversation = crowded();
    conversation.emit(ConversationEvent::AgentCreated {
        agent_id: agent("long"),
        label: "A delegate whose label is a whole sentence about what it is doing".to_owned(),
        status: plexmaton_core::AgentStatus::Running,
    });
    let rows = text(&conversation, STRIP, ALL);
    // The sentence is longer than a third of the strip, so the name column stops there and the
    // name is cut. What the assertion below proves is that it is cut rather than allowed to push
    // the two columns beside it along.
    let name_col = (usize::from(STRIP) - GUTTER) / 3;
    assert!(
        rows.iter().any(|row| row.starts_with("A delegate whose")),
        "the long name is in the roster: {rows:?}"
    );
    for row in &rows {
        let state = row
            .char_indices()
            .nth(name_col)
            .map(|(index, _)| &row[index..])
            .unwrap_or_else(|| panic!("every row reaches the state column: {row:?}"));
        assert!(
            state.starts_with(&" ".repeat(GAP)),
            "the state begins in the same cell on every row: {row:?}"
        );
        let word: String = state.chars().skip(GAP).take(STATE_COLUMN).collect();
        assert!(
            !word.trim().is_empty() && word.trim().chars().all(char::is_alphabetic),
            "and the state column holds one word: {word:?} in {row:?}"
        );
    }
}

/// The colour is the instrument: name and state both carry the agent's place in the attention
/// hierarchy, so a scan down the strip finds it without reading a word.
#[test]
fn a_rows_name_and_state_carry_its_attention_role() {
    let state = crowded().state;
    let palette = Palette::pastel();
    let rows = roster(&state, &palette, STRIP, ALL);
    for (name, role) in [
        ("Sol", Role::Failure),
        ("Vega", Role::ActionRequired),
        ("Orion", Role::Ambient),
    ] {
        let line = rows
            .lines
            .iter()
            .find(|line| line.to_string().trim_start().starts_with(name))
            .unwrap_or_else(|| panic!("{name} is in the roster"));
        assert_eq!(
            line.spans.first().map(|span| span.style),
            Some(palette.style(role).add_modifier(Modifier::BOLD)),
            "the name carries its own state, with weight: {name} at {role:?}"
        );
        assert_eq!(
            line.spans.get(1).map(|span| span.style),
            Some(palette.style(role)),
            "and the rest of the row carries it without: {name} at {role:?}"
        );
    }
}

/// An ask outranks a tally: the third column says what the agent wants when it wants something,
/// and what it has done when it does not.
#[test]
fn the_detail_column_is_the_ask_when_there_is_one_and_the_counts_when_there_is_not() {
    let conversation = crowded();
    let rows = text(&conversation, STRIP, ALL);
    let joined = rows.join("\n");
    assert!(
        joined.contains("Write the migration?"),
        "the ask is the detail: {joined}"
    );
    let agent_b = rows
        .iter()
        .find(|row| row.starts_with("Agent B"))
        .unwrap_or_else(|| panic!("canonical state includes Agent B: {joined}"));
    assert!(
        agent_b.contains("overlap study"),
        "an asking agent shows its ask, not its counts: {agent_b:?}"
    );

    // The same agent once the request is answered: counts come back.
    let mut conversation = crowded();
    conversation.emit(ConversationEvent::AttentionResolved {
        agent_id: agent("agent-b"),
        attention_id: AttentionId::new("attention-b-1")
            .unwrap_or_else(|error| panic!("invalid fixture: {error}")),
    });
    let rows = text(&conversation, STRIP, ALL);
    let detail = rows
        .iter()
        .find(|row| row.starts_with("Agent B"))
        .unwrap_or_else(|| panic!("Agent B stays in the roster: {rows:?}"));
    assert!(detail.contains('\u{f1323}'), "tools: {detail:?}");
    assert!(detail.contains('\u{f03e2}'), "artifacts: {detail:?}");
    // Agent B wrote that letter rather than receiving it. A roster says where the user's work is
    // waiting, and an agent's own outbound letter is not work waiting in it.
    assert!(
        !detail.contains('\u{f01ee}'),
        "a sent letter is counted in the conversation it arrived in, not the one it left: \
         {detail:?}"
    );
}

/// A roster counts what arrived for an agent, never what the agent sent.
///
/// One letter lands in both conversations, so counting every addressed entry told each agent how
/// many letters it had *handled* — a number that answers no question the roster is asked. The two
/// directions are exercised in one fixture so neither can be made to pass by a rule that simply
/// counts less.
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

    let counts = crate::content::tally(
        conversation
            .state
            .agent(&agent("agent-b"))
            .unwrap_or_else(|| panic!("the canonical timeline creates Agent B")),
    );
    assert!(
        counts.contains("\u{f01ee} 1"),
        "Agent B received exactly one of the two letters: {counts:?}"
    );

    // Both letters are in Agent B's conversation, so a rule that counted entries rather than
    // arrivals would say two here. That is the number this test exists to refuse.
    assert!(
        !counts.contains("\u{f01ee} 2"),
        "the letter Agent B wrote is counted where it arrived, not where it left: {counts:?}"
    );
}

/// SURF-2: the pointer lands on the agent under it, and on nothing past the last row.
#[test]
fn every_painted_row_resolves_to_the_agent_it_belongs_to() {
    let state = crowded().state;
    let palette = Palette::pastel();
    let rows = roster(&state, &palette, STRIP, STRIP_AGENTS);
    assert_eq!(rows.lines.len(), STRIP_AGENTS);
    for (index, expected) in ["sol", "agent-b", "vega"].into_iter().enumerate() {
        assert_eq!(
            agent_at_row(&state, &palette, STRIP, STRIP_AGENTS, index),
            Some(agent(expected)),
            "row {index} resolves to the agent painted on it: {:?}",
            rows.lines[index].to_string()
        );
    }
    // A click below the last agent selects nobody. The strip is short, and the rows under it
    // belong to the conversation.
    assert!(agent_at_row(&state, &palette, STRIP, STRIP_AGENTS, STRIP_AGENTS).is_none());
}
