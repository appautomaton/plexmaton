use plexmaton_core::{AgentStatus, ToolActivityStatus, TranscriptRole};
use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use crate::{
    NoticeView, ViewState,
    layout::{self, LayoutClass, MIN_HEIGHT, MIN_WIDTH},
    surface::{SurfaceId, SurfaceTree},
    theme::{Palette, Role, agent_role, tool_role},
};

/// Projects the current view state into a Ratatui frame without mutating it.
///
/// Returns the surfaces this frame actually drew, which is what the router must hit-test against.
/// Handing the registry back rather than recomputing it elsewhere is what keeps SURF-1 true: routing
/// cannot be given geometry the renderer did not use.
pub fn render(frame: &mut Frame<'_>, state: &ViewState, palette: &Palette) -> SurfaceTree {
    let area = frame.area();
    if LayoutClass::for_size(area.width, area.height) == LayoutClass::TooSmall {
        render_too_small(frame, palette, area);
        return SurfaceTree::default();
    }

    let surfaces = layout::workspace(area, state.notices().next().is_some());
    // Drawing walks the registry, so a region that layout computed without registering has no
    // rectangle to be drawn into, and a new surface identity will not compile until it has an arm.
    for surface in surfaces.iter() {
        match surface.id {
            SurfaceId::Agents => render_agents(frame, state, palette, surface.bounds),
            SurfaceId::Transcript => render_transcript(frame, state, palette, surface.bounds),
            SurfaceId::Activity => render_activity(frame, state, palette, surface.bounds),
            SurfaceId::Notices => render_notices(frame, state, palette, surface.bounds),
            SurfaceId::Footer => render_footer(frame, palette, surface.bounds),
        }
    }

    surfaces
}

fn render_footer(frame: &mut Frame<'_>, palette: &Palette, area: Rect) {
    // Escape resolves the topmost layer and never quits, so the hint must not offer it as an exit.
    let footer = Line::from(vec![
        Span::styled(" ↑↓ ", palette.style(Role::KeyHint)),
        Span::styled(" select  ·  ", palette.style(Role::Muted)),
        Span::styled(" q ", palette.style(Role::KeyHint)),
        Span::styled(
            " quit  ·  deterministic Phase 00 timeline",
            palette.style(Role::Muted),
        ),
    ]);
    frame.render_widget(Paragraph::new(footer), area);
}

fn render_too_small(frame: &mut Frame<'_>, palette: &Palette, area: Rect) {
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

fn panel(palette: &Palette, title: impl Into<String>) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(palette.style(Role::Border))
        .title(Span::styled(title.into(), palette.style(Role::Muted)))
}

fn render_agents(frame: &mut Frame<'_>, state: &ViewState, palette: &Palette, area: Rect) {
    let selected = state.selected_agent().map(|agent| agent.id.clone());
    let items = state.agents().map(|agent| {
        let (marker, marker_role) = if selected.as_ref() == Some(&agent.id) {
            ("●", Role::Accent)
        } else {
            ("○", Role::Muted)
        };
        ListItem::new(Line::from(vec![
            Span::styled(format!("{marker} "), palette.style(marker_role)),
            Span::styled(agent.label.clone(), palette.style(Role::Body)),
            Span::styled(
                format!("  {}", agent_status_label(agent.status)),
                palette.style(agent_role(agent.status)),
            ),
        ]))
    });

    let attention = state.attention_count();
    let title = format!(" Agents · attention {attention} ");
    let block = if attention == 0 {
        panel(palette, title)
    } else {
        // An unanswered request must read as action required, not as ambient decoration.
        Block::default()
            .borders(Borders::ALL)
            .border_style(palette.style(Role::Border))
            .title(Span::styled(title, palette.style(Role::ActionRequired)))
    };

    frame.render_widget(List::new(items).block(block), area);
}

fn render_transcript(frame: &mut Frame<'_>, state: &ViewState, palette: &Palette, area: Rect) {
    let Some(agent) = state.selected_agent() else {
        frame.render_widget(
            Paragraph::new(Line::styled(
                "Waiting for the first semantic event…",
                palette.style(Role::Muted),
            ))
            .block(panel(palette, " Transcript ")),
            area,
        );
        return;
    };

    let mut lines = Vec::new();
    for item in agent.transcript() {
        let author = match item.role {
            TranscriptRole::User => "you",
            TranscriptRole::Assistant => "assistant",
            TranscriptRole::System => "system",
        };
        lines.push(Line::styled(author, palette.style(Role::SectionHeading)));
        lines.push(Line::styled(item.source.clone(), palette.style(Role::Body)));
        lines.push(Line::raw(""));
    }

    if lines.is_empty() {
        lines.push(Line::styled(
            "Agent is active; no transcript item has started yet.",
            palette.style(Role::Muted),
        ));
    }

    let title = format!(" {} · {} ", agent.label, agent_status_label(agent.status));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(panel(palette, title)),
        area,
    );
}

