//! What each surface is drawn as: the panel, chrome and cursor for one surface identity.
//!
//! Separated from the draw loop so that file keeps one question, which surface is drawn when,
//! and this one keeps the other: what drawing each of them means.

use ratatui::{
    Frame,
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
};

use super::{
    chrome::{composer_title, title},
    configuration::render_configuration,
    panel::{Body, Chrome, Edges, Panel, place_cursor, render_steer},
    permission_review,
};
use crate::{
    ViewState, content,
    layout::{self, WorkspaceInput},
    state::{Caret, inner_width},
    surface::{SurfaceId, SurfaceTree, Viewport},
    theme::{Palette, Role},
};

/// The Configuration and Permissions pages, which paint themselves; `None` for the page list.
pub(super) fn drawer_page(
    frame: &mut Frame<'_>,
    palette: &Palette,
    state: &ViewState,
    bounds: Rect,
    has_focus: bool,
) -> Option<Viewport> {
    if state.configuration().is_some() {
        Some(render_configuration(
            frame,
            palette,
            state,
            bounds,
            has_focus,
            state.scroll_position(SurfaceId::Drawer),
        ))
    } else {
        permission_review::render(frame, palette, state, bounds, has_focus)
    }
}

/// The Drawer: a box of its own, not a section of anyone's.
///
/// The workspace owns it rather than a conversation, which is what separates it from an approval —
/// an approval is a question one agent is waiting on, so it renders inside that agent's box. Its
/// title names the addressee, then the open page.
pub(super) fn drawer_panel(state: &ViewState, palette: &Palette, bounds: Rect) -> Panel {
    let insets = crate::surface::ContentInsets::for_surface(SurfaceId::Drawer, bounds.height);
    Panel {
        insets,
        chrome: Chrome::Box,
        footer: None,
        body: Body::Whole {
            lines: content::drawer(state, palette, insets.width(bounds.width), bounds.height),
            follows_tail: false,
        },
        title: title(palette, drawer_title(state), Role::SectionHeading, ""),
        badge: None,
        edges: Edges::All,
    }
}

pub(super) fn drawer_retract(
    frame: &mut Frame<'_>,
    palette: &Palette,
    state: &ViewState,
    bounds: Rect,
) {
    let control = layout::drawer_retract_control(bounds);
    if control.is_empty() {
        return;
    }
    let hovered = state.drawer_retract_hovered();
    let mut face = palette
        .style(if hovered { Role::Accent } else { Role::Muted })
        .add_modifier(Modifier::UNDERLINED);
    if hovered {
        face.bg = palette.style(Role::Chosen).bg;
    }
    // Downward corners join the existing rule. Underline draws the lower edge within this
    // same row: no second border row, upper outline, graphics protocol or animation owner.
    // U+FE3D is one character occupying two terminal cells; equal padding centers its glyph.
    let border = palette
        .style(Role::BorderFocused)
        .add_modifier(Modifier::UNDERLINED);
    let line = Line::from(vec![
        Span::styled("┐", border),
        Span::styled("  ︽  ", face),
        Span::styled("┌", border),
    ]);
    frame.render_widget(ratatui::widgets::Paragraph::new(line), control);
}

/// `Workspace`, the addressee, then the page that is open (ui-ux §product vocabulary).
fn drawer_title(state: &ViewState) -> String {
    state
        .drawer()
        .and_then(crate::state::Drawer::page)
        .map_or_else(
            || "Workspace".to_owned(),
            |page| format!("Workspace · {}", page.name()),
        )
}

/// Puts the workspace's one cursor in the surface that owns it (COM-1): the inspector's input
/// strip, or the caret of the text this panel painted.
pub(super) fn draw_cursor(
    frame: &mut Frame<'_>,
    palette: &Palette,
    state: &ViewState,
    id: SurfaceId,
    bounds: Rect,
    panel: &Panel,
    steer: Option<&(layout::SteerSplit, plexmaton_core::AgentId)>,
) {
    match steer {
        Some((split, agent_id)) if id == SurfaceId::Inspector => {
            render_steer(frame, palette, state, agent_id, split.input);
        }
        _ => place_cursor(
            frame,
            panel.insets.inset(bounds),
            input_caret(
                state,
                id,
                panel.insets.width(bounds.width),
                crate::state::input_window(bounds),
            ),
            panel.edges,
        ),
    }
}

/// Resolve the input painted in this surface, using the same width, window and inset as its text.
pub(super) fn input_caret(state: &ViewState, id: SurfaceId, width: u16, window: u16) -> Caret {
    if id == SurfaceId::Drawer {
        return state
            .drawer()
            .map_or(Caret::default(), |drawer| drawer.filter_view(width).1);
    }
    state.composer().caret(width, window)
}

/// The primary input's return target while an entered worker holds the cursor (INS-5).
pub(super) fn collapsed_composer_panel(
    state: &ViewState,
    palette: &Palette,
    stacking: &Stacking,
) -> Panel {
    Panel {
        insets: crate::surface::ContentInsets::default(),
        chrome: Chrome::Rules,
        footer: None,
        body: Body::Whole {
            lines: content::composer_collapsed(state, palette),
            follows_tail: false,
        },
        title: Line::default(),
        badge: None,
        edges: if stacking.composer_under.is_some() {
            Edges::Closing
        } else {
            Edges::All
        },
    }
}

