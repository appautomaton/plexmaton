//! Pointer gesture reduction and transcript-row hit resolution.

use std::time::{Duration, Instant};

use plexmaton_core::AgentId;
use ratatui::layout::Rect;

use crate::{
    Workspace, content,
    intent::{PointerIntent, ScrollDirection},
    state::{CopyRequest, EntryTarget, TextPoint, inner_width},
    surface::{Point, SurfaceId},
};

const DRAG_AUTOSCROLL_INTERVAL: Duration = Duration::from_millis(60);
const DRAG_INSIDE_EDGE_ROWS: u16 = 1;

/// The content or action resolved from the frame where the primary button went down.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PressedEntry {
    target: EntryTarget,
    at: Point,
    action: PressAction,
    anchor: Option<(TextPoint, u16)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PressAction {
    Content,
    TextOnly,
    Selecting,
    Copy,
    CancelledCopy,
}

/// A held pointer at a conversation edge and the next monotonic step it owns.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct DragAutoScroll {
    surface: SurfaceId,
    at: Point,
    next_at: Instant,
}

impl Workspace {
    pub(super) fn cancel_pointer_click(&mut self) {
        if self
            .pressed_entry
            .as_ref()
            .is_some_and(|pressed| pressed.action != PressAction::Selecting)
        {
            self.pressed_entry = None;
        }
    }
    /// Routes a complete pointer gesture, keeping a cancelled or dragged press from opening work.
    ///
    /// Only an explicit Copy activation or completed selection drag emits copied source.
    /// A plain content click changes focus/disclosure without a clipboard side effect (SEL-4).
    pub(super) fn pointer(&mut self, pointer: PointerIntent, now: Instant) -> Option<CopyRequest> {
        let surface = match pointer {
            PointerIntent::Press { .. } => None,
            PointerIntent::Drag { surface, .. }
            | PointerIntent::Release { surface, .. }
            | PointerIntent::Cancel { surface }
            | PointerIntent::Suspend { surface } => Some(surface),
        };
        if surface.is_some_and(|surface| self.state.input_dragging(surface)) {
            return self.state.drag_text_input(&self.surfaces, pointer);
        }
        match pointer {
            PointerIntent::Press { surface, at } => {
                self.drag_autoscroll = None;
                // Read both before focus changes the inspector's input geometry: an event resolves
                // against the frame the user pressed in (FR-3).
                let entry = self.entry_at(surface, at);
                let compact_target = self.entry_target_at(surface, at);
                let anchor = self.text_point_at(surface, at, false);
                let target = compact_target.clone().or_else(|| {
                    anchor
                        .as_ref()
                        .and_then(|(_, point, _)| self.state.entry_target(surface, point.index))
                });
                let copy_button = target.as_ref().is_some_and(|target| {
                    self.copy_button_hit(target, at)
                        && self
                            .state
                            .entry_appearance(surface, &target.agent, &target.item, false)
                            .hovered
                });
                let input = self.state.click_text_input(&self.surfaces, surface, at);
                self.state.hover_entry(target.clone());
                self.pressed_entry = target.map(|target| PressedEntry {
                    target,
                    at,
                    action: if copy_button {
                        PressAction::Copy
                    } else if compact_target.is_none() {
                        PressAction::TextOnly
                    } else {
                        PressAction::Content
                    },
                    anchor: anchor.map(|(_, point, width)| (point, width)),
                });
                if copy_button {
                    return None;
                }
                self.state.focus_surface(&self.surfaces, surface);
                if input
                    || matches!(
                        surface,
                        SurfaceId::CommandPalette | SurfaceId::Configuration
                    )
                {
                    if input {
                        self.state.clear_selection();
                    }
                    return None;
                }
                if surface == SurfaceId::Agents {
                    self.click_agent(at);
                }
                match entry {
                    // The press remains pending until movement; a click does not select a block.
                    Some(_) => {
                        self.state.clear_selection();
                    }
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
                if let Some(mut pressed) = self.pressed_entry.take() {
                    if matches!(
                        pressed.action,
                        PressAction::Copy | PressAction::CancelledCopy
                    ) {
                        pressed.action = PressAction::CancelledCopy;
                        self.pressed_entry = Some(pressed);
                        return None;
                    }
                    if matches!(pressed.action, PressAction::Content | PressAction::TextOnly)
                        && let Some((anchor, width)) = pressed.anchor.take()
                    {
                        self.state.begin_text_selection(
                            surface,
                            pressed.target.agent.clone(),
                            anchor,
                            width,
                        );
                        pressed.action = PressAction::Selecting;
                    }
                    self.pressed_entry = Some(pressed);
                }
                self.extend_pointer_text(surface, at);
                self.state.drag(&self.surfaces, pointer);
                self.update_drag_autoscroll(surface, at, now);
                None
            }
            PointerIntent::Suspend { .. } => {
                self.drag_autoscroll = None;
                self.state.hover_entry(None);
                if let Some(pressed) = &mut self.pressed_entry
                    && pressed.action != PressAction::Selecting
                {
                    pressed.action = PressAction::CancelledCopy;
                }
                None
            }
            PointerIntent::Cancel { .. } => {
                self.drag_autoscroll = None;
                self.state.hover_entry(None);
                self.pressed_entry = None;
                self.state.drag(&self.surfaces, pointer);
                None
            }
            PointerIntent::Release { surface, at } => self.release_pointer(surface, at),
        }
    }

    fn release_pointer(&mut self, surface: SurfaceId, at: Point) -> Option<CopyRequest> {
        self.drag_autoscroll = None;
        if self
            .pressed_entry
            .as_ref()
            .is_some_and(|pressed| pressed.action == PressAction::Selecting)
        {
            self.extend_pointer_text(surface, at);
        }
        let released = self.entry_target_at(surface, at);
        self.state.hover_entry(released.clone());
        let pressed = self.pressed_entry.take();
        self.state
            .drag(&self.surfaces, PointerIntent::Release { surface, at });
        if let Some(pressed) = &pressed
            && matches!(
                pressed.action,
                PressAction::Copy | PressAction::CancelledCopy
            )
        {
            return (pressed.action == PressAction::Copy
                && pressed.at == at
                && released.as_ref() == Some(&pressed.target)
                && self.copy_button_hit(&pressed.target, at))
            .then(|| self.state.copy_message(&pressed.target))
            .flatten();
        }
        if pressed
            .as_ref()
            .is_some_and(|pressed| pressed.action == PressAction::Selecting)
        {
            let copied = self.state.copy();
            if copied.is_none() {
                self.state.clear_selection();
            }
            return copied;
        }
        if let Some(pressed) = pressed
            && pressed.action == PressAction::Content
            && pressed.target.surface == surface
            && (released.as_ref() == Some(&pressed.target) || pressed.at == at)
        {
            // Focus may have inserted the inspector's input strip between press and
            // release. An unchanged cell still completes the gesture against the frame
            // pressed; otherwise both frames must resolve the same stable item (FR-3).
            self.state
                .toggle_pointer_entry(&self.surfaces, &self.metrics, pressed.target);
            return None;
        }
        None
    }

    pub(super) fn copy_button_hit(&self, target: &EntryTarget, at: Point) -> bool {
        if self.state.message_source(target).is_none() {
            return false;
        }
        let Some(bounds) = self.conversation_bounds(target.surface) else {
            return false;
        };
        let Some(viewport) = self.surfaces.viewport(target.surface) else {
            return false;
        };
        let Some(x) = at.x.checked_sub(bounds.x + 1) else {
            return false;
        };
        if x < viewport.content_width.saturating_sub(4) || x >= viewport.content_width {
            return false;
        }
        let Some(local) = at.y.checked_sub(bounds.y + 1).map(usize::from) else {
            return false;
        };
        if local >= usize::from(viewport.visible_rows) {
            return false;
        }
        let slack = usize::from(viewport.visible_rows).saturating_sub(viewport.content_rows);
        let Some(row) = local.checked_sub(slack) else {
            return false;
        };
        self.metrics.entry_header_at_row(
            &target.agent,
            viewport.content_width,
            viewport.offset + row,
        ) == Some(&target.item)
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
        let selected = self.extend_pointer_text(active.surface, active.at);
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

    pub(super) fn conversation_bounds(&self, surface: SurfaceId) -> Option<Rect> {
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
