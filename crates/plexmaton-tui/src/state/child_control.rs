//! CCV-1–CCV-4: controller snapshots are presentation facts, never runtime authority.

use plexmaton_core::AgentId;
use thiserror::Error;

use super::ViewState;

/// A V1 delegated conversation's acknowledged control state.
///
/// Pending retains Main control. Only the durable owner's acknowledged snapshot may report User;
/// the TUI never derives it from lifecycle, interruption or closing a surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildControl {
    Main,
    HandoffPending,
    User,
}

/// An immutable projection of one child's collaboration owner, not an execution capability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChildControlSnapshot {
    pub revision: u64,
    pub control: ChildControl,
}

/// Refusal leaves the previous controller and all interaction state intact (CCV-1).
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ChildControlRefusal {
    #[error("child control cannot replace the primary controller")]
    Primary,
    #[error("unknown child: {0}")]
    UnknownChild(AgentId),
    #[error("stale child control revision")]
    Stale,
    #[error("conflicting child control at the same revision")]
    Conflict,
}

impl ViewState {
    pub(crate) fn finish_child_input_drag(&mut self, child: &AgentId) {
        if let Some(input) = self.inputs.get_mut(child) {
            let _ = input.finish_selection();
        }
    }

    pub(crate) fn set_child_control(
        &mut self,
        child: &AgentId,
        snapshot: ChildControlSnapshot,
    ) -> Result<bool, ChildControlRefusal> {
        if self
            .primary_agent()
            .is_some_and(|primary| primary.id == *child)
        {
            return Err(ChildControlRefusal::Primary);
        }
        let agent = self
            .agents
            .get_mut(child)
            .map_err(|_| ChildControlRefusal::UnknownChild(child.clone()))?;
        if let Some(previous) = agent.control {
            if snapshot.revision < previous.revision {
                return Err(ChildControlRefusal::Stale);
            }
            if snapshot.revision == previous.revision {
                return if snapshot == previous {
                    Ok(false)
                } else {
                    Err(ChildControlRefusal::Conflict)
                };
            }
        }
        let changed = agent
            .control
            .is_none_or(|previous| previous.control != snapshot.control);
        agent.control = Some(snapshot);
        if changed {
            self.touch();
        }
        Ok(changed)
    }
}
