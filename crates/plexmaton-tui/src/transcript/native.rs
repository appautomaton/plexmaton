//! Native operations use the same prepared source maps, viewport origin and stacking as cells.

use ratatui::{Frame, layout::Rect, style::Style};
use unicode_width::UnicodeWidthStr as _;

use super::TranscriptMetrics;
use crate::{
    Palette, Role, ViewState,
    math::{NativeFrame, NativeText},
    surface::{SurfaceId, SurfaceTree},
    text_layout::{math::FormulaContent, paint::Colors},
};

#[derive(Clone, Copy, Eq, PartialEq)]
enum Issue {
    Clipped,
    Capacity,
}

impl TranscriptMetrics {
    pub(crate) fn native_frame(
        &self,
        frame: &mut Frame<'_>,
        surfaces: &SurfaceTree,
        state: &ViewState,
        palette: &Palette,
    ) -> NativeFrame {
        let mut output = NativeFrame::new(frame.area());
        let colors = Colors::new(palette);
        let mut issues = Vec::new();
        for entry in &self.drawing_text.entries {
            let Some(surface) = surfaces.get(entry.surface) else {
                continue;
            };
            let Some(viewport) = surface.viewport else {
                continue;
            };
            let bounds = if entry.surface == SurfaceId::Inspector {
                state
                    .inspector_conversation_bounds(surfaces)
                    .unwrap_or(surface.bounds)
            } else {
                surface.bounds
            };
            let clip = Rect::new(
                bounds.x + 1,
                bounds.y + 1,
                viewport.content_width,
                viewport.visible_rows,
            )
            .intersection(frame.area());
            let slack = usize::from(viewport.visible_rows).saturating_sub(viewport.content_rows);
            let Some(origin) = screen_origin(entry.start, viewport.offset, slack, bounds.y + 1)
            else {
                continue;
            };
            let selected = state.selected_text_range(
                entry.surface,
                &entry.key.agent,
                entry.index,
                entry.layout.text.len(),
            );
            let whole = state
                .selected_in(entry.surface, &entry.key.agent)
                .contains(entry.index);
            for formula in &entry.layout.formulas {
                let FormulaContent::Native(native) = &formula.content else {
                    continue;
                };
                let mut style = formula.style.resolve(&colors);
                let selected = whole
                    || selected.as_ref().is_some_and(|range| {
                        range.start < formula.text.end && range.end > formula.text.start
                    });
                if selected {
                    style = style.patch(palette.style(Role::Selection));
                }
                for run in native.runs() {
                    let x = usize::from(bounds.x) + 1 + formula.column + usize::from(run.x);
                    let y = origin
                        + i64::try_from(formula.row + usize::from(run.y)).unwrap_or(i64::MAX);
                    let mut first = None;
                    let mut visible = 0;
                    for dy in 0..run.rows {
                        for dx in 0..run.columns {
                            let (Ok(x), Ok(y)) = (
                                u16::try_from(x + usize::from(dx)),
                                u16::try_from(y + i64::from(dy)),
                            ) else {
                                continue;
                            };
                            if visible_cell(clip, surfaces, entry.surface, (x, y)) {
                                first.get_or_insert((x, y));
                                visible += 1;
                            }
                        }
                    }
                    let Some(first) = first else {
                        continue;
                    };
                    let issue = if visible != usize::from(run.columns) * usize::from(run.rows) {
                        Some(Issue::Clipped)
                    } else {
                        let mut glyph = run.clone();
                        glyph.x = first.0;
                        glyph.y = first.1;
                        (!output.push(NativeText { glyph, style })).then_some(Issue::Capacity)
                    };
                    if let Some(issue) = issue {
                        // Native multicells cannot be bisected safely. Keep their logical origin
                        // and atomic source map; mark the visible omission instead of overprinting.
                        frame.buffer_mut()[first]
                            .set_symbol("⋮")
                            .set_style(style.patch(palette.style(Role::Muted)));
                        if !issues.contains(&(entry.surface, issue)) {
                            issues.push((entry.surface, issue));
                        }
                    }
                }
            }
        }
        for (surface, issue) in issues {
            if let Some(surface) = surfaces.get(surface) {
                notice(
                    frame,
                    surface.bounds,
                    match issue {
                        Issue::Clipped => " Math clipped ",
                        Issue::Capacity => " Math limit ",
                    },
                    palette.style(Role::Muted),
                );
            }
        }
        output.finish();
        output
    }
}

fn screen_origin(start: usize, offset: usize, slack: usize, top: u16) -> Option<i64> {
    Some(
        i64::try_from(start).ok()? - i64::try_from(offset).ok()?
            + i64::try_from(slack).ok()?
            + i64::from(top),
    )
}

fn visible_cell(clip: Rect, surfaces: &SurfaceTree, owner: SurfaceId, point: (u16, u16)) -> bool {
    let point = ratatui::layout::Position::from(point);
    if !clip.contains(point) {
        return false;
    }
    let Some(owner) = surfaces.get(owner) else {
        return false;
    };
    !surfaces.iter().any(|other| {
        (other.z_index, other.id) > (owner.z_index, owner.id) && other.bounds.contains(point)
    })
}

fn notice(frame: &mut Frame<'_>, bounds: Rect, label: &str, style: Style) {
    // Use only an existing empty border stretch: no title, attention badge or overlay is replaced.
    let bounds = bounds.intersection(frame.area());
    let mut start = bounds.x;
    let mut length = 0;
    for x in bounds.x..bounds.right() {
        if frame.buffer_mut()[(x, bounds.y)].symbol() == "─" {
            if length == 0 {
                start = x;
            }
            length += 1;
            if length >= label.width() {
                frame.buffer_mut().set_string(start, bounds.y, label, style);
                return;
            }
        } else {
            length = 0;
        }
    }
}
