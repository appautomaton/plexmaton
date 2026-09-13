//! Label, branch-name and exact-source requests for the retained tree selection (TRE-8).

use plexmaton_core::{
    HeadName, MAX_TREE_LABEL_BYTES, TreeEdit, TreeEditAction, TreeLabel, TreeSourceRequest,
};

use crate::{intent::TextIntent, state::TextInput};

use super::{ConversationTree, TreeEditor, TreeMode};

impl ConversationTree {
    pub(crate) fn begin_rename_head(&mut self) -> bool {
        if self.mode() != TreeMode::Heads || self.is_pending() || self.editor().is_some() {
            return false;
        }
        let Some(head) = self.selected_head().cloned() else {
            return false;
        };
        let mut input = TextInput::new();
        input.replace_all(head.as_str());
        self.editor = Some(TreeEditor::RenameHead { head, input });
        self.notice = None;
        true
    }

    pub(crate) fn begin_set_label(&mut self) -> bool {
        if self.mode() != TreeMode::Entries || self.is_pending() || self.editor().is_some() {
            return false;
        }
        let Some(row) = self.selected_entry_row() else {
            return false;
        };
        let entry_id = row.entry_id.clone();
        let mut input = TextInput::new();
        if let Some(label) = &row.label {
            input.replace_all(label.as_str());
        }
        self.editor = Some(TreeEditor::SetLabel { entry_id, input });
        self.notice = None;
        true
    }

    pub(crate) fn begin_abandon_head(&mut self) -> bool {
        if self.mode() != TreeMode::Heads || self.is_pending() || self.editor().is_some() {
            return false;
        }
        let Some(head) = self.selected_head().cloned() else {
            return false;
        };
        self.editor = Some(TreeEditor::ConfirmAbandon(head));
        self.notice = None;
        true
    }

    /// Applies only single-line edits; Enter and Escape are reduced by the tree owner.
    pub(crate) fn edit_input(&mut self, intent: TextIntent) -> bool {
        let Some(input) = self.editor.as_mut().and_then(TreeEditor::input_mut) else {
            return false;
        };
        match intent {
            TextIntent::Insert(character) if !character.is_control() => {
                input.insert(character);
                true
            }
            TextIntent::Insert(_) | TextIntent::Newline | TextIntent::Submit => false,
            TextIntent::Paste(text) => {
                let clean = text
                    .chars()
                    .map(|character| {
                        if character.is_control() {
                            ' '
                        } else {
                            character
                        }
                    })
                    .collect::<String>();
                input.paste(&clean)
            }
            TextIntent::DeleteBackward => input.delete_backward(),
            TextIntent::DeleteForward => input.delete_forward(),
            TextIntent::DeleteWordBackward => input.delete_word_backward(),
            TextIntent::KillToLineStart => input.kill_to_line_start(),
            TextIntent::KillToLineEnd => input.kill_to_line_end(),
            TextIntent::Move(motion) => input.move_caret(motion),
            TextIntent::MoveRow(_) => false,
        }
    }

    pub(crate) fn edit_request(&self) -> Result<Option<TreeEdit>, String> {
        let Some(snapshot) = &self.snapshot else {
            return Err("History is unavailable. Press r to refresh.".to_owned());
        };
        let Some(editor) = self.editor() else {
            return Ok(None);
        };
        let action = match editor {
            TreeEditor::RenameHead { head, input } => {
                if input.text().len() > MAX_TREE_LABEL_BYTES {
                    return Err("A branch name must fit in 256 UTF-8 bytes.".to_owned());
                }
                let renamed = HeadName::new(input.text().to_owned())
                    .map_err(|_| "A branch name must contain text.".to_owned())?;
                if &renamed == head {
                    return Ok(None);
                }
                TreeEditAction::RenameHead {
                    head: head.clone(),
                    renamed,
                }
            }
            TreeEditor::SetLabel { entry_id, input } => {
                let label = if input.text().trim().is_empty() {
                    None
                } else {
                    Some(
                        TreeLabel::new(input.text().to_owned())
                            .map_err(|error| error.to_string())?,
                    )
                };
                if self
                    .rows()
                    .iter()
                    .find(|row| &row.entry_id == entry_id)
                    .is_some_and(|row| row.label == label)
                {
                    return Ok(None);
                }
                TreeEditAction::SetLabel {
                    entry_id: entry_id.clone(),
                    label,
                }
            }
            TreeEditor::ConfirmAbandon(head) => TreeEditAction::AbandonHead { head: head.clone() },
        };
        Ok(Some(TreeEdit {
            origin: snapshot.origin.clone(),
            action,
        }))
    }

    pub(crate) fn source_request(&self) -> Option<TreeSourceRequest> {
        let snapshot = self.snapshot.as_ref()?;
        let entry_id = match self.mode() {
            TreeMode::Entries => self.selected_entry(),
            TreeMode::Heads => self
                .selected_head()
                .and_then(|selected| {
                    snapshot
                        .rows
                        .iter()
                        .find(|row| row.head_markers.contains(selected))
                })
                .map(|row| &row.entry_id),
        }?;
        Some(TreeSourceRequest {
            origin: snapshot.origin.clone(),
            entry_id: entry_id.clone(),
        })
    }
}
