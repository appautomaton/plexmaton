//! Interactive, offline fixture for the roster, rendered directly by the terminal.
//!
//! The canonical fixture has one sub-agent, which shows that the roster exists but not what it is
//! for. Several agents in different states at once is the case the panel's ordering, its ruled
//! break and its colour hierarchy are the answer to, so that is what this starts in.
//!
//! `Ctrl-B` puts the panel away and brings it back; `↑`/`↓` move the selection and `Enter` enters
//! an agent, which for one that is asking is also going to its request. `Ctrl-D` twice exits.
//! Resize the window to watch the panel move from a column to a shelf over the conversation.

use std::{
    io,
    time::{Duration, Instant},
};

use plexmaton_core::{
    AgentId, AgentStatus, ApprovalId, ApprovalReason, AttentionId, AttentionRequest,
    ConversationEvent, ConversationEventEnvelope, EventSequence, ToolCallId, ToolCapability,
};
use plexmaton_tui::{Flow, SurfaceId, Workspace};
use ratatui::{
    DefaultTerminal,
    crossterm::{
        event::{
            self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture,
            EnableBracketedPaste, EnableFocusChange, EnableMouseCapture, Event, KeyCode, KeyEvent,
            KeyModifiers,
        },
        execute,
    },
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[path = "support/native_effects.rs"]
mod native_effects;

fn restore() {
    let _ = execute!(
        io::stdout(),
        DisableBracketedPaste,
        DisableFocusChange,
        DisableMouseCapture
    );
    ratatui::restore();
}

/// One agent of each class the hierarchy distinguishes, created out of order on purpose: the panel
/// sorts by attention rather than by arrival, and a fixture already in the right order would prove
/// that by accident.
///
/// Labels are short because the roster's row is not where full nouns go — the conversation title
/// is (ui-ux §responsive layout classes). A long one crowds the lifecycle word onto the second row
/// and takes the detail line with it, which is the degradation rather than the design.
const ROSTER: [(&str, &str, AgentStatus); 5] = [
    ("orion", "Orion / low", AgentStatus::Running),
    ("sol", "Sol / high", AgentStatus::Running),
    ("iris", "Iris / max", AgentStatus::Idle),
    ("vega", "Vega / max", AgentStatus::Running),
    ("luna", "Luna / max", AgentStatus::Running),
];

fn agent(name: &str) -> Result<AgentId> {
    Ok(AgentId::new(name)?)
}

fn fixture() -> Result<Workspace> {
    let mut workspace = Workspace::default();
    let mut sequence = 0_u64;
    let mut events: Vec<ConversationEvent> = Vec::new();

    events.push(ConversationEvent::AgentCreated {
        agent_id: agent("main")?,
        label: "Plexmaton".to_owned(),
        status: AgentStatus::Running,
    });
    for (id, label, status) in ROSTER {
        events.push(ConversationEvent::AgentCreated {
            agent_id: agent(id)?,
            label: label.to_owned(),
            status,
        });
    }
    // Failure outranks a request, which outranks work. All three are on screen at once so the
    // ordering has something to order and the colours have something to separate.
    events.push(ConversationEvent::AgentStatusChanged {
        agent_id: agent("sol")?,
        status: AgentStatus::Failed,
    });
    events.push(ConversationEvent::AttentionRequested {
        agent_id: agent("vega")?,
        attention_id: AttentionId::new("vega-write")?,
        request: AttentionRequest::Approval {
            reason: ApprovalReason::PermissionRequired,
            remember: None,
            approval_id: ApprovalId::new("approval-vega-1")?,
            call_id: ToolCallId::new("tool-vega-write")?,
            tool: "edit".to_owned(),
            capabilities: vec![ToolCapability::FileWrite],
            detail: "Write migrations/0007_collaboration.sql and keep its current revision."
                .to_owned(),
        },
    });
    events.push(ConversationEvent::AttentionRequested {
        agent_id: agent("luna")?,
        attention_id: AttentionId::new("luna-scope")?,
        request: AttentionRequest::Clarification {
            summary: "Should the overlap study cover narrow screens too?".to_owned(),
        },
    });

    workspace.emit(
        events
            .into_iter()
            .map(|event| {
                sequence = sequence.saturating_add(1);
                ConversationEventEnvelope {
                    sequence: EventSequence::new(sequence),
                    event,
                }
            })
            .collect::<Vec<_>>(),
    );
    Ok(workspace)
}

fn main() -> Result<()> {
    let mut workspace = fixture()?;
    let mut terminal = match ratatui::try_init() {
        Ok(terminal) => terminal,
        Err(error) => {
            restore();
            return Err(error.into());
        }
    };
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        previous(info);
    }));
    let result = run(&mut workspace, &mut terminal);
    restore();
    result
}

fn run(workspace: &mut Workspace, terminal: &mut DefaultTerminal) -> Result<()> {
    execute!(
        io::stdout(),
        EnableBracketedPaste,
        EnableFocusChange,
        EnableMouseCapture
    )?;
    draw(workspace, terminal)?;
    focus(workspace, terminal, SurfaceId::Agents)?;
    loop {
        draw(workspace, terminal)?;
        if event::poll(Duration::from_millis(50))?
            && native_effects::handle(workspace, &event::read()?) == Flow::Quit
        {
            return Ok(());
        }
        workspace.expire_note(Instant::now());
    }
}

fn focus(
    workspace: &mut Workspace,
    terminal: &mut DefaultTerminal,
    target: SurfaceId,
) -> Result<()> {
    for _ in 0..=workspace.surfaces().len() {
        if workspace.state().focused(workspace.surfaces()) == Some(target) {
            return Ok(());
        }
        workspace.handle(&Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
        draw(workspace, terminal)?;
    }
    Err("preview focus target is unavailable".into())
}

fn draw(workspace: &mut Workspace, terminal: &mut DefaultTerminal) -> Result<()> {
    for _ in 0..1024 {
        workspace.draw(terminal)?;
        if let Some(work) = workspace.take_preparation() {
            match plexmaton_tui::preparation::prepare_batch(&work.requests) {
                Ok(prepared) => {
                    if !workspace.complete_preparation(work.token, prepared) {
                        return Err("preview preparation was rejected".into());
                    }
                }
                Err(plexmaton_tui::preparation::BatchRefusal::Capacity) => workspace
                    .fail_preparation(work.token, plexmaton_tui::preparation::Refusal::Capacity),
            }
        } else if !workspace.needs_draw() {
            return Ok(());
        }
    }
    Err("preview preparation did not settle".into())
}
