//! Resolves text endpoints against the last frame; selection and clipboard remain view-state data.
use super::*;
use crate::{Point, SurfaceId, state::TextPoint};

impl Workspace {
    pub(super) fn text_point_at(
        &mut self,
        surface: SurfaceId,
        mut at: Point,
        captured: bool,
    ) -> Option<(AgentId, TextPoint, u16)> {
        let bounds = self.conversation_bounds(surface)?;
        if bounds.width < 3 || bounds.height < 3 {
            return None;
        }
        let viewport = self.surfaces.viewport(surface)?;
        if viewport.visible_rows == 0 {
            return None;
        }
        if captured {
            at.x = at.x.clamp(bounds.x + 1, bounds.right() - 2);
            // FR-3: a conversation may share its bottom rule with the composer. Its painted
            // viewport, not an assumed fourth border, names the final selectable content row.
            at.y = at.y.clamp(bounds.y + 1, bounds.y + viewport.visible_rows);
        } else if at.x <= bounds.x || at.x >= bounds.right() - 1 {
            return None;
        }
        let local = usize::from(at.y.checked_sub(bounds.y + 1)?);
        if local >= usize::from(viewport.visible_rows) {
            return None;
        }
        let slack = usize::from(viewport.visible_rows).saturating_sub(viewport.content_rows);
        let row = viewport.offset + local.checked_sub(slack)?;
        let width = viewport.content_width;
        let agent = self.state.agent_shown_by(surface)?;
        let (index, key, inside, layout) =
            self.metrics.painted_entry(surface, &agent, width, row)?;
        let column = usize::from(at.x - bounds.x - 1);
        let offset = layout.offset_at(inside, column).or_else(|| {
            // Header/separator rows carry no copy text. The nearest text edge is a caret boundary.
            layout
                .rows
                .iter()
                .enumerate()
                .filter(|(_, fragments)| !fragments.is_empty())
                .min_by_key(|(row, _)| row.abs_diff(inside))
                .and_then(|(row, fragments)| {
                    if row < inside {
                        fragments.last().map(|f| f.text.end)
                    } else {
                        fragments.first().map(|f| f.text.start)
                    }
                })
        })?;
        let point = if let Some(range) = layout.atom_at(inside, column) {
            TextPoint::atomic(index, key.item.clone(), range, &layout.text)?
        } else {
            TextPoint::new(index, key.item.clone(), offset, &layout.text)?
        };
        Some((agent, point, width))
    }

    pub(super) fn extend_pointer_text(&mut self, surface: SurfaceId, at: Point) -> bool {
        self.text_point_at(surface, at, true)
            .is_some_and(|(agent, point, _)| {
                self.state.extend_text_selection(surface, &agent, point)
            })
    }

    /// Appending after the endpoints preserves selection. Reinterpreted selected text cannot copy a different slice.
    pub(super) fn validate_text_selection(&mut self) {
        let Some((surface, agent, points, width)) = self.state.text_selection_points() else {
            return;
        };
        let valid = points.into_iter().all(|point| {
            let Some(entry) = self
                .state
                .agent(agent)
                .and_then(|agent| agent.entries().nth(point.index))
            else {
                return false;
            };
            let key = crate::preparation::Key::new(
                agent,
                entry,
                width,
                self.state
                    .entry_appearance(surface, agent, entry.id(), false)
                    .open,
            )
            .with_math(self.metrics.math());
            match self.metrics.prepared_source(&key) {
                Some(Ok(layout)) => point.matches(entry, &layout),
                Some(Err(_)) => false,
                None => &point.item == entry.id(),
            }
        });
        if !valid {
            self.state.clear_selection();
            self.pressed_entry = None;
            self.drag_autoscroll = None;
        }
    }
}
