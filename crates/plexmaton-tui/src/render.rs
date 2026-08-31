use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    ViewState, content,
    layout::{self, LayoutClass, MIN_HEIGHT, MIN_WIDTH, WorkspaceInput},
    surface::{SurfaceId, SurfaceKind, SurfaceTree, Viewport},
    theme::{Palette, Role},
};

/// Rows a bordered block spends on its own frame.
const BORDER_ROWS: u16 = 2;

/// Projects the current view state into a Ratatui frame without mutating it.
///
/// Returns the surfaces this frame actually drew, with each one's measured viewport, which is what
/// the router hit-tests and scrolls against. Handing the registry back rather than recomputing it
/// elsewhere is what keeps SURF-1 true: routing cannot be given geometry the renderer did not use.
pub fn render(frame: &mut Frame<'_>, state: &ViewState, palette: &Palette) -> SurfaceTree {
    let area = frame.area();
    if LayoutClass::for_size(area.width, area.height) == LayoutClass::TooSmall {
        render_too_small(frame, palette, area);
        return SurfaceTree::default();
    }

    let mut surfaces = layout::workspace(
        area,
        WorkspaceInput {
            has_notices: state.notices().next().is_some(),
            composer_rows: state.composer().requested_rows(),
        },
    );
    let focused = state.focused(&surfaces);
    // Identities first, so each surface's viewport can be recorded as it is measured.
    let drawn: Vec<(SurfaceId, Rect, SurfaceKind)> = surfaces
        .iter()
        .map(|surface| (surface.id, surface.bounds, surface.kind))
        .collect();

    for (id, bounds, kind) in drawn {
        let has_focus = focused == Some(id);
        // An exhaustive match, so a new surface identity cannot be added without stating how it is
        // drawn and whether it scrolls.
        let panel = match id {
            SurfaceId::Agents => Some(Panel {
                lines: content::agents(state, palette),
                title: agents_title(state),
                title_role: attention_role(state),
                follows_tail: false,
            }),
            SurfaceId::Transcript => Some(Panel {
                lines: content::transcript(state, palette),
                title: transcript_title(state),
                title_role: Role::Muted,
                // A conversation opens at its newest line; that is where the reader is.
                follows_tail: true,
            }),
            SurfaceId::Activity => Some(Panel {
                lines: content::activity(state, palette),
                title: " Activity ".to_owned(),
                title_role: Role::Muted,
                follows_tail: false,
            }),
            SurfaceId::Notices => Some(Panel {
                lines: content::notices(state, palette),
                title: notices_title(state),
                title_role: Role::Muted,
                // A bounded tail view: the newest defect is the one worth showing.
                follows_tail: true,
            }),
            SurfaceId::Composer => Some(Panel {
                lines: content::composer(state, palette, has_focus),
                title: composer_title(state),
                title_role: Role::Muted,
                follows_tail: true,
            }),
            SurfaceId::Footer => {
                render_footer(frame, palette, bounds);
                None
            }
        };

        let Some(panel) = panel else { continue };
        let viewport = draw_panel(
            frame,
            palette,
            bounds,
            has_focus,
            &panel,
            state.scroll_offset(id),
        );
        // Measurement is what the wheel resolves against, so it goes back into the registry the
        // router will be handed. Only the hint strip has nothing to measure.
        surfaces.set_viewport(id, viewport);

        if kind == SurfaceKind::Composer && has_focus {
            place_cursor(frame, bounds, &panel.lines);
        }
    }

    surfaces
}

/// One bordered, scrollable region, ready to draw.
struct Panel {
    lines: Vec<Line<'static>>,
    title: String,
    title_role: Role,
    /// Whether an untouched viewport opens at the end of its content rather than the start.
    follows_tail: bool,
}

/// Draws a panel through its viewport and returns what it measured.
///
/// The measurement comes from the same `Paragraph` that paints, so the wrap that decides how tall
/// the content is and the wrap that puts it on screen are the same computation.
fn draw_panel(
    frame: &mut Frame<'_>,
    palette: &Palette,
    area: Rect,
    focused: bool,
    panel: &Panel,
    stored_offset: Option<u16>,
) -> Viewport {
    let block = block(palette, panel.title.clone(), panel.title_role, focused);
    let paragraph = Paragraph::new(panel.lines.clone())
        .wrap(Wrap { trim: false })
        .block(block);

    // `line_count` wraps at exactly the width it is given and then adds the block's border rows, so
    // it is asked for the inner width and those rows are taken back off.
    let inner_width = area.width.saturating_sub(BORDER_ROWS);
    let measured = u16::try_from(paragraph.line_count(inner_width)).unwrap_or(u16::MAX);
    let mut viewport = Viewport {
        content_rows: measured.saturating_sub(BORDER_ROWS),
        visible_rows: area.height.saturating_sub(BORDER_ROWS),
        offset: 0,
    };
    // An untouched surface takes its anchor from its content, not from zero.
    viewport.offset = stored_offset
        .unwrap_or(if panel.follows_tail {
            viewport.max_offset()
        } else {
            0
        })
        .min(viewport.max_offset());

    frame.render_widget(paragraph.scroll((viewport.offset, 0)), area);
    viewport
}

