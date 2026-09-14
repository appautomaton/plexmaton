//! Explicit synthetic control snapshots for native UI review; no durable Handoff is performed.

use plexmaton_core::{
    AgentId, AgentStatus, ConversationEvent, ConversationEventEnvelope, EventSequence,
};
use plexmaton_sim::{Scenario, ScriptedRuntime};
use plexmaton_tui::{ChildControl, ChildControlSnapshot, Palette, Workspace};
use ratatui::crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Copy)]
enum Stage {
    MainRunning,
    MainIdle,
    HandoffPending,
    UserIdle,
}

pub struct ControlDemo {
    stage: Stage,
    revision: u64,
    sequence: u64,
    child: AgentId,
}

impl ControlDemo {
    pub fn start() -> Result<(Self, Workspace)> {
        let mut workspace = Workspace::with_palette(Palette::pastel());
        let mut events = ScriptedRuntime::new(Scenario::canonical()?).ready(u64::MAX);
        // Attention ownership is not this review's subject. Keep the same layout engine while
        // choosing a fixture with no action-required request, rather than inventing its source.
        events.retain(|envelope| {
            !matches!(envelope.event, ConversationEvent::AttentionRequested { .. })
        });
        for (index, envelope) in events.iter_mut().enumerate() {
            envelope.sequence = EventSequence::new(index as u64 + 1);
            if let ConversationEvent::AgentCreated {
                agent_id, label, ..
            } = &mut envelope.event
            {
                *label = if agent_id.as_str() == "agent-a" {
                    "Main"
                } else {
                    "Luna / max"
                }
                .into();
            }
        }
        let sequence = events.len() as u64;
        workspace.emit(events);
        let mut demo = Self {
            stage: Stage::MainRunning,
            revision: 0,
            sequence,
            child: AgentId::new("agent-b")?,
        };
        demo.publish(&mut workspace)?;
        Ok((demo, workspace))
    }

    pub fn advance(&mut self, workspace: &mut Workspace, event: &Event) -> Result<bool> {
        if !matches!(event, Event::Key(key) if key.code == KeyCode::F(6)
            && key.modifiers == KeyModifiers::NONE && key.kind == KeyEventKind::Press)
        {
            return Ok(false);
        }
        self.stage = match self.stage {
            Stage::MainRunning => Stage::MainIdle,
            Stage::MainIdle => Stage::HandoffPending,
            Stage::HandoffPending => Stage::UserIdle,
            // Do not portray User -> Main as an implicit reversible handoff.
            Stage::UserIdle => return Ok(true),
        };
        self.publish(workspace)?;
        Ok(true)
    }

    fn publish(&mut self, workspace: &mut Workspace) -> Result<()> {
        let (control, status, next) = match self.stage {
            Stage::MainRunning => (ChildControl::Main, AgentStatus::Running, "F6: idle"),
            Stage::MainIdle => (ChildControl::Main, AgentStatus::Idle, "F6: pending"),
            Stage::HandoffPending => (
                ChildControl::HandoffPending,
                AgentStatus::Idle,
                "F6: acknowledge",
            ),
            Stage::UserIdle => (
                ChildControl::User,
                AgentStatus::Idle,
                "Handoff acknowledged",
            ),
        };
        self.revision += 1;
        self.sequence += 1;
        workspace.emit(vec![ConversationEventEnvelope {
            sequence: EventSequence::new(self.sequence),
            event: ConversationEvent::AgentStatusChanged {
                agent_id: self.child.clone(),
                status,
            },
        }]);
        workspace.set_child_control(
            &self.child,
            ChildControlSnapshot {
                revision: self.revision,
                control,
            },
        )?;
        workspace.set_working_directory(format!("UI fixture | {next} | ^D twice: exit"));
        Ok(())
    }
}
