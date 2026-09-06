//! Offline single-agent Markdown color review, using the actual workspace and unchanged footer.
//! cargo run -p plexmaton-tui --example markdown_style_preview -- <output-directory>
use plexmaton_core::{
    AgentId, AgentStatus, ConversationEvent, ConversationEventEnvelope, EventSequence,
    TranscriptItemId, TranscriptRole,
};
use plexmaton_tui::{Palette, StatusLineText, Workspace};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};
use std::{
    io::Write as _,
    path::Path,
    process::{Command, Stdio},
};

#[path = "support/frame_svg.rs"]
mod frame_svg;
#[path = "support/prepared_frame.rs"]
mod prepared_frame;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const MARKDOWN: &str = "# A calmer place to think\n\nA clear answer with **useful emphasis**, *a little nuance*, and colors that help you find your way.\n\n## Keep the important things visible\n\n- **Blue and green** lead the hierarchy.\n- Run `cargo test` before continuing.\n- Keep 中文 and e\u{301} intact when selecting text.\n\n> Soft colors should support reading, not compete with it.\n\n### Small, explicit steps\n\n```rust\nfn main() {\n    let message = \"hello, 世界\";\n    println!(\"{message}\");\n}\n```\n\n## Check the essentials\n\n| Area | Result |\n| --- | --- |\n| Exact copy | **Preserved** |\n| Status line | Unchanged |\n| Main conversation | Always here |\n\nRead the [design notes](https://example.com/notes) when you are ready.";

fn main() -> Result<()> {
    let directory = std::env::args()
        .nth(1)
        .ok_or("provide an output directory")?;
    let directory = Path::new(&directory);
    std::fs::create_dir_all(directory)?;
    for (width, height) in [(120, 40), (88, 42), (60, 46), (88, 20)] {
        let footer = footer(width)?;
        for (name, palette) in [
            ("before", Palette::ansi()),
            ("pastel", Palette::pastel()),
            ("mono", Palette::monochrome()),
        ] {
            let buffer = preview(palette, footer.clone(), width, height)?;
            std::fs::write(
                directory.join(format!("{name}-{width}x{height}.svg")),
                frame_svg::svg(&buffer),
            )?;
        }
    }
    Ok(())
}

fn preview(palette: Palette, footer: StatusLineText, width: u16, height: u16) -> Result<Buffer> {
    let mut workspace = Workspace::with_palette(Palette::ansi());
    workspace.set_model(plexmaton_tui::ConfigurationSummary {
        provider: "local".into(),
        model: "plexmaton-dev".into(),
        reasoning_effort: "high".into(),
    });
    let agent = AgentId::new("primary")?;
    let mut events = vec![ConversationEvent::AgentCreated {
        agent_id: agent.clone(),
        label: "Plexmaton".into(),
        status: AgentStatus::Idle,
    }];
    for (name, role, source) in [
        (
            "question",
            TranscriptRole::User,
            "Show me a softer Markdown style — blue, green, and the rest of our pastel palette.",
        ),
        ("answer", TranscriptRole::Assistant, MARKDOWN),
    ] {
        let item = TranscriptItemId::new(name)?;
        events.push(ConversationEvent::TranscriptItemStarted {
            agent_id: agent.clone(),
            item_id: item.clone(),
            role,
        });
        events.push(ConversationEvent::TranscriptDelta {
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
            .map(|(i, event)| ConversationEventEnvelope {
                sequence: EventSequence::new(i as u64 + 1),
                event,
            })
            .collect(),
    );
    workspace.set_status_line(footer, 6);
    let mut terminal = Terminal::new(TestBackend::new(width, height))?;
    prepared_frame::draw(&mut workspace, &mut terminal)?;
    workspace.set_palette(palette);
    prepared_frame::draw(&mut workspace, &mut terminal)?;
    Ok(terminal.backend().buffer().clone())
}

fn footer(width: u16) -> Result<StatusLineText> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/statusline-pastel.sh");
    // Display-only fixture values; no provider, journal, user config or real session is involved.
    let snapshot = r#"{
      "model":{"display_name":"Luna"},"effort":{"level":"high"},
      "workspace":{"current_dir":"/workspace/dev/agents/plexmaton"},
      "context_window":{"context_window_size":128000,"total_input_tokens":24800,"total_output_tokens":3200,
        "current_usage":{"input_tokens":400,"cache_read_input_tokens":9600,"cache_creation_input_tokens":0}},
      "cost":{"total_cost_usd":0.024},
      "plexmaton":{"terminal":{"columns":WIDTH},"usage":{"coverage":"complete"},
        "latest_request":{"terminal":{"usage":{"counts":{"input":10000}}}}}
    }"#.replace("WIDTH", &width.to_string());
    let mut child = Command::new("/bin/bash")
        .arg(script)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let write = child
        .stdin
        .take()
        .ok_or("footer stdin")?
        .write_all(snapshot.as_bytes());
    let result = child.wait_with_output()?;
    write?;
    if !result.status.success() {
        return Err("fixture footer failed".into());
    }
    Ok(StatusLineText::parse(&result.stdout)?)
}
