//! What `/compact` says when the conversation is already inside its retention window (CPL-9).
//!
//! Three frames of the same moment. The decline is not a failure and must not read as one: the
//! request was understood, answered, and left entirely with the user, who can ask again by name.
//! It carries the emphasis of a waiting action because something is waiting on them — the same
//! weight `Ctrl-D`'s question carries, and deliberately not the weight of the red failure row
//! underneath it, which is what this message used to look like.
//!
//! 60 columns is the one that decides the wording: the sentence and the override it names have to
//! survive the narrowest column anyone reads this in.
use plexmaton_core::{
    AgentId, AgentStatus, ConversationEvent, ConversationEventEnvelope, EventSequence,
    ReasoningEffort,
};
use plexmaton_tui::{
    CompactRefusal, CompactionNote, ConfigurationSummary, Palette, SurfaceId, Workspace,
};
use ratatui::{Terminal, backend::TestBackend};
use std::path::Path;

#[path = "support/frame_svg.rs"]
mod frame_svg;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args().nth(1).ok_or("provide output directory")?;
    let output = Path::new(&output);
    std::fs::create_dir_all(output)?;
    let primary = AgentId::new("primary")?;
    for width in [120, 88, 60] {
        let mut workspace = Workspace::with_palette(Palette::pastel());
        workspace.emit(vec![ConversationEventEnvelope {
            sequence: EventSequence::new(1),
            event: ConversationEvent::AgentCreated {
                agent_id: primary.clone(),
                label: "Plexmaton".into(),
                status: AgentStatus::Idle,
            },
        }]);
        workspace.set_model(ConfigurationSummary {
            provider: "local".into(),
            configured_name: "luna".into(),
            model: "gpt-5.6-luna".into(),
            reasoning_effort: ReasoningEffort::Max,
        });
        workspace.set_working_directory("~/dev/plexmaton · main".into());
        let mut terminal = Terminal::new(TestBackend::new(width, 24))?;
        workspace.draw(&mut terminal)?;
        let _ = workspace.state().focused(workspace.surfaces()) == Some(SurfaceId::Composer);
        workspace.report_compaction(
            &primary,
            CompactionNote::Refused(CompactRefusal::WithinRetention),
        );
        workspace.draw(&mut terminal)?;
        std::fs::write(
            output.join(format!("compaction-decline-{width}.svg")),
            frame_svg::svg(terminal.backend().buffer()),
        )?;
    }
    Ok(())
}
