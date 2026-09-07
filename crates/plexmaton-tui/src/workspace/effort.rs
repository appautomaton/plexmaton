//! Visible max labels share one deadline; animation invalidates presentation, never semantic state.

use super::Workspace;
use plexmaton_core::ReasoningEffort;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub(super) struct EffortAnimation {
    start: Instant,
    next: Instant,
}

impl Workspace {
    /// Publish the model's declared choices; absent capabilities are not inferred.
    pub fn set_effort_choices(&mut self, choices: Option<Vec<ReasoningEffort>>) {
        self.state.set_effort_choices(choices);
    }

    /// Publish runtime acceptance or a visible refusal; only acceptance changes the model label.
    pub fn report_effort(&mut self, result: Result<crate::ConfigurationSummary, String>) {
        self.state.report_effort(result);
    }

    fn effort_animation_visible(&self) -> bool {
        let selector = self.state.effort_visible()
            && self.state.selected_effort() == Some(ReasoningEffort::Max)
            && self
                .surfaces
                .get(crate::SurfaceId::ComposerMenu)
                .is_some_and(|surface| surface.bounds.height >= 4 && surface.bounds.width >= 20);
        selector || crate::render::effort::composer_max_visible(&self.state, &self.surfaces)
    }

    /// Arm a single shared deadline while the composer or selector has a visible max label.
    pub fn effort_animation_deadline(&mut self, now: Instant) -> Option<Instant> {
        if !self.effort_animation_visible() {
            self.effort_animation = None;
            return None;
        }
        Some(
            self.effort_animation
                .get_or_insert(EffortAnimation {
                    start: now,
                    next: now + Duration::from_millis(67),
                })
                .next,
        )
    }

    /// Late wakes coalesce into the current phase, without queuing missed animation frames.
    pub fn advance_effort_animation(&mut self, now: Instant) {
        if !self.effort_animation_visible() {
            self.effort_animation = None;
            return;
        }
        let Some(animation) = &mut self.effort_animation else {
            return;
        };
        if now < animation.next {
            return;
        }
        let phase =
            (now.saturating_duration_since(animation.start).as_millis() * 15 / 1000 % 480) as u16;
        animation.next = now + Duration::from_millis(67);
        if phase != self.state.effort_phase() {
            self.state.set_effort_phase(phase);
            self.painted = None;
        }
    }
}