fn render_activity(frame: &mut Frame<'_>, state: &ViewState, palette: &Palette, area: Rect) {
    let block = panel(palette, " Activity ");
    let Some(agent) = state.selected_agent() else {
        frame.render_widget(
            Paragraph::new(Line::styled(
                "No agent selected.",
                palette.style(Role::Muted),
            ))
            .block(block),
            area,
        );
        return;
    };

    let mut lines = vec![Line::styled("Tools", palette.style(Role::SectionHeading))];
    let mut tools = 0_usize;
    for tool in agent.tool_activity() {
        tools += 1;
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} ", tool_marker(tool.status)),
                palette.style(tool_role(tool.status)),
            ),
            Span::styled(tool.label.clone(), palette.style(Role::Body)),
        ]));
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
        artifacts += 1;
        lines.push(Line::from(vec![
            Span::styled("@ ", palette.style(Role::NewInformation)),
            Span::styled(artifact.label.clone(), palette.style(Role::Body)),
        ]));
        lines.push(Line::styled(
            format!("  {}", artifact.pointer),
            palette.style(Role::Muted),
        ));
    }
    if artifacts == 0 {
        lines.push(Line::styled("  none", palette.style(Role::Muted)));
    }

    lines.push(Line::styled("Mail", palette.style(Role::SectionHeading)));
    let mut mail = 0_usize;
    for item in agent.inbox() {
        mail += 1;
        lines.push(Line::from(vec![
            Span::styled("<- ", palette.style(Role::NewInformation)),
            Span::styled(item.from.to_string(), palette.style(Role::Body)),
        ]));
        lines.push(Line::styled(
            format!("  {}", item.summary),
            palette.style(Role::Muted),
        ));
    }
    if mail == 0 {
        lines.push(Line::styled("  none", palette.style(Role::Muted)));
    }

    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
}

fn render_notices(frame: &mut Frame<'_>, state: &ViewState, palette: &Palette, area: Rect) {
    // The strip is a bounded tail view: older entries stay in the log but the workspace must not
    // give unbounded screen space to producer defects.
    let visible = usize::from(area.height.saturating_sub(2));
    let notices: Vec<_> = state.notices().collect();
    let lines: Vec<_> = notices
        .iter()
        .rev()
        .take(visible)
        .rev()
        .map(|notice| notice_line(notice, palette))
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
            .block(panel(palette, title)),
        area,
    );
}

fn notice_line(notice: &NoticeView, palette: &Palette) -> Line<'static> {
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

/// Tool markers stay legible without colour so monochrome terminals keep the same status grammar.
const fn tool_marker(status: ToolActivityStatus) -> &'static str {
    match status {
        ToolActivityStatus::Queued => "[ ]",
        ToolActivityStatus::Running => "[~]",
        ToolActivityStatus::Succeeded => "[+]",
        ToolActivityStatus::Failed => "[!]",
        ToolActivityStatus::Cancelled => "[-]",
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        surface::SurfaceId,
        test_support::{canonical_state, degraded_state, draw, draw_frame, draw_with, region_text},
        theme::Palette,
    };

    /// SURF-1: every registered surface is painted inside the rectangle it registered.
    ///
    /// Move any panel's draw call to a different rectangle and its signature leaves the region
    /// this walks, which is the "the click landed one panel over" defect caught before it ships.
    #[test]
    fn every_registered_surface_is_drawn_inside_its_own_bounds() {
        let (surfaces, buffer) = draw_frame(&degraded_state(), &Palette::default(), 120, 24);

        assert_eq!(surfaces.len(), 5, "a degraded workspace registers all five");
        for surface in surfaces.iter() {
            // An exhaustive match, so a new surface identity cannot be added without stating what
            // proves it was drawn.
            let signature = match surface.id {
                SurfaceId::Agents => "Agents",
                SurfaceId::Transcript => "Agent A · primary",
                SurfaceId::Activity => "Artifacts",
                SurfaceId::Notices => "[drop]",
                SurfaceId::Footer => "quit",
            };
            let painted = region_text(&buffer, surface.bounds);
            assert!(
                painted.contains(signature),
                "{:?} registered {:?} but {signature:?} was not painted there",
                surface.id,
                surface.bounds
            );
        }
    }

    #[test]
    fn a_too_small_terminal_gets_a_notice_instead_of_a_clipped_workspace() {
        let rendered = draw(&canonical_state(), 40, 10);

        assert!(rendered.contains("Terminal too small"));
        assert!(rendered.contains("48 x 12"), "states the requirement");
        assert!(rendered.contains("40 x 10"), "states what it got");
        // Nothing from the workspace may leak through; a half-drawn rail is the failure this
        // notice exists to prevent.
        assert!(!rendered.contains("Agent A"));
        assert!(!rendered.contains("Activity"));
    }

    #[test]
    fn ultrawide_gives_the_conversation_more_room_than_wide() {
        let state = canonical_state();
        let wide = draw(&state, 120, 30);
        let ultrawide = draw(&state, 140, 30);

        assert!(wide.contains("Agent A · primary"));
        assert!(ultrawide.contains("Agent A · primary"));
        assert_ne!(
            wide.lines().next(),
            ultrawide.lines().next(),
            "ultrawide must compose differently, not merely be a wider wide"
        );
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
        assert!(!draw(&canonical_state(), 120, 24).contains("Notices"));

        let rendered = draw(&degraded_state(), 120, 24);
        assert!(rendered.contains("Notices"));
        assert!(rendered.contains("[drop]"));
    }

    #[test]
    fn every_palette_paints_the_same_text() {
        // Swapping the palette must change styling only. A palette that alters which characters
        // reach the buffer would mean colour is carrying meaning that the glyphs do not.
        let state = canonical_state();
        let ansi = draw_with(&state, &Palette::ansi(), 120, 24);
        let truecolor = draw_with(&state, &Palette::truecolor(), 120, 24);
        let monochrome = draw_with(&state, &Palette::monochrome(), 120, 24);

        assert_eq!(ansi, truecolor);
        assert_eq!(ansi, monochrome);
    }
}
