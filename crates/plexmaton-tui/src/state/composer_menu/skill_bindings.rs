//! Selected skill ownership across completion, submission and returned input (SKP-2).

use super::{MenuRow, ViewState, binding_matches};
use plexmaton_core::AgentId;

impl ViewState {
    pub(crate) fn accept_skill(&mut self, name: Option<String>) -> bool {
        let Some(primary) = self.primary_agent().map(|agent| agent.id.clone()) else {
            return false;
        };
        let name = name.or_else(|| match self.composer_menu.chosen() {
            Some(MenuRow::Skill(name)) => Some(name.clone()),
            _ => None,
        });
        let Some(name) = name.filter(|name| {
            self.menu_rows()
                .iter()
                .any(|row| matches!(row, MenuRow::Skill(listed) if listed == name))
        }) else {
            return false;
        };
        self.inputs
            .entry(primary.clone())
            .or_default()
            .complete_initial_token(&name);
        self.skill_bindings.insert(primary, name);
        self.composer_menu.close();
        self.touch();
        true
    }

    pub(crate) fn selected_skill(&self, agent: &AgentId) -> Option<&str> {
        self.skill_bindings.get(agent).map(String::as_str)
    }

    pub(crate) fn take_skill_binding(&mut self, agent: &AgentId) -> Option<String> {
        self.skill_bindings.remove(agent)
    }

    pub(crate) fn clear_skill_binding(&mut self, agent: &AgentId) {
        self.skill_bindings.remove(agent);
        if self
            .primary_agent()
            .is_some_and(|primary| &primary.id == agent)
        {
            self.composer_menu.close();
        }
    }

    pub(crate) fn retain_primary_skill_binding(&mut self) {
        let Some(primary) = self.primary_agent().map(|agent| agent.id.clone()) else {
            return;
        };
        let keep = self
            .skill_bindings
            .get(&primary)
            .is_some_and(|name| binding_matches(self.composer().text(), name));
        if !keep {
            self.skill_bindings.remove(&primary);
        }
    }
}
