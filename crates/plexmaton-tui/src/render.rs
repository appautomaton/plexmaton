use plexmaton_core::{AgentStatus, ToolActivityStatus, TranscriptRole};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use crate::{NoticeView, ViewState};

/// Rows reserved for the notice strip when it has something to report.
const NOTICE_HEIGHT: u16 = 4;

/// Responsive composition selected from terminal width.
///
/// The thresholds are Phase 00 provisional values derived from the current panes. The UI/UX
/// contract locks them only after the prototype demonstrates all three under realistic content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayoutClass {
    /// Agent rail, transcript, and activity coexist as columns.
    Wide,
    /// Agent rail plus transcript; activity collapses beneath the transcript.
    Medium,
    /// One column; every region becomes a stacked band.
    Narrow,
}

impl LayoutClass {
    /// Chooses the composition for a terminal width in cells.
    #[must_use]
    pub const fn for_width(width: u16) -> Self {
        if width >= 96 {
            Self::Wide
        } else if width >= 72 {
            Self::Medium
        } else {
            Self::Narrow
        }
    }
}

struct Regions {
    agents: Rect,
    transcript: Rect,
    activity: Rect,
    notices: Option<Rect>,
    footer: Rect,
}

/// Projects the current view state into a Ratatui frame without mutating it.
pub fn render(frame: &mut Frame<'_>, state: &ViewState) {
    let has_notices = state.notices().next().is_some();
    let regions = regions(frame.area(), has_notices);

    render_agents(frame, state, regions.agents);
    render_transcript(frame, state, regions.transcript);
    render_activity(frame, state, regions.activity);
    if let Some(area) = regions.notices {
        render_notices(frame, state, area);
    }

    let footer = Line::from(vec![
        Span::styled(
            " q / Esc ",
            Style::default().fg(Color::Black).bg(Color::Cyan),
        ),
        Span::raw(" quit  ·  deterministic Phase 00 timeline"),
    ]);
    frame.render_widget(Paragraph::new(footer), regions.footer);
}

fn regions(area: Rect, has_notices: bool) -> Regions {
    let notice_height = if has_notices { NOTICE_HEIGHT } else { 0 };
    let [body, notices, footer] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(notice_height),
        Constraint::Length(1),
    ])
    .areas(area);

    let (agents, transcript, activity) = match LayoutClass::for_width(area.width) {
        LayoutClass::Wide => {
            let [agents, transcript, activity] = Layout::horizontal([
                Constraint::Length(26),
                Constraint::Min(30),
                Constraint::Length(30),
            ])
            .areas(body);
            (agents, transcript, activity)
        }
        LayoutClass::Medium => {
            let [agents, main] =
                Layout::horizontal([Constraint::Length(26), Constraint::Min(24)]).areas(body);
            let [transcript, activity] =
                Layout::vertical([Constraint::Min(6), Constraint::Length(8)]).areas(main);
            (agents, transcript, activity)
        }
        LayoutClass::Narrow => {
            let [agents, transcript, activity] = Layout::vertical([
                Constraint::Length(5),
                Constraint::Min(6),
                Constraint::Length(8),
            ])
            .areas(body);
            (agents, transcript, activity)
        }
    };

    Regions {
        agents,
        transcript,
        activity,
        notices: has_notices.then_some(notices),
        footer,
    }
}

fn render_agents(frame: &mut Frame<'_>, state: &ViewState, area: Rect) {
    let selected = state.selected_agent().map(|agent| agent.id.clone());
    let items = state.agents().map(|agent| {
        let marker = if selected.as_ref() == Some(&agent.id) {
            "●"
        } else {
            "○"
        };
        ListItem::new(Line::from(vec![
            Span::styled(format!("{marker} "), Style::default().fg(Color::Cyan)),
            Span::raw(agent.label.clone()),
            Span::styled(
                format!("  {}", agent_status_label(agent.status)),
                Style::default().fg(Color::DarkGray),
            ),
        ]))
    });

    let title = format!(" Agents · attention {} ", state.attention_count());
    frame.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title(title)),
        area,
    );
}

