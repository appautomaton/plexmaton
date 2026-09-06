use super::*;
use crate::{
    Point, SurfaceId,
    math::{MathPresentation, NativeStage, NativeText},
};
use plexmaton_core::{
    AgentStatus, ConversationEvent, EventSequence, TranscriptItemId, TranscriptRole,
};
use ratatui::{
    backend::TestBackend,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    style::Modifier,
};
use unicode_width::UnicodeWidthStr as _;

fn events(source: &str) -> Vec<ConversationEventEnvelope> {
    let agent = AgentId::new("primary").expect("agent");
    let item = TranscriptItemId::new("formula").expect("item");
    [
        ConversationEvent::AgentCreated {
            agent_id: agent.clone(),
            label: "Plexmaton".into(),
            status: AgentStatus::Idle,
        },
        ConversationEvent::TranscriptItemStarted {
            agent_id: agent.clone(),
            item_id: item.clone(),
            role: TranscriptRole::Assistant,
        },
        ConversationEvent::TranscriptDelta {
            agent_id: agent,
            item_id: item,
            item_revision: 1,
            text: source.into(),
        },
    ]
    .into_iter()
    .enumerate()
    .map(|(index, event)| ConversationEventEnvelope {
        sequence: EventSequence::new(index as u64 + 1),
        event,
    })
    .collect()
}

fn fixture(width: u16, source: &str, math: MathPresentation) -> (Workspace, Terminal<TestBackend>) {
    let mut workspace = Workspace::with_presentation(Palette::ansi(), math);
    workspace.emit(events(source));
    let mut terminal = Terminal::new(TestBackend::new(width, 24)).expect("terminal");
    paint(&mut workspace, &mut terminal);
    (workspace, terminal)
}

fn paint(workspace: &mut Workspace, terminal: &mut Terminal<TestBackend>) -> Vec<NativeText> {
    let mut painted = Vec::new();
    for _ in 0..16 {
        workspace
            .draw_with_native(terminal, |_, stage| {
                if let NativeStage::End { changed: text, .. } = stage {
                    painted.extend(text.iter().map(|text| (*text).clone()));
                }
                Ok(())
            })
            .expect("cell/native projection");
        let Some(work) = workspace.take_preparation() else {
            return painted;
        };
        let prepared =
            crate::preparation::prepare_batch(&work.requests).expect("bounded worker fixture");
        assert!(workspace.complete_preparation(work.token, prepared));
    }
    panic!("preparation did not settle");
}

/// MD-4/MTH-5/FR-3: a pending delta keeps the previous native scene and its atomic copy map.
/// Reusing text rows without pinning native reservations would erase and resend every formula.
#[test]
fn streaming_preparation_preserves_native_runs_without_rewriting_them() {
    for width in [120, 88, 60] {
        let formula = r"\[\frac{ab}{c}\]";
        let (mut workspace, mut terminal) = fixture(
            width,
            &format!("Before {formula} after."),
            MathPresentation::Native,
        );
        for (index, text) in [" More prose.", "\nA new line.", " More text again."]
            .into_iter()
            .enumerate()
        {
            let previous = workspace.native.text().to_vec();
            assert!(!previous.is_empty());
            workspace.emit(vec![ConversationEventEnvelope {
                sequence: EventSequence::new(index as u64 + 4),
                event: ConversationEvent::TranscriptDelta {
                    agent_id: AgentId::new("primary").expect("agent"),
                    item_id: TranscriptItemId::new("formula").expect("item"),
                    item_revision: index as u64 + 2,
                    text: text.into(),
                },
            }]);
            workspace
                .draw_with_native(&mut terminal, |_, stage| {
                    if let NativeStage::End { current, changed } = stage {
                        assert_eq!(current, previous.as_slice());
                        assert!(
                            changed.is_empty(),
                            "a pending delta must not rewrite native runs"
                        );
                    }
                    Ok(())
                })
                .expect("retained native frame");
            let at = atoms(&mut workspace)[0];
            workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), at));
            let copy = workspace
                .handle(&mouse(MouseEventKind::Up(MouseButton::Left), at))
                .copied
                .expect("retained atomic source");
            assert_eq!(copy.text, formula);
            paint(&mut workspace, &mut terminal);
        }
    }
}

