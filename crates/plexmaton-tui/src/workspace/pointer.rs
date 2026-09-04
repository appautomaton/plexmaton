//! Pointer gesture reduction and transcript-row hit resolution.

use std::time::{Duration, Instant};

use plexmaton_core::AgentId;
use ratatui::layout::Rect;

use crate::{
    Workspace, content,
    intent::{PointerIntent, ScrollDirection},
    state::{CopyRequest, EntryTarget, inner_width},
    surface::{Point, SurfaceId},
};

const DRAG_AUTOSCROLL_INTERVAL: Duration = Duration::from_millis(60);
const DRAG_INSIDE_EDGE_ROWS: u16 = 1;

/// A foldable row resolved from the frame where the primary button went down.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PressedEntry {
    target: EntryTarget,
    at: Point,
}

/// A held pointer at a conversation edge and the next monotonic step it owns.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct DragAutoScroll {
    surface: SurfaceId,
    at: Point,
    next_at: Instant,
}

impl Workspace {
    /// Routes a complete pointer gesture, keeping a cancelled or dragged press from opening work.
    ///
    /// Returns what the finished gesture put on the clipboard, if anything. Selecting with the
    /// mouse and then copying with a key is a gesture nobody makes: on macOS the terminal keeps
    /// `Cmd-C` for its own selection, which over an owned screen is empty, so a mouse selection
    /// that waited to be copied could not be copied at all. Releasing the button is the copy
    /// (SEL-4 still applies: the text leaves as a value and this crate reaches no clipboard).
    pub(super) fn pointer(&mut self, pointer: PointerIntent, now: Instant) -> Option<CopyRequest> {
        match pointer {
            PointerIntent::Press { surface, at } => {
                self.drag_autoscroll = None;
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
                if let Some((agent, index)) = self.dragged_entry_at(surface, at) {
                    let _changed = self.state.extend_selection_to(surface, &agent, index);
                }
                self.state.drag(&self.surfaces, pointer);
                self.update_drag_autoscroll(surface, at, now);
                None
            }
            PointerIntent::Suspend { .. } => {
                self.drag_autoscroll = None;
                self.state.hover_entry(None);
                self.pressed_entry = None;
                None
            }
            PointerIntent::Cancel { .. } => {
                self.drag_autoscroll = None;
                self.state.hover_entry(None);
                self.pressed_entry = None;
                self.state.drag(&self.surfaces, pointer);
                None
            }
            PointerIntent::Release { surface, at } => {
                self.drag_autoscroll = None;
                if let Some((agent, index)) = self.dragged_entry_at(surface, at) {
                    let _changed = self.state.extend_selection_to(surface, &agent, index);
                }
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

    /// Next edge-drag wakeup, absent while no conversation drag needs motion.
    pub fn drag_autoscroll_deadline(&self) -> Option<Instant> {
        self.drag_autoscroll.map(|active| active.next_at)
    }

    /// Advances one owned edge-drag step and extends the semantic selection into the new viewport.
    pub fn advance_drag_autoscroll(&mut self, now: Instant) -> bool {
        let Some(mut active) = self.drag_autoscroll else {
            return false;
        };
        if now < active.next_at {
            return false;
        }
        let selected = self
            .dragged_entry_at(active.surface, active.at)
            .is_some_and(|(agent, index)| {
                self.state
                    .extend_selection_to(active.surface, &agent, index)
            });
        let Some((direction, rows)) = self.autoscroll_motion(active.surface, active.at) else {
            self.drag_autoscroll = None;
            return selected;
        };
        let moved = self.state.scroll_conversation_by(
            &self.surfaces,
            &self.metrics,
            active.surface,
            direction,
            rows,
        );
        if moved {
            active.next_at = now + DRAG_AUTOSCROLL_INTERVAL;
            self.drag_autoscroll = Some(active);
        } else {
            self.drag_autoscroll = None;
        }
        selected || moved
    }

    fn update_drag_autoscroll(&mut self, surface: SurfaceId, at: Point, now: Instant) {
        if self.autoscroll_motion(surface, at).is_none() {
            self.drag_autoscroll = None;
            return;
        }
        let next_at = self
            .drag_autoscroll
            .filter(|active| active.surface == surface)
            .map_or(now + DRAG_AUTOSCROLL_INTERVAL, |active| active.next_at);
        self.drag_autoscroll = Some(DragAutoScroll {
            surface,
            at,
            next_at,
        });
    }

    fn autoscroll_motion(&self, surface: SurfaceId, at: Point) -> Option<(ScrollDirection, usize)> {
        let bounds = self.conversation_bounds(surface)?;
        let viewport = self.surfaces.viewport(surface)?;
        let top = bounds.y.saturating_add(1);
        let bottom = bounds.bottom().saturating_sub(1);
        if bottom <= top || !viewport.is_scrollable() {
            return None;
        }
        let (direction, distance) = if at.y < top.saturating_add(DRAG_INSIDE_EDGE_ROWS) {
            (
                ScrollDirection::Up,
                top.saturating_add(DRAG_INSIDE_EDGE_ROWS)
                    .saturating_sub(at.y),
            )
        } else if at.y >= bottom.saturating_sub(DRAG_INSIDE_EDGE_ROWS) {
            (
                ScrollDirection::Down,
                at.y.saturating_sub(bottom.saturating_sub(DRAG_INSIDE_EDGE_ROWS))
                    .saturating_add(1),
            )
        } else {
            return None;
        };
        if matches!(direction, ScrollDirection::Up) && viewport.offset == 0
            || matches!(direction, ScrollDirection::Down)
                && viewport.offset >= viewport.max_offset()
        {
            return None;
        }
        let rows = match distance {
            0 | 1 => 1,
            2 => 2,
            _ => 3,
        };
        Some((direction, rows))
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
        let bounds = self.conversation_bounds(surface)?;
        if at.x <= bounds.x || at.x >= bounds.right().saturating_sub(1) {
            return None;
        }
        self.entry_at_inside(surface, at, bounds)
    }

    /// Resolves the nearest content row while a captured drag has crossed an edge.
    fn dragged_entry_at(&self, surface: SurfaceId, at: Point) -> Option<(AgentId, usize)> {
        let bounds = self.conversation_bounds(surface)?;
        if bounds.width < 3 || bounds.height < 3 {
            return None;
        }
        let at = Point {
            x: at
                .x
                .clamp(bounds.x.saturating_add(1), bounds.right().saturating_sub(2)),
            y: at.y.clamp(
                bounds.y.saturating_add(1),
                bounds.bottom().saturating_sub(2),
            ),
        };
        self.entry_at_inside(surface, at, bounds)
    }

    fn conversation_bounds(&self, surface: SurfaceId) -> Option<Rect> {
        match surface {
            SurfaceId::Inspector => self.state.inspector_conversation_bounds(&self.surfaces),
            SurfaceId::Transcript => self.surfaces.get(surface).map(|surface| surface.bounds),
            _ => None,
        }
    }

    fn entry_at_inside(
        &self,
        surface: SurfaceId,
        at: Point,
        bounds: Rect,
    ) -> Option<(AgentId, usize)> {
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