fn render_transcript(frame: &mut Frame<'_>, state: &ViewState, area: Rect) {
    let Some(agent) = state.selected_agent() else {
        frame.render_widget(
            Paragraph::new("Waiting for the first semantic event…")
                .block(Block::default().borders(Borders::ALL).title(" Transcript ")),
            area,
        );
        return;
    };

    let mut lines = Vec::new();
    for item in agent.transcript() {
        let role = match item.role {
            TranscriptRole::User => "you",
            TranscriptRole::Assistant => "assistant",
            TranscriptRole::System => "system",
        };
        lines.push(Line::from(Span::styled(
            role,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::raw(item.source.clone()));
        lines.push(Line::raw(""));
    }

    if lines.is_empty() {
        lines.push(hint("Agent is active; no transcript item has started yet."));
    }

    let title = format!(" {} · {} ", agent.label, agent_status_label(agent.status));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title(title)),
        area,
    );
}

fn render_activity(frame: &mut Frame<'_>, state: &ViewState, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title(" Activity ");
    let Some(agent) = state.selected_agent() else {
        frame.render_widget(Paragraph::new("No agent selected.").block(block), area);
        return;
    };

    let mut lines = vec![section("Tools")];
    let mut tools = 0_usize;
    for tool in agent.tool_activity() {
        tools += 1;
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} ", tool_marker(tool.status)),
                Style::default().fg(tool_color(tool.status)),
            ),
            Span::raw(tool.label.clone()),
        ]));
    }
    if tools == 0 {
        lines.push(hint("  none"));
    }

    lines.push(section("Artifacts"));
    let mut artifacts = 0_usize;
    for artifact in agent.artifacts() {
        artifacts += 1;
        lines.push(Line::from(vec![
            Span::styled("@ ", Style::default().fg(Color::Magenta)),
            Span::raw(artifact.label.clone()),
        ]));
        lines.push(hint(format!("  {}", artifact.pointer)));
    }
    if artifacts == 0 {
        lines.push(hint("  none"));
    }

    lines.push(section("Mail"));
    let mut mail = 0_usize;
    for item in agent.inbox() {
        mail += 1;
        lines.push(Line::from(vec![
            Span::styled("<- ", Style::default().fg(Color::Green)),
            Span::raw(item.from.to_string()),
        ]));
        lines.push(hint(format!("  {}", item.summary)));
    }
    if mail == 0 {
        lines.push(hint("  none"));
    }

    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
}

fn render_notices(frame: &mut Frame<'_>, state: &ViewState, area: Rect) {
    // The strip is a bounded tail view: older entries stay in the log but the workspace must not
    // give unbounded screen space to producer defects.
    let visible = usize::from(area.height.saturating_sub(2));
    let notices: Vec<_> = state.notices().collect();
    let lines: Vec<_> = notices
        .iter()
        .rev()
        .take(visible)
        .rev()
        .map(|notice| notice_line(notice))
        .collect();

    let dropped = state.notices_dropped();
    let title = if dropped == 0 {
        format!(" Notices · {} ", notices.len())
    } else {
        format!(" Notices · {} · {dropped} discarded ", notices.len())
    };

    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title(title)),
        area,
    );
}

fn notice_line(notice: &NoticeView) -> Line<'static> {
    match notice {
        NoticeView::RuntimeWarning { message } => Line::from(vec![
            Span::styled("[warn] ", Style::default().fg(Color::Yellow)),
            Span::raw(message.clone()),
        ]),
        NoticeView::SequenceGap { expected, received } => Line::from(vec![
            Span::styled("[gap]  ", Style::default().fg(Color::Yellow)),
            Span::raw(format!("resynchronized from {expected} to {received}")),
        ]),
        NoticeView::Rejected { sequence, error } => Line::from(vec![
            Span::styled("[drop] ", Style::default().fg(Color::Red)),
            Span::raw(format!("sequence {}: {error}", sequence.get())),
        ]),
    }
}

fn section(title: &str) -> Line<'static> {
    Line::from(Span::styled(
        title.to_owned(),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ))
}

fn hint(text: impl Into<String>) -> Line<'static> {
    Line::from(Span::styled(
        text.into(),
        Style::default().fg(Color::DarkGray),
    ))
}

const fn agent_status_label(status: AgentStatus) -> &'static str {
    match status {
        AgentStatus::Idle => "idle",
        AgentStatus::Running => "running",
        AgentStatus::Waiting => "waiting",
        AgentStatus::Completed => "done",
        AgentStatus::Failed => "failed",
        AgentStatus::Cancelled => "cancelled",
    }
}

