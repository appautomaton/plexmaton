//! Read-only command modal over the still-pending approval.
use super::{
    chrome::title,
    panel::{Body, Chrome, Edges, Panel},
};
use crate::{Palette, Role, ViewState, state::wrap_line, surface::ContentInsets};
use ratatui::{Frame, layout::Rect, text::Line};

pub(super) fn panel(state: &ViewState, palette: &Palette, bounds: Rect) -> Panel {
    let insets = ContentInsets {
        sides: 1,
        vertical: 1,
    };
    let width = insets.width(bounds.width);
    let mut lines = Vec::new();
    if let Some((source, root, timeout)) = state.approval_command() {
        for row in wrap_line(
            &format!("cwd: {root:?} · timeout: {timeout} ms"),
            usize::from(width),
        ) {
            lines.push(Line::styled(row, palette.style(Role::Muted)));
        }
        lines.push(Line::default());
        for row in crate::content::command_display_source(source).split('\n') {
            lines.extend(
                wrap_line(row, usize::from(width))
                    .into_iter()
                    .map(|row| Line::styled(row, palette.style(Role::Body))),
            );
        }
    }
    Panel {
        body: Body::Whole {
            lines,
            follows_tail: false,
        },
        title: title(palette, "Command", Role::SectionHeading, ""),
        badge: None,
        edges: Edges::All,
        chrome: Chrome::Box,
        insets,
        footer: Some(Line::styled(
            "c copy · ↑↓ scroll · Esc back",
            palette.style(Role::Muted),
        )),
    }
}

pub(super) fn controls(frame: &mut Frame<'_>, palette: &Palette, bounds: Rect) {
    let [copy, close] = crate::layout::command_inspection_controls(bounds);
    frame.render_widget(Line::styled(" ⧉ ", palette.style(Role::Accent)), copy);
    frame.render_widget(Line::styled(" × ", palette.style(Role::Failure)), close);
}
