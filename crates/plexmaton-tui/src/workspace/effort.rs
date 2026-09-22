//! Effort's choices and refusals, and whether its max label is one of the workspace's moving things.

use super::Workspace;
use plexmaton_core::ReasoningEffort;

impl Workspace {
    /// Publish the model's declared choices; absent capabilities are not inferred.
    pub fn set_effort_choices(&mut self, choices: Option<Vec<ReasoningEffort>>) {
        self.state.set_effort_choices(choices);
    }

    /// Publish runtime acceptance or a visible refusal; only acceptance changes the model label.
    pub fn report_effort(&mut self, result: Result<crate::ConfigurationSummary, String>) {
        self.state.report_effort(result);
    }

    /// Whether a max label is on screen, which is what makes effort one of the moving things
    /// (EFF-4 under MOT-1).
    pub(super) fn effort_moving(&self) -> bool {
        let selector = self.state.effort_visible()
            && self.state.selected_effort() == Some(ReasoningEffort::Max)
            && self
                .surfaces
                .get(crate::SurfaceId::ComposerMenu)
                .is_some_and(|surface| surface.bounds.height >= 4 && surface.bounds.width >= 20);
        selector || crate::render::effort::composer_max_visible(&self.state, &self.surfaces)
    }
}
