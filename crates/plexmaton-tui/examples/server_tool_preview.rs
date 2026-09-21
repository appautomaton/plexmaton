//! Provider-run calls beside a local tool, painted through the real renderer for colour review.
//! cargo run -p plexmaton-tui --example server_tool_preview -- <output-directory>

use std::path::Path;

use plexmaton_core::{
    AgentId, AgentStatus, ConversationEvent, ConversationEventEnvelope, EventSequence, ServerTool,
    ServerToolAction, ServerToolCall, ServerToolStatus, ToolCallId, ToolCallStatus, ToolDetail,
    ToolPresentation, TranscriptItemId, TranscriptRole,
};
use plexmaton_tui::{MarkdownTheme, Palette, Workspace};
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
    for (width, height) in [(120, 40), (88, 42), (60, 46)] {
        let mut workspace = fixture()?;
        let mut terminal = Terminal::new(TestBackend::new(width, height))?;
        prepared_frame::draw(&mut workspace, &mut terminal)?;
        let buffer = terminal.backend().buffer();
        std::fs::write(
            directory.join(format!("server-tool-{}.svg", buffer.area.width)),
            frame_svg::svg(buffer),
        )?;
    }
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
    text(
        &mut events,
        &agent,
        "question",
        TranscriptRole::User,
        "What is the latest stable Rust release, and when did it ship?",
    )?;
    text(
        &mut events,
        &agent,
        "reasoning",
        TranscriptRole::Reasoning,
        "A version number is a current fact, so I will search rather than answer from memory, then confirm the date on the release post.",
    )?;
    for (revision, status) in [
        (0, ToolCallStatus::Queued),
        (1, ToolCallStatus::Running),
        (2, ToolCallStatus::Succeeded),
    ] {
        events.push(ConversationEvent::ToolCallChanged {
            agent_id: agent.clone(),
            item_id: TranscriptItemId::new("local-read")?,
            item_revision: revision,
            call_id: ToolCallId::new("local-read")?,
            label: "read_file".into(),
            status,
            presentation: ToolPresentation {
                invocation: (revision > 0).then(|| ToolDetail::Text {
                    source: "path: Cargo.toml".into(),
                    omitted_bytes: 0,
                }),
                outcome: None,
            },
        });
    }
    let search = |queries: &[&str]| ServerToolAction::Search {
        queries: queries.iter().map(|query| (*query).to_owned()).collect(),
    };
    for (name, action, status) in [
        (
            "search",
            search(&["latest stable Rust release"]),
            ServerToolStatus::Completed,
        ),
        (
            "search-two",
            search(&["Rust 1.98.1 release date", "Rust 1.98.1 point release"]),
            ServerToolStatus::Completed,
        ),
        ("search-bare", search(&[]), ServerToolStatus::Completed),
        (
            "open",
            ServerToolAction::OpenPage {
                url: "blog.rust-lang.org/releases/".into(),
            },
            ServerToolStatus::Completed,
        ),
        (
            "find",
            ServerToolAction::FindInPage {
                url: "releases.rs/docs/1.98.1".into(),
                pattern: "1.98.1".into(),
            },
            ServerToolStatus::Completed,
        ),
        (
            "failed",
            search(&["Rust nightly changelog"]),
            ServerToolStatus::Failed,
        ),
    ] {
        events.push(ConversationEvent::ServerToolCalled {
            agent_id: agent.clone(),
            item_id: TranscriptItemId::new(name)?,
            item_revision: 0,
            call: ServerToolCall {
                tool: ServerTool::WebSearch,
                action,
                status,
            },
        });
    }
    events.push(ConversationEvent::ServerToolStarted {
        agent_id: agent.clone(),
        item_id: TranscriptItemId::new("running")?,
        tool: ServerTool::WebSearch,
    });
    text(
        &mut events,
        &agent,
        "answer",
        TranscriptRole::Assistant,
        "Rust 1.98.1 is the latest stable release. It is a point release over 1.98.0, and the release post on the Rust blog lists what it fixes.",
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
    workspace.set_working_directory("~/plexmaton".into());
    Ok(workspace)
}