/// Places the workspace's one cursor at the end of the composer's last visible line.
///
/// The only `set_cursor_position` call site in the workspace. Ratatui hides the cursor unless a
/// frame asks for it, so "exactly one cursor" (COM-1) is a property of there being one caller.
fn place_cursor(frame: &mut Frame<'_>, area: Rect, lines: &[Line<'_>]) {
    let last = lines.last();
    // Display width, not character count: a wide glyph occupies two cells and the caret has to
    // land after both.
    let column = last.map_or(0, |line| {
        u16::try_from(UnicodeWidthStr::width(line.to_string().as_str())).unwrap_or(u16::MAX)
    });
    let rows = u16::try_from(lines.len()).unwrap_or(1).max(1);
    let inside_width = area.width.saturating_sub(BORDER_ROWS);
    let inside_height = area.height.saturating_sub(BORDER_ROWS);
    frame.set_cursor_position((
        area.x
            .saturating_add(1)
            .saturating_add(column.min(inside_width)),
        area.y.saturating_add(rows.min(inside_height)),
    ));
}

fn agents_title(state: &ViewState) -> String {
    format!(" Agents · attention {} ", state.attention_count())
}

/// An unanswered request must read as action required, not as ambient decoration.
fn attention_role(state: &ViewState) -> Role {
    if state.attention_count() == 0 {
        Role::Muted
    } else {
        Role::ActionRequired
    }
}

fn transcript_title(state: &ViewState) -> String {
    state.selected_agent().map_or_else(
        || " Transcript ".to_owned(),
        |agent| {
            format!(
                " {} · {} ",
                agent.label,
                content::agent_status_label(agent.status)
            )
        },
    )
}

fn notices_title(state: &ViewState) -> String {
    let retained = state.notices().count();
    let dropped = state.notices_dropped();
    if dropped == 0 {
        format!(" Notices · {retained} ")
    } else {
        format!(" Notices · {retained} · {dropped} discarded ")
    }
}

/// The title names the target, which keeps the binding visible rather than remembered when the
/// selection is on a different agent (COM-4).
fn composer_title(state: &ViewState) -> String {
    state.primary_agent().map_or_else(
        || " Message ".to_owned(),
        |agent| format!(" Message {} ", agent.label),
    )
}

