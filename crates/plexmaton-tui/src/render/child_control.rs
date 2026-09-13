//! CCV-3: fixed child control chrome, outside the semantic transcript viewport.

use plexmaton_core::AgentStatus;
use ratatui::text::{Line, Span};

use crate::{AgentView, ChildControl, Palette, Role, ViewState};

pub(super) fn lifecycle(agent: &AgentView) -> &'static str {
    if agent
        .control()
        .is_some_and(|view| view.control == ChildControl::HandoffPending)
    {
        return "Handoff pending";
    }
    match agent.status {
        AgentStatus::Idle => "Idle",
        AgentStatus::Running => "Running",
        AgentStatus::Waiting => "Waiting",
        AgentStatus::Completed => "Completed",
        AgentStatus::Failed => "Failed",
        AgentStatus::Cancelled => "Cancelled",
    }
}

pub(super) fn footer(
    state: &ViewState,
    palette: &Palette,
    width: u16,
    has_focus: bool,
) -> Line<'static> {
    let Some(agent) = state.inspector().and_then(|view| state.agent(&view.agent)) else {
        return Line::default();
    };
    let Some(snapshot) = agent.control() else {
        return Line::styled(
            "Controller unavailable | Input locked",
            palette.style(Role::Muted),
        );
    };
    let controller = match snapshot.control {
        ChildControl::Main | ChildControl::HandoffPending => "Controller: Main",
        ChildControl::User => "Controller: User",
    };
    let mut line = Line::from(vec![
        Span::styled(controller, palette.style(Role::SectionHeading)),
        Span::styled(" | Read-only files | No shell", palette.style(Role::Muted)),
    ]);
    // A keyboard hint, not a pointer button. The existing Ctrl-C route names the focused agent.
    if has_focus
        && matches!(agent.status, AgentStatus::Running | AgentStatus::Waiting)
        && line.width() + " | ^C Stop".len() <= usize::from(width)
    {
        line.spans
            .push(Span::styled(" | ^C Stop", palette.style(Role::Accent)));
    }
    line
}
