//! Pointer gesture reduction and transcript-row hit resolution.

use crate::{
    Workspace, content,
    intent::PointerIntent,
    state::{EntryTarget, inner_width},
    surface::{Point, SurfaceId},
};

/// A foldable row resolved from the frame where the primary button went down.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PressedEntry {
    target: EntryTarget,
    at: Point,
}

impl Workspace {
    /// Routes a complete pointer gesture, keeping a cancelled or dragged press from opening work.
    pub(super) fn pointer(&mut self, pointer: PointerIntent) {
        match pointer {
            PointerIntent::Press { surface, at } => {
                // Read the target before focus changes the inspector's input geometry: an event
                // resolves against the frame the user pressed in (FR-3).
                let target = self.entry_target_at(surface, at);
                self.state.hover_entry(target.clone());
                self.pressed_entry = target.map(|target| PressedEntry { target, at });
                self.state.focus_surface(&self.surfaces, surface);
                if surface == SurfaceId::Agents {
                    self.click_agent(at);
                }
                self.state.drag(&self.surfaces, pointer);
            }
            PointerIntent::Drag { .. } | PointerIntent::Cancel { .. } => {
                self.state.hover_entry(None);
                self.pressed_entry = None;
                self.state.drag(&self.surfaces, pointer);
            }
            PointerIntent::Release { surface, at } => {
                let released = self.entry_target_at(surface, at);
                self.state.hover_entry(released.clone());
                let pressed = self.pressed_entry.take();
                self.state.drag(&self.surfaces, pointer);
                if let Some(pressed) = pressed
                    && pressed.target.surface == surface
                    && (released.as_ref() == Some(&pressed.target) || pressed.at == at)
                {
                    // Focus may have inserted the inspector's input strip between press and
                    // release. An unchanged cell still completes the gesture against the frame
                    // pressed; otherwise both frames must resolve the same stable item (FR-3).
                    self.state
                        .toggle_pointer_entry(&self.surfaces, &self.metrics, pressed.target);
                }
            }
        }
    }

    /// Resolves a compact foldable row through the viewport the last frame measured.
    pub(super) fn entry_target_at(&self, surface: SurfaceId, at: Point) -> Option<EntryTarget> {
        if !matches!(surface, SurfaceId::Transcript | SurfaceId::Inspector) {
            return None;
        }
        let mut bounds = self.surfaces.get(surface)?.bounds;
        if surface == SurfaceId::Inspector
            && let Some((split, _)) = self.state.steer_input(&self.surfaces)
        {
            bounds = split.conversation;
        }
        if at.x <= bounds.x || at.x >= bounds.right().saturating_sub(1) {
            return None;
        }
        let viewport = self.surfaces.viewport(surface)?;
        let local = usize::from(at.y.checked_sub(bounds.y.saturating_add(1))?);
        if local >= usize::from(viewport.visible_rows) {
            return None;
        }
        let slack = usize::from(viewport.visible_rows).saturating_sub(viewport.content_rows);
        let content_row = viewport.offset.saturating_add(local.checked_sub(slack)?);
        let agent = self.state.agent_shown_by(surface)?;
        let index =
            self.metrics
                .compact_entry_at_row(&agent, viewport.content_width, content_row)?;
        self.state.entry_target(surface, index)
    }

    /// Selects the agent painted under a press in the list, if the press landed on one.
    fn click_agent(&mut self, at: Point) {
        let Some(bounds) = self
            .surfaces
            .get(SurfaceId::Agents)
            .map(|surface| surface.bounds)
        else {
            return;
        };
        // Inside the border, then past whatever the list is scrolled by.
        let Some(row) = at.y.checked_sub(bounds.y.saturating_add(1)) else {
            return;
        };
        if row >= bounds.height.saturating_sub(2) {
            return;
        }
        let offset = self
            .surfaces
            .viewport(SurfaceId::Agents)
            .map_or(0, |viewport| viewport.offset);
        let width = inner_width(bounds.width);
        if let Some(agent) = content::agent_at_row(
            &self.state,
            &self.palette,
            width,
            usize::from(row).saturating_add(offset),
        ) {
            // The agent came from the roster one line ago, so an unknown one is a race with
            // nothing, and selecting it again is the no-op the reducer already makes it.
            let _known = self.state.select_agent(&agent);
        }
    }
}
