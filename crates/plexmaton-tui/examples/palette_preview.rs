//! Offline palette review: the workspace, the Drawer and an approval, in the designed palette
//! beside the terminal-resolved ANSI one it replaced.
//! cargo run -p plexmaton-tui --example palette_preview -- <output-directory>
use plexmaton_core::{
    AgentId, ApprovalId, ApprovalReason, AttentionId, AttentionRequest, ConversationEvent,
    ConversationEventEnvelope, EventSequence, ToolCallId, ToolCapability, TranscriptItemId,
    TranscriptRole,
};
use plexmaton_sim::{Scenario, ScriptedRuntime};
use plexmaton_tui::{MarkdownTheme, Palette, SurfaceId, Workspace};
use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::Buffer,
    crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers},
    style::Color,
};
use std::{fmt::Write as _, path::Path};

#[path = "support/frame_svg.rs"]
mod frame_svg;
#[path = "support/prepared_frame.rs"]
mod prepared_frame;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const MARKDOWN: &str = "## What changed\n\nThe Drawer now opens from the **top edge** at full width. Run `cargo test -p plexmaton-tui` before continuing, then read the [design notes](https://example.com/notes).\n\n- Configuration, conversations and permissions are *pages*\n- `Escape` returns one layer per press\n\n```rust\nfn main() {\n    println!(\"hello, 世界\");\n}\n```";

/// The sixteen slots as the user's terminal resolves them: kitty, `current-theme.conf`.
fn kitty(color: Color, reset: Color) -> Color {
    let rgb = |v: u32| Color::Rgb((v >> 16) as u8, (v >> 8 & 0xff) as u8, (v & 0xff) as u8);
    match color {
        Color::Black => rgb(0x86_86_86),
        Color::DarkGray => rgb(0x54_54_54),
        Color::Red => rgb(0xff_66_00),
        Color::LightRed => rgb(0xff_00_00),
        Color::Green => rgb(0xcc_ff_04),
        Color::LightGreen => rgb(0x00_ff_00),
        Color::Yellow => rgb(0xff_cc_00),
        Color::LightYellow => rgb(0xff_ff_00),
        Color::Blue | Color::Cyan => rgb(0x44_b3_cc),
        Color::LightBlue => rgb(0x00_00_ff),
        Color::Magenta => rgb(0x99_33_cc),
        Color::LightMagenta => rgb(0xff_00_ff),
        Color::LightCyan => rgb(0x00_ff_ff),
        Color::Gray => rgb(0xf4_f4_f4),
        Color::White => rgb(0xe5_e5_e5),
        Color::Reset => reset,
        other => other,
    }
}

