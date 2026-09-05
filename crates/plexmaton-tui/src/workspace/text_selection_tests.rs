use super::*;
use crate::Point;
use plexmaton_core::{AgentStatus, EventSequence, SessionEvent, TranscriptItemId, TranscriptRole};
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
};
use unicode_width::UnicodeWidthStr;

fn fixture(
    width: u16,
    messages: &[(TranscriptRole, &str)],
) -> (Workspace, Terminal<TestBackend>, u64) {
    let mut workspace = Workspace::with_palette(Palette::pastel());
    let agent = AgentId::new("primary").expect("agent");
    let mut events = vec![SessionEvent::AgentCreated {
        agent_id: agent.clone(),
        label: "Plexmaton".into(),
        status: AgentStatus::Idle,
    }];
    for (index, (role, text)) in messages.iter().enumerate() {
        let item = TranscriptItemId::new(format!("item-{index}")).expect("item");
        events.push(SessionEvent::TranscriptItemStarted {
            agent_id: agent.clone(),
            item_id: item.clone(),
            role: *role,
        });
        events.push(SessionEvent::TranscriptDelta {
            agent_id: agent.clone(),
            item_id: item,
            item_revision: 1,
            text: (*text).into(),
        });
    }
    let sequence = events.len() as u64 + 1;
    workspace.emit(
        events
            .into_iter()
            .enumerate()
            .map(|(i, event)| SessionEventEnvelope {
                sequence: EventSequence::new(i as u64 + 1),
                event,
            })
            .collect(),
    );
    let mut terminal = Terminal::new(TestBackend::new(width, 24)).expect("terminal");
    workspace.draw(&mut terminal).expect("draw");
    (workspace, terminal, sequence)
}

fn point(terminal: &Terminal<TestBackend>, needle: &str) -> Point {
    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        let mut text = String::new();
        let mut x = 0;
        while x < buffer.area.width {
            let cell = buffer[(x, y)].symbol();
            text.push_str(cell);
            x += cell.width().max(1) as u16;
        }
        if let Some(byte) = text.find(needle) {
            return Point {
                x: text[..byte].width() as u16,
                y,
            };
        }
    }
    panic!("{needle:?} not visible");
}

fn mouse(kind: MouseEventKind, at: Point) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column: at.x,
        row: at.y,
        modifiers: KeyModifiers::NONE,
    })
}

fn drag(
    workspace: &mut Workspace,
    terminal: &mut Terminal<TestBackend>,
    start: Point,
    end: Point,
) -> String {
    workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), start));
    assert!(workspace.state().selection().is_none());
    workspace.handle(&mouse(MouseEventKind::Drag(MouseButton::Left), end));
    workspace.draw(terminal).expect("highlight");
    workspace
        .handle(&mouse(MouseEventKind::Up(MouseButton::Left), end))
        .copied
        .expect("auto copy on release")
        .text
}

/// SEL-1/SEL-2/MD-1: pointer copy is plain text; the independent action still returns raw Markdown.
#[test]
fn mouse_selects_only_visible_graphemes_and_copy_icon_keeps_markdown() {
    let source = "Before **bold 中🙂e\u{301}** after.";
    for width in [60, 95, 120] {
        let (mut workspace, mut terminal, _) =
            fixture(width, &[(TranscriptRole::Assistant, source)]);
        let start = point(&terminal, "bold");
        let end = Point {
            x: start.x + "bold 中🙂e\u{301}".width() as u16,
            ..start
        };
        let layouts = workspace.metrics().text_layouts();
        assert_eq!(
            drag(&mut workspace, &mut terminal, start, end),
            "bold 中🙂e\u{301}"
        );
        assert_eq!(
            workspace.metrics().text_layouts(),
            layouts,
            "drag reuses measured layout"
        );
        assert!(
            terminal.backend().buffer()[(start.x, start.y)]
                .modifier
                .contains(ratatui::style::Modifier::REVERSED)
        );
        let before = point(&terminal, "Before");
        assert!(
            !terminal.backend().buffer()[(before.x, before.y)]
                .modifier
                .contains(ratatui::style::Modifier::REVERSED)
        );
        let copied = workspace
            .handle(&Event::Key(KeyEvent::new(
                KeyCode::Char('y'),
                KeyModifiers::CONTROL,
            )))
            .copied
            .expect("copy key");
        assert_eq!(copied.text, "bold 中🙂e\u{301}");
        workspace.handle(&mouse(MouseEventKind::Moved, start));
        workspace.draw(&mut terminal).expect("hover action");
        let icon = point(&terminal, "󰆏");
        workspace.handle(&mouse(MouseEventKind::Moved, icon));
        workspace.draw(&mut terminal).expect("hover icon");
        workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), icon));
        assert_eq!(
            workspace
                .handle(&mouse(MouseEventKind::Up(MouseButton::Left), icon))
                .copied
                .expect("raw copy")
                .text,
            source
        );
        assert_eq!(
            workspace
                .state()
                .copy()
                .expect("selection survives icon")
                .text,
            "bold 中🙂e\u{301}"
        );
    }
}

