//! Offline single-agent Markdown color review, using the actual workspace and unchanged footer.
//! cargo run -p plexmaton-tui --example markdown_style_preview -- <output-directory>
use plexmaton_core::{
    AgentId, AgentStatus, ConversationEvent, ConversationEventEnvelope, EventSequence,
    TranscriptItemId, TranscriptRole,
};
use plexmaton_tui::{Palette, StatusLineText, Workspace};
use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::Buffer,
    crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers},
};
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

const SYNTAX: &str = r#"# Readable code

**Useful emphasis**, *a little nuance*, and `cargo test`.

> Syntax colors help you follow the code.

## Rust

```rust
// Keep the original source.
fn greet(name: &str) -> String {
    let count: u32 = 42;
    format!("Hello, {name}: {count}")
}
```

## Python

```python
def greet(name: str) -> str:
    """A small, readable function."""
    return f"Hello, {name}"
```

## JSON

```json
{
  "name": "世界",
  "enabled": true,
  "count": 42
}
```

Read the [design notes](https://example.com/notes)."#;

fn main() -> Result<()> {
    let directory = std::env::args()
        .nth(1)
        .ok_or("provide an output directory")?;
    let syntax = std::env::args().nth(2).is_some_and(|arg| arg == "syntax");
    let directory = Path::new(&directory);
    std::fs::create_dir_all(directory)?;
    let sizes = if syntax {
        [(120, 50), (88, 52), (60, 56), (88, 20)]
    } else {
        [(120, 40), (88, 42), (60, 46), (88, 20)]
    };
    for (width, height) in sizes {
        let footer = footer(width)?;
        for (name, palette, selected) in [
            ("before", Palette::ansi(), false),
            ("pastel", Palette::pastel(), false),
            ("mono", Palette::monochrome(), false),
            ("selected", Palette::pastel(), true),
        ] {
            let buffer = preview(palette, footer.clone(), width, height, syntax, selected)?;
            std::fs::write(
                directory.join(format!("{name}-{width}x{height}.svg")),
                frame_svg::svg(&buffer),
            )?;
        }
    }
    Ok(())
}

fn preview(
    palette: Palette,
    footer: StatusLineText,
    width: u16,
    height: u16,
    syntax: bool,
    selected: bool,
) -> Result<Buffer> {
    let mut workspace = Workspace::with_palette(Palette::ansi());
    workspace.set_model(plexmaton_tui::ConfigurationSummary {
        configured_name: "fixture".into(),
        provider: "local".into(),
        model: "plexmaton-dev".into(),
        reasoning_effort: plexmaton_core::ReasoningEffort::High,
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
            if syntax {
                "Show Rust, Python and JSON in our Markdown theme."
            } else {
                "Show me a softer Markdown style — blue, green, and the rest of our pastel palette."
            },
        ),
        (
            "answer",
            TranscriptRole::Assistant,
            if syntax { SYNTAX } else { MARKDOWN },
        ),
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
    if selected {
        workspace.handle(&Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT)));
    }
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
