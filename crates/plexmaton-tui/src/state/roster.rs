//! Which agents exist, and which one the user is looking at.

use plexmaton_core::{AgentId, AgentStatus};

use super::{AgentView, ReduceError, ordered::OrderedById};
use crate::intent::Direction;

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

    /// The agent the composer addresses: the first one to appear, never the selected one (D-017).
    pub(super) fn primary(&self) -> Option<&AgentView> {
        self.agents.iter().next()
    }

    /// The agent whose transcript and activity are on screen.
    pub(super) fn selected(&self) -> Option<&AgentView> {
        self.selected.as_ref().and_then(|id| self.agents.get(id))
    }

    pub(super) fn get(&self, agent_id: &AgentId) -> Option<&AgentView> {
        self.agents.get(agent_id)
    }

    pub(super) fn contains(&self, agent_id: &AgentId) -> bool {
        self.agents.contains(agent_id)
    }

    /// Adds a newly visible agent, selecting it if nothing was selected yet.
    pub(super) fn add(
        &mut self,
        agent_id: AgentId,
        label: String,
        status: AgentStatus,
    ) -> Result<(), ReduceError> {
        if self.agents.contains(&agent_id) {
            return Err(ReduceError::DuplicateAgent(agent_id));
        }
        // The first agent to arrive is what the user is looking at; a later one must not steal the
        // view, because a background agent never navigates on the user's behalf.
        self.selected.get_or_insert_with(|| agent_id.clone());
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
    pub(super) fn select(&mut self, agent_id: &AgentId) -> Result<bool, ReduceError> {
        if !self.agents.contains(agent_id) {
            return Err(ReduceError::UnknownAgent(agent_id.clone()));
        }
        if self.selected.as_ref() == Some(agent_id) {
            return Ok(false);
        }
        self.selected = Some(agent_id.clone());
        Ok(true)
    }

    /// Moves the selection one step in arrival order, clamped at both ends.
    ///
    /// Clamping rather than wrapping keeps a held key idempotent at the boundary. A list that wraps
    /// sends the user back to the first agent at the moment they stop reading the keys — which is
    /// the opposite of the focus ring, where a `Tab` that stops cycling is a dead key.
    pub(super) fn move_selection(&mut self, direction: Direction) -> bool {
        let Some(current) = self.selected.clone() else {
            // Nothing is selected yet, so either arrow lands on the first agent.
            let first = self.agents.iter().next().map(|agent| agent.id.clone());
            let moved = first.is_some();
            self.selected = first;
            return moved;
        };
        let Some(index) = self.agents.iter().position(|agent| agent.id == current) else {
            return false;
        };
        let target = match direction {
            Direction::Forward => index.saturating_add(1),
            Direction::Backward => index.saturating_sub(1),
        };
        let Some(next) = self.agents.iter().nth(target).map(|agent| agent.id.clone()) else {
            return false;
        };
        if next == current {
            return false;
        }
        self.selected = Some(next);
        true
    }
}
