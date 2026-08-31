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
    state::ScrollPosition,
    surface::{SurfaceId, SurfaceKind, SurfaceTree, Viewport},
    theme::{Palette, Role},
    transcript::TranscriptMetrics,
};

/// Rows a bordered block spends on its own frame.
const BORDER_ROWS: u16 = 2;

/// Projects the current view state into a Ratatui frame without mutating it.
///
/// Returns the surfaces this frame actually drew, with each one's measured viewport, which is what
/// the router hit-tests and scrolls against. Handing the registry back rather than recomputing it
/// elsewhere is what keeps SURF-1 true: routing cannot be given geometry the renderer did not use.
///
/// `metrics` is the one thing here that outlives the frame. The projection stays immutable, so
/// wrapped heights have nowhere else to be remembered, and remembering them is what keeps a frame's
/// cost proportional to what changed rather than to the conversation's length (TR-1).
pub fn render(
    frame: &mut Frame<'_>,
    state: &ViewState,
    palette: &Palette,
    metrics: &mut TranscriptMetrics,
) -> SurfaceTree {
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
                body: Body::Whole {
                    lines: content::agents(state, palette),
                    follows_tail: false,
                },
                title: agents_title(state),
                title_role: attention_role(state),
            }),
            SurfaceId::Transcript => Some(Panel {
                body: transcript_body(state, palette, metrics, bounds),
                title: transcript_title(state),
                title_role: Role::Muted,
            }),
            SurfaceId::Activity => Some(Panel {
                body: Body::Whole {
                    lines: content::activity(state, palette),
                    follows_tail: false,
                },
                title: " Activity ".to_owned(),
                title_role: Role::Muted,
            }),
            SurfaceId::Notices => Some(Panel {
                body: Body::Whole {
                    lines: content::notices(state, palette),
                    follows_tail: true,
                },
                title: notices_title(state),
                title_role: Role::Muted,
            }),
            SurfaceId::Composer => Some(Panel {
                body: Body::Whole {
                    lines: content::composer(state, palette, has_focus),
                    follows_tail: true,
                },
                title: composer_title(state),
                title_role: Role::Muted,
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
            state.scroll_position(id),
        );
        // Measurement is what the wheel resolves against, so it goes back into the registry the
        // router will be handed. Only the hint strip has nothing to measure.
        surfaces.set_viewport(id, viewport);

        if kind == SurfaceKind::Composer && has_focus {
            place_cursor(frame, bounds, panel.body.lines());
        }
    }

    surfaces
}

/// One bordered, scrollable region, ready to draw.
struct Panel {
    body: Body,
    title: String,
    title_role: Role,
}

/// What a panel has to draw, and how much of it the frame had to build.
///
/// `follows_tail` belongs to the whole-body arm alone: a windowed body arrives with its offset
/// already resolved through the reader's anchor, so a second answer here could only disagree.
enum Body {
    /// Content short enough that building all of it costs nothing, measured as one paragraph.
    Whole {
        lines: Vec<Line<'static>>,
        /// Whether an untouched viewport opens at the end of its content rather than the start.
        follows_tail: bool,
    },
    /// A conversation, measured item by item and built only where the viewport reaches (TR-2).
    Window {
        lines: Vec<Line<'static>>,
        /// Rows to skip inside the first built item.
        skip_rows: u16,
        viewport: Viewport,
    },
}

impl Body {
    fn lines(&self) -> &[Line<'static>] {
        match self {
            Self::Whole { lines, .. } | Self::Window { lines, .. } => lines,
        }
    }
}

