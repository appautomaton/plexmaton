use super::*;
use crate::{
    RetryAction, RetryActions, RetrySubmission, RetryTarget,
    surface::{Point, SurfaceId},
};

impl Workspace {
    /// Installs the runtime's current eligibility projection, or removes stale actions.
    pub fn set_retry_actions(&mut self, actions: Option<RetryActions>) {
        self.state.set_retry_actions(actions);
    }
    /// Shares message-action dispatch between contextual keys and inline pointer activation.
    pub fn perform_retry_action(&mut self, command: RetryAction) -> Option<RetrySubmission> {
        match command {
            RetryAction::Retry => {
                let target = self.state.retry_actions()?.target.clone();
                self.state.finish_retry_edit();
                self.state.close_drawer();
                self.state.set_retry_actions(None);
                Some(RetrySubmission {
                    target,
                    edited_text: None,
                    skill: None,
                })
            }
            RetryAction::EditRetry => {
                self.state.begin_retry_edit();
                None
            }
        }
    }
    /// Acknowledges edited input and restores the draft displaced by edit mode.
    pub fn complete_retry_edit(&mut self) {
        self.state.finish_retry_edit();
    }
    /// Replaces semantic history while keeping local input and its focus; drops old geometry.
    pub fn replace_projection(&mut self, events: Vec<ConversationEventEnvelope>) {
        self.state.replace_projection(events);
        self.metrics = TranscriptMetrics::with_math(self.metrics.math());
        self.preparation = preparation::Preparation::default();
        self.copy = copy::CopyPreparation::default();
        self.router = Router::default();
        self.surfaces = SurfaceTree::default();
        self.pressed_entry = None;
        self.pressed = None;
        self.drag_autoscroll = None;
        self.painted = None;
    }
    pub(super) fn retry_hit(
        &self,
        surface: SurfaceId,
        at: Point,
    ) -> Option<(RetryTarget, RetryAction)> {
        if surface != SurfaceId::Transcript {
            return None;
        }
        let actions = self.state.retry_actions()?;
        let agent = self.state.primary_agent()?;
        let bounds = self.surfaces.get(surface)?.bounds;
        let viewport = self.surfaces.viewport(surface)?;
        let x = at.x.checked_sub(bounds.x.checked_add(1)?)?;
        let y = usize::from(at.y.checked_sub(bounds.y.checked_add(1)?)?);
        if x >= viewport.content_width || y >= usize::from(viewport.visible_rows) {
            return None;
        }
        let slack = usize::from(viewport.visible_rows).saturating_sub(viewport.content_rows);
        let row = viewport.offset.saturating_add(y.checked_sub(slack)?);
        if self
            .metrics
            .retry_entry_at_row(&agent.id, viewport.content_width, row)?
            != &actions.error_item
        {
            return None;
        }
        let command = match x {
            0..=8 => RetryAction::Retry,
            12..=27 => RetryAction::EditRetry,
            _ => return None,
        };
        Some((actions.target.clone(), command))
    }
}

#[cfg(test)]
mod tests;