fn mouse(kind: MouseEventKind, at: Point) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column: at.x,
        row: at.y,
        modifiers: KeyModifiers::NONE,
    })
}

fn atoms(workspace: &mut Workspace) -> Vec<Point> {
    let bounds = workspace
        .conversation_bounds(SurfaceId::Transcript)
        .expect("conversation");
    let viewport = workspace
        .surfaces
        .viewport(SurfaceId::Transcript)
        .expect("viewport");
    let mut cells = Vec::new();
    for y in bounds.y + 1..bounds.y + 1 + viewport.visible_rows {
        for x in bounds.x + 1..bounds.x + 1 + viewport.content_width {
            let point = Point { x, y };
            if workspace
                .text_point_at(SurfaceId::Transcript, point, false)
                .is_some_and(|(_, point, _)| point.is_atomic())
            {
                cells.push(point);
            }
        }
    }
    cells
}

fn point(terminal: &Terminal<TestBackend>, needle: &str) -> Point {
    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        let text = crate::test_support::snapshot_text(
            buffer,
            ratatui::layout::Rect::new(0, y, buffer.area.width, 1),
        );
        if let Some(start) = text.find(needle) {
            return Point {
                x: u16::try_from(text[..start].width()).expect("column"),
                y,
            };
        }
    }
    panic!(
        "missing {needle:?}: {}",
        crate::test_support::snapshot_text(buffer, buffer.area)
    );
}

fn click(workspace: &mut Workspace, terminal: &mut Terminal<TestBackend>, at: Point) -> String {
    assert!(
        workspace
            .handle(&mouse(MouseEventKind::Down(MouseButton::Left), at))
            .copied
            .is_none()
    );
    paint(workspace, terminal);
    let copy = workspace
        .handle(&mouse(MouseEventKind::Up(MouseButton::Left), at))
        .copied
        .expect("whole formula click");
    paint(workspace, terminal);
    copy.text
}

/// MTH-1/SEL-1/SEL-2: native glyph, blank and edge clicks highlight and copy original paired delimiters.
#[test]
fn formula_clicks_and_reverse_edge_drags_select_highlight_and_copy_the_complete_source() {
    for width in [120, 88, 60] {
        for (open, close) in [("$", "$"), ("$$", "$$"), (r"\(", r"\)"), (r"\[", r"\]")] {
            let formula = format!("{open}\\frac{{ab}}{{c}}{close}");
            let (mut workspace, mut terminal) = fixture(
                width,
                &format!("Before {formula} after."),
                MathPresentation::Native,
            );
            let cells = atoms(&mut workspace);
            assert!(cells.len() > 3, "blank rectangle cells must be exercised");
            for at in cells.iter().copied() {
                assert_eq!(click(&mut workspace, &mut terminal, at), formula);
                for cell in &cells {
                    assert!(
                        terminal.backend().buffer()[(cell.x, cell.y)]
                            .modifier
                            .contains(Modifier::REVERSED)
                    );
                }
                assert_eq!(
                    workspace.copy_selection().expect("retained selection").text,
                    formula
                );
            }
            let start = point(&terminal, "Before");
            for (start, end) in [(start, cells[0]), (cells[cells.len() - 1], start)] {
                workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), start));
                workspace.handle(&mouse(MouseEventKind::Drag(MouseButton::Left), end));
                paint(&mut workspace, &mut terminal);
                let copied = workspace
                    .handle(&mouse(MouseEventKind::Up(MouseButton::Left), end))
                    .copied
                    .expect("mixed range");
                let separator = if open.len() == 2 && open != r"\(" {
                    "Before \n"
                } else {
                    "Before "
                };
                assert_eq!(copied.text, format!("{separator}{formula}"));
                for cell in &cells {
                    assert!(
                        terminal.backend().buffer()[(cell.x, cell.y)]
                            .modifier
                            .contains(Modifier::REVERSED)
                    );
                }
            }
        }
    }
}