/// Tool markers stay legible without color so monochrome terminals keep the same status grammar.
const fn tool_marker(status: ToolActivityStatus) -> &'static str {
    match status {
        ToolActivityStatus::Queued => "[ ]",
        ToolActivityStatus::Running => "[~]",
        ToolActivityStatus::Succeeded => "[+]",
        ToolActivityStatus::Failed => "[!]",
        ToolActivityStatus::Cancelled => "[-]",
    }
}

const fn tool_color(status: ToolActivityStatus) -> Color {
    match status {
        ToolActivityStatus::Queued => Color::DarkGray,
        ToolActivityStatus::Running => Color::Yellow,
        ToolActivityStatus::Succeeded => Color::Green,
        ToolActivityStatus::Failed => Color::Red,
        ToolActivityStatus::Cancelled => Color::DarkGray,
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{EventSequence, PrototypeEvent, PrototypeEventEnvelope};
    use plexmaton_sim::Scenario;
    use ratatui::{Terminal, backend::TestBackend};

    use super::LayoutClass;
    use crate::{ViewState, render};

    fn canonical_state() -> ViewState {
        let scenario =
            Scenario::canonical().unwrap_or_else(|error| panic!("invalid fixture: {error}"));
        let mut state = ViewState::default();
        for step in scenario.into_steps() {
            state.apply(step.envelope);
        }
        state
    }

    fn draw(state: &ViewState, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        terminal
            .draw(|frame| render(frame, state))
            .unwrap_or_else(|error| panic!("test render: {error}"));
        terminal.backend().to_string()
    }

    #[test]
    fn layout_class_covers_three_widths() {
        assert_eq!(LayoutClass::for_width(120), LayoutClass::Wide);
        assert_eq!(LayoutClass::for_width(96), LayoutClass::Wide);
        assert_eq!(LayoutClass::for_width(95), LayoutClass::Medium);
        assert_eq!(LayoutClass::for_width(72), LayoutClass::Medium);
        assert_eq!(LayoutClass::for_width(71), LayoutClass::Narrow);
    }

    #[test]
    fn wide_projection_shows_transcript_and_reduced_domain_data() {
        let rendered = draw(&canonical_state(), 120, 24);

        assert!(rendered.contains("Agent A · primary"));
        assert!(rendered.contains("attention 1"));
        assert!(rendered.contains("remains interactive"));
        // Mail, artifacts, and tool activity are reduced for agent A's inspector; a projection
        // that silently dropped them would still render a plausible-looking transcript.
        assert!(rendered.contains("agent-b"), "mail sender must be visible");
    }

    #[test]
    fn activity_panel_renders_tools_and_artifacts_of_the_selected_agent() {
        let mut state = canonical_state();
        let agent_b = plexmaton_core::AgentId::new("agent-b")
            .unwrap_or_else(|error| panic!("invalid fixture: {error}"));
        state
            .select_agent(&agent_b)
            .unwrap_or_else(|error| panic!("agent-b exists: {error}"));

        let rendered = draw(&state, 120, 24);

        assert!(rendered.contains("[+]"), "succeeded tool marker");
        assert!(rendered.contains("interaction findings"), "artifact label");
        assert!(rendered.contains("artifact://"), "artifact pointer");
    }

    #[test]
    fn narrow_projection_keeps_every_region() {
        let rendered = draw(&canonical_state(), 60, 30);

        assert!(rendered.contains("Agents"));
        assert!(rendered.contains("Activity"));
        assert!(rendered.contains("quit"));
    }

    #[test]
    fn notice_strip_appears_only_once_a_notice_exists() {
        let clean = canonical_state();
        assert!(!draw(&clean, 120, 24).contains("Notices"));

        let mut degraded = canonical_state();
        degraded.apply(PrototypeEventEnvelope {
            // A stale sequence: the canonical scenario has already advanced well past 1.
            sequence: EventSequence::new(1),
            event: PrototypeEvent::RuntimeWarning {
                message: "producer replayed an old event".into(),
            },
        });

        let rendered = draw(&degraded, 120, 24);
        assert!(rendered.contains("Notices"));
        assert!(rendered.contains("[drop]"));
    }
}
