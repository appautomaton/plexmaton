//! Offline frames of the production pending and failure representations, before any completion.

use plexmaton_core::{
    AgentId, AgentStatus, EventSequence, SessionEvent, SessionEventEnvelope, TranscriptItemId,
    TranscriptRole,
};
use plexmaton_tui::{Point, SurfaceId, Workspace, preparation::Refusal};
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
};
use std::path::Path;

#[path = "support/frame_svg.rs"]
mod frame_svg;
#[path = "support/prepared_frame.rs"]
mod prepared_frame;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::args()
        .nth(1)
        .ok_or("provide an output directory")?;
    let directory = Path::new(&directory);
    std::fs::create_dir_all(directory)?;
    for width in [120, 88, 60] {
        let mut workspace = Workspace::default();
        let agent = AgentId::new("primary")?;
        let mut events = vec![SessionEvent::AgentCreated {
            agent_id: agent.clone(),
            label: "Plexmaton".into(),
            status: AgentStatus::Running,
        }];
        for (index, role, source) in [
            (
                0,
                TranscriptRole::User,
                "Keep the conversation responsive while preparing rich text.",
            ),
            (
                1,
                TranscriptRole::Assistant,
                "## Prepared off the input loop\n\nA **bounded** reply with `code` and 中文.\n\n- The composer stays usable.\n- Copy follows prepared source ranges.",
            ),
        ] {
            let item = TranscriptItemId::new(format!("text-{index}"))?;
            events.push(SessionEvent::TranscriptItemStarted {
                agent_id: agent.clone(),
                item_id: item.clone(),
                role,
            });
            events.push(SessionEvent::TranscriptDelta {
                agent_id: agent.clone(),
                item_id: item,
                item_revision: 1,
                text: source.into(),
            });
        }
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
        let mut terminal = Terminal::new(TestBackend::new(width, 24))?;
        workspace.draw(&mut terminal)?;
        let work = workspace
            .take_preparation()
            .ok_or("expected pending preparation")?;
        std::fs::write(
            directory.join(format!("pending-{width}.svg")),
            frame_svg::svg(terminal.backend().buffer()),
        )?;
        workspace.fail_preparation(work.token, Refusal::Unavailable);
        workspace.draw(&mut terminal)?;
        std::fs::write(
            directory.join(format!("unavailable-{width}.svg")),
            frame_svg::svg(terminal.backend().buffer()),
        )?;
        copy_pending(directory, width)?;
    }
    Ok(())
}

fn copy_pending(directory: &Path, width: u16) -> Result<(), Box<dyn std::error::Error>> {
    let mut workspace = Workspace::default();
    let agent = AgentId::new("primary")?;
    let mut events = vec![SessionEvent::AgentCreated {
        agent_id: agent.clone(),
        label: "Plexmaton".into(),
        status: AgentStatus::Idle,
    }];
    for index in 0..200 {
        let item = TranscriptItemId::new(format!("copy-{index}"))?;
        events.push(SessionEvent::TranscriptItemStarted {
            agent_id: agent.clone(),
            item_id: item.clone(),
            role: TranscriptRole::Assistant,
        });
        events.push(SessionEvent::TranscriptDelta {
            agent_id: agent.clone(),
            item_id: item,
            item_revision: 1,
            text: format!("**message {index:03}** with prepared copy"),
        });
    }
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
    let mut terminal = Terminal::new(TestBackend::new(width, 24))?;
    prepared_frame::draw(&mut workspace, &mut terminal)?;
    let bounds = workspace
        .surfaces()
        .get(SurfaceId::Transcript)
        .ok_or("transcript")?
        .bounds;
    let inside = Point {
        x: bounds.x + 2,
        y: bounds.y + 4,
    };
    for _ in 0..150 {
        workspace.handle(&mouse(MouseEventKind::ScrollUp, inside));
        prepared_frame::draw(&mut workspace, &mut terminal)?;
    }
    let first = point(&terminal, "message 000").ok_or("first message not visible")?;
    workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), first));
    for _ in 0..150 {
        workspace.handle(&mouse(MouseEventKind::ScrollDown, inside));
        prepared_frame::draw(&mut workspace, &mut terminal)?;
    }
    let last = point(&terminal, "message 199").ok_or("last message not visible")?;
    let end = Point {
        x: last.x + 11,
        ..last
    };
    workspace.handle(&mouse(MouseEventKind::Drag(MouseButton::Left), end));
    if workspace
        .handle(&mouse(MouseEventKind::Up(MouseButton::Left), end))
        .copied
        .is_some()
    {
        return Err("fixture must need evicted selection data".into());
    }
    workspace.draw(&mut terminal)?;
    std::fs::write(
        directory.join(format!("copy-pending-{width}.svg")),
        frame_svg::svg(terminal.backend().buffer()),
    )?;
    Ok(())
}

fn mouse(kind: MouseEventKind, at: Point) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column: at.x,
        row: at.y,
        modifiers: KeyModifiers::NONE,
    })
}

fn point(terminal: &Terminal<TestBackend>, text: &str) -> Option<Point> {
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height).find_map(|y| {
        (0..buffer.area.width.saturating_sub(text.len() as u16))
            .find(|x| {
                text.chars()
                    .enumerate()
                    .all(|(offset, ch)| buffer[(*x + offset as u16, y)].symbol() == ch.to_string())
            })
            .map(|x| Point { x, y })
    })
}
