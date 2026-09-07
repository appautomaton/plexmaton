use ratatui::{Frame, layout::Rect, text::Line, widgets::Clear};

mod chrome;
mod command_inspection;
mod configuration;
pub(crate) mod effort;
mod message_actions;
mod panel;
pub(crate) mod permission_review;
mod surfaces;

use panel::{Body, Chrome, Edges, Panel, draw_panel};
use surfaces::{
    Stacking, approval_panel, collapsed_composer_panel, composer_menu_panel, composer_panel,
    draw_cursor, drawer_page, drawer_panel, workspace_input,
};

use chrome::{
    agents_title, attention_title, inspector_title, notices_title, render_status, render_too_small,
};

use crate::{
    ViewState, content,
    layout::{self, LayoutClass},
    state::inner_width,
    surface::{KeyboardFocus, SurfaceId, SurfaceTree, Viewport},
    theme::Palette,
    transcript::TranscriptMetrics,
};

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

    let mut surfaces = layout::workspace(area, workspace_input(area, state));
    let stacking = Stacking::of(&surfaces);
    let focused = state.focused(&surfaces);
    // Both are resolved before anything is painted, and both come from the projection: whether the
    // inspector has an input is a fact about state and geometry, not a decision a draw call gets to
    // make. The renderer then has one answer to obey rather than a second to derive.
    let steer = state.steer_input(&surfaces);
    let cursor_owner = (state.keyboard_focus(&surfaces) == KeyboardFocus::TextInput)
        .then_some(focused)
        .flatten();
    // Identities first, so each surface's viewport can be recorded as it is measured; painted
    // bottom layer first, so a surface above another covers it rather than the reverse.
    let mut drawn: Vec<(u32, SurfaceId, Rect)> = surfaces
        .iter()
        .map(|surface| (surface.z_index, surface.id, surface.bounds))
        .collect();
    drawn.sort_unstable_by_key(|(z, id, _)| (*z, *id));

    for (z, id, bounds) in drawn {
        let has_focus = focused == Some(id);
        // Two Drawer pages paint their own scrolling body under a fixed footer; the rest is a
        // panel like any other.
        if id == SurfaceId::Drawer
            && let Some(viewport) = drawer_page(frame, palette, state, bounds, has_focus)
        {
            surfaces.set_viewport(id, viewport);
            surfaces::drawer_retract(frame, palette, state, bounds);
            continue;
        }
        // The inspector's own input takes a strip out of the inspector's rectangle, never out of
        // the conversation's ten-row guarantee (INS-5). What is left is what its
        // conversation is drawn into, so the two are laid out before either is built.
        let bounds = if id == SurfaceId::Inspector {
            state
                .inspector_conversation_bounds(&surfaces)
                .unwrap_or(bounds)
        } else {
            bounds
        };
        // An exhaustive match, so a new surface identity cannot be added without stating how it is
        // drawn and whether it scrolls.
        let panel = match id {
            SurfaceId::Agents => Some(Panel {
                insets: crate::surface::ContentInsets::default(),
                chrome: Chrome::Box,
                footer: None,
                body: Body::Whole {
                    lines: content::agents(state, palette),
                    follows_tail: false,
                },
                title: agents_title(palette),
                badge: None,
                edges: Edges::All,
            }),
            // The primary conversation has no box: its text runs into the composer's top rule,
            // and its last row is the activity line, which also carries the selection note and
            // the attention pill now that there is no border for them (ui-ux §input).
            SurfaceId::Transcript => Some(Panel {
                insets: crate::surface::ContentInsets::default(),
                chrome: Chrome::Bare,
                footer: Some(chrome::activity_line(
                    state,
                    palette,
                    inner_width(bounds.width),
                    state.approval_in_primary() && surfaces.get(SurfaceId::Approval).is_some(),
                )),
                body: conversation_body(
                    state,
                    palette,
                    metrics,
                    bounds,
                    id,
                    stacking.over_composer(SurfaceId::Transcript),
                    1,
                ),
                title: Line::default(),
                badge: None,
                edges: stacking.over_composer(SurfaceId::Transcript),
            }),
            // The inspected agent's conversation uses the same unified entry grammar as the
            // primary: the workspace shows one conversation, and the journey needs it to show two.
            SurfaceId::Inspector => Some(Panel {
                insets: crate::surface::ContentInsets::default(),
                chrome: Chrome::Box,
                footer: None,
                body: conversation_body(
                    state,
                    palette,
                    metrics,
                    bounds,
                    id,
                    stacking.over_composer(SurfaceId::Inspector),
                    0,
                ),
                title: inspector_title(state, palette),
                badge: None,
                edges: stacking.over_composer(SurfaceId::Inspector),
            }),
            SurfaceId::Notices => Some(Panel {
                insets: crate::surface::ContentInsets::default(),
                chrome: Chrome::Box,
                footer: None,
                body: Body::Whole {
                    lines: content::notices(state, palette),
                    follows_tail: true,
                },
                title: notices_title(state, palette),
                badge: None,
                edges: Edges::All,
            }),
            SurfaceId::Attention => Some(Panel {
                insets: crate::surface::ContentInsets::default(),
                chrome: Chrome::Box,
                footer: None,
                body: Body::Whole {
                    lines: content::attention(state, palette, inner_width(bounds.width)),
                    // Oldest first, and the oldest unanswered request is the one that has been
                    // waiting longest: this band opens at its head, not at its tail.
                    follows_tail: false,
                },
                title: attention_title(state, palette),
                badge: None,
                edges: Edges::All,
            }),
            SurfaceId::CommandInspection => Some(command_inspection::panel(state, palette, bounds)),
            SurfaceId::Approval => Some(approval_panel(state, palette, bounds, &stacking)),
            SurfaceId::Drawer => Some(drawer_panel(state, palette, bounds)),
            SurfaceId::ComposerMenu => Some(composer_menu_panel(state, palette, bounds)),
            // While a sub-agent's input holds the cursor the composer is one row — where typing
            // would go and how to get back — not a box (INS-5). The row closes the conversation's
            // box, so the only thing that changes is the divider and the empty line going away.
            SurfaceId::Composer if steer.is_some() => {
                Some(collapsed_composer_panel(state, palette, &stacking))
            }
            SurfaceId::Composer => {
                Some(composer_panel(state, palette, has_focus, bounds, &stacking))
            }
            SurfaceId::Status => {
                render_status(frame, state, palette, bounds);
                None
            }
        };

        let Some(panel) = panel else { continue };
        // A surface above the base layer paints over whatever is beneath it, so the cells it does
        // not write must not show through as fragments of the conversation it covers.
        if z > 0 {
            frame.render_widget(Clear, bounds);
        }
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
        message_actions::render(frame, state, palette, metrics, id, bounds, viewport);
        match id {
            SurfaceId::Composer => effort::composer_rules(frame, state, bounds, panel.edges),
            SurfaceId::Drawer => surfaces::drawer_retract(frame, palette, state, bounds),
            SurfaceId::CommandInspection => command_inspection::controls(frame, palette, bounds),
            _ => {}
        }

        // The cursor belongs to whichever surface the projection says owns it, which is the same
        // answer routing and editing use (SURF-3, COM-1, INS-7) rather than a second one derived
        // here. An inspector's input is the strip below its conversation, so the cursor follows the
        // rectangle the text was drawn into rather than the surface's.
        if cursor_owner == Some(id) {
            draw_cursor(frame, palette, state, id, bounds, &panel, steer.as_ref());
        }
    }

    surfaces
}

