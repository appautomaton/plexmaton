//! Approval choices and exact command inspection through the production router and renderer.
use plexmaton_core::{
    AgentId, AgentStatus, ApprovalId, AttentionId, AttentionRequest, ConversationEvent,
    ConversationEventEnvelope, EventSequence, ToolCallId, ToolCallStatus, ToolCapability,
    ToolDetail, ToolPresentation, TranscriptItemId,
};
use plexmaton_tui::{Palette, Workspace};
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{
        Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    },
};
use std::path::Path;
use unicode_width::UnicodeWidthStr as _;
#[path = "support/frame_svg.rs"]
mod frame_svg;
#[path = "support/prepared_frame.rs"]
mod prepared_frame;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const COMMAND: &str = "set -o pipefail\npayload='{\"model\":\"example-model\",\"messages\":[{\"role\":\"user\",\"content\":\"Check the local development server\"}]}'\ncurl --fail --silent \\\n  --header 'Content-Type: application/json' \\\n  --data \"$payload\" \\\n  http://localhost:8080/v1/responses";
fn main() -> Result<()> {
    let directory = std::env::args()
        .nth(1)
        .ok_or("provide an output directory")?;
    let directory = Path::new(&directory);
    std::fs::create_dir_all(directory)?;
    for (width, height) in [(120, 32), (88, 32), (60, 30)] {
        let mut workspace = fixture()?;
        let mut terminal = Terminal::new(TestBackend::new(width, height))?;
        prepared_frame::draw(&mut workspace, &mut terminal)?;
        let at = point(&terminal, "1. Allow once")?;
        workspace.handle(&pointer(MouseEventKind::Moved, at));
        prepared_frame::draw(&mut workspace, &mut terminal)?;
        save(directory, "choices", &terminal)?;
        let summary = point(&terminal, "Command ")?;
        workspace.handle(&pointer(MouseEventKind::Down(MouseButton::Left), summary));
        workspace.handle(&pointer(MouseEventKind::Up(MouseButton::Left), summary));
        prepared_frame::draw(&mut workspace, &mut terminal)?;
        let copied = workspace.handle(&Event::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::NONE,
        )));
        if copied.copied.as_ref().map(|copy| copy.text.as_str()) != Some(COMMAND) {
            return Err("inspection copy changed shell source".into());
        }
        save(directory, "command", &terminal)?;
        workspace.handle(&Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        prepared_frame::draw(&mut workspace, &mut terminal)?;
    }
    Ok(())
}
fn fixture() -> Result<Workspace> {
    let mut workspace = Workspace::with_palette(Palette::pastel());
    let agent = AgentId::new("main")?;
    let call = ToolCallId::new("command-1")?;
    let item = TranscriptItemId::new("command-entry")?;
    let presentation = ToolPresentation {
        invocation: Some(ToolDetail::Command(Box::new(
            plexmaton_core::CommandInvocation {
                source: COMMAND.into(),
                workspace_root: "/workspace/plexmaton".into(),
                timeout_ms: 120_000,
            },
        ))),
        outcome: None,
    };
    let events = [
        ConversationEvent::AgentCreated {
            agent_id: agent.clone(),
            label: "Plexmaton".into(),
            status: AgentStatus::Waiting,
        },
        ConversationEvent::ToolCallChanged {
            agent_id: agent.clone(),
            item_id: item.clone(),
            item_revision: 0,
            call_id: call.clone(),
            label: "exec_command".into(),
            status: ToolCallStatus::Queued,
            presentation: presentation.clone(),
        },
        ConversationEvent::ToolCallChanged {
            agent_id: agent.clone(),
            item_id: item,
            item_revision: 1,
            call_id: call.clone(),
            label: "exec_command".into(),
            status: ToolCallStatus::AwaitingApproval,
            presentation,
        },
        ConversationEvent::AttentionRequested {
            agent_id: agent,
            attention_id: AttentionId::new("attention-1")?,
            request: AttentionRequest::Approval {
                reason: plexmaton_core::ApprovalReason::CommandExecution,
                remember: Some(plexmaton_core::RememberPermissionOffer {
                    id: plexmaton_core::PermissionOfferId::new(1),
                    label: "this exact command in this checkout".into(),
                    note: None,
                    scopes: plexmaton_core::PermissionScopes::SessionAndProject,
                }),
                approval_id: ApprovalId::new("approval-1")?,
                call_id: call,
                tool: "exec_command".into(),
                capabilities: vec![ToolCapability::ProcessSpawn],
                detail: format!("Command {COMMAND:?} · cwd /workspace/plexmaton"),
            },
        },
    ];
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
    Ok(workspace)
}
fn point(terminal: &Terminal<TestBackend>, text: &str) -> Result<(u16, u16)> {
    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        let row: String = (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect();
        if let Some(offset) = row.find(text) {
            return Ok((row[..offset].width() as u16, y));
        }
    }
    Err(format!("missing {text}").into())
}
fn pointer(kind: MouseEventKind, at: (u16, u16)) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column: at.0,
        row: at.1,
        modifiers: KeyModifiers::NONE,
    })
}
fn save(directory: &Path, kind: &str, terminal: &Terminal<TestBackend>) -> Result<()> {
    let buffer = terminal.backend().buffer();
    std::fs::write(
        directory.join(format!("{kind}-{}.svg", buffer.area.width)),
        frame_svg::svg(buffer),
    )?;
    Ok(())
}