/// Builds the part of the selected conversation this frame will draw.
///
/// The whole history is measured, from the cache; only the items the viewport reaches are turned
/// into lines. A conversation with no items falls back to a whole body, because a placeholder has
/// nothing to virtualize.
fn transcript_body(
    state: &ViewState,
    palette: &Palette,
    metrics: &mut TranscriptMetrics,
    area: Rect,
) -> Body {
    let Some(agent) = state.selected_agent() else {
        return Body::Whole {
            lines: content::transcript_placeholder(palette, false),
            follows_tail: false,
        };
    };
    let visible_rows = area.height.saturating_sub(BORDER_ROWS);
    if metrics.measure(agent, palette, area.width.saturating_sub(BORDER_ROWS)) == 0 {
        return Body::Whole {
            lines: content::transcript_placeholder(palette, true),
            follows_tail: false,
        };
    }

    let mut viewport = Viewport {
        content_rows: metrics.total_rows(&agent.id),
        visible_rows,
        offset: 0,
    };
    // An untouched conversation opens at its newest line; a parked one resolves through the item
    // its reader stopped at, so this width's rows are recomputed rather than remembered (TR-3).
    viewport.offset = state.conversation_position().map_or_else(
        || viewport.max_offset(),
        |position| metrics.offset_of(&agent.id, position, viewport.max_offset()),
    );
    let window = metrics.window(&agent.id, viewport.offset, visible_rows);

    let lines = agent
        .transcript()
        .skip(window.items.start)
        .take(window.items.len())
        .flat_map(|item| content::transcript_item(item, palette))
        .collect();
    Body::Window {
        lines,
        skip_rows: window.skip_rows,
        viewport,
    }
}

/// Draws a panel through its viewport and returns what it measured.
///
/// A whole body is measured by the same `Paragraph` that paints it, so the wrap deciding how tall
/// the content is and the wrap putting it on screen are one computation. A windowed body arrives
/// already measured, and scrolls by the rows into its first item rather than by rows into a history
/// it never built.
fn draw_panel(
    frame: &mut Frame<'_>,
    palette: &Palette,
    area: Rect,
    focused: bool,
    panel: &Panel,
    parked: Option<ScrollPosition>,
) -> Viewport {
    let block = block(palette, panel.title.clone(), panel.title_role, focused);
    let paragraph = Paragraph::new(panel.body.lines().to_vec())
        .wrap(Wrap { trim: false })
        .block(block);

    let (viewport, scroll) = match &panel.body {
        Body::Whole { follows_tail, .. } => {
            // `line_count` wraps at exactly the width it is given and then adds the block's border
            // rows, so it is asked for the inner width and those rows are taken back off.
            let inner_width = area.width.saturating_sub(BORDER_ROWS);
            let measured = u16::try_from(paragraph.line_count(inner_width)).unwrap_or(u16::MAX);
            let mut viewport = Viewport {
                content_rows: measured.saturating_sub(BORDER_ROWS),
                visible_rows: area.height.saturating_sub(BORDER_ROWS),
                offset: 0,
            };
            viewport.offset = resolve_offset(parked, *follows_tail, viewport.max_offset());
            (viewport, viewport.offset)
        }
        Body::Window {
            skip_rows,
            viewport,
            ..
        } => (*viewport, *skip_rows),
    };

    frame.render_widget(paragraph.scroll((scroll, 0)), area);
    viewport
}

