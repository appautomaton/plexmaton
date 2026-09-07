//! Actual composer model selection at three widths, including a retained refusal.
use plexmaton_core::{
    AgentId, AgentStatus, ConversationEvent, ConversationEventEnvelope, EventSequence,
    ReasoningEffort,
};
use plexmaton_tui::{
    ConfigurationSummary, ModelChoice, ModelIdentity, Palette, SurfaceId, Workspace,
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
    let output = std::env::args().nth(1).ok_or("provide output directory")?;
    let output = Path::new(&output);
    std::fs::create_dir_all(output)?;
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
        workspace.set_model(ConfigurationSummary {
            provider: "work".into(),
            configured_name: "balanced".into(),
            model: "balanced-v1".into(),
            reasoning_effort: ReasoningEffort::High,
        });
        workspace.set_working_directory("~/dev/plexmaton · main".into());
        workspace.set_model_choices(
            [
                ("work", "balanced", "Balanced"),
                ("work", "fast", "Fast"),
                ("personal", "balanced", "Balanced"),
                ("personal", "deep", "Deep reasoning"),
                ("local", "small", "Local small model"),
                ("local", "large", "Local large model"),
            ]
            .into_iter()
            .map(|(provider, model, display)| ModelChoice {
                identity: ModelIdentity {
                    provider: provider.into(),
                    model: model.into(),
                },
                display_name: display.into(),
                wire_id: format!("{model}-v1"),
            }),
        );
        let mut terminal = Terminal::new(TestBackend::new(width, 30))?;
        workspace.draw(&mut terminal)?;
        for _ in 0..workspace.surfaces().len() {
            if workspace.state().focused(workspace.surfaces()) == Some(SurfaceId::Composer) {
                break;
            }
            workspace.handle(&Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
            workspace.draw(&mut terminal)?;
        }
        workspace.handle(&Event::Paste("/model ".into()));
        workspace.draw(&mut terminal)?;
        std::fs::write(
            output.join(format!("models-{width}.svg")),
            frame_svg::svg(terminal.backend().buffer()),
        )?;
        workspace.handle(&Event::Paste("personal".into()));
        workspace.report_model(Err(
            "This conversation contains history the selected model cannot replay.".into(),
        ));
        workspace.draw(&mut terminal)?;
        std::fs::write(
            output.join(format!("refusal-{width}.svg")),
            frame_svg::svg(terminal.backend().buffer()),
        )?;
    }
    Ok(())
}