/// SEL-1/SEL-3: reverse and forward drags share exact partial endpoints across roles and entries.
#[test]
fn text_drag_crosses_entries_without_selecting_their_uncovered_text() {
    for width in [60, 95, 120] {
        let (mut workspace, mut terminal, _) = fixture(
            width,
            &[
                (TranscriptRole::Assistant, "**alpha** bravo\ncharlie delta"),
                (TranscriptRole::User, "echo foxtrot"),
            ],
        );
        let start = point(&terminal, "pha bravo");
        let last = point(&terminal, "foxtrot");
        let end = Point {
            x: last.x + 3,
            ..last
        };
        for (a, b) in [(start, end), (end, start)] {
            assert_eq!(
                drag(&mut workspace, &mut terminal, a, b),
                "pha bravo\ncharlie delta\n\necho fox"
            );
        }
        let selected = workspace.state().selection().cloned();
        workspace.handle(&Event::FocusLost);
        assert_eq!(workspace.state().selection(), selected.as_ref());
        workspace.handle(&Event::FocusGained);
        for next_width in [120, 60, width] {
            terminal.backend_mut().resize(next_width, 24);
            workspace.handle(&Event::Resize(next_width, 24));
            workspace.draw(&mut terminal).expect("resize selection");
            assert_eq!(
                workspace.state().copy().expect("stable offsets").text,
                "pha bravo\ncharlie delta\n\necho fox"
            );
        }
        workspace.handle(&Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        assert!(workspace.state().copy().is_none());
    }
}

/// SEL-1/MD-2: soft wrapping does not add copy newlines; code keeps indentation without fences/chrome.
#[test]
fn text_drag_copies_wrapped_code_without_its_frame() {
    let source = "```rust\n    let greeting = \"hello world from a deliberately long line\";\n    println!(\"中文🙂\");\n```";
    let (mut workspace, mut terminal, _) = fixture(60, &[(TranscriptRole::Assistant, source)]);
    let start = point(&terminal, "    let greeting");
    let last = point(&terminal, "    println!");
    let end = Point {
        x: last.x + "    println!(\"中文🙂\");".width() as u16,
        ..last
    };
    assert_eq!(
        drag(&mut workspace, &mut terminal, start, end),
        "    let greeting = \"hello world from a deliberately long line\";\n    println!(\"中文🙂\");"
    );
    let selected = workspace.state().copy().expect("copy").text;
    terminal.backend_mut().resize(120, 24);
    workspace.handle(&Event::Resize(120, 24));
    workspace.draw(&mut terminal).expect("unwrapped code");
    assert_eq!(workspace.state().copy().expect("same text").text, selected);
}

/// SEL-1/MD-3: append preserves endpoints; changed prefix cannot silently retarget a selection.
#[test]
fn streamed_text_preserves_or_invalidates_selection_by_its_exact_prefix() {
    for (source, suffix, retained) in [("**hello**", " world", true), ("**hello", "**", false)] {
        let (mut workspace, mut terminal, sequence) =
            fixture(60, &[(TranscriptRole::Assistant, source)]);
        let start = point(&terminal, "hello");
        let end = Point {
            x: start.x + 5,
            ..start
        };
        assert_eq!(drag(&mut workspace, &mut terminal, start, end), "hello");
        workspace.emit(vec![SessionEventEnvelope {
            sequence: EventSequence::new(sequence),
            event: SessionEvent::TranscriptDelta {
                agent_id: AgentId::new("primary").expect("agent"),
                item_id: TranscriptItemId::new("item-0").expect("item"),
                item_revision: 2,
                text: suffix.into(),
            },
        }]);
        workspace.draw(&mut terminal).expect("streamed frame");
        assert_eq!(workspace.state().selection().is_some(), retained);
        assert_eq!(
            workspace.state().copy().map(|copy| copy.text),
            retained.then(|| "hello".into())
        );
    }
}

/// SEL-1/SEL-3/FR-3: the inspector uses its conversation rectangle; empty drags copy nothing.
#[test]
fn inspector_text_drag_survives_input_geometry_and_empty_drag_clears() {
    for width in [60, 95, 160] {
        let (mut workspace, mut terminal, sequence) = fixture(
            width,
            &[(TranscriptRole::Assistant, "primary text stays separate")],
        );
        let agent = AgentId::new("helper").expect("agent");
        let item = TranscriptItemId::new("helper-answer").expect("item");
        let events = [
            SessionEvent::AgentCreated {
                agent_id: agent.clone(),
                label: "Helper".into(),
                status: AgentStatus::Idle,
            },
            SessionEvent::TranscriptItemStarted {
                agent_id: agent.clone(),
                item_id: item.clone(),
                role: TranscriptRole::Assistant,
            },
            SessionEvent::TranscriptDelta {
                agent_id: agent.clone(),
                item_id: item,
                item_revision: 1,
                text: "side **window text** stays here".into(),
            },
        ];
        workspace.emit(
            events
                .into_iter()
                .enumerate()
                .map(|(i, event)| SessionEventEnvelope {
                    sequence: EventSequence::new(sequence + i as u64),
                    event,
                })
                .collect(),
        );
        workspace.state.select_agent(&agent).expect("peek");
        workspace.draw(&mut terminal).expect("inspector");
        let start = point(&terminal, "window text");
        workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), start));
        workspace
            .draw(&mut terminal)
            .expect("focused inspector adds input");
        let focused = point(&terminal, "window text");
        let end = Point {
            x: focused.x + 6,
            ..focused
        };
        workspace.handle(&mouse(MouseEventKind::Drag(MouseButton::Left), end));
        workspace.draw(&mut terminal).expect("inspector selection");
        assert_eq!(
            workspace
                .handle(&mouse(MouseEventKind::Up(MouseButton::Left), end))
                .copied
                .expect("copy")
                .text,
            "window"
        );
        assert_eq!(
            workspace.state().selection().expect("selection").agent,
            agent
        );
        let start = point(&terminal, "window text");
        workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), start));
        workspace.handle(&mouse(MouseEventKind::Drag(MouseButton::Left), start));
        assert!(
            workspace
                .handle(&mouse(MouseEventKind::Up(MouseButton::Left), start))
                .copied
                .is_none()
        );
        assert!(workspace.state().selection().is_none());
    }
}

