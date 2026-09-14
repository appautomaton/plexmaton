//! Display-only ingress for the independent collaboration control projection.

use super::Workspace;
use crate::{ChildControl, ChildControlRefusal, ChildControlSnapshot, SurfaceId};
use plexmaton_core::AgentId;

impl Workspace {
    /// Publishes a revisioned V1 child control view without granting runtime authority (CCV-1).
    ///
    /// The exact durable owner must supply this snapshot in production; offline examples may
    /// provide explicitly synthetic values. Unknown control keeps child input closed.
    pub fn set_child_control(
        &mut self,
        child: &AgentId,
        snapshot: ChildControlSnapshot,
    ) -> Result<bool, ChildControlRefusal> {
        let changed = self.state.set_child_control(child, snapshot)?;
        if changed {
            self.state.hover_entry(None);
            let hidden_drag = snapshot.control != ChildControl::User
                && self
                    .state
                    .inspector()
                    .is_some_and(|view| view.agent == *child)
                && self.state.draft(child).is_dragging();
            if hidden_drag {
                self.state.finish_child_input_drag(child);
                if self.router.capture() == Some(SurfaceId::Inspector) {
                    self.router = crate::Router::default();
                }
                self.pressed = None;
                self.pressed_entry = None;
                self.drag_autoscroll = None;
            }
        }
        Ok(changed)
    }
}

#[cfg(test)]
#[path = "child_control_tests.rs"]
mod tests;