/// Builds the part of one surface's conversation this frame will draw.
///
/// The whole history is measured, from the cache; only the items the viewport reaches are turned
/// into lines. A conversation with no items falls back to a whole body, because a placeholder has
/// nothing to virtualize.
///
/// Two surfaces call this — the conversation for the selected agent, an inspector for the one being
/// checked on — and each carries its own agent, its own reader and its own selection through it.
/// One function rather than two, because a second conversation renderer is a second set of TR
/// invariants to keep in step, and the cache is already keyed by agent.
fn conversation_body(
    state: &ViewState,
    palette: &Palette,
    metrics: &mut TranscriptMetrics,
    area: Rect,
    surface: SurfaceId,
    edges: Edges,
    footer_rows: u16,
) -> Body {
    let Some(agent) = state
        .agent_shown_by(surface)
        .and_then(|agent_id| state.agent(&agent_id))
    else {
        return Body::Whole {
            lines: content::conversation_placeholder(palette, surface, false),
            follows_tail: false,
        };
    };
    let visible_rows = area
        .height
        .saturating_sub(edges.rows())
        .saturating_sub(footer_rows);
    // Every height below belongs to this width, and the viewport carries it out of the frame so the
    // scroll path resolves against the same one rather than against whatever was measured last.
    let width = inner_width(area.width);
    if metrics.measure_with(agent, palette, width, state.disclosure()) == 0 {
        return Body::Whole {
            lines: agent.note.as_ref().map_or_else(
                || content::conversation_placeholder(palette, surface, true),
                |anchored| content::note_lines(&anchored.note, palette),
            ),
            follows_tail: false,
        };
    }

    let mut viewport = Viewport {
        content_rows: metrics.total_rows(&agent.id, width),
        content_width: width,
        visible_rows,
        offset: 0,
    };
    // An untouched conversation opens at its newest line; a parked one resolves through the item
    // its reader stopped at, so this width's rows are recomputed rather than remembered (TR-3).
    viewport.offset = state.conversation_position(&agent.id).map_or_else(
        || viewport.max_offset(),
        |position| metrics.offset_of(&agent.id, width, position, viewport.max_offset()),
    );
    let window = metrics.window(&agent.id, width, viewport.offset, visible_rows);
    let (lines, skip_rows) = metrics.build(agent, palette, &window, state, surface);

    Body::Window {
        lines,
        skip_rows,
        viewport,
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{
        AgentId, AgentStatus, ApprovalId, AttentionId, AttentionRequest, ConversationEvent,
        ToolCallId, ToolCallStatus, ToolPresentation, TranscriptItemId,
    };
    use ratatui::{
        Terminal,
        backend::TestBackend,
        buffer::Buffer,
        layout::Rect,
        style::{Color, Modifier, Style},
        text::Line,
        widgets::{Padding, Paragraph, Wrap},
    };

    use super::{
        chrome::{activity_line, block_with, composer_title},
        panel::{Body, Chrome, Edges, Panel, draw_panel},
    };

    use crate::{
        TranscriptMetrics, ViewState,
        intent::{Direction, ScrollDirection, TextIntent},
        layout,
        layout::WorkspaceInput,
        render,
        surface::{SurfaceId, SurfaceTree},
        test_support::{
            Conversation, RenderFixture, canonical_state, current_responding_state,
            current_running_tool_state, degraded_state, draw, draw_frame, draw_with, region_text,
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

    /// SURF-1: a partial frame spends two columns even when it spends only one row.
    #[test]
    fn a_partial_frame_measures_each_axis_from_the_edges_it_paints() {
        let area = Rect::new(0, 0, 8, 2);
        let palette = Palette::default();
        let panel = Panel {
            insets: crate::surface::ContentInsets::default(),
            chrome: Chrome::Box,
            footer: None,
            body: Body::Whole {
                lines: vec![Line::raw("abcdefg")],
                follows_tail: false,
            },
            title: Line::default(),
            badge: None,
            edges: Edges::Closing,
        };
        let mut measured = None;
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));

        terminal
            .draw(|frame| {
                measured = Some(draw_panel(frame, &palette, area, false, &panel, None));
            })
            .unwrap_or_else(|error| panic!("test render: {error}"));

        let viewport = measured.unwrap_or_else(|| panic!("the panel was measured"));
        assert_eq!(
            viewport.content_width, 6,
            "the two side edges spend two cells"
        );
        assert_eq!(
            viewport.content_rows, 2,
            "seven cells wrap at the six-cell painted width"
        );
        assert_eq!(
            viewport.visible_rows, 1,
            "only the bottom edge spends a row"
        );
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

    /// COM-5: the activity line names each current-work state in its role, idle draws nothing,
    /// and the composer's rule never carries any of it (ui-ux §input).
    #[test]
    fn the_activity_line_names_each_current_work_state_and_the_rule_carries_none() {
        let palette = Palette::default();
        let canonical = canonical_state();
        let primary = canonical
            .primary_agent()
            .map(|agent| agent.id.clone())
            .unwrap_or_else(|| panic!("the canonical scenario creates a primary agent"));
        let assert_activity = |state: &ViewState, expected: &str, role: Role| {
            let line = activity_line(state, &palette, 80, false);
            assert!(
                line.to_string().starts_with(expected),
                "{expected:?} leads the activity line: {line}"
            );
            assert_eq!(
                line.spans[1].style,
                palette.style(role),
                "the current-work label must carry {role:?}"
            );
            assert_eq!(
                composer_title(state, &palette).to_string(),
                " Message Agent A · primary ",
                "the rule names the addressee and nothing the agent is doing"
            );
        };

        assert_activity(&canonical, "· Thinking…", Role::Ambient);
        assert_activity(&current_responding_state(), "· Responding…", Role::Ambient);
        assert_activity(
            &current_running_tool_state(),
            "· Running read_file…",
            Role::Ambient,
        );

        let mut approval = Conversation::canonical();
        approval.emit(ConversationEvent::AttentionRequested {
            agent_id: primary.clone(),
            attention_id: AttentionId::new("current-approval")
                .unwrap_or_else(|error| panic!("fixture: {error}")),
            request: AttentionRequest::Approval {
                reason: plexmaton_core::ApprovalReason::PermissionRequired,
                remember: None,
                approval_id: ApprovalId::new("current-approval")
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                call_id: ToolCallId::new("current-approval")
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                tool: "edit_file".to_owned(),
                capabilities: Vec::new(),
                detail: "Change one file".to_owned(),
            },
        });
        let line = activity_line(&approval.state, &palette, 80, false);
        assert!(line.to_string().starts_with("· Approval required"));
        assert_eq!(line.spans[1].style, palette.style(Role::ActionRequired));
        assert!(
            line.to_string().trim_end().ends_with("( !2 )"),
            "the pill counts both unanswered requests at the activity line's right end: {line}"
        );

        let mut idle = Conversation::canonical();
        idle.emit(ConversationEvent::AgentStatusChanged {
            agent_id: primary,
            status: AgentStatus::Idle,
        });
        // Idle draws no label; the pill on the right is the canonical scenario's own request.
        let idle_line = activity_line(&idle.state, &palette, 80, false);
        assert!(
            idle_line.to_string().trim().starts_with('('),
            "idle draws nothing but what waits on the right: {idle_line}"
        );
        let mut named = idle.state.clone();
        named.set_model(crate::test_support::configuration_summary());
        let title = composer_title(&named, &palette);
        assert_eq!(
            title.to_string(),
            format!(
                " Message Agent A · primary · {} ",
                crate::test_support::configuration_summary().reasoning_effort
            )
        );
        assert_eq!(title.spans[2].style, palette.style(Role::Accent));
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
            let mut session = RenderFixture::canonical(width, height);
            // Enough to overflow every size below now that a message costs no heading row.
            session.conversation.extend(10);
            session.draw();

            for notches in 0..4 {
                let viewport = session.viewport(SurfaceId::Transcript);
                assert!(
                    viewport.is_scrollable(),
                    "the fixture has to overflow at {width}x{height} or this proves nothing"
                );
                // The conversation's last row is its activity line, not content (ui-ux §input);
                // the reference paints only the rows the conversation itself occupies.
                let painted = session.region(SurfaceId::Transcript);
                let (conversation, _activity) = painted
                    .rsplit_once('\n')
                    .unwrap_or_else(|| panic!("the region has an activity row under it"));
                assert_eq!(
                    conversation,
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
            .primary_agent()
            .unwrap_or_else(|| panic!("the canonical timeline creates a primary agent"))
            .entries()
            .flat_map(|item| {
                crate::content::transcript_entry(
                    item,
                    palette,
                    crate::state::EntryAppearance::compact(false),
                    crate::state::inner_width(bounds.width),
                )
            })
            .collect();
        // The conversation is bare and open at the bottom, so the reference reserves the same
        // blank top row and side columns a box would have spent, and leaves the activity row out.
        let bounds = Rect {
            height: bounds.height.saturating_sub(1),
            ..bounds
        };
        let paragraph = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(
                block_with(palette, Line::default(), false, Edges::Upper, Chrome::Bare)
                    .padding(Padding::new(1, 1, 1, 0)),
            )
            .scroll((
                u16::try_from(viewport.offset)
                    .unwrap_or_else(|_| panic!("reference fixture offset fits the terminal")),
                0,
            ));

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
        let mut session = RenderFixture::canonical(48, 12);
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
        let mut session = RenderFixture::canonical(60, 20);
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

    /// TR-3 across a resize that comes back: a width the reader left and returned to paints the
    /// frame it had, and a wander across the layout classes accumulates no drift.
    ///
    /// The test above proves the anchor still names the same message, which is the half that
    /// would keep passing if a resize wrote the clamped row back into the stored anchor. That is
    /// the natural optimization, and it is why content creeps in a terminal resized twice: each
    /// conversion loses the depth into the message, and the loss compounds. Comparing the whole
    /// region is what catches it, because the row inside the item is the other half of an anchor.
    #[test]
    fn a_conversation_resized_away_and_back_paints_the_frame_it_had() {
        let mut session = RenderFixture::canonical(60, 20);
        session.conversation.extend(10);
        session.draw();
        session.wheel(SurfaceId::Transcript, ScrollDirection::Up, 6);

        let narrow = session.viewport(SurfaceId::Transcript);
        let parked = session.region(SurfaceId::Transcript);
        assert!(
            !markers(&parked).is_empty(),
            "the reader has to be somewhere in the filler or this proves nothing"
        );

        session.resize(160, 20);
        assert_ne!(
            narrow.content_rows,
            session.viewport(SurfaceId::Transcript).content_rows,
            "the resize has to rewrap the conversation, or nothing was re-measured"
        );

        session.resize(60, 20);
        assert_eq!(
            session.region(SurfaceId::Transcript),
            parked,
            "one width painted two different frames across a single round trip"
        );

        // Every class boundary, because a class change swaps which surfaces exist beside the
        // conversation, and the reader must survive that too.
        for width in [160, 132, 95, 72, 48] {
            session.resize(width, 20);
        }
        session.resize(60, 20);
        assert_eq!(
            session.region(SurfaceId::Transcript),
            parked,
            "a wander across the widths moved the reader that one resize did not"
        );
    }

    /// TR-5: each conversation keeps its own reading position (canonical journey, step 4).
    #[test]
    fn each_conversation_keeps_its_own_reading_position() {
        let agent_b =
            AgentId::new("agent-b").unwrap_or_else(|error| panic!("invalid fixture: {error}"));
        let mut session = RenderFixture::canonical(60, 20);
        session.conversation.extend(10);
        session.conversation.extend_agent(&agent_b, 10);
        session.draw();

        session.wheel(SurfaceId::Transcript, ScrollDirection::Up, 3);
        let reading_a = session.region(SurfaceId::Transcript);

        // B opens in the window — at this size, over the whole region — and is read there.
        session.select(&agent_b);
        session.wheel(SurfaceId::Inspector, ScrollDirection::Up, 9);
        let reading_b = session.region(SurfaceId::Inspector);
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
            "closing the window must find the conversation where its reader was, not where the \
             other conversation was left"
        );
        session.select(&agent_b);
        assert_eq!(
            session.region(SurfaceId::Inspector),
            reading_b,
            "and reopening B finds it where its own reader stopped (TR-5)"
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
        let mut session = RenderFixture::canonical(60, 20);
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

        session.wheel(SurfaceId::Transcript, ScrollDirection::Up, 2);
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

    /// COM-1: a cursor is on screen exactly while a text input holds focus.
    ///
    /// Read from the backend rather than from state, because the question is what the terminal was
    /// told. A focus model the renderer ignores would leave the user typing with no caret.
    ///
    /// Typing now requires focus — a text intent exists only while a cursor does (INV-2) — so the
    /// unfocused case is reached by typing and then walking away, which is also the case that
    /// matters: the draft has to survive, and the caret has to not.
    #[test]
    fn the_cursor_exists_only_while_a_text_input_holds_focus() {
        let palette = Palette::default();
        let mut state = canonical_state();
        let surfaces = layout::workspace(Rect::new(0, 0, 120, 24), WorkspaceInput::default());
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

        state.focus_surface(&surfaces, SurfaceId::Composer);
        state.edit(&surfaces, TextIntent::Insert('h'));
        let bounds = surfaces
            .get(SurfaceId::Composer)
            .unwrap_or_else(|| panic!("the composer is always registered"))
            .bounds;
        let at = cursor(&state).unwrap_or_else(|| panic!("a focused composer owns the cursor"));
        assert!(
            bounds.contains(at),
            "the cursor landed at {at:?}, outside the composer at {bounds:?}"
        );

        state.focus_surface(&surfaces, SurfaceId::Transcript);
        assert_eq!(
            cursor(&state),
            None,
            "a draft nobody is focused on still shows no cursor"
        );
        assert_eq!(
            state.composer().text(),
            "h",
            "and walking away must not discard what was typed"
        );
    }

    /// COM-4: the composer's target is on screen and does not follow the selection.
    #[test]
    fn the_composer_names_its_target_while_another_agent_is_selected() {
        let mut state = canonical_state();
        let agent_b =
            AgentId::new("agent-b").unwrap_or_else(|error| panic!("invalid fixture: {error}"));
        state
            .select_agent(&agent_b)
            .unwrap_or_else(|error| panic!("agent-b exists: {error}"));

        let (surfaces, buffer) = draw_frame(&state, &Palette::default(), 120, 40);
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
            region(SurfaceId::Inspector).contains("Agent B"),
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
        for palette in [
            Palette::ansi(),
            Palette::pastel(),
            Palette::truecolor(),
            Palette::monochrome(),
        ] {
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
            // The composer's rules are its border; the conversation, bare, shows focus only by
            // what it takes away from the composer (ui-ux §input).
            assert_eq!(
                border_ink(&buffer, &surfaces, SurfaceId::Composer),
                role_ink(&palette, Role::Border)
            );

            // One step: the conversation follows the one agent-column surface.
            state.cycle_focus(&surfaces, Direction::Forward);
            let (surfaces, buffer) = draw_frame(&state, &palette, 120, 24);
            assert_eq!(state.focused(&surfaces), Some(SurfaceId::Transcript));
            assert_eq!(
                border_ink(&buffer, &surfaces, SurfaceId::Agents),
                role_ink(&palette, Role::Border),
                "the focused border moved with the ring rather than being painted twice"
            );
            assert_eq!(
                border_ink(&buffer, &surfaces, SurfaceId::Composer),
                role_ink(&palette, Role::Border)
            );

            // Another: the composer, whose rules light up.
            state.cycle_focus(&surfaces, Direction::Forward);
            let (surfaces, buffer) = draw_frame(&state, &palette, 120, 24);
            assert_eq!(state.focused(&surfaces), Some(SurfaceId::Composer));
            assert_eq!(
                border_ink(&buffer, &surfaces, SurfaceId::Composer),
                role_ink(&palette, Role::BorderFocused)
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

        assert_eq!(
            surfaces.len(),
            6,
            "a degraded workspace registers its six visible surfaces"
        );
        for surface in surfaces.iter() {
            // An exhaustive match, so a new surface identity cannot be added without stating what
            // proves it was drawn.
            let signature = match surface.id {
                SurfaceId::Agents => "Agents",
                // The conversation has no title; its activity line is what it always paints.
                SurfaceId::Transcript => "Thinking…",
                SurfaceId::Composer => "Message Agent A",
                SurfaceId::Notices => "[drop]",
                SurfaceId::Attention => "Attention",
                SurfaceId::Approval => "Approval required",
                SurfaceId::CommandInspection => "Command",
                SurfaceId::Inspector => "Agent B",
                SurfaceId::Status => "~/plexmaton",
                SurfaceId::Drawer => "Workspace",
                SurfaceId::ComposerMenu => "Skills",
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
        let (_surfaces, buffer) = draw_frame(&canonical_state(), &Palette::default(), 120, 24);
        let rendered = region_text(&buffer, *buffer.area());

        assert!(rendered.contains("Agent A · primary"));
        assert!(
            rendered.contains("( !1 )"),
            "the pill carries what is unanswered"
        );
        assert!(
            !rendered.contains("Agents · !1"),
            "the rail names the rail and nothing else"
        );
        assert!(rendered.contains("remains interactive"));
        assert!(
            !rendered.contains("Routing stays"),
            "B's outgoing mail must not be projected as mail owned by A"
        );
    }

    /// ENT-1: warning and error remain visibly distinct without relying on colour.
    #[test]
    fn monochrome_transcript_names_warning_and_error_separately() {
        let mut conversation = Conversation::canonical();
        let agent_id = AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}"));
        conversation.emit(ConversationEvent::RuntimeWarning {
            agent_id: agent_id.clone(),
            item_id: TranscriptItemId::new("warning-visible")
                .unwrap_or_else(|error| panic!("fixture: {error}")),
            message: "retrying the request".to_owned(),
        });
        conversation.emit(ConversationEvent::RuntimeError {
            agent_id,
            item_id: TranscriptItemId::new("error-visible")
                .unwrap_or_else(|error| panic!("fixture: {error}")),
            message: "request failed".to_owned(),
        });

        let rendered = draw_with(&conversation.state, &Palette::monochrome(), 120, 40);
        assert!(rendered.contains("warning"));
        assert!(rendered.contains("error"));
        assert!(rendered.contains("retrying the request"));
        assert!(rendered.contains("request failed"));
    }

    #[test]
    fn inspected_transcript_renders_tools_artifacts_and_mail_in_one_conversation() {
        let mut state = canonical_state();
        let agent_b =
            AgentId::new("agent-b").unwrap_or_else(|error| panic!("invalid fixture: {error}"));
        state
            .select_agent(&agent_b)
            .unwrap_or_else(|error| panic!("agent-b exists: {error}"));

        let rendered = draw(&state, 120, 24);

        assert!(rendered.contains("[+]"), "succeeded tool marker");
        assert!(rendered.contains("interaction findings"), "artifact label");
        assert!(rendered.contains("artifact://"), "artifact pointer");
        assert!(rendered.contains("Routing stays"), "outgoing mail summary");
    }

    /// ENT-1/ENT-2: render order is first appearance, never sibling completion order.
    #[test]
    fn interleaved_text_and_tools_keep_their_positions_when_tools_finish_out_of_order() {
        let mut conversation = Conversation::canonical();
        let agent_id = AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}"));
        let tool = |entry: &str, call: &str, label: &str, revision, status| {
            ConversationEvent::ToolCallChanged {
                agent_id: agent_id.clone(),
                item_id: TranscriptItemId::new(entry)
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                item_revision: revision,
                call_id: ToolCallId::new(call).unwrap_or_else(|error| panic!("fixture: {error}")),
                label: label.to_owned(),
                status,
                presentation: ToolPresentation::default(),
            }
        };
        conversation.emit(tool(
            "entry-a",
            "call-a",
            "first tool",
            0,
            ToolCallStatus::Queued,
        ));
        conversation.extend(1);
        conversation.emit(tool(
            "entry-b",
            "call-b",
            "second tool",
            0,
            ToolCallStatus::Queued,
        ));
        for event in [
            tool(
                "entry-a",
                "call-a",
                "first tool",
                1,
                ToolCallStatus::Running,
            ),
            tool(
                "entry-b",
                "call-b",
                "second tool",
                1,
                ToolCallStatus::Running,
            ),
            tool(
                "entry-b",
                "call-b",
                "second tool",
                2,
                ToolCallStatus::Succeeded,
            ),
            tool(
                "entry-a",
                "call-a",
                "first tool",
                2,
                ToolCallStatus::Succeeded,
            ),
        ] {
            conversation.emit(event);
        }

        let rendered = draw_with(&conversation.state, &Palette::monochrome(), 120, 40);
        let first = rendered
            .find("first tool")
            .unwrap_or_else(|| panic!("first tool is visible:\n{rendered}"));
        let text = rendered
            .find("Filler message 1")
            .unwrap_or_else(|| panic!("interleaved message is visible:\n{rendered}"));
        let second = rendered
            .find("second tool")
            .unwrap_or_else(|| panic!("second tool is visible:\n{rendered}"));
        assert!(
            first < text && text < second,
            "first appearance was reordered:\n{rendered}"
        );
        assert_eq!(rendered.matches("· succeeded").count(), 2);
    }

    #[test]
    fn narrow_projection_keeps_every_region() {
        let rendered = draw(&canonical_state(), 60, 30);

        assert!(rendered.contains("Agents"));
        assert!(
            !rendered.contains("Activity"),
            "domain entries have no second panel"
        );
        assert!(rendered.contains("1 tool"));
        assert!(rendered.contains("@1"), "compact artifact count");
        assert!(rendered.contains("1 mail"));
        assert!(rendered.contains("Message Agent A"));
        assert!(
            rendered.contains("~/plexmaton"),
            "the status line is the last row"
        );
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
        let pastel = draw_with(&state, &Palette::pastel(), 120, 24);
        let truecolor = draw_with(&state, &Palette::truecolor(), 120, 24);
        let monochrome = draw_with(&state, &Palette::monochrome(), 120, 24);

        assert_eq!(ansi, pastel);
        assert_eq!(ansi, truecolor);
        assert_eq!(ansi, monochrome);
    }

    /// The pill is the number that is unanswered, coloured, on the conversation being read.
    ///
    /// It rides the border the panel already draws, so it takes no row from the conversation and
    /// no rectangle from the surface tree. The rail is left naming the rail: the same fact painted
    /// in two places is the one that drifts.
    #[test]
    fn the_pill_carries_what_is_unanswered_and_costs_the_conversation_no_row() {
        for palette in [
            Palette::ansi(),
            Palette::pastel(),
            Palette::truecolor(),
            Palette::monochrome(),
        ] {
            let (surfaces, buffer) = draw_frame(&canonical_state(), &palette, 120, 24);
            let conversation = surfaces
                .get(SurfaceId::Transcript)
                .expect("the conversation is registered at wide")
                .bounds;
            // The pill rides the activity line, the conversation's last row (ui-ux §input).
            let border = Rect {
                y: conversation.bottom().saturating_sub(1),
                height: 1,
                ..conversation
            };
            let top = region_text(&buffer, border);
            assert!(top.contains("( !1 )"), "{top:?}");
            assert!(
                top.trim_end().ends_with("( !1 )"),
                "the pill sits at the far end of the row, not beside the label: {top:?}"
            );

            let rail = region_text(
                &buffer,
                Rect {
                    height: 1,
                    ..surfaces
                        .get(SurfaceId::Agents)
                        .expect("the rail is registered at wide")
                        .bounds
                },
            );
            assert!(
                !rail.contains('!'),
                "the count belongs to one place: {rail:?}"
            );

            let column = |needle: char| {
                let at = top
                    .chars()
                    .position(|c| c == needle)
                    .unwrap_or_else(|| panic!("{needle:?} is painted: {top:?}"));
                border.x + u16::try_from(at).unwrap_or_else(|_| panic!("a column fits a u16"))
            };
            assert_eq!(
                ink(buffer[(column('!'), border.y)].style()),
                role_ink(&palette, Role::ActionRequired),
                "the pill carries the action-required role"
            );
        }
    }
}