/// SEL-1/MD-2: frame review covers composed selection across paragraphs/code at three widths.
#[test]
fn text_selection_frames_cover_three_widths() {
    for (width, name) in [(120, "wide"), (95, "medium"), (60, "narrow")] {
        let (mut workspace, mut terminal, _) = fixture(
            width,
            &[
                (TranscriptRole::User, "Review the Markdown update."),
                (
                    TranscriptRole::Assistant,
                    "## Summary\n\nText **selection** now copies only the chosen words.\n\n```rust\n    println!(\"hello, 世界\");\n```",
                ),
            ],
        );
        let start = point(&terminal, "selection");
        let code = point(&terminal, "    println!");
        let end = Point {
            x: code.x + "    println!(\"hello, 世界\");".width() as u16,
            ..code
        };
        assert_eq!(
            drag(&mut workspace, &mut terminal, start, end),
            "selection now copies only the chosen words.\n\n    println!(\"hello, 世界\");"
        );
        let buffer = terminal.backend().buffer();
        let mut frame = String::new();
        for y in 0..buffer.area.height {
            let mut row = String::new();
            let mut x = 0;
            while x < width {
                let symbol = buffer[(x, y)].symbol();
                row.push_str(symbol);
                x += symbol.width().max(1) as u16;
            }
            frame.push_str(row.trim_end());
            frame.push('\n');
        }
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("frames/text-selection-{name}.txt"));
        if std::env::var_os("PLEXMATON_WRITE_FRAMES").is_some() {
            std::fs::write(&path, &frame).expect("write frame");
        }
        assert_eq!(std::fs::read_to_string(path).expect("frame"), frame);
    }
}
