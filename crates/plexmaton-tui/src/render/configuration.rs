//! The Configuration page: scrollable values under the Drawer's title, with a fixed footer.

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
    ViewState, content,
    state::ScrollPosition,
    surface::{ContentInsets, SurfaceId, Viewport},
    theme::{Palette, Role},
};

pub(super) fn render_configuration(
    frame: &mut Frame<'_>,
    palette: &Palette,
    state: &ViewState,
    area: Rect,
    focused: bool,
    parked: Option<ScrollPosition>,
) -> Viewport {
    let insets = ContentInsets::for_surface(SurfaceId::Drawer, area.height);
    let border = block(
        palette,
        title(
            palette,
            "Workspace · Configuration",
            Role::SectionHeading,
            " · read only",
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
    // The footer keeps its row, and a blank row above it, while the values scroll.
    let gap = u16::from(inside.height >= 6);
    let body = Rect {
        height: inside.height.saturating_sub(1 + gap),
        ..inside
    };
    let footer = Rect::new(inside.x, inside.bottom().saturating_sub(1), inside.width, 1);
    let paragraph =
        Paragraph::new(content::configuration(state, palette)).wrap(Wrap { trim: false });
    let mut viewport = Viewport {
        content_rows: paragraph.line_count(body.width),
        content_width: body.width,
        visible_rows: body.height,
        offset: 0,
    };
    viewport.offset = parked.map_or(0, |position| position.offset(viewport.max_offset()));
    let offset = u16::try_from(viewport.offset).unwrap_or(u16::MAX);
    frame.render_widget(Clear, area);
    frame.render_widget(border, area);
    frame.render_widget(paragraph.scroll((offset, 0)), body);
    frame.render_widget(
        Paragraph::new(Line::styled(
            "Esc back · ↑↓ scroll",
            palette.style(Role::Muted),
        )),
        footer,
    );
    viewport
}
