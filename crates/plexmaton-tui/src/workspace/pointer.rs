//! Pointer gesture reduction and transcript-row hit resolution.

use plexmaton_core::AgentId;

use crate::{
    Workspace, content,
    intent::PointerIntent,
    state::{CopyRequest, EntryTarget, inner_width},
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
    ///
    /// Returns what the finished gesture put on the clipboard, if anything. Selecting with the
    /// mouse and then copying with a key is a gesture nobody makes: on macOS the terminal keeps
    /// `Cmd-C` for its own selection, which over an owned screen is empty, so a mouse selection
    /// that waited to be copied could not be copied at all. Releasing the button is the copy
    /// (SEL-4 still applies: the text leaves as a value and this crate reaches no clipboard).
    pub(super) fn pointer(&mut self, pointer: PointerIntent) -> Option<CopyRequest> {
        match pointer {
            PointerIntent::Press { surface, at } => {
                // Read both before focus changes the inspector's input geometry: an event resolves
                // against the frame the user pressed in (FR-3).
                let entry = self.entry_at(surface, at);
                let target = self.entry_target_at(surface, at);
                self.state.hover_entry(target.clone());
                self.pressed_entry = target.map(|target| PressedEntry { target, at });
                self.state.focus_surface(&self.surfaces, surface);
                if surface == SurfaceId::Agents {
                    self.click_agent(at);
                }
                match entry {
                    // Where the drag anchors, and what a click on its own selects. A press is the
                    // start of a selection whatever kind of entry it landed on (SEL-1).
                    Some((agent, index)) => self.state.begin_selection(surface, agent, index),
                    // Pressing where there is no content is how a selection ends. Without it the
                    // only way out of a highlight is a key, and the gesture that made it has no
                    // undo of its own.
                    None => {
                        let _cleared = self.state.clear_selection();
                    }
                }
                self.state.drag(&self.surfaces, pointer);
                None
            }
            PointerIntent::Drag { surface, at } => {
                self.state.hover_entry(None);
                self.pressed_entry = None;
                if let Some((agent, index)) = self.entry_at(surface, at) {
                    self.state.extend_selection_to(surface, &agent, index);
                }
                self.state.drag(&self.surfaces, pointer);
                None
            }
            PointerIntent::Cancel { .. } => {
                self.state.hover_entry(None);
                self.pressed_entry = None;
                self.state.drag(&self.surfaces, pointer);
                None
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
                self.state.copy()
            }
        }
    }

    /// Resolves a compact foldable row through the viewport the last frame measured.
    pub(super) fn entry_target_at(&self, surface: SurfaceId, at: Point) -> Option<EntryTarget> {
        let (_, index) = self.entry_at(surface, at)?;
        self.state.entry_target(surface, index)
    }

    /// Resolves whichever entry a pointer is over, foldable or not.
    ///
    /// Separate from [`Self::entry_target_at`] because disclosure and selection address different
    /// sets: only a tool with retained detail can be opened, while every entry can be selected and
    /// copied. Sharing one resolver made the narrower set the only thing the mouse could reach.
    fn entry_at(&self, surface: SurfaceId, at: Point) -> Option<(AgentId, usize)> {
        if !matches!(surface, SurfaceId::Transcript | SurfaceId::Inspector) {
            return None;
        }
        let bounds = match surface {
            SurfaceId::Inspector => self.state.inspector_conversation_bounds(&self.surfaces)?,
            SurfaceId::Transcript => self.surfaces.get(surface)?.bounds,
            _ => return None,
        };
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
        Some((agent, index))
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
