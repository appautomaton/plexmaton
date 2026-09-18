//! Which agents exist, and which one the user is looking at.

use plexmaton_core::{AgentId, AgentStatus};

use super::{AgentView, ReduceError, ordered::OrderedById};
use crate::{intent::Direction, surface::SurfaceId};

/// One visit to the narrow full-region agent navigator.
///
/// The cursor is deliberately separate from the selected conversation. Moving through the
/// navigator previews rows without replacing the conversation underneath it; only `Enter` or a
/// completed click commits that choice. `return_focus` puts the user back on the exact control
/// they left when the navigator is dismissed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RosterNavigation {
    pub(super) return_focus: SurfaceId,
    pub(super) cursor: Option<AgentId>,
}

/// The agents the workspace knows about, in the order they appeared.
///
/// Arrival order rather than identifier order, because the rail is a history of what happened and
/// re-sorting it under the user would move the row they were aiming at.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct Roster {
    agents: OrderedById<AgentId, AgentView>,
    selected: Option<AgentId>,
}

impl Roster {
    /// Iterates agents in arrival order.
    pub(super) fn iter(&self) -> impl Iterator<Item = &AgentView> {
        self.agents.iter()
    }

    /// The agent the composer addresses: the first one to appear, never the selected one (COM-4).
    pub(super) fn primary(&self) -> Option<&AgentView> {
        self.agents.iter().next()
    }

    /// The sub-agents, in arrival order: everyone but the primary, which is what the list holds.
    pub(super) fn sub_agents(&self) -> impl Iterator<Item = &AgentView> {
        self.agents.iter().skip(1)
    }

    /// The children a conversation switch would interrupt, as this roster names them.
    ///
    /// The roster answers rather than the owner because it is what the user is reading when they
    /// choose: the name in the sentence a switch offers is the name in the row above it (`ui-ux.md`
    /// one name per thing), and asking the owner would put a writer round trip inside a keystroke.
    ///
    /// `Waiting` counts — a child blocked on an approval still loses that turn. This can be one
    /// frame stale, and it errs toward offering the choice rather than taking the work silently:
    /// the Stop itself goes through the owner, which refuses a runner that is already gone. A child
    /// restored without being woken (CHB-3) is `Idle` and correctly asks nothing.
    pub(super) fn working(&self) -> impl Iterator<Item = &AgentView> {
        self.sub_agents()
            .filter(|agent| matches!(agent.status, AgentStatus::Running | AgentStatus::Waiting))
    }

    /// The sub-agent the user is looking at, if any.
    pub(super) fn selected(&self) -> Option<&AgentView> {
        self.selected.as_ref().and_then(|id| self.agents.get(id))
    }

    pub(super) fn selected_id(&self) -> Option<&AgentId> {
        self.selected.as_ref()
    }

    /// The agent the second window shows, which is the selected sub-agent.
    ///
    /// Selecting is looking. The primary's conversation is always on screen and is not in the
    /// list, so a selection is always a second agent and always opens their conversation over it.
    /// This is the whole of when a second window is open (INS-1); nothing stores it separately.
    pub(super) fn peeked(&self) -> Option<&AgentView> {
        let selected = self.selected()?;
        let is_primary = self
            .primary()
            .is_some_and(|primary| primary.id == selected.id);
        (!is_primary).then_some(selected)
    }

    pub(super) fn get(&self, agent_id: &AgentId) -> Option<&AgentView> {
        self.agents.get(agent_id)
    }

    pub(super) fn contains(&self, agent_id: &AgentId) -> bool {
        self.agents.contains(agent_id)
    }

    /// Adds a newly visible agent. Nothing is selected by arrival: a background agent never
    /// navigates on the user's behalf, and the primary is on screen without being selected.
    pub(super) fn add(
        &mut self,
        agent_id: AgentId,
        label: String,
        status: AgentStatus,
    ) -> Result<(), ReduceError> {
        if self.agents.contains(&agent_id) {
            return Err(ReduceError::DuplicateAgent(agent_id));
        }
        self.agents
            .upsert(agent_id.clone(), AgentView::new(agent_id, label, status));
        Ok(())
    }

    pub(super) fn get_mut(&mut self, agent_id: &AgentId) -> Result<&mut AgentView, ReduceError> {
        self.agents
            .get_mut(agent_id)
            .ok_or_else(|| ReduceError::UnknownAgent(agent_id.clone()))
    }

    /// Selects an existing agent. Returns whether the selection actually moved.
    ///
    /// Selecting the primary means looking at nobody else: it is not in the list, so the selection
    /// clears rather than landing on it. That is how going to the primary's own request behaves.
    pub(super) fn select(&mut self, agent_id: &AgentId) -> Result<bool, ReduceError> {
        if !self.agents.contains(agent_id) {
            return Err(ReduceError::UnknownAgent(agent_id.clone()));
        }
        let target = if self
            .primary()
            .is_some_and(|primary| &primary.id == agent_id)
        {
            None
        } else {
            Some(agent_id.clone())
        };
        if self.selected == target {
            return Ok(false);
        }
        self.selected = target;
        Ok(true)
    }

    /// Clears the selection, which closes the second window. Returns whether there was one.
    pub(super) fn clear_selection(&mut self) -> bool {
        self.selected.take().is_some()
    }

    /// Moves the selection one step through the sub-agents in arrival order, clamped at both ends.
    ///
    /// Clamping rather than wrapping keeps a held key idempotent at the boundary. A list that wraps
    /// sends the user back to the first agent at the moment they stop reading the keys — which is
    /// the opposite of the focus ring, where a `Tab` that stops cycling is a dead key. Leaving the
    /// list is `Escape`'s job, not the arrows'.
    pub(super) fn move_selection(&mut self, direction: Direction) -> bool {
        let next = self.moved_from(self.selected.as_ref(), direction);
        if next == self.selected {
            return false;
        }
        self.selected = next;
        true
    }

    /// Resolves one clamped list step without changing the selected conversation.
    pub(super) fn moved_from(
        &self,
        current: Option<&AgentId>,
        direction: Direction,
    ) -> Option<AgentId> {
        let Some(current) = current else {
            // Nothing is selected yet, so either arrow lands on the first sub-agent.
            return self.sub_agents().next().map(|agent| agent.id.clone());
        };
        let Some(index) = self.sub_agents().position(|agent| &agent.id == current) else {
            return self.sub_agents().next().map(|agent| agent.id.clone());
        };
        let target = match direction {
            Direction::Forward => index.saturating_add(1),
            Direction::Backward => index.saturating_sub(1),
        };
        self.sub_agents()
            .nth(target)
            .map(|agent| agent.id.clone())
            .or_else(|| Some(current.clone()))
    }
}

impl super::ViewState {
    /// The delegated children a conversation switch would interrupt (SPK-2).
    pub fn working_delegates(&self) -> impl Iterator<Item = &AgentView> {
        self.agents.working()
    }
}
