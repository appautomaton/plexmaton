//! What the workspace's inputs are *for*.
//!
//! The text itself is [`super::TextInput`]. This is the other half: which agent an input addresses,
//! what submitting it means, and where undelivered text goes back to. Keeping them apart is what
//! lets one editing model serve the primary composer, an entered worker's steering input and the
//! Drawer's filter without any of them growing a copy of the other's rules.

use plexmaton_core::AgentId;

use super::{Submission, SubmissionKind, TextInput, ViewState};
use crate::{
    intent::TextIntent,
    surface::{SurfaceId, SurfaceTree},
};

impl ViewState {
    /// Place the caret using the input rectangle from the frame receiving the click (COM-1).
    pub(crate) fn click_text_input(
        &mut self,
        surfaces: &SurfaceTree,
        surface: SurfaceId,
        at: crate::Point,
    ) -> bool {
        let Some(area) = self.input_area(surfaces, surface) else {
            return false;
        };
        if at.x <= area.x || at.x >= area.right() - 1 || at.y <= area.y || at.y >= area.bottom() - 1
        {
            return false;
        }
        let width = super::inner_width(area.width);
        let column = at.x - area.x - 1;
        let row = at.y - area.y - 1;
        if surface == SurfaceId::Drawer && row != 0 {
            return false;
        }
        if surface == SurfaceId::Drawer {
            if let Some(drawer) = self.drawer.as_mut() {
                drawer.click_filter(width, column);
            }
        } else if let Some(input) = self.input_mut(surface) {
            input.click(width, row, column);
        }
        if let Some(input) = self.input_mut(surface) {
            input.begin_selection();
        }
        if surface == SurfaceId::Composer {
            self.sync_skill_picker();
        }
        self.touch();
        true
    }

    fn input_area(
        &self,
        surfaces: &SurfaceTree,
        surface: SurfaceId,
    ) -> Option<ratatui::layout::Rect> {
        if surface == SurfaceId::Drawer && !self.drawer.as_ref()?.takes_text() {
            return None;
        }
        match surface {
            SurfaceId::Inspector => self.steer_input(surfaces).map(|(split, _)| split.input),
            SurfaceId::Composer | SurfaceId::Drawer => surfaces.get(surface).map(|entry| {
                crate::surface::ContentInsets::for_surface(surface, entry.bounds.height)
                    .inset(entry.bounds)
            }),
            _ => None,
        }
    }

    fn input_mut(&mut self, surface: SurfaceId) -> Option<&mut TextInput> {
        if surface == SurfaceId::Drawer {
            self.drawer.as_mut().and_then(super::Drawer::filter_mut)
        } else {
            let target = match surface {
                SurfaceId::Composer => self.agents.primary().map(|agent| agent.id.clone()),
                SurfaceId::Inspector => self.inspector().map(|inspector| inspector.agent),
                _ => None,
            };
            target.map(|id| self.inputs.entry(id).or_default())
        }
    }

    pub(crate) fn input_dragging(&mut self, surface: SurfaceId) -> bool {
        self.input_mut(surface)
            .is_some_and(|input| input.is_dragging())
    }

    pub(crate) fn drag_text_input(
        &mut self,
        surfaces: &SurfaceTree,
        pointer: crate::PointerIntent,
    ) -> Option<super::CopyRequest> {
        use crate::PointerIntent;
        let (surface, at, release) = match pointer {
            PointerIntent::Drag { surface, at } => (surface, at, false),
            PointerIntent::Release { surface, at } => (surface, at, true),
            PointerIntent::Cancel { surface } => {
                if let Some(input) = self.input_mut(surface) {
                    input.clear_selection();
                }
                self.touch();
                return None;
            }
            _ => return None,
        };
        let area = self.input_area(surfaces, surface)?;
        let width = super::inner_width(area.width);
        let column = at.x.saturating_sub(area.x + 1).min(width);
        let row =
            at.y.saturating_sub(area.y + 1)
                .min(area.height.saturating_sub(3));
        if surface == SurfaceId::Drawer {
            if let Some(drawer) = self.drawer.as_mut() {
                drawer.drag_filter(width, column);
            }
        } else if let Some(input) = self.input_mut(surface) {
            input.drag_to(width, row, column);
        }
        let copied = if release {
            self.input_mut(surface)?.finish_selection()
        } else {
            None
        };
        self.touch();
        copied.map(|text| super::CopyRequest { text, entries: 0 })
    }