/// MTH-1/PRE-3: source-only capability uses the same atomic source; reflow cannot change its copy.
#[test]
fn formula_source_fallback_and_reflow_preserve_atomic_selection_without_repreparing_for_paint() {
    for math in [MathPresentation::Native, MathPresentation::default()] {
        let original = "\\[\n \\frac{a}{b} \r\n\\]";
        let (mut workspace, mut terminal) = fixture(120, original, math);
        let at = atoms(&mut workspace)[0];
        assert_eq!(click(&mut workspace, &mut terminal, at), original);
        for width in [88, 60, 120] {
            terminal.backend_mut().resize(width, 24);
            workspace.handle(&Event::Resize(width, 24));
            paint(&mut workspace, &mut terminal);
            assert_eq!(
                workspace.copy_selection().expect("after reflow").text,
                original
            );
            for cell in atoms(&mut workspace) {
                assert!(
                    terminal.backend().buffer()[(cell.x, cell.y)]
                        .modifier
                        .contains(Modifier::REVERSED)
                );
            }
        }
        let prepared = workspace.metrics.text_layouts();
        workspace.handle(&Event::Key(KeyEvent::new(
            KeyCode::Char('z'),
            KeyModifiers::NONE,
        )));
        assert!(
            paint(&mut workspace, &mut terminal).is_empty(),
            "unchanged native content is already on screen"
        );
        assert_eq!(workspace.metrics.text_layouts(), prepared);
    }
}

/// PRE-3/MTH-4: same-ID projection replacement revokes old work and retains the terminal capability.
#[test]
fn replacing_projection_revokes_math_work_even_when_semantic_keys_are_identical() {
    let mut workspace = Workspace::with_presentation(Palette::ansi(), MathPresentation::Native);
    workspace.emit(events(r"\(x_i\)"));
    let mut terminal = Terminal::new(TestBackend::new(88, 24)).expect("terminal");
    workspace.draw(&mut terminal).expect("pending");
    let old = workspace.take_preparation().expect("old request");
    workspace.replace_projection(events(r"\(x_i\)"));
    workspace.draw(&mut terminal).expect("new pending");
    let new = workspace.take_preparation().expect("new request");
    assert_ne!(old.token, new.token);
    assert!(!workspace.complete_preparation(
        old.token,
        crate::preparation::prepare_batch(&old.requests).expect("old result")
    ));
    assert!(workspace.complete_preparation(
        new.token,
        crate::preparation::prepare_batch(&new.requests).expect("new result")
    ));
    assert!(
        !paint(&mut workspace, &mut terminal).is_empty(),
        "replacement retained native capability"
    );
}

/// MTH-1/SEL-1/PRE-3: completing or reinterpreting an atom revokes the old range, never copies a prefix.
#[test]
fn streamed_formula_completion_and_markdown_reinterpretation_cannot_leave_partial_tex_selected() {
    for (source, appended, survives) in [
        (r"before \(x_i\)", " after", true),
        (r"before \(x_i", r"\)", false),
        (r"before ` \(x_i\)", "`", false),
    ] {
        let (mut workspace, mut terminal) = fixture(88, source, MathPresentation::Native);
        let at = atoms(&mut workspace)[0];
        let before = click(&mut workspace, &mut terminal, at);
        workspace.emit(vec![ConversationEventEnvelope {
            sequence: EventSequence::new(4),
            event: ConversationEvent::TranscriptDelta {
                agent_id: AgentId::new("primary").expect("agent"),
                item_id: TranscriptItemId::new("formula").expect("item"),
                item_revision: 2,
                text: appended.into(),
            },
        }]);
        paint(&mut workspace, &mut terminal);
        if survives {
            assert_eq!(
                workspace
                    .copy_selection()
                    .expect("unmodified complete formula")
                    .text,
                before
            );
        } else {
            assert!(
                workspace.state.selection().is_none(),
                "{source} + {appended}"
            );
            assert!(workspace.copy_selection().is_none());
            assert!(workspace.take_copy().is_none());
        }
    }
}