/// Derives layout inputs once from the current projection and terminal geometry.
pub(super) fn workspace_input(area: Rect, state: &ViewState) -> WorkspaceInput {
    let inspector = state.inspector_request();
    let composer_width = layout::composer_width(area, inspector);
    WorkspaceInput {
        status_rows: state.status().rows(),
        has_notices: state.notices().next().is_some(),
        attention: state.attention_listed_count(),
        decision_rows: state.decision_rows(composer_width),
        command_inspection: state.command_inspection_open(),
        decision_mode: if state.approval_in_primary() {
            layout::DecisionMode::Inline
        } else {
            layout::DecisionMode::Modal
        },
        drawer_rows: state.drawer_rows(area.width),
        drawer_focus: state.drawer_focus(),
        composer_menu_rows: state.composer_menu_rows(inner_width(composer_width)),
        rail: state.sub_agents().next().is_some(),
        composer_rows: state.composer_rows(composer_width, layout::composer_cap(area.height)),
        inspector,
    }
}

/// The menu is a titled rule and its rows above the composer's top rule, which closes it.
pub(super) fn composer_menu_panel(state: &ViewState, palette: &Palette, bounds: Rect) -> Panel {
    Panel {
        insets: crate::surface::ContentInsets::default(),
        chrome: Chrome::Rules,
        footer: None,
        body: Body::Whole {
            lines: content::composer_menu(state, palette, inner_width(bounds.width), bounds.height),
            follows_tail: false,
        },
        title: title(palette, state.menu_title(), Role::SectionHeading, ""),
        badge: None,
        edges: Edges::Upper,
    }
}

/// The decision region is a section of its conversation, above rather than covering the composer.
pub(super) fn approval_panel(
    state: &ViewState,
    palette: &Palette,
    bounds: Rect,
    stacking: &Stacking,
) -> Panel {
    let tool = state
        .approval()
        .map_or_else(String::new, |approval| approval.tool.to_owned());
    Panel {
        insets: crate::surface::ContentInsets::for_surface(SurfaceId::Approval, bounds.height),
        chrome: Chrome::Rules,
        footer: None,
        body: Body::Whole {
            lines: content::approval(
                state,
                palette,
                crate::surface::ContentInsets::for_surface(SurfaceId::Approval, bounds.height)
                    .width(bounds.width),
                bounds.height.saturating_sub(
                    1 + 2 * crate::surface::ContentInsets::for_surface(
                        SurfaceId::Approval,
                        bounds.height,
                    )
                    .vertical,
                ),
            ),
            follows_tail: false,
        },
        title: title(
            palette,
            match state.approval().map(|view| view.stage) {
                Some(crate::ApprovalStage::Remember) => "Remember permission",
                Some(crate::ApprovalStage::Submitting) => "Applying decision",
                _ => "Approval required",
            },
            Role::ActionRequired,
            format!(" · {tool}"),
        ),
        badge: None,
        edges: if stacking.composer_under.is_some() {
            Edges::Middle
        } else {
            Edges::Upper
        },
    }
}

/// Which surfaces share one outline this frame.
///
/// A conversation and the composer it addresses share one outline (`ui-ux.md` §input —
/// the input lives inside the surface it addresses). Read from geometry rather than from layout
/// class, so the painter and the layout cannot disagree about what is stacked.
pub(super) struct Stacking {
    composer_under: Option<SurfaceId>,
}

impl Stacking {
    pub(super) fn of(surfaces: &SurfaceTree) -> Self {
        let stacked =
            |upper: SurfaceId, lower: SurfaceId| match (surfaces.get(upper), surfaces.get(lower)) {
                (Some(upper), Some(lower)) => {
                    lower.bounds.x == upper.bounds.x && lower.bounds.y == upper.bounds.bottom()
                }
                _ => false,
            };
        // The decision region, when open, is between them: the conversation is still the section
        // with something beneath it, and the composer is still the one that closes the box.
        let below = |id| stacked(id, SurfaceId::Approval) || stacked(id, SurfaceId::Composer);
        let composer_under = if below(SurfaceId::Transcript) {
            Some(SurfaceId::Transcript)
        } else if below(SurfaceId::Inspector) {
            Some(SurfaceId::Inspector)
        } else {
            None
        };
        Self { composer_under }
    }

    /// The edges of a conversation that may have an input section beneath it.
    pub(super) fn over_composer(&self, id: SurfaceId) -> Edges {
        if self.composer_under == Some(id) {
            Edges::Upper
        } else {
            Edges::All
        }
    }
}

/// The primary composer: the input between two rules under its conversation (ui-ux §input).
pub(super) fn composer_panel(
    state: &ViewState,
    palette: &Palette,
    has_focus: bool,
    bounds: Rect,
    stacking: &Stacking,
) -> Panel {
    Panel {
        insets: crate::surface::ContentInsets::default(),
        chrome: Chrome::Rules,
        footer: None,
        body: Body::Whole {
            lines: content::composer(
                state,
                palette,
                has_focus,
                inner_width(bounds.width),
                crate::state::input_window(bounds),
            ),
            follows_tail: true,
        },
        title: composer_title(state, palette),
        badge: None,
        edges: if stacking.composer_under.is_some() {
            Edges::Lower
        } else {
            Edges::All
        },
    }
}