fn render_footer(frame: &mut Frame<'_>, palette: &Palette, area: Rect) {
    // Escape resolves the topmost layer and never quits, so the hint must not offer it as an exit.
    let footer = Line::from(vec![
        Span::styled(" ↑↓ ", palette.style(Role::KeyHint)),
        Span::styled(" select  ·  ", palette.style(Role::Muted)),
        Span::styled(" ⇥ ", palette.style(Role::KeyHint)),
        Span::styled(" focus  ·  ", palette.style(Role::Muted)),
        Span::styled(" q ", palette.style(Role::KeyHint)),
        Span::styled(" quit", palette.style(Role::Muted)),
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

/// A bordered region.
///
/// The border carries focus and the title carries attention, so the two never compete for the same
/// pixels and a focused panel with a pending request still reads as both.
fn block(palette: &Palette, title: String, title_role: Role, focused: bool) -> Block<'static> {
    let border = if focused {
        Role::BorderFocused
    } else {
        Role::Border
    };
    Block::default()
        .borders(Borders::ALL)
        .border_style(palette.style(border))
        .title(Span::styled(title, palette.style(title_role)))
}

#[cfg(test)]
mod tests {
    use ratatui::{
        Terminal,
        backend::TestBackend,
        buffer::Buffer,
        layout::Rect,
        style::{Color, Modifier, Style},
    };

    use crate::{
        ViewState,
        intent::{Direction, ScrollDirection, TextIntent},
        layout,
        layout::WorkspaceInput,
        render,
        surface::{SurfaceId, SurfaceTree},
        test_support::{canonical_state, degraded_state, draw, draw_frame, draw_with, region_text},
        theme::{Palette, Role},
    };

    /// The visible part of a style: colour and modifiers.
    ///
    /// A whole `Style` cannot be compared against a palette entry, because the buffer fills unset
    /// fields with `Reset` where the palette leaves them `None`. That difference is plumbing; these
    /// two fields are what the user sees, and between them every palette carries the distinction.
    type Ink = (Option<Color>, Modifier);

    fn ink(style: Style) -> Ink {
        // An unset colour and an explicit reset paint the same thing; the buffer stores the second
        // where a palette stores the first, and monochrome sets neither.
        (
            style.fg.filter(|colour| *colour != Color::Reset),
            style.add_modifier,
        )
    }

    fn border_ink(buffer: &Buffer, surfaces: &SurfaceTree, id: SurfaceId) -> Ink {
        let bounds = surfaces
            .get(id)
            .unwrap_or_else(|| panic!("{id:?} must be registered"))
            .bounds;
        ink(buffer[(bounds.x, bounds.y)].style())
    }

    fn role_ink(palette: &Palette, role: Role) -> Ink {
        ink(palette.style(role))
    }

    /// A conversation opens at its newest line, and the wheel moves it from there.
    ///
    /// Measured through the same `Paragraph` that paints, so what the viewport believes about its
    /// content and what reaches the screen are one computation.
    #[test]
    fn the_transcript_opens_at_its_tail_and_the_wheel_moves_it() {
        let palette = Palette::default();
        let mut state = canonical_state();

        // Narrow enough that the conversation wraps past the rows it is given.
        let (surfaces, buffer) = draw_frame(&state, &palette, 48, 12);
        let bounds = surfaces
            .get(SurfaceId::Transcript)
            .unwrap_or_else(|| panic!("the conversation is always registered"))
            .bounds;
        let viewport = surfaces
            .viewport(SurfaceId::Transcript)
            .unwrap_or_else(|| panic!("a drawn surface has been measured"));

        assert!(
            viewport.is_scrollable(),
            "the fixture has to overflow or this test proves nothing: {viewport:?}"
        );
        assert_eq!(
            viewport.offset,
            viewport.max_offset(),
            "an untouched conversation opens at its newest line, not its oldest"
        );
        let at_the_tail = region_text(&buffer, bounds);

        state.scroll(&surfaces, SurfaceId::Transcript, ScrollDirection::Up);
        let (scrolled, buffer) = draw_frame(&state, &palette, 48, 12);

        let moved = scrolled
            .viewport(SurfaceId::Transcript)
            .unwrap_or_else(|| panic!("still measured"));
        assert!(
            moved.offset < viewport.offset,
            "the wheel moved the viewport"
        );
        assert_ne!(
            region_text(&buffer, bounds),
            at_the_tail,
            "and the rows that reached the screen changed with it"
        );
    }

    /// SURF-5, scroll half: the offset belongs to the surface, not to the frame that drew it.
    #[test]
    fn a_scrolled_surface_is_where_the_user_left_it_after_a_resize() {
        let palette = Palette::default();
        let mut state = canonical_state();
        let (surfaces, _) = draw_frame(&state, &palette, 48, 12);

        state.scroll(&surfaces, SurfaceId::Transcript, ScrollDirection::Up);
        let parked = state
            .scroll_offset(SurfaceId::Transcript)
            .unwrap_or_else(|| panic!("the wheel stored an offset"));

        // A different terminal size relays out every rectangle and re-measures every viewport.
        let (wide, _) = draw_frame(&state, &palette, 120, 24);
        assert!(wide.viewport(SurfaceId::Transcript).is_some());
        assert_eq!(
            state.scroll_offset(SurfaceId::Transcript),
            Some(parked),
            "re-laying out the workspace must not reset where the user was reading"
        );
    }

    /// COM-1: a cursor is on screen exactly when the composer holds focus.
    ///
    /// Read from the backend rather than from state, because the question is what the terminal was
    /// told. A focus model the renderer ignores would leave the user typing with no caret.
    #[test]
    fn the_cursor_exists_only_while_the_composer_holds_focus() {
        let palette = Palette::default();
        let mut state = canonical_state();
        let cursor = |state: &ViewState| {
            let mut terminal = Terminal::new(TestBackend::new(120, 24))
                .unwrap_or_else(|error| panic!("test terminal: {error}"));
            terminal
                .draw(|frame| {
                    render(frame, state, &palette);
                })
                .unwrap_or_else(|error| panic!("test render: {error}"));
            let backend = terminal.backend();
            backend.cursor_visible().then(|| backend.cursor_position())
        };

        assert_eq!(cursor(&state), None, "focus starts on the agent rail");
        // Type first, so a hidden cursor cannot be mistaken for an empty composer.
        state.edit(TextIntent::Insert('h'));
        assert_eq!(
            cursor(&state),
            None,
            "a draft nobody is focused on still shows no cursor"
        );

        let surfaces = layout::workspace(Rect::new(0, 0, 120, 24), WorkspaceInput::default());
        state.focus_surface(&surfaces, SurfaceId::Composer);
        let bounds = surfaces
            .get(SurfaceId::Composer)
            .unwrap_or_else(|| panic!("the composer is always registered"))
            .bounds;
        let at = cursor(&state).unwrap_or_else(|| panic!("a focused composer owns the cursor"));

        assert!(
            bounds.contains(at),
            "the cursor landed at {at:?}, outside the composer at {bounds:?}"
        );
    }

    /// COM-4: the composer's target is on screen and does not follow the selection (D-017).
    #[test]
    fn the_composer_names_its_target_while_another_agent_is_selected() {
        let mut state = canonical_state();
        let agent_b = plexmaton_core::AgentId::new("agent-b")
            .unwrap_or_else(|error| panic!("invalid fixture: {error}"));
        state
            .select_agent(&agent_b)
            .unwrap_or_else(|error| panic!("agent-b exists: {error}"));

        let (surfaces, buffer) = draw_frame(&state, &Palette::default(), 120, 24);
        let region = |id| {
            region_text(
                &buffer,
                surfaces
                    .get(id)
                    .unwrap_or_else(|| panic!("{id:?} must be registered"))
                    .bounds,
            )
        };

        assert!(
            region(SurfaceId::Transcript).contains("Agent B"),
            "the selection really did move, so this is not a test of nothing changing"
        );
        assert!(
            region(SurfaceId::Composer).contains("Message Agent A"),
            "typing still goes to the primary agent, and the title has to say so"
        );
    }

    /// SURF-3: exactly one surface holds focus, and the screen says which.
    ///
    /// Reading it back from painted cells is what makes this more than a state assertion: a focus
    /// model the renderer ignores would leave the user with no way to tell where `Tab` went. Run
    /// against every palette, because a monochrome terminal must show focus too.
    #[test]
    fn only_the_focused_panel_carries_the_focused_border() {
        for palette in [Palette::ansi(), Palette::truecolor(), Palette::monochrome()] {
            let mut state = canonical_state();

            let (surfaces, buffer) = draw_frame(&state, &palette, 120, 24);
            assert_eq!(
                state.focused(&surfaces),
                Some(SurfaceId::Agents),
                "focus starts at the first stop on the ring"
            );
            assert_eq!(
                border_ink(&buffer, &surfaces, SurfaceId::Agents),
                role_ink(&palette, Role::BorderFocused)
            );
            assert_eq!(
                border_ink(&buffer, &surfaces, SurfaceId::Transcript),
                role_ink(&palette, Role::Border)
            );

            state.cycle_focus(&surfaces, Direction::Forward);
            let (surfaces, buffer) = draw_frame(&state, &palette, 120, 24);
            assert_eq!(
                border_ink(&buffer, &surfaces, SurfaceId::Agents),
                role_ink(&palette, Role::Border)
            );
            assert_eq!(
                border_ink(&buffer, &surfaces, SurfaceId::Transcript),
                role_ink(&palette, Role::BorderFocused),
                "the focused border moved with the ring rather than being painted twice"
            );
        }
    }

    /// SURF-1: every registered surface is painted inside the rectangle it registered.
    ///
    /// Move any panel's draw call to a different rectangle and its signature leaves the region
    /// this walks, which is the "the click landed one panel over" defect caught before it ships.
    #[test]
    fn every_registered_surface_is_drawn_inside_its_own_bounds() {
        let (surfaces, buffer) = draw_frame(&degraded_state(), &Palette::default(), 120, 24);

        assert_eq!(surfaces.len(), 6, "a degraded workspace registers all six");
        for surface in surfaces.iter() {
            // An exhaustive match, so a new surface identity cannot be added without stating what
            // proves it was drawn.
            let signature = match surface.id {
                SurfaceId::Agents => "Agents",
                SurfaceId::Transcript => "Agent A · primary",
                SurfaceId::Activity => "Artifacts",
                SurfaceId::Composer => "Message Agent A",
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
