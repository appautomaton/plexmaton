//! Effort catalog and tentative menu selection; accepted model state comes from the runtime.

use super::{Listing, MenuRow, ViewState};
use plexmaton_core::ReasoningEffort;

impl ViewState {
    pub(crate) fn set_effort_choices(&mut self, choices: Option<Vec<ReasoningEffort>>) {
        if self.composer_menu.efforts == choices {
            return;
        }
        self.composer_menu.efforts = choices.map(|choices| {
            ReasoningEffort::EXPLICIT
                .into_iter()
                .filter(|effort| choices.contains(effort))
                .collect()
        });
        self.sync_composer_menu();
        self.touch();
    }

    pub(crate) fn effort_available(&self, effort: ReasoningEffort) -> bool {
        self.composer_menu
            .efforts
            .as_ref()
            .is_some_and(|choices| choices.contains(&effort))
    }

    pub(crate) fn selected_effort(&self) -> Option<ReasoningEffort> {
        match self.menu_chosen() {
            Some(MenuRow::Effort(effort)) => Some(effort),
            _ => None,
        }
    }

    pub(crate) fn choose_effort(&mut self, effort: ReasoningEffort) {
        if !self.effort_available(effort) {
            return;
        }
        let text = self.composer().text().to_owned();
        let cursor = self.composer().cursor();
        self.composer_menu
            .choose(&text, cursor, MenuRow::Effort(effort));
        self.composer_menu.effort_feedback = None;
        self.touch();
    }

    pub(crate) fn report_effort(&mut self, result: Result<super::ConfigurationSummary, String>) {
        match result {
            Ok(summary) => {
                self.set_model(summary);
                self.take_command_draft();
            }
            Err(message) => {
                self.composer_menu.effort_feedback = Some(message);
                self.touch();
            }
        }
    }

    pub(crate) fn effort_phase(&self) -> u16 {
        self.composer_menu.effort_phase
    }

    pub(crate) fn set_effort_phase(&mut self, phase: u16) {
        self.composer_menu.effort_phase = phase;
    }

    pub(crate) fn effort_visible(&self) -> bool {
        self.menu_listing() == Some(Listing::Effort)
            && self.composer_menu.is_open()
            && self.focus.prefers(crate::SurfaceId::Composer)
            && self.drawer.is_none()
    }
}
