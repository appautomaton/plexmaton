//! The collapsed narrow-width route back to the full-region Agents navigator.

use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    ViewState,
    surface::{Point, SurfaceId, SurfaceTree},
    theme::{Palette, Role},
};

/// One handle painted into a conversation's already-reserved top row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AgentsHandle {
    pub(crate) owner: SurfaceId,
    pub(crate) bounds: Rect,
}

/// Returns the handle shared by drawing and hit resolution.
pub(crate) fn geometry(state: &ViewState, surfaces: &SurfaceTree) -> Option<AgentsHandle> {
    let width = surfaces.get(SurfaceId::Status)?.bounds.width;
    if width >= 72
        || surfaces.get(SurfaceId::Agents).is_some()
        || state.sub_agents().next().is_none()
        || surfaces.iter().any(|surface| surface.kind.blocks_below())
    {
        return None;
    }
    let owner = if surfaces.get(SurfaceId::Inspector).is_some() {
        SurfaceId::Inspector
    } else {
        SurfaceId::Transcript
    };
    let conversation = surfaces.get(owner)?.bounds;
    let label_width = u16::try_from(label(state, owner).width()).unwrap_or(u16::MAX);
    let inside_right = conversation.right().saturating_sub(1);
    let inside_width = conversation.width.saturating_sub(2);
    if label_width == 0 || label_width > inside_width {
        return None;
    }
    Some(AgentsHandle {
        owner,
        bounds: Rect::new(
            inside_right.saturating_sub(label_width),
            conversation.y,
            label_width,
            1,
        ),
    })
}

pub(crate) fn hit(
    state: &ViewState,
    surfaces: &SurfaceTree,
    surface: SurfaceId,
    at: Point,
) -> bool {
    geometry(state, surfaces).is_some_and(|handle| {
        handle.owner == surface
            && at.x >= handle.bounds.x
            && at.x < handle.bounds.right()
            && at.y == handle.bounds.y
    })
}

pub(super) fn render(
    frame: &mut Frame<'_>,
    state: &ViewState,
    palette: &Palette,
    handle: AgentsHandle,
) {
    let mut spans = vec![Span::styled(" Agents", palette.style(Role::SectionHeading))];
    let pending = state.attention_pending();
    if handle.owner == SurfaceId::Inspector && pending > 0 {
        spans.push(Span::styled(
            format!(" !{pending}"),
            palette.style(Role::ActionRequired),
        ));
    }
    spans.push(Span::styled(" ^B ", palette.style(Role::Muted)));
    frame.render_widget(Clear, handle.bounds);
    frame.render_widget(Paragraph::new(Line::from(spans)), handle.bounds);
}

fn label(state: &ViewState, owner: SurfaceId) -> String {
    match (owner, state.attention_pending()) {
        (SurfaceId::Inspector, pending) if pending > 0 => format!(" Agents !{pending} ^B "),
        (_, 0) => " Agents ^B ".to_owned(),
        _ => " Agents ^B ".to_owned(),
    }
}