    pub(crate) fn copy_input(&self, surfaces: &SurfaceTree) -> Option<super::CopyRequest> {
        let input = if self.focus.resolve(surfaces) == Some(SurfaceId::Drawer) {
            self.drawer.as_ref()?.filter()
        } else {
            self.draft(&self.text_target(surfaces)?)
        };
        input.selected_text().map(|text| super::CopyRequest {
            text: text.to_owned(),
            entries: 0,
        })
    }

    pub(crate) fn clear_input_selection(&mut self, surfaces: &SurfaceTree) -> bool {
        let changed = self
            .focus
            .resolve(surfaces)
            .and_then(|surface| self.input_mut(surface))
            .is_some_and(TextInput::clear_selection);
        if changed {
            self.touch();
        }
        changed
    }

    /// Restores runtime-returned user text without inventing a transcript item (COM-3, LOOP-6).
    pub(crate) fn return_input(&mut self, to: AgentId, text: String) {
        self.return_skill_input(to, text, None);
    }

    pub(crate) fn return_skill_input(&mut self, to: AgentId, text: String, skill: Option<String>) {
        let was_empty = self
            .inputs
            .get(&to)
            .is_none_or(|input| input.text().is_empty());
        self.inputs
            .entry(to.clone())
            .or_default()
            .append_returned(&text);
        if was_empty
            && let Some(skill) = skill
            && super::skill_picker::binding_matches(&text, &skill)
        {
            self.skill_bindings.insert(to, skill);
        }
        self.touch();
    }

    /// Applies one edit to whichever input holds the cursor.
    ///
    /// The returned submission is a *command* for the runtime, never something to write into the
    /// transcript here: the projection has one writer, and it is the event stream (COM-3).
    ///
    /// The target comes from focus rather than from the intent. The router only produces a text
    /// intent while a text input holds the cursor, and it reads that from this same state, so the
    /// two cannot disagree about which of the two inputs is being typed into (INV-2).
    pub fn edit(&mut self, surfaces: &SurfaceTree, intent: TextIntent) -> Option<Submission> {
        // The Drawer is a text input too, but its text is a filter rather than a message: it never
        // leaves as a `Submission`, so it resolves here and returns nothing.
        if self.focus.resolve(surfaces)? == SurfaceId::Drawer {
            self.edit_drawer_filter(intent);
            return None;
        }
        let kind = match self.focus.resolve(surfaces)? {
            SurfaceId::Composer => SubmissionKind::Message,
            SurfaceId::Inspector => SubmissionKind::Steering,
            _ => return None,
        };
        let to = self.text_target(surfaces)?;
        if let TextIntent::Submit = intent {
            let skill = (kind == SubmissionKind::Message)
                .then(|| self.selected_skill(&to).map(str::to_owned))
                .flatten();
            let submitted = self.inputs.entry(to.clone()).or_default().take();
            if submitted.is_some() {
                self.take_skill_binding(&to);
                self.close_skill_picker();
                self.touch();
            }
            return submitted.map(|text| Submission {
                to,
                text,
                kind,
                skill,
            });
        }
        let input = self.inputs.entry(to).or_default();
        let changed = apply_text(input, intent);
        if changed {
            if kind == SubmissionKind::Message {
                self.sync_skill_picker();
            }
            self.touch();
        }
        None
    }
}

/// Applies one edit to whichever input the caller owns.
///
/// One table, so the composer, an entered worker's input and the command filter cannot drift into
/// three slightly different editing grammars. `Submit` is not here: what submitting *means* is the
/// caller's, which is the whole reason the text and its purpose are separate types.
pub(crate) fn apply_text(input: &mut TextInput, intent: TextIntent) -> bool {
    match intent {
        TextIntent::Paste(text) => input.paste(&text),
        TextIntent::Insert(character) => {
            input.insert(character);
            true
        }
        TextIntent::Newline => {
            input.newline();
            true
        }
        TextIntent::DeleteBackward => input.delete_backward(),
        TextIntent::DeleteForward => input.delete_forward(),
        TextIntent::DeleteWordBackward => input.delete_word_backward(),
        TextIntent::KillToLineStart => input.kill_to_line_start(),
        TextIntent::KillToLineEnd => input.kill_to_line_end(),
        TextIntent::Move(motion) => input.move_caret(motion),
        TextIntent::Submit => false,
    }
}
