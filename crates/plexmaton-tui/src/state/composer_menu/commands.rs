//! Canonical and alias command spellings edit one composer draft (CMC-2).

use super::{Command, ViewState, grammar::exact_command};

impl ViewState {
    /// The Command the whole draft is, if it is one (CMC-2).
    pub(crate) fn exact_command(&self) -> Option<Command> {
        exact_command(self.composer().text())
    }

    /// Completes one canonical or alias spelling without changing the underlying Command action.
    pub(crate) fn complete_command_named(&mut self, name: &str) {
        let Some(primary) = self.primary_agent().map(|agent| agent.id.clone()) else {
            return;
        };
        let input = self.inputs.entry(primary).or_default();
        if !input.text().starts_with(&format!("/{name} ")) {
            input.replace_all(&format!("/{name} "));
        }
        self.composer_menu.reopen();
        self.sync_composer_menu();
        self.touch();
    }

    /// Takes the composer's draft for a Command that consumed it, and closes the menu.
    pub(crate) fn take_command_draft(&mut self) {
        if let Some(primary) = self.primary_agent().map(|agent| agent.id.clone()) {
            self.inputs.entry(primary.clone()).or_default().take();
            self.take_skill_binding(&primary);
        }
        self.composer_menu.close();
        self.touch();
    }
}
