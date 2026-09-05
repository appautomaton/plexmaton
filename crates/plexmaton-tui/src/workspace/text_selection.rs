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
        if captured {
            at.x = at.x.clamp(bounds.x + 1, bounds.right() - 2);
            at.y = at.y.clamp(bounds.y + 1, bounds.bottom() - 2);
        } else if at.x <= bounds.x || at.x >= bounds.right() - 1 {
            return None;
        }
        let viewport = self.surfaces.viewport(surface)?;
        let local = usize::from(at.y.checked_sub(bounds.y + 1)?);
        if local >= usize::from(viewport.visible_rows) {
            return None;
        }
        let slack = usize::from(viewport.visible_rows).saturating_sub(viewport.content_rows);
        let row = viewport.offset + local.checked_sub(slack)?;
        let width = viewport.content_width;
        let agent = self.state.agent_shown_by(surface)?;
        let (index, inside) = self.metrics.text_entry_at_row(&agent, width, row)?;
        let item = self.state.agent(&agent)?.entries().nth(index)?;
        let appearance = self
            .state
            .entry_appearance(surface, &agent, item.id(), false);
        let layout = self
            .metrics
            .mapped_entry(&agent, item, &self.palette, width, appearance);
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
        Some((
            agent,
            TextPoint::new(index, item.id().clone(), offset, &layout.text)?,
            width,
        ))
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
            let layout = self.metrics.mapped_entry(
                agent,
                entry,
                &self.palette,
                width,
                self.state
                    .entry_appearance(surface, agent, entry.id(), false),
            );
            point.matches(entry, &layout)
        });
        if !valid {
            self.state.clear_selection();
            self.pressed_entry = None;
            self.drag_autoscroll = None;
        }
    }
}
