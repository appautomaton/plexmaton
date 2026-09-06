//! Offline single-agent skill completion review using the actual workspace buffer.
//! cargo run -p plexmaton-tui --example skill_picker_preview -- <output-directory>

use std::path::Path;

use plexmaton_core::{
    AgentId, AgentStatus, ConversationEvent, ConversationEventEnvelope, EventSequence,
};
use plexmaton_tui::{SkillChoice, SkillChoiceSource, Workspace};
use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::Buffer,
    crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers},
};

#[path = "support/frame_svg.rs"]
mod frame_svg;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() -> Result<()> {
    let directory = std::env::args()
        .nth(1)
        .ok_or("provide an output directory")?;
    let directory = Path::new(&directory);
    std::fs::create_dir_all(directory)?;
    for (name, width) in [("wide", 120), ("medium", 95), ("narrow", 60)] {
        let buffer = preview(width, 32)?;
        std::fs::write(
            directory.join(format!("skill-picker-{name}.svg")),
            frame_svg::svg(&buffer),
        )?;
    }
    Ok(())
}

fn preview(width: u16, height: u16) -> Result<Buffer> {
    let mut workspace = Workspace::default();
    workspace.emit(vec![ConversationEventEnvelope {
        sequence: EventSequence::new(1),
        event: ConversationEvent::AgentCreated {
            agent_id: AgentId::new("primary")?,
            label: "Plexmaton".to_owned(),
            status: AgentStatus::Idle,
        },
    }]);
    workspace.set_skills(vec![
        SkillChoice {
            name: "review".to_owned(),
            description: "Review this change for correctness across every affected boundary"
                .to_owned(),
            source: SkillChoiceSource::ProjectNative,
        },
        SkillChoice {
            name: "research".to_owned(),
            description: "Gather focused source evidence".to_owned(),
            source: SkillChoiceSource::ProjectShared,
        },
        SkillChoice {
            name: "release".to_owned(),
            description: "Prepare release notes".to_owned(),
            source: SkillChoiceSource::User,
        },
    ]);
    let mut terminal = Terminal::new(TestBackend::new(width, height))?;
    workspace.draw(&mut terminal)?;
    for code in [KeyCode::Tab, KeyCode::Char('$'), KeyCode::Char('r')] {
        workspace.handle(&Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
        workspace.draw(&mut terminal)?;
    }
    Ok(terminal.backend().buffer().clone())
}
