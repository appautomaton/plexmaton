//! Read-only configuration supplied by the composition root, with no provider or file access.

use super::{CommandPalette, ViewState, focus::Focus};
use crate::surface::SurfaceId;

/// Display projection of the model resolved for this running process. Contains no credentials.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigurationSummary {
    pub provider: String,
    pub model: String,
    pub reasoning_effort: String,
}

impl ConfigurationSummary {
    /// The fields shown by the configuration page, in reading order.
    pub(crate) fn fields(&self) -> [(&str, &str); 3] {
        [
            ("Provider", &self.provider),
            ("Model", &self.model),
            ("Reasoning effort", &self.reasoning_effort),
        ]
    }
}

/// An open configuration page retains the focus preference it temporarily covers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ConfigurationView {
    summary: ConfigurationSummary,
    return_focus: Focus,
    return_palette: Option<CommandPalette>,
}

impl ViewState {
    /// The resolved settings while the configuration page is open.
    #[must_use]
    pub fn configuration(&self) -> Option<&ConfigurationSummary> {
        self.configuration.as_ref().map(|view| &view.summary)
    }

    /// Opens the page after the composition root handles the command (INV-12).
    pub(crate) fn show_configuration(&mut self, summary: ConfigurationSummary) {
        let palette = self.command_palette.take();
        if let Some(view) = self.configuration.as_mut() {
            view.summary = summary;
        } else {
            self.configuration = Some(ConfigurationView {
                summary,
                // Move the originating palette into this page so Back restores the exact filter,
                // caret and choice without creating a second editable copy (SURF-5).
                return_focus: self.focus,
                return_palette: palette,
            });
        }
        self.focus.prefer(SurfaceId::Configuration);
        self.touch();
    }

    pub(super) fn close_configuration(&mut self) -> bool {
        let Some(view) = self.configuration.take() else {
            return false;
        };
        self.focus = view.return_focus;
        self.command_palette = view.return_palette;
        self.touch();
        true
    }

    /// One label, value and separating row per field, then a note, a footer and two borders.
    pub(crate) fn configuration_rows(&self) -> u16 {
        self.configuration().map_or(0, |summary| {
            u16::try_from(summary.fields().len()).unwrap_or(0) * 3 + 4
        })
    }
}
