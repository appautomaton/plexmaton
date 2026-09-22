//! The user's turn on its band, beside the agent's, painted through the real renderer for review.
//! cargo run -p plexmaton-tui --example user_message_preview -- <output-directory>

use std::path::Path;

use plexmaton_core::{
    AgentId, AgentStatus, ConversationEvent, ConversationEventEnvelope, EventSequence, ServerTool,
    ServerToolAction, ServerToolCall, ServerToolStatus, TranscriptItemId, TranscriptRole,
};
use plexmaton_tui::{MarkdownTheme, Palette, Workspace};
use ratatui::crossterm::event::{Event, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend};

#[path = "support/frame_svg.rs"]
mod frame_svg;
#[path = "support/prepared_frame.rs"]
mod prepared_frame;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() -> Result<()> {
    let directory = std::env::args()
        .nth(1)
        .ok_or("provide an output directory")?;
    let directory = Path::new(&directory);
    std::fs::create_dir_all(directory)?;
    for (width, height) in [(120, 30), (88, 32), (60, 36)] {
        let mut workspace = fixture()?;
        let mut terminal = Terminal::new(TestBackend::new(width, height))?;
        prepared_frame::draw(&mut workspace, &mut terminal)?;
        write(directory, &format!("user-message-{width}"), &terminal)?;
        if width == 120 {
            // Hover the first question, which reveals its copy action and boundary rules.
            let row = (0..height)
                .find(|&y| {
                    (0..width)
                        .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                        .collect::<String>()
                        .contains("› What is")
                })
                .ok_or("the first question is on screen")?;
            workspace.handle(&Event::Mouse(MouseEvent {
                kind: MouseEventKind::Moved,
                column: 20,
                row,
                modifiers: KeyModifiers::NONE,
            }));
            prepared_frame::draw(&mut workspace, &mut terminal)?;
            write(directory, "user-message-120-hover", &terminal)?;
        }
    }
    Ok(())
}

fn write(directory: &Path, name: &str, terminal: &Terminal<TestBackend>) -> Result<()> {
    std::fs::write(
        directory.join(format!("{name}.svg")),
        frame_svg::svg(terminal.backend().buffer()),
    )?;
    Ok(())
}

fn text(
    events: &mut Vec<ConversationEvent>,
    agent: &AgentId,
    name: &str,
    role: TranscriptRole,
    source: &str,
) -> Result<()> {
    let item = TranscriptItemId::new(name)?;
    events.push(ConversationEvent::TranscriptItemStarted {
        agent_id: agent.clone(),
        item_id: item.clone(),
        role,
    });
    events.push(ConversationEvent::TranscriptDelta {
        agent_id: agent.clone(),
        item_id: item.clone(),
        item_revision: 1,
        text: source.to_owned(),
    });
    events.push(ConversationEvent::TranscriptItemFinalized {
        agent_id: agent.clone(),
        item_id: item,
        item_revision: 2,
    });
    Ok(())
}

fn fixture() -> Result<Workspace> {
    let agent = AgentId::new("primary")?;
    let mut events = vec![ConversationEvent::AgentCreated {
        agent_id: agent.clone(),
        label: "Plexmaton".into(),
        status: AgentStatus::Idle,
    }];
    let user = TranscriptRole::User;
    let assistant = TranscriptRole::Assistant;
    text(
        &mut events,
        &agent,
        "q1",
        user,
        "What is the latest news on the Qwen Image 2.1 model?",
    )?;
    text(
        &mut events,
        &agent,
        "a1",
        assistant,
        "Finding the latest on Qwen Image 2.1, checking fresh announcements.",
    )?;
    events.push(ConversationEvent::ServerToolCalled {
        agent_id: agent.clone(),
        item_id: TranscriptItemId::new("search")?,
        item_revision: 0,
        call: ServerToolCall {
            tool: ServerTool::WebSearch,
            action: ServerToolAction::Search {
                queries: vec!["Qwen Image 2.1 release".into()],
            },
            status: ServerToolStatus::Completed,
        },
    });
    text(
        &mut events,
        &agent,
        "a2",
        assistant,
        "It shipped on September 20th under Apache 2.0, with the base weights open.",
    )?;
    text(
        &mut events,
        &agent,
        "q2",
        user,
        "And compare it with Flux 2 on licensing. Keep it short: I only care whether I can ship it in a commercial app.\nOne line each is enough.",
    )?;
    text(
        &mut events,
        &agent,
        "a3",
        assistant,
        "Qwen Image 2.1: yes, Apache 2.0 permits commercial use. Flux 2: only under its paid licence.",
    )?;
    let mut workspace =
        Workspace::with_palette(Palette::pastel().with_markdown_theme(MarkdownTheme::Pastel));
    workspace.emit(
        events
            .into_iter()
            .enumerate()
            .map(|(index, event)| ConversationEventEnvelope {
                sequence: EventSequence::new(index as u64 + 1),
                event,
            })
            .collect(),
    );
    workspace.set_model(plexmaton_tui::ConfigurationSummary {
        provider: "local".into(),
        model: "muse-spark-1.3".into(),
        display_name: "Muse Spark 1.3".into(),
        configured_name: "muse".into(),
        reasoning_effort: plexmaton_core::ReasoningEffort::High,
    });
    workspace.set_working_directory("~/plexmaton".into());
    Ok(workspace)
}
