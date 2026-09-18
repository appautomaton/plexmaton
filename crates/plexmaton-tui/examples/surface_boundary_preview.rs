//! What each conversation surface's boundary says.
//!
//! The workspace draws one conversation today and can draw two. Each carries a box in its own hue —
//! the roster's, the user's own, a delegate's — at full strength where the keys are going and
//! carried toward the ground at rest. Both widths where two conversations are on screen are
//! exported: ultrawide, where they are columns, and wide, where the child takes the shelf.
use plexmaton_core::{
    AgentId, AgentStatus, ConversationEvent, ConversationEventEnvelope, EventSequence,
    ReasoningEffort,
};
use plexmaton_tui::{
    ChildControl, ChildControlSnapshot, ConfigurationSummary, Palette, SurfaceId, Workspace,
};
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers},
};
use std::path::Path;

#[path = "support/frame_svg.rs"]
mod frame_svg;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let output = arguments.next().ok_or("provide output directory")?;
    let variant = arguments.next().unwrap_or_else(|| "current".into());
    let output = Path::new(&output);
    std::fs::create_dir_all(output)?;
    for width in [160, 120] {
        let mut workspace = Workspace::with_palette(Palette::pastel());
        workspace.emit(vec![
            ConversationEventEnvelope {
                sequence: EventSequence::new(1),
                event: ConversationEvent::AgentCreated {
                    agent_id: AgentId::new("primary")?,
                    label: "Plexmaton".into(),
                    status: AgentStatus::Idle,
                },
            },
            ConversationEventEnvelope {
                sequence: EventSequence::new(2),
                event: ConversationEvent::AgentCreated {
                    agent_id: AgentId::new("delegated-1")?,
                    label: "Delegated 1".into(),
                    status: AgentStatus::Running,
                },
            },
        ]);
        workspace.set_model(ConfigurationSummary {
            provider: "local".into(),
            configured_name: "luna".into(),
            model: "gpt-5.6-luna".into(),
            reasoning_effort: ReasoningEffort::Max,
        });
        workspace.set_working_directory("~/dev/plexmaton · main".into());
        workspace.set_child_control(
            &AgentId::new("delegated-1")?,
            ChildControlSnapshot {
                revision: 1,
                control: ChildControl::Main,
            },
        )?;
        let mut terminal = Terminal::new(TestBackend::new(width, 34))?;
        workspace.draw(&mut terminal)?;
        // Open the child the way the user does: focus the roster, step onto it, enter.
        for _ in 0..workspace.surfaces().len() {
            if workspace.state().focused(workspace.surfaces()) == Some(SurfaceId::Agents) {
                break;
            }
            workspace.handle(&Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
            workspace.draw(&mut terminal)?;
        }
        workspace.handle(&Event::Key(KeyEvent::new(
            KeyCode::Down,
            KeyModifiers::NONE,
        )));
        workspace.handle(&Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )));
        workspace.draw(&mut terminal)?;
        for _ in 0..workspace.surfaces().len() {
            if workspace.state().focused(workspace.surfaces()) == Some(SurfaceId::Composer) {
                break;
            }
            workspace.handle(&Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
            workspace.draw(&mut terminal)?;
        }
        workspace.handle(&Event::Paste(
            "Compare the heat equation derivations".into(),
        ));
        workspace.draw(&mut terminal)?;
        std::fs::write(
            output.join(format!("boundary-{variant}-{width}.svg")),
            frame_svg::svg(terminal.backend().buffer()),
        )?;
    }
    Ok(())
}