fn main() -> Result<()> {
    let directory = std::env::args()
        .nth(1)
        .ok_or("provide an output directory")?;
    let directory = Path::new(&directory);
    std::fs::create_dir_all(directory)?;
    let variants: [(&str, &str, Palette); 2] = [
        (
            "current",
            "what ran before: ANSI slots resolved by your kitty theme on black, Catppuccin Markdown",
            Palette::ansi().with_markdown_theme(MarkdownTheme::Pastel),
        ),
        (
            "designed",
            "the new default on truecolor terminals: your named tokens. Sky where you are, teal working, mint done, orange needs you, coral failed, violet who speaks, gold what Enter acts on",
            Palette::pastel(),
        ),
    ];
    let frames = ["workspace", "drawer", "approval"];
    let mut index = String::from(
        r#"<!doctype html><meta charset=utf-8><title>Plexmaton · palette</title>
<style>body{background:#000;color:#e6e9f0;font:14px/1.4 -apple-system,system-ui,sans-serif;margin:2rem}
h1{font-weight:600}h2{font-size:15px;margin:2.5rem 0 .25rem}.what{color:#8ea2c4;margin:0 0 .75rem;max-width:90ch}
.row{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:1rem}.row svg{width:100%;height:auto;border:1px solid #1c2233;border-radius:6px}</style>
<h1>Palette</h1>
"#,
    );
    for (name, what, palette) in variants {
        writeln!(
            index,
            "<h2>{name}</h2><p class=what>{what}</p><div class=row>"
        )?;
        for frame in frames {
            let mut buffer = render(palette, frame)?;
            for cell in &mut buffer.content {
                cell.fg = kitty(cell.fg, Color::Rgb(0xff, 0xff, 0xff));
                cell.bg = kitty(cell.bg, Color::Rgb(0x00, 0x00, 0x00));
            }
            // The user's terminal ground is pure black, not the helper's navy.
            let svg = frame_svg::svg(&buffer).replacen(
                &format!("fill=\"{}\"", frame_svg::BACKGROUND),
                "fill=\"#000000\"",
                1,
            );
            std::fs::write(directory.join(format!("{name}-{frame}.svg")), &svg)?;
            index.push_str(&svg);
        }
        index.push_str("</div>");
    }
    std::fs::write(directory.join("index.html"), index)?;
    Ok(())
}

fn render(palette: Palette, frame: &str) -> Result<Buffer> {
    let mut workspace = Workspace::with_palette(palette);
    workspace.set_model(plexmaton_tui::ConfigurationSummary {
        configured_name: "fixture".into(),
        provider: "local".into(),
        model: "plexmaton-dev".into(),
        reasoning_effort: plexmaton_core::ReasoningEffort::High,
    });
    let mut events = ScriptedRuntime::new(Scenario::canonical()?).ready(u64::MAX);
    let next = events.len() as u64 + 1;
    let agent = events
        .iter()
        .find_map(|envelope| match &envelope.event {
            ConversationEvent::AgentCreated { agent_id, .. } => Some(agent_id.clone()),
            _ => None,
        })
        .unwrap_or(AgentId::new("primary")?);
    let item = TranscriptItemId::new("palette-answer")?;
    events.push(ConversationEventEnvelope {
        sequence: EventSequence::new(next),
        event: ConversationEvent::TranscriptItemStarted {
            agent_id: agent.clone(),
            item_id: item.clone(),
            role: TranscriptRole::Assistant,
        },
    });
    events.push(ConversationEventEnvelope {
        sequence: EventSequence::new(next + 1),
        event: ConversationEvent::TranscriptDelta {
            agent_id: agent,
            item_id: item,
            item_revision: 1,
            text: MARKDOWN.into(),
        },
    });
    // A waiting approval from the worker, so the card's action-required and chosen-choice colours
    // are on screen (the same request the approval fixtures use).
    events.push(ConversationEventEnvelope {
        sequence: EventSequence::new(next + 2),
        event: ConversationEvent::AttentionRequested {
            agent_id: AgentId::new("agent-b")?,
            attention_id: AttentionId::new("attention-b-approval")?,
            request: AttentionRequest::Approval {
                reason: ApprovalReason::PermissionRequired,
                remember: None,
                approval_id: ApprovalId::new("approval-b-1")?,
                call_id: ToolCallId::new("tool-b-write")?,
                tool: "edit".to_owned(),
                capabilities: vec![ToolCapability::FileWrite],
                detail:
                    "Change crates/plexmaton-core/src/lib.rs and preserve its current revision."
                        .to_owned(),
            },
        },
    });
    workspace.emit(events);
    let mut terminal = Terminal::new(TestBackend::new(95, 40))?;
    prepared_frame::draw(&mut workspace, &mut terminal)?;
    let key = |code: KeyCode, modifiers: KeyModifiers| Event::Key(KeyEvent::new(code, modifiers));
    match frame {
        "drawer" => {
            workspace.handle(&key(KeyCode::Char('p'), KeyModifiers::CONTROL));
            prepared_frame::draw(&mut workspace, &mut terminal)?;
            workspace.handle(&key(KeyCode::Down, KeyModifiers::NONE));
        }
        "approval" => {
            for _ in 0..8 {
                if workspace.state().focused(workspace.surfaces()) == Some(SurfaceId::Attention) {
                    break;
                }
                workspace.handle(&key(KeyCode::Tab, KeyModifiers::NONE));
                prepared_frame::draw(&mut workspace, &mut terminal)?;
            }
            workspace.handle(&key(KeyCode::Down, KeyModifiers::NONE));
            prepared_frame::draw(&mut workspace, &mut terminal)?;
            workspace.handle(&key(KeyCode::Enter, KeyModifiers::NONE));
        }
        _ => {}
    }
    prepared_frame::draw(&mut workspace, &mut terminal)?;
    Ok(terminal.backend().buffer().clone())
}
