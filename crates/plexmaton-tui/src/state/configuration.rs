//! Read-only configuration supplied by the composition root, with no provider or file access.

use super::{ViewState, drawer::Shown};

/// Display projection of the model resolved for this running process. Contains no credentials.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigurationSummary {
    pub provider: String,
    pub model: String,
    pub reasoning_effort: plexmaton_core::ReasoningEffort,
}

impl ConfigurationSummary {
    /// The fields shown by the page, in reading order.
    pub(crate) fn fields(&self) -> [(&str, &str); 3] {
        [
            ("Provider", &self.provider),
            ("Model", &self.model),
            ("Reasoning effort", self.reasoning_effort.as_str()),
        ]
    }

    /// A label, a value and a blank row per field; the note, a blank row and the footer; then the
    /// Drawer's padding and borders.
    pub(crate) fn preferred_rows(&self) -> u16 {
        u16::try_from(self.fields().len()).unwrap_or(0) * 3 + 3 + 4
    }
}

impl ViewState {
    /// The resolved settings while the Configuration page is open.
    #[must_use]
    pub fn configuration(&self) -> Option<&ConfigurationSummary> {
        self.drawer.as_ref()?.configuration()
    }

    /// Opens the page once the composition root has projected the summary (DRW-4).
    pub(crate) fn show_configuration(&mut self, summary: ConfigurationSummary) {
        self.show_page(Shown::Configuration(summary));
    }
}
