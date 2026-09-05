use super::*;
use plexmaton_core::{AgentStatus, EventSequence, SessionEvent, TranscriptItemId, TranscriptRole};
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
};

const SOURCE: &str = "## Build summary\n\n**Ready** for review — 中文也要清楚。 Use `cargo test` before merging.\n\n- Preserve **exact source** when copying\n- Keep *streaming* readable\n  - No hidden model requests\n\n> Small changes, clear ownership.\n\n```rust\nfn main() {\n    println!(\"hello, 世界\");\n}\n```\n\n| Item | State | Time | Input | Cache | Note |\n| --- | --- | ---: | ---: | ---: | --- |\n| tests | passed | 2s | 1800 | 84% | offline |\n\nSee [the docs](https://example.com/docs).";

fn fixture(width: u16) -> (Workspace, Terminal<TestBackend>) {
    let mut workspace = Workspace::with_palette(Palette::pastel());
    let agent = AgentId::new("primary").expect("agent");
    let user = TranscriptItemId::new("question").expect("item");
    let assistant = TranscriptItemId::new("answer").expect("item");
    let events = [
        SessionEvent::AgentCreated {
            agent_id: agent.clone(),
            label: "Plexmaton".into(),
            status: AgentStatus::Idle,
        },
        SessionEvent::TranscriptItemStarted {
            agent_id: agent.clone(),
            item_id: user.clone(),
            role: TranscriptRole::User,
        },
        SessionEvent::TranscriptDelta {
            agent_id: agent.clone(),
            item_id: user,
            item_revision: 1,
            text: "Show **Markdown** and a Rust example.".into(),
        },
        SessionEvent::TranscriptItemStarted {
            agent_id: agent.clone(),
            item_id: assistant.clone(),
            role: TranscriptRole::Assistant,
        },
        SessionEvent::TranscriptDelta {
            agent_id: agent,
            item_id: assistant,
            item_revision: 1,
            text: SOURCE.into(),
        },
    ];
    workspace.emit(
        events
            .into_iter()
            .enumerate()
            .map(|(index, event)| SessionEventEnvelope {
                sequence: EventSequence::new(index as u64 + 1),
                event,
            })
            .collect(),
    );
    let mut terminal = Terminal::new(TestBackend::new(width, 48)).expect("terminal");
    workspace.draw(&mut terminal).expect("draw");
    (workspace, terminal)
}

/// MD-2/MD-4/TR-3: table reflow changes displayed rows without rewriting a parked semantic anchor.
#[test]
fn markdown_resize_round_trip_preserves_the_parked_frame() {
    let (mut workspace, mut terminal) = fixture(60);
    terminal.backend_mut().resize(60, 20);
    workspace.handle(&Event::Resize(60, 20));
    workspace.draw(&mut terminal).expect("short viewport");
    let bounds = workspace
        .surfaces()
        .get(crate::SurfaceId::Transcript)
        .expect("transcript")
        .bounds;
    workspace.handle(&mouse(MouseEventKind::ScrollUp, bounds.x + 2, bounds.y + 2));
    workspace.draw(&mut terminal).expect("parked");
    let agent = AgentId::new("primary").expect("agent");
    let anchor = workspace
        .state()
        .conversation_position(&agent)
        .expect("parked anchor")
        .clone();
    assert!(matches!(
        anchor,
        crate::transcript::TranscriptPosition::At { .. }
    ));
    let before = terminal.backend().buffer().clone();
    for width in [120, 60] {
        terminal.backend_mut().resize(width, 20);
        workspace.handle(&Event::Resize(width, 20));
        workspace.draw(&mut terminal).expect("reflow");
        assert_eq!(
            workspace.state().conversation_position(&agent),
            Some(&anchor)
        );
    }
    assert_eq!(terminal.backend().buffer(), &before);
}
fn mouse(kind: MouseEventKind, x: u16, y: u16) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    })
}

/// MD-1/MD-4/SEL-7: hover, source copy and selection reuse layout; a streamed delta changes one entry.
#[test]
fn markdown_hover_copy_and_streaming_share_cached_geometry_and_exact_source() {
    for width in [120, 95, 60] {
        let (mut workspace, mut terminal) = fixture(width);
        let initial = workspace.metrics().text_layouts();
        assert_eq!(initial, 1);
        let bounds = workspace
            .surfaces()
            .get(crate::SurfaceId::Transcript)
            .expect("transcript")
            .bounds;
        let y = (bounds.y..bounds.bottom())
            .find(|y| {
                (bounds.x..bounds.right())
                    .map(|x| terminal.backend().buffer()[(x, *y)].symbol())
                    .collect::<String>()
                    .contains("Build summary")
            })
            .expect("heading visible");
        workspace.handle(&mouse(MouseEventKind::Moved, bounds.x + 2, y));
        workspace.draw(&mut terminal).expect("hover");
        let copy = (bounds.x..bounds.right())
            .find(|x| terminal.backend().buffer()[(*x, y)].symbol() == "󰆏")
            .expect("copy glyph");
        workspace.handle(&mouse(MouseEventKind::Moved, copy, y));
        workspace.draw(&mut terminal).expect("copy hover");
        workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), copy, y));
        let outcome = workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), copy, y));
        assert_eq!(outcome.copied.expect("exact source").text, SOURCE);
        workspace.handle(&Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT)));
        workspace.draw(&mut terminal).expect("selection");
        assert_eq!(
            workspace.metrics().text_layouts(),
            initial,
            "interaction reparsed Markdown"
        );
        assert_eq!(
            workspace
                .handle(&Event::Key(KeyEvent::new(
                    KeyCode::Char('y'),
                    KeyModifiers::CONTROL
                )))
                .copied
                .expect("selection source")
                .text,
            SOURCE
        );
        let before = workspace.metrics().wrapped();
        workspace.emit(vec![SessionEventEnvelope {
            sequence: EventSequence::new(6),
            event: SessionEvent::TranscriptDelta {
                agent_id: AgentId::new("primary").expect("agent"),
                item_id: TranscriptItemId::new("answer").expect("item"),
                item_revision: 2,
                text: "\n\n**Finished.**".into(),
            },
        }]);
        workspace.draw(&mut terminal).expect("streamed suffix");
        assert_eq!(workspace.metrics().wrapped(), before + 1);
        assert_eq!(workspace.metrics().text_layouts(), initial + 1);
    }
}

/// MD-1/MD-2: review full messages, code indentation, tables and the unchanged literal user role.
#[test]
fn markdown_frames_show_messages_at_three_widths() {
    for (width, name) in [(120, "wide"), (95, "medium"), (60, "narrow")] {
        let (_, terminal) = fixture(width);
        let frame = (0..48)
            .map(|y| {
                let mut row = String::new();
                let mut x = 0;
                while x < width {
                    let symbol = terminal.backend().buffer()[(x, y)].symbol();
                    row.push_str(symbol);
                    x += unicode_width::UnicodeWidthStr::width(symbol).max(1) as u16;
                }
                row.trim_end().to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        assert!(
            frame.contains("Show **Markdown**")
                && frame.contains("Build summary")
                && frame.contains("println!")
        );
        if width == 60 {
            assert!(frame.contains("Item: tests") && frame.contains("Cache: 84%"));
        }
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("frames/markdown-{name}.txt"));
        if std::env::var_os("PLEXMATON_WRITE_FRAMES").is_some() {
            std::fs::write(&path, &frame).expect("write frame");
        }
        assert_eq!(std::fs::read_to_string(path).expect("read frame"), frame);
    }
}
