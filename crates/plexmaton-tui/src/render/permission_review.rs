//! Whole configured scopes scroll independently of the fixed continue/back affordance.
use ratatui::{
    Frame,
    layout::Rect,
    text::Line,
    widgets::{Clear, Padding, Paragraph, Wrap},
};

use super::{
    chrome::{block, title},
    panel::Edges,
};
use crate::{
    ViewState,
    surface::{ContentInsets, SurfaceId, Viewport},
    theme::{Palette, Role},
};

/// Shared with pointer hit testing; the footer is never part of the scrolling rule body.
pub(crate) fn choice_row(area: Rect) -> u16 {
    let insets = ContentInsets::for_surface(SurfaceId::Drawer, area.height);
    area.bottom().saturating_sub(4 + insets.vertical)
}

pub(super) fn render(
    frame: &mut Frame<'_>,
    palette: &Palette,
    state: &ViewState,
    area: Rect,
    focused: bool,
) -> Option<Viewport> {
    let panel = state
        .drawer()?
        .permissions()
        .filter(|panel| panel.is_reading())?;
    let parked = state.scroll_position(SurfaceId::Drawer);
    let insets = ContentInsets::for_surface(SurfaceId::Drawer, area.height);
    let border = block(
        palette,
        title(
            palette,
            "Project configuration rules",
            Role::SectionHeading,
            "",
        ),
        focused,
        Edges::All,
    )
    .padding(Padding::new(
        insets.sides,
        insets.sides,
        insets.vertical,
        insets.vertical,
    ));
    let inside = border.inner(area);
    let body = Rect {
        height: inside.height.saturating_sub(4),
        ..inside
    };
    let footer = Rect::new(
        inside.x,
        choice_row(area).saturating_add(2),
        inside.width,
        1,
    );
    let lines: Vec<_> = panel
        .description()
        .into_iter()
        .map(|line| Line::styled(line, palette.style(Role::Body)))
        .collect();
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    let mut viewport = Viewport {
        content_rows: paragraph.line_count(body.width),
        content_width: body.width,
        visible_rows: body.height,
        offset: 0,
    };
    viewport.offset = parked.map_or(0, |position| position.offset(viewport.max_offset()));
    frame.render_widget(Clear, area);
    frame.render_widget(border, area);
    frame.render_widget(
        paragraph.scroll((u16::try_from(viewport.offset).unwrap_or(u16::MAX), 0)),
        body,
    );
    if viewport.offset < viewport.max_offset() {
        frame.render_widget(
            Paragraph::new(Line::styled("↓ More to read", palette.style(Role::Muted))),
            Rect::new(
                inside.x,
                choice_row(area).saturating_sub(1),
                inside.width,
                1,
            ),
        );
    }
    let choice = panel
        .choices()
        .first()
        .map_or(String::new(), |(_, label)| format!("> {label}"));
    frame.render_widget(
        Paragraph::new(crate::content::chosen_row(
            vec![ratatui::text::Span::raw(choice)],
            palette,
            inside.width,
        )),
        Rect::new(inside.x, choice_row(area), inside.width, 1),
    );
    frame.render_widget(
        Paragraph::new(Line::styled(panel.hint(), palette.style(Role::Muted))),
        footer,
    );
    Some(viewport)
}
