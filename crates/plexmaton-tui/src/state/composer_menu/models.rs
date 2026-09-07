//! Bounded configured identities; no provider configuration or credentials cross this boundary.
use super::{MenuRow, ViewState};

/// Exact configured pair; display names and wire IDs never supply authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelIdentity {
    pub provider: String,
    pub model: String,
}

/// Inert presentation of one configured model.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelChoice {
    pub identity: ModelIdentity,
    pub display_name: String,
    pub wire_id: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct ModelCatalog {
    choices: Vec<ModelChoice>,
    limited: bool,
}

impl ModelCatalog {
    pub(super) fn rows(&self, query: &str) -> Vec<MenuRow> {
        let query = query.to_lowercase();
        self.choices
            .iter()
            .filter(|choice| {
                [
                    &choice.identity.provider,
                    &choice.identity.model,
                    &choice.display_name,
                    &choice.wire_id,
                ]
                .iter()
                .any(|value| value.to_lowercase().contains(&query))
            })
            .map(|choice| MenuRow::Model(choice.identity.clone()))
            .collect()
    }
}

impl super::ComposerMenu {
    pub(crate) fn model_choice(&self, identity: &ModelIdentity) -> Option<&ModelChoice> {
        self.models
            .choices
            .iter()
            .find(|choice| &choice.identity == identity)
    }
}

impl ViewState {
    pub(crate) fn set_model_choices(&mut self, choices: impl Iterator<Item = ModelChoice>) {
        let mut catalog = ModelCatalog::default();
        let mut bytes = 0_usize;
        for choice in choices {
            let size = choice
                .identity
                .provider
                .len()
                .saturating_add(choice.identity.model.len())
                .saturating_add(choice.display_name.len())
                .saturating_add(choice.wire_id.len());
            if catalog.choices.len() == 256 || bytes.saturating_add(size) > 64 * 1024 {
                catalog.limited = true;
                break;
            }
            bytes += size;
            catalog.choices.push(choice);
        }
        self.composer_menu.models = catalog;
        self.sync_composer_menu();
        self.touch();
    }

    pub(crate) fn model_heading(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if let Some(message) = &self.composer_menu.model_feedback {
            lines.push(message.clone());
        }
        if self.composer_menu.models.limited {
            lines.push("Model list limited to 256 entries / 64 KiB.".to_owned());
        }
        if self.menu_rows().is_empty() {
            lines.push(
                if self.composer_menu.models.choices.is_empty() {
                    "No configured models."
                } else {
                    "No matching models."
                }
                .to_owned(),
            );
        }
        lines
    }

    pub(crate) fn report_model(
        &mut self,
        result: Result<super::super::ConfigurationSummary, String>,
    ) {
        match result {
            Ok(summary) => {
                self.set_model(summary);
                self.composer_menu.model_feedback = None;
                self.take_command_draft();
            }
            Err(message) => {
                self.composer_menu.model_feedback = Some(message);
                self.touch();
            }
        }
    }

    pub(crate) fn is_current_model(&self, identity: &ModelIdentity) -> bool {
        self.model.as_ref().is_some_and(|model| {
            model.provider == identity.provider && model.configured_name == identity.model
        })
    }
}
