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

    /// The sub-agents, in arrival order: everyone but the primary, which is what the list holds.
    pub(super) fn sub_agents(&self) -> impl Iterator<Item = &AgentView> {
        self.agents.iter().skip(1)
    }

    /// The sub-agent the user is looking at, if any.
    pub(super) fn selected(&self) -> Option<&AgentView> {
        self.selected.as_ref().and_then(|id| self.agents.get(id))
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
        let Some(current) = self.selected.clone() else {
            // Nothing is selected yet, so either arrow lands on the first sub-agent.
            let first = self.sub_agents().next().map(|agent| agent.id.clone());
            let moved = first.is_some();
            self.selected = first;
            return moved;
        };
        let Some(index) = self.sub_agents().position(|agent| agent.id == current) else {
            return false;
        };
        let target = match direction {
            Direction::Forward => index.saturating_add(1),
            Direction::Backward => index.saturating_sub(1),
        };
        let Some(next) = self.sub_agents().nth(target).map(|agent| agent.id.clone()) else {
            return false;
        };
        if next == current {
            return false;
        }
        self.selected = Some(next);
        true
    }
}