/// Turns a stored scroll position into a row offset for a viewport this deep.
///
/// An untouched surface has no stored position at all, and takes its anchor from its own kind of
/// content: a conversation opens at its newest line, a list at its first (TR-4).
const fn resolve_offset(
    parked: Option<ScrollPosition>,
    follows_tail: bool,
    max_offset: u16,
) -> u16 {
    match parked {
        Some(position) => position.offset(max_offset),
        None if follows_tail => max_offset,
        None => 0,
    }
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
        widgets::{Paragraph, Wrap},
    };

    use super::{block, transcript_title};

    use crate::{
        TranscriptMetrics, ViewState,
        intent::{Direction, ScrollDirection, TextIntent},
        layout,
        layout::WorkspaceInput,
        render,
        surface::{SurfaceId, SurfaceTree},
        test_support::{
            Session, canonical_state, degraded_state, draw, draw_frame, draw_with, region_text,
        },
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

    /// TR-2: virtualizing changed what a frame builds, not what reaches the screen.
    ///
    /// The reference is the whole conversation in one `Paragraph` scrolled by the same offset —
    /// exactly what this panel did before it was virtualized. Differential rather than a snapshot
    /// on purpose: a snapshot proves the screen is stable, this proves it is unchanged, and it
    /// keeps proving it as the content grows.
    #[test]
    fn a_virtualized_conversation_paints_what_the_whole_one_did() {
        for (width, height) in [(48_u16, 12_u16), (120, 24), (72, 30)] {
            let mut session = Session::canonical(width, height);
            session.conversation.extend(6);
            session.draw();

            for notches in 0..4 {
                let viewport = session.viewport(SurfaceId::Transcript);
                assert!(
                    viewport.is_scrollable(),
                    "the fixture has to overflow at {width}x{height} or this proves nothing"
                );
                assert_eq!(
                    session.region(SurfaceId::Transcript),
                    whole_conversation(
                        &session.conversation.state,
                        &Palette::default(),
                        session.bounds(SurfaceId::Transcript),
                        viewport,
                        height,
                    ),
                    "virtualized and whole disagreed at {width}x{height}, {notches} notches up"
                );
                session.wheel(SurfaceId::Transcript, ScrollDirection::Up, 1);
            }
        }
    }

    /// Draws the whole conversation the pre-virtualization way, for the test above to compare with.
    fn whole_conversation(
        state: &ViewState,
        palette: &Palette,
        bounds: Rect,
        viewport: crate::Viewport,
        height: u16,
    ) -> String {
        let lines: Vec<_> = state
            .selected_agent()
            .unwrap_or_else(|| panic!("the canonical timeline selects an agent"))
            .transcript()
            .flat_map(|item| crate::content::transcript_item(item, palette))
            .collect();
        let paragraph = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block(palette, transcript_title(state), Role::Muted, false))
            .scroll((viewport.offset, 0));

        let mut terminal =
            Terminal::new(TestBackend::new(bounds.right().max(bounds.width), height))
                .unwrap_or_else(|error| panic!("test terminal: {error}"));
        terminal
            .draw(|frame| frame.render_widget(paragraph, bounds))
            .unwrap_or_else(|error| panic!("test render: {error}"));
        region_text(terminal.backend().buffer(), bounds)
    }

    /// A conversation opens at its newest line, and the wheel moves it from there.
    ///
    /// Measured through the same `Paragraph` that paints, so what the viewport believes about its
    /// content and what reaches the screen are one computation.
    #[test]
    fn the_transcript_opens_at_its_tail_and_the_wheel_moves_it() {
        // Narrow enough that the conversation wraps past the rows it is given.
        let mut session = Session::canonical(48, 12);
        let viewport = session.viewport(SurfaceId::Transcript);

        assert!(
            viewport.is_scrollable(),
            "the fixture has to overflow or this test proves nothing: {viewport:?}"
        );
        assert_eq!(
            viewport.offset,
            viewport.max_offset(),
            "an untouched conversation opens at its newest line, not its oldest"
        );
        let at_the_tail = session.region(SurfaceId::Transcript);

        session.wheel(SurfaceId::Transcript, ScrollDirection::Up, 1);
        assert!(
            session.viewport(SurfaceId::Transcript).offset < viewport.offset,
            "the wheel moved the viewport"
        );
        assert_ne!(
            session.region(SurfaceId::Transcript),
            at_the_tail,
            "and the rows that reached the screen changed with it"
        );
    }

    /// TR-3, and SURF-5's scroll half: a resize keeps the reader on the same message.
    ///
    /// This replaces a step-4 test that asserted the stored *offset* survived a resize. The number
    /// surviving is not the property anyone wants — at a new width the same row number names
    /// different text, so preserving it moves the reader while looking like it did not.
    #[test]
    fn a_resized_conversation_keeps_the_reader_on_the_same_message() {
        let mut session = Session::canonical(60, 20);
        session.conversation.extend(10);
        session.draw();
        session.wheel(SurfaceId::Transcript, ScrollDirection::Up, 6);

        let narrow = session.viewport(SurfaceId::Transcript);
        let reading = markers(&session.region(SurfaceId::Transcript));
        assert!(
            !reading.is_empty(),
            "the reader has to be somewhere in the filler or this proves nothing"
        );

        session.resize(160, 20);
        let wide = session.viewport(SurfaceId::Transcript);
        assert_ne!(
            narrow.content_rows, wide.content_rows,
            "the resize has to rewrap the conversation, or a stored row would have survived too"
        );
        assert_eq!(
            markers(&session.region(SurfaceId::Transcript)).first(),
            reading.first(),
            "the topmost message on screen changed when only the width did"
        );
    }

    /// TR-5: each conversation keeps its own reading position (canonical journey, step 4).
    #[test]
    fn each_conversation_keeps_its_own_reading_position() {
        let agent_b = plexmaton_core::AgentId::new("agent-b")
            .unwrap_or_else(|error| panic!("invalid fixture: {error}"));
        let mut session = Session::canonical(60, 20);
        session.conversation.extend(10);
        session.conversation.extend_agent(&agent_b, 10);
        session.draw();

        session.wheel(SurfaceId::Transcript, ScrollDirection::Up, 3);
        let reading_a = session.region(SurfaceId::Transcript);

        session.select(&agent_b);
        session.wheel(SurfaceId::Transcript, ScrollDirection::Up, 9);
        let reading_b = session.region(SurfaceId::Transcript);
        assert_ne!(
            markers(&reading_a),
            markers(&reading_b),
            "the two conversations have to be left in different places to tell them apart"
        );

        let primary = session
            .conversation
            .state
            .primary_agent()
            .map(|agent| agent.id.clone())
            .unwrap_or_else(|| panic!("the canonical timeline creates a primary agent"));
        session.select(&primary);
        assert_eq!(
            session.region(SurfaceId::Transcript),
            reading_a,
            "returning to a conversation must restore where its reader was, not where the other \
             conversation was left"
        );

        session.select(&agent_b);
        assert_eq!(
            session.region(SurfaceId::Transcript),
            reading_b,
            "and going back the other way restores the other one"
        );
    }

    /// The filler message numbers visible in a region, in the order they appear.
    fn markers(text: &str) -> Vec<u32> {
        text.match_indices("Filler message ")
            .filter_map(|(at, marker)| {
                text.get(at.saturating_add(marker.len())..)?
                    .split_whitespace()
                    .next()?
                    .parse()
                    .ok()
            })
            .collect()
    }

    /// TR-4 through a real frame: a reader who scrolled back to the end is carried on by the
    /// stream, and one who stopped short is not moved under.
    ///
    /// The scroll away and back is the point. Reading a conversation that was never touched only
    /// exercises "untouched surfaces open at the tail", which is a different rule and would keep
    /// passing if following were dropped entirely.
    #[test]
    fn a_conversation_scrolled_back_to_the_end_keeps_up_and_a_parked_one_stays_put() {
        let mut session = Session::canonical(60, 20);
        session.conversation.extend(8);
        session.draw();

        // Away from the end and back again, which is what leaves a stored position behind.
        session.wheel(SurfaceId::Transcript, ScrollDirection::Up, 1);
        session.wheel(SurfaceId::Transcript, ScrollDirection::Down, 4);

        session.conversation.extend(1);
        let text = session.draw().region(SurfaceId::Transcript);
        assert!(
            text.contains("Filler message 9"),
            "a reader who returned to the end is carried on by the stream:\n{text}"
        );

        session.wheel(SurfaceId::Transcript, ScrollDirection::Up, 1);
        let parked = session.region(SurfaceId::Transcript);
        assert!(
            !parked.contains("Filler message 9"),
            "the reader really did leave the end, or the rest of this proves nothing"
        );

        session.conversation.extend(1);
        assert_eq!(
            session.draw().region(SurfaceId::Transcript),
            parked,
            "content arriving below a parked reader must not move what they are reading"
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
                    render(frame, state, &palette, &mut TranscriptMetrics::default());
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
