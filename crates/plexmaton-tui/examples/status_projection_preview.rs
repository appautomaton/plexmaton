//! Offline review of the supplied script's partial snapshot in the real footer renderer.
//! cargo run -p plexmaton-tui --example status_projection_preview -- <output-directory>
use plexmaton_core::{
    AgentId, AgentStatus, ConversationEvent, ConversationEventEnvelope, EventSequence, ToolCallId,
    ToolCallStatus, ToolPresentation, TranscriptItemId, TranscriptRole,
};
use plexmaton_tui::{Palette, StatusLineText, Workspace};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Rect};
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

fn main() -> Result<()> {
    let directory = std::env::args()
        .nth(1)
        .ok_or("provide an output directory")?;
    let directory = Path::new(&directory);
    std::fs::create_dir_all(directory)?;
    for width in [120, 95, 60] {
        let footer = footer(width)?;
        let rows = u16::try_from(footer.lines().len())?;
        assert!(rows <= 6, "fixture must fit the configured footer cap");
        let mut workspace = Workspace::with_palette(Palette::pastel());
        workspace.set_status_line(footer, 6);
        let mut terminal = Terminal::new(TestBackend::new(width, 30))?;
        prepared_frame::draw(&mut workspace, &mut terminal)?;
        // STL-4: review only the changed region, using cells painted by Workspace.
        let mut crop = Buffer::empty(Rect::new(0, 0, width, rows));
        for y in 0..rows {
            for x in 0..width {
                crop[(x, y)] = terminal.backend().buffer()[(x, 30 - rows + y)].clone();
            }
        }
        let text = (0..rows)
            .map(|y| {
                (0..width)
                    .map(|x| crop[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        for retained in [
            "Luna",
            "↑24.8k",
            "↓3.2k",
            "$0.024",
            "History incompatible with model · /model",
        ] {
            assert!(text.contains(retained), "{width}: {text}");
        }
        assert!(!text.contains(""), "no incompatible occupancy percentage");
        std::fs::write(
            directory.join(format!("partial-{width}.svg")),
            frame_svg::svg(&crop),
        )?;
        combined_frame(&mut workspace, &mut terminal, directory, width)?;
    }
    Ok(())
}

fn combined_frame(
    workspace: &mut Workspace,
    terminal: &mut Terminal<TestBackend>,
    directory: &Path,
    width: u16,
) -> Result<()> {
    // ENT-1/STL-3: the same rendered version must retain the footer and compact reasoning.
    let agent = AgentId::new("primary")?;
    let mut events = vec![ConversationEvent::AgentCreated {
        agent_id: agent.clone(),
        label: "Plexmaton".into(),
        status: AgentStatus::Idle,
    }];
    for label in ["read_file", "check_layout"] {
        for (revision, status) in [
            ToolCallStatus::Queued,
            ToolCallStatus::Running,
            ToolCallStatus::Succeeded,
        ]
        .into_iter()
        .enumerate()
        {
            events.push(ConversationEvent::ToolCallChanged {
                agent_id: agent.clone(),
                item_id: TranscriptItemId::new(label)?,
                item_revision: revision as u64,
                call_id: ToolCallId::new(label)?,
                label: label.into(),
                status,
                presentation: ToolPresentation::default(),
            });
        }
    }
    for (id, role, text) in [
        (
            "reasoning",
            TranscriptRole::Reasoning,
            "Checking once.\n\n\n",
        ),
        ("answer", TranscriptRole::Assistant, "Answer follows."),
    ] {
        let item = TranscriptItemId::new(id)?;
        events.push(ConversationEvent::TranscriptItemStarted {
            agent_id: agent.clone(),
            item_id: item.clone(),
            role,
        });
        events.push(ConversationEvent::TranscriptDelta {
            agent_id: agent.clone(),
            item_id: item,
            item_revision: 1,
            text: text.into(),
        });
    }
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
    prepared_frame::draw(workspace, terminal)?;
    let buffer = terminal.backend().buffer();
    let lines = (0..30)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    let thought = lines
        .iter()
        .position(|line| line.contains("Checking once."))
        .ok_or("reasoning not visible")?;
    let answer = lines
        .iter()
        .position(|line| line.contains("Answer follows."))
        .ok_or("answer not visible")?;
    assert_eq!(answer, thought + 2, "only one inter-entry blank row");
    let tool = lines
        .iter()
        .position(|line| line.contains("check_layout"))
        .ok_or("tool not visible")?;
    assert_eq!(
        thought,
        tool + 3,
        "one tool-group separator before reasoning heading"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("History incompatible with model · /model"))
    );
    assert!(lines.iter().any(|line| line.contains("↑24.8k")));
    use ratatui::crossterm::event::{Event, KeyModifiers, MouseEvent, MouseEventKind};
    workspace.handle(&Event::Mouse(MouseEvent {
        kind: MouseEventKind::Moved,
        column: 3,
        row: u16::try_from(thought)?,
        modifiers: KeyModifiers::NONE,
    }));
    prepared_frame::draw(workspace, terminal)?;
    let buffer = terminal.backend().buffer();
    let top = u16::try_from(tool.saturating_sub(1))?;
    let mut crop = Buffer::empty(Rect::new(0, 0, width, 30 - top));
    for y in top..30 {
        for x in 0..width {
            crop[(x, y - top)] = buffer[(x, y)].clone();
        }
    }
    std::fs::write(
        directory.join(format!("combined-{width}.svg")),
        frame_svg::svg(&crop),
    )?;
    Ok(())
}

fn footer(width: u16) -> Result<StatusLineText> {
    // Display-only fixture; the CLI resume witness proves these independently projected fields.
    let input = serde_json::json!({
        "model":{"display_name":"Luna"}, "effort":{"level":"high"},
        "workspace":{"current_dir":"/workspace/dev/agents/plexmaton"},
        "context_window":{"context_window_size":272000, "used_percentage":null,
            "total_input_tokens":24800,"total_output_tokens":3200,
            "current_usage":{"input_tokens":400,"cache_read_input_tokens":9600,"cache_creation_input_tokens":0}},
        "cost":{"total_cost_usd":0.024},
        "plexmaton":{"terminal":{"columns":width},"usage":{"coverage":"complete"},
            "context":{"availability":"unavailable","reason":"history_incompatible"},
            "latest_request":{"terminal":{"usage":{"counts":{"input":10000}}}}}
    });
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/statusline-pastel.sh");
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
        .write_all(&serde_json::to_vec(&input)?);
    let result = child.wait_with_output()?;
    write?;
    if !result.status.success() {
        return Err("fixture footer failed".into());
    }
    Ok(StatusLineText::parse(&result.stdout)?)
}
