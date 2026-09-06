//! Diagnostic of the pinned TUI, not a regression contract endorsing the observed defects.
//! Run as a temporary plexmaton-tui example; approval-flow.md owns the command and interpretation.

use std::{error::Error, path::Path};

use plexmaton_core::{
    AgentId, AgentStatus, ApprovalId, AttentionId, AttentionRequest, EventSequence, ConversationEvent,
    ConversationEventEnvelope, ToolCallId, ToolCapability,
};
use plexmaton_tui::{SurfaceId, Workspace};
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{
        Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    },
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn agent(name: &str) -> AgentId {
    AgentId::new(name).expect("bounded fixture identity")
}

fn key(workspace: &mut Workspace, code: KeyCode) -> plexmaton_tui::Outcome {
    workspace.handle(&Event::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}

fn click(workspace: &mut Workspace, surface: SurfaceId) {
    let bounds = workspace
        .surfaces()
        .get(surface)
        .expect("drawn surface")
        .bounds;
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        workspace.handle(&Event::Mouse(MouseEvent {
            kind,
            column: bounds.x + 2,
            row: bounds.y + 1,
            modifiers: KeyModifiers::NONE,
        }));
    }
}

fn emit(workspace: &mut Workspace, sequence: &mut u64, event: ConversationEvent) {
    workspace.emit(vec![ConversationEventEnvelope {
        sequence: EventSequence::new(*sequence),
        event,
    }]);
    *sequence += 1;
}

fn request(owner: &str) -> ConversationEvent {
    ConversationEvent::AttentionRequested {
        agent_id: agent(owner),
        attention_id: AttentionId::new(format!("attention-{owner}")).expect("fixture identity"),
        request: AttentionRequest::Approval {
            remember: None,
            approval_id: ApprovalId::new(format!("approval-{owner}")).expect("fixture identity"),
            call_id: ToolCallId::new(format!("call-{owner}")).expect("fixture identity"),
            tool: "edit_file".into(),
            capabilities: vec![ToolCapability::FileWrite],
            detail: format!("Edit src/{owner}.rs with the admitted changes"),
        },
    }
}

fn save_frame(terminal: &Terminal<TestBackend>, destination: &Path) -> Result<()> {
    let buffer = terminal.backend().buffer();
    let mut frame = String::new();
    for y in 0..buffer.area.height {
        let line = (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>();
        frame.push_str(line.trim_end());
        frame.push('\n');
    }
    std::fs::write(destination, frame)?;
    Ok(())
}

fn observe(width: u16, label: &str, destination: &Path) -> Result<()> {
    let mut workspace = Workspace::default();
    let mut terminal = Terminal::new(TestBackend::new(width, 24))?;
    let mut sequence = 1;
    for (owner, name) in [("primary", "Plexmaton"), ("worker", "Worker B")] {
        emit(
            &mut workspace,
            &mut sequence,
            ConversationEvent::AgentCreated {
                agent_id: agent(owner),
                label: name.into(),
                status: AgentStatus::Waiting,
            },
        );
    }
    workspace.return_input(agent("primary"), "Keep this draft".into());
    emit(&mut workspace, &mut sequence, request("primary"));
    workspace.draw(&mut terminal)?;
    assert_eq!(
        workspace.state().focused(workspace.surfaces()),
        Some(SurfaceId::Approval)
    );

    // APV-4/ATT-3 distinguish dispatch from producer resolution. Current presentation has no
    // submitted state: repeated Enter produces duplicate answers before any resolution arrives.
    key(&mut workspace, KeyCode::Up);
    let first = key(&mut workspace, KeyCode::Enter)
        .approval
        .expect("first submission");
    let duplicate = key(&mut workspace, KeyCode::Enter)
        .approval
        .expect("duplicate submission");
    assert_eq!(first, duplicate);
    assert!(workspace.state().approval().is_some());

    emit(&mut workspace, &mut sequence, request("worker"));
    workspace.draw(&mut terminal)?;
    key(&mut workspace, KeyCode::Esc);
    workspace.draw(&mut terminal)?;
    click(&mut workspace, SurfaceId::Attention);
    workspace.draw(&mut terminal)?;
    let listed: Vec<_> = workspace
        .state()
        .attention_listed()
        .map(|item| item.agent_id.clone())
        .collect();
    assert_eq!(listed, vec![agent("worker")]);
    save_frame(
        &terminal,
        &destination.join(format!("attention-cursor-{label}.txt")),
    )?;

    // ATT-2/INV-10 gap: the only listed row names Worker B, but Enter visits hidden primary.
    key(&mut workspace, KeyCode::Enter);
    assert_eq!(
        workspace
            .state()
            .approval()
            .expect("opened request")
            .agent_id,
        &agent("primary")
    );

    // Reach Worker B by advancing the full queue's hidden cursor, then dismiss its modal.
    key(&mut workspace, KeyCode::Esc);
    workspace.draw(&mut terminal)?;
    click(&mut workspace, SurfaceId::Attention);
    key(&mut workspace, KeyCode::Down);
    key(&mut workspace, KeyCode::Enter);
    workspace.draw(&mut terminal)?;
    assert_eq!(
        workspace
            .state()
            .approval()
            .expect("worker request")
            .agent_id,
        &agent("worker")
    );
    key(&mut workspace, KeyCode::Esc);
    workspace.draw(&mut terminal)?;

    // ATT-1 gap: dismissing the worker leaves the primary pending, unlisted and without a card.
    // No producer event has resolved either request. Only another qualifying transition revives it.
    assert!(workspace.state().approval().is_none());
    assert_eq!(workspace.state().attention_count(), 2);
    assert!(
        workspace
            .state()
            .attention()
            .any(|item| item.agent_id == agent("primary"))
    );
    assert!(
        workspace
            .state()
            .attention_listed()
            .all(|item| item.agent_id != agent("primary"))
    );
    assert!(workspace.surfaces().get(SurfaceId::Approval).is_none());
    assert_eq!(workspace.state().composer().text(), "Keep this draft");
    save_frame(
        &terminal,
        &destination.join(format!("displaced-primary-{label}.txt")),
    )?;
    println!(
        "{label} ({width}x24): duplicate submission, hidden cursor and displaced primary reproduced"
    );
    Ok(())
}

fn main() -> Result<()> {
    let destination = std::env::args_os()
        .nth(1)
        .ok_or("frame destination required")?;
    let destination = Path::new(&destination);
    std::fs::create_dir_all(destination)?;
    for (label, width) in [("wide", 120), ("medium", 95), ("narrow", 60)] {
        observe(width, label, destination)?;
    }
    Ok(())
}
