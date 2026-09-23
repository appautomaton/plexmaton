//! Launch's greeting in the workspace, for review (phase 04 stage 10).
//! cargo run -p plexmaton-tui --example greeting_preview -- <output-directory>
//!
//! Two moments of the greeting over a new, empty conversation at three widths: the centre grown
//! into a circle, and centre and frame turned into diamonds. Drawn by the real workspace, which
//! carries the greeting on its motion clock, for a monospace cell of 9 by 20 pixels.

use std::path::Path;
use std::time::{Duration, Instant};

use plexmaton_core::{
    AgentId, AgentStatus, ConversationEvent, ConversationEventEnvelope, EventSequence,
};
use plexmaton_tui::{ConfigurationSummary, Palette, Workspace, mark::CellSize};
use ratatui::{Terminal, backend::TestBackend};

#[path = "support/frame_svg.rs"]
mod frame_svg;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() -> Result<()> {
    let directory = std::env::args()
        .nth(1)
        .ok_or("provide an output directory")?;
    let directory = Path::new(&directory);
    std::fs::create_dir_all(directory)?;
    for (width, height) in [(120, 30), (88, 32), (60, 36)] {
        for (name, at) in [("circle", 700), ("turned", 1850)] {
            let mut workspace = Workspace::with_palette(Palette::pastel());
            workspace.emit(vec![ConversationEventEnvelope {
                sequence: EventSequence::new(1),
                event: ConversationEvent::AgentCreated {
                    agent_id: AgentId::new("primary")?,
                    label: "Plexmaton".into(),
                    status: AgentStatus::Idle,
                },
            }]);
            workspace.set_model(ConfigurationSummary {
                provider: "local".into(),
                model: "muse-spark-1.3".into(),
                display_name: "Muse Spark 1.3".into(),
                configured_name: "muse".into(),
                reasoning_effort: plexmaton_core::ReasoningEffort::High,
            });
            workspace.set_working_directory("~/plexmaton".into());
            workspace.set_cell_size(Some(CellSize {
                width: 9,
                height: 20,
            }));
            let start = Instant::now();
            workspace.greet(start);
            let until = start + Duration::from_millis(at);
            let mut now = start;
            while let Some(deadline) = workspace.motion_deadline(now) {
                if deadline > until {
                    break;
                }
                now = deadline;
                workspace.advance_motion(now);
            }
            let mut terminal = Terminal::new(TestBackend::new(width, height))?;
            workspace.draw(&mut terminal)?;
            std::fs::write(
                directory.join(format!("greeting-{name}-{width}.svg")),
                frame_svg::svg(terminal.backend().buffer()),
            )?;
        }
    }
    Ok(())
}