/// MTH-3/FR-3: every scroll slice and overlay obeys the actual viewport; partial multicells are marked.
#[test]
fn native_runs_keep_their_origin_and_never_cross_viewport_or_overlay_edges() {
    let formula = r"\[\sum_{i=1}^{n} x_i\]";
    let source = format!(
        "{}\n{formula}\n{}",
        "before\n".repeat(20),
        "after\n".repeat(20)
    );
    for width in [120, 88, 60] {
        let (mut workspace, mut terminal) = fixture(width, &source, MathPresentation::Native);
        let mut clipped = false;
        let mut complete = false;
        loop {
            let bounds = workspace
                .conversation_bounds(SurfaceId::Transcript)
                .expect("bounds");
            let viewport = workspace
                .surfaces
                .viewport(SurfaceId::Transcript)
                .expect("viewport");
            for native in workspace
                .native
                .changed(&crate::math::NativeFrame::default())
            {
                let glyph = &native.glyph;
                assert!(glyph.x > bounds.x && glyph.x + glyph.columns < bounds.right());
                assert!(
                    glyph.y > bounds.y
                        && glyph.y + glyph.rows <= bounds.y + 1 + viewport.visible_rows
                );
                complete |= glyph.scale == crate::math::TextScale::Large;
            }
            let text = crate::test_support::snapshot_text(
                terminal.backend().buffer(),
                terminal.backend().buffer().area,
            );
            if text.contains("Math clipped") {
                clipped = true;
                assert!(text.contains('⋮'), "an explicit visible clipping mark");
                let at = atoms(&mut workspace)[0];
                assert_eq!(
                    click(&mut workspace, &mut terminal, at),
                    formula,
                    "clipping cannot clip source ownership"
                );
            }
            if !workspace.state.scroll_conversation_by(
                &workspace.surfaces,
                &workspace.metrics,
                SurfaceId::Transcript,
                crate::intent::ScrollDirection::Up,
                1,
            ) {
                break;
            }
            paint(&mut workspace, &mut terminal);
        }
        assert!(
            clipped && complete,
            "both complete and bisected large operators were exercised at {width}"
        );
        // Move to the formula, then cover it with the real Drawer.
        workspace.state.scroll_conversation_by(
            &workspace.surfaces,
            &workspace.metrics,
            SurfaceId::Transcript,
            crate::intent::ScrollDirection::Down,
            18,
        );
        paint(&mut workspace, &mut terminal);
        workspace.handle(&Event::Key(KeyEvent::new(
            KeyCode::Char('p'),
            KeyModifiers::CONTROL,
        )));
        paint(&mut workspace, &mut terminal);
        let overlay = workspace
            .surfaces
            .get(SurfaceId::Drawer)
            .expect("overlay")
            .bounds;
        for native in workspace
            .native
            .changed(&crate::math::NativeFrame::default())
        {
            let glyph = &native.glyph;
            let rect = ratatui::layout::Rect::new(glyph.x, glyph.y, glyph.columns, glyph.rows);
            assert!(rect.intersection(overlay).is_empty());
        }
    }
}

/// PRE-3/MTH-5: a failed native write cannot publish the source map or commit a successful frame.
#[test]
fn failed_native_output_keeps_the_last_painted_hit_map_and_frame_identity() {
    use ratatui::{TerminalOptions, Viewport, backend::CrosstermBackend, layout::Rect};

    for width in [120, 88, 60] {
        let mut workspace = Workspace::with_presentation(Palette::ansi(), MathPresentation::Native);
        workspace.emit(events(r"\(x_{ij}^2\)"));
        let mut terminal = Terminal::with_options(
            CrosstermBackend::new(Vec::<u8>::new()),
            TerminalOptions {
                viewport: Viewport::Fixed(Rect::new(0, 0, width, 24)),
            },
        )
        .expect("fixed terminal with no host I/O");
        workspace.draw(&mut terminal).expect("pending frame");
        let frames = workspace.frames();
        let work = workspace.take_preparation().expect("math request");
        assert!(workspace.complete_preparation(
            work.token,
            crate::preparation::prepare_batch(&work.requests).expect("prepared math")
        ));
        let painted = workspace.painted;
        let result = workspace.draw_with_native(&mut terminal, |_, stage| match stage {
            NativeStage::Begin => Ok(()),
            NativeStage::End { changed, .. } => {
                assert!(!changed.is_empty(), "exercise actual native output");
                Err(std::io::Error::other("injected terminal write failure"))
            }
        });
        assert!(result.is_err());
        assert_eq!(workspace.frames(), frames);
        assert_eq!(workspace.painted, painted);
        assert!(workspace.needs_draw());
        assert!(workspace.native.text().is_empty());
        assert!(
            atoms(&mut workspace).is_empty(),
            "unpainted math is not selectable"
        );
    }
}
