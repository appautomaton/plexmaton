//! Connects one process owner to workspace generations without giving it terminal authority.

use std::path::PathBuf;

use plexmaton_tui::{
    Workspace,
    preparation::{Refusal as TextRefusal, Token},
};

use super::{Completion, Failure, Preparation, Refusal, Ticket};

/// The live/measurement adapter: process tickets never stand in for workspace-local identities.
pub struct LivePreparation {
    process: Preparation,
    active: Option<(Ticket, Token)>,
}

impl LivePreparation {
    /// The real executable or an explicit external-process fixture; no ambient configuration.
    pub fn new(executable: PathBuf) -> Self {
        Self {
            process: Preparation::new(executable),
            active: None,
        }
    }

    /// Starts or replaces bounded work declared by the last frame. Returns true when admission
    /// itself produced visible feedback, so the loop can repaint without waiting for an event.
    pub fn sync(&mut self, workspace: &mut Workspace) -> bool {
        let dirty = workspace.needs_draw();
        if let Some(work) = workspace.take_preparation() {
            match self.process.submit(work.requests) {
                Ok(ticket) => self.active = Some((ticket, work.token)),
                Err(error) => {
                    let reason =
                        if matches!(error, Failure::Protocol(super::ProtocolError::Capacity)) {
                            TextRefusal::Capacity
                        } else {
                            TextRefusal::Unavailable
                        };
                    workspace.fail_preparation(work.token, reason);
                }
            }
        }
        if self
            .active
            .as_ref()
            .is_some_and(|(_, token)| !workspace.owns_preparation(token))
        {
            self.process.cancel();
            self.active = None;
        }
        !dirty && workspace.needs_draw()
    }

    /// Poll beside input; the retained process future preserves partial pipe progress.
    pub async fn next(&mut self) -> Completion {
        self.process.next().await
    }

    /// Whether a workspace request awaits completion; an idle persistent child is not pending work.
    pub fn is_pending(&self) -> bool {
        self.active.is_some()
    }

    /// Old tickets and replaced workspace generations cannot attach data or trigger copied text.
    pub fn apply(&mut self, completion: Completion, workspace: &mut Workspace) {
        if matches!(
            completion,
            Completion::Failed(_, Failure::Cleanup(_) | Failure::Unavailable)
        ) {
            // A failed cleanup quarantines the process, including its latest pending batch. The
            // failure may name an older ticket; leaving the newer token pending would hang it.
            if let Some((_, token)) = self.active.take() {
                workspace.fail_preparation(token, TextRefusal::Unavailable);
            }
            return;
        }
        let ticket = match &completion {
            Completion::Ready(ticket, _) | Completion::Cancelled(ticket) => Some(*ticket),
            Completion::Failed(ticket, _) => *ticket,
        };
        if ticket.is_none() || self.active.as_ref().map(|(active, _)| *active) != ticket {
            return;
        }
        let (_, token) = self
            .active
            .take()
            .expect("matching ticket has an owned workspace token");
        match completion {
            Completion::Ready(_, Ok(results)) => {
                workspace.complete_preparation(token, results);
            }
            Completion::Ready(_, Err(Refusal::Capacity)) => {
                workspace.fail_preparation(token, TextRefusal::Capacity)
            }
            Completion::Failed(_, _) | Completion::Cancelled(_) => {
                workspace.fail_preparation(token, TextRefusal::Unavailable);
            }
        }
    }

    /// Call on every loop exit before terminal restoration, including error paths.
    pub async fn shutdown(&mut self) -> Result<(), Failure> {
        self.active = None;
        self.process.shutdown().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexmaton_core::{
        AgentId, AgentStatus, EventSequence, SessionEvent, SessionEventEnvelope, TranscriptItemId,
        TranscriptRole,
    };
    use ratatui::{Terminal, backend::TestBackend};

    /// PRE-2/PRE-3: quarantining an older process operation must settle its latest queued token,
    /// not leave a current workspace permanently waiting for work that can no longer start.
    #[tokio::test]
    async fn cleanup_failure_on_an_old_ticket_settles_the_latest_workspace_request() {
        let mut workspace = Workspace::default();
        let agent = AgentId::new("primary").expect("agent");
        let item = TranscriptItemId::new("text").expect("item");
        let mut sequence = 0;
        let mut emit = |workspace: &mut Workspace, event| {
            sequence += 1;
            workspace.emit(vec![SessionEventEnvelope {
                sequence: EventSequence::new(sequence),
                event,
            }]);
        };
        emit(
            &mut workspace,
            SessionEvent::AgentCreated {
                agent_id: agent.clone(),
                label: "Plexmaton".into(),
                status: AgentStatus::Running,
            },
        );
        emit(
            &mut workspace,
            SessionEvent::TranscriptItemStarted {
                agent_id: agent.clone(),
                item_id: item.clone(),
                role: TranscriptRole::Assistant,
            },
        );
        emit(
            &mut workspace,
            SessionEvent::TranscriptDelta {
                agent_id: agent.clone(),
                item_id: item.clone(),
                item_revision: 1,
                text: "**first**".into(),
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(88, 24)).expect("terminal");
        let mut owner = LivePreparation::new("/must-not-start-a-process".into());
        workspace.draw(&mut terminal).expect("first pending");
        owner.sync(&mut workspace);
        let old_ticket = owner.active.as_ref().expect("first ticket").0;
        emit(
            &mut workspace,
            SessionEvent::TranscriptDelta {
                agent_id: agent,
                item_id: item,
                item_revision: 2,
                text: " second".into(),
            },
        );
        workspace.draw(&mut terminal).expect("replacement pending");
        owner.sync(&mut workspace);
        assert_ne!(owner.active.as_ref().expect("new ticket").0, old_ticket);
        owner.apply(
            Completion::Failed(
                Some(old_ticket),
                Failure::Cleanup(std::io::Error::other("injected cleanup refusal")),
            ),
            &mut workspace,
        );
        owner
            .shutdown()
            .await
            .expect("unpolled process is cancelled without spawning");
        assert!(!owner.is_pending());
        workspace.draw(&mut terminal).expect("failure frame");
        assert!(workspace.take_preparation().is_none());
        assert!(!workspace.needs_draw());
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("Text unavailable"));
    }
}
