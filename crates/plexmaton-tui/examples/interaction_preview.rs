//! Actual frames for shared navigation, Drawer chrome and transient interaction feedback.
use plexmaton_core::{
    AgentId, AgentStatus, ConversationEvent, ConversationEventEnvelope, EventSequence,
    ReasoningEffort,
};
use plexmaton_tui::{ConfigurationSummary, Palette, Workspace};
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind},
};
use std::path::Path;
use unicode_width::UnicodeWidthStr as _;
#[path = "support/frame_svg.rs"]
mod frame_svg;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn main() -> Result<()> {
    let directory = std::env::args()
        .nth(1)
        .ok_or("provide an output directory")?;
    let directory = Path::new(&directory);
    std::fs::create_dir_all(directory)?;
    for width in [120, 88, 60] {
        let mut workspace = Workspace::with_palette(Palette::pastel());
        workspace.emit(vec![ConversationEventEnvelope {
            sequence: EventSequence::new(1),
            event: ConversationEvent::AgentCreated {
                agent_id: AgentId::new("primary")?,
                label: "Plexmaton".into(),
                status: AgentStatus::Idle,
            },
        }]);
        workspace.set_working_directory("~/dev/plexmaton · main".into());
        let mut terminal = Terminal::new(TestBackend::new(width, 30))?;
        workspace.draw(&mut terminal)?;
        workspace.handle(&Event::Key(KeyEvent::new(
            KeyCode::Char('p'),
            KeyModifiers::CONTROL,
        )));
        workspace.draw(&mut terminal)?;
        let (x, y) = point(&terminal, "Permissions")?;
        workspace.handle(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        }));
        workspace.draw(&mut terminal)?;
        save(directory, "drawer", &terminal)?;
        workspace.show_configuration(ConfigurationSummary {
            configured_name: "fixture".into(),
            provider: "Local development".into(),
            model: "example-model".into(),
            reasoning_effort: ReasoningEffort::High,
        });
        workspace.draw(&mut terminal)?;
        let (x, y) = point(&terminal, "︽")?;
        workspace.handle(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        }));
        workspace.draw(&mut terminal)?;
        save(directory, "configuration", &terminal)?;
        for _ in 0..2 {
            workspace.handle(&Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
            workspace.draw(&mut terminal)?;
        }
        for _ in 0..workspace.surfaces().len() {
            if workspace.state().focused(workspace.surfaces())
                == Some(plexmaton_tui::SurfaceId::Composer)
            {
                break;
            }
            workspace.handle(&Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
            workspace.draw(&mut terminal)?;
        }
        workspace.handle(&Event::Paste("Review the changes".into()));
        workspace.handle(&Event::Key(KeyEvent::new(
            KeyCode::Char('j'),
            KeyModifiers::CONTROL,
        )));
        workspace.handle(&Event::Paste("and check the edge cases.".into()));
        workspace.report_copy(
            plexmaton_tui::CopyReceipt::Copied,
            std::time::Instant::now(),
        );
        workspace.draw(&mut terminal)?;
        save(directory, "copied", &terminal)?;
        workspace.report_copy(plexmaton_tui::CopyReceipt::Sent, std::time::Instant::now());
        workspace.draw(&mut terminal)?;
        save(directory, "sent", &terminal)?;
        workspace.handle(&Event::Key(KeyEvent::new(
            KeyCode::Char('d'),
            KeyModifiers::CONTROL,
        )));
        workspace.draw(&mut terminal)?;
        save(directory, "quit", &terminal)?;
    }
    Ok(())
}
fn point(terminal: &Terminal<TestBackend>, text: &str) -> Result<(u16, u16)> {
    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        let row: String = (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect();
        if let Some(offset) = row.find(text) {
            return Ok((row[..offset].width() as u16, y));
        }
    }
    Err(format!("missing {text}").into())
}
fn save(directory: &Path, kind: &str, terminal: &Terminal<TestBackend>) -> Result<()> {
    let buffer = terminal.backend().buffer();
    // The full copied frame owns composition; the other receipts claim only the final row.
    let rendered = if matches!(kind, "sent" | "quit") {
        let mut row =
            ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, buffer.area.width, 1));
        for x in 0..buffer.area.width {
            row[(x, 0)] = buffer[(x, buffer.area.height - 1)].clone();
        }
        frame_svg::svg(&row)
    } else {
        frame_svg::svg(buffer)
    };
    std::fs::write(
        directory.join(format!("{kind}-{}.svg", buffer.area.width)),
        rendered,
    )?;
    Ok(())
}
