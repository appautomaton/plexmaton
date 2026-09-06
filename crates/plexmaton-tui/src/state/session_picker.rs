//! Read-only session discovery supplied by the composition root; no storage access here.
use super::{CommandPalette, ViewState};
use crate::SurfaceId;
use plexmaton_core::ConversationId;

pub(crate) const VISIBLE_CONVERSATIONS: usize = 6;
pub const MAX_CONVERSATION_CHOICES: usize = 200;

/// A bounded display projection, never a journal or provider replay payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationChoice {
    pub id: ConversationId,
    pub title: String,
}

/// Explicit discovery/open lifecycle; failures leave the current conversation untouched.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversationPickerStatus {
    Loading,
    Ready,
    Opening,
    ListFailed,
    OpenFailed,
    Busy,
    DraftPresent,
}

impl ConversationPickerStatus {
    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::Loading => "Loading saved conversations…",
            Self::Ready => "No saved conversation matches",
            Self::Opening => "Opening conversation…",
            Self::ListFailed => "Could not read saved conversations. Esc to return.",
            Self::OpenFailed => "Cannot open: check configuration or conversation file.",
            Self::Busy => "Stop the current run before switching conversations.",
            Self::DraftPresent => "Send or clear your draft before switching conversations.",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConversationPicker {
    pub entries: Vec<ConversationChoice>,
    pub status: ConversationPickerStatus,
    pub limited: bool,
}

impl ConversationPicker {
    pub fn matches(&self, query: &str) -> Vec<&ConversationChoice> {
        let query = query.trim().to_lowercase();
        self.entries
            .iter()
            .filter(|entry| {
                entry.title.to_lowercase().contains(&query)
                    || entry.id.as_str().to_lowercase().contains(&query)
            })
            .collect()
    }
}

impl CommandPalette {
    pub fn is_conversation_picker(&self) -> bool {
        self.conversations().is_some()
    }

    pub(crate) fn choice_window(&self, height: u16) -> std::ops::Range<usize> {
        let reserved = 4
            + 2 * self.choice_gap(height)
            + u16::from(self.conversation_note_visible(height))
            + 2 * crate::surface::ContentInsets::for_surface(SurfaceId::CommandPalette, height)
                .vertical;
        let limit = if self.is_conversation_picker() {
            VISIBLE_CONVERSATIONS
        } else {
            super::Command::ALL.len()
        };
        let count = usize::from(height.saturating_sub(reserved)).clamp(1, limit);
        let start = self.chosen_index().saturating_sub(count - 1);
        start..start + count
    }

    pub(crate) fn conversation_rows_visible(&self, height: u16) -> bool {
        self.content_height(height) >= 4
            || self
                .conversations()
                .is_some_and(|s| s.status == ConversationPickerStatus::Ready)
    }

    /// INV-13: optional spacing yields before the selected row, status or footer.
    pub(crate) fn choice_gap(&self, height: u16) -> u16 {
        u16::from(
            self.content_height(height) >= 5 + u16::from(self.conversation_note_visible(height)),
        )
    }

    pub(crate) fn conversation_note_visible(&self, height: u16) -> bool {
        self.is_conversation_picker()
            && (self.content_height(height) >= 4
                || self.match_count() == 0
                || !self.conversation_rows_visible(height))
    }

    fn content_height(&self, height: u16) -> u16 {
        height.saturating_sub(
            2 + 2 * crate::surface::ContentInsets::for_surface(SurfaceId::CommandPalette, height)
                .vertical,
        )
    }

    pub(crate) fn chosen_conversation(&self) -> Option<ConversationId> {
        let sessions = self.conversations()?;
        if !matches!(
            sessions.status,
            ConversationPickerStatus::Ready | ConversationPickerStatus::OpenFailed
        ) {
            return None;
        }
        sessions
            .matches(self.filter().text())
            .get(self.chosen_index())
            .map(|entry| entry.id.clone())
    }
}

impl ViewState {
    pub(crate) fn open_conversation_picker(&mut self) {
        let focus = self
            .command_palette
            .as_ref()
            .map_or(SurfaceId::Composer, CommandPalette::return_focus);
        let mut palette = CommandPalette::opened_from(focus);
        palette.page = super::command_palette::PalettePage::Conversations(ConversationPicker {
            entries: Vec::new(),
            status: ConversationPickerStatus::Loading,
            limited: false,
        });
        self.command_palette = Some(palette);
        self.focus.prefer(SurfaceId::CommandPalette);
        self.touch();
    }

    pub(crate) fn set_conversation_choices(
        &mut self,
        mut entries: Vec<ConversationChoice>,
        limited: bool,
    ) {
        let Some(palette) = self.command_palette.as_mut() else {
            return;
        };
        let Some(sessions) = palette.conversations_mut() else {
            return;
        };
        sessions.limited = limited || entries.len() > MAX_CONVERSATION_CHOICES;
        entries.truncate(MAX_CONVERSATION_CHOICES);
        sessions.entries = entries;
        sessions.status = ConversationPickerStatus::Ready;
        palette.reclamp();
        self.touch();
    }

    pub(crate) fn session_picker_status(&mut self, status: ConversationPickerStatus) {
        if let Some(sessions) = self
            .command_palette
            .as_mut()
            .and_then(|p| p.conversations_mut())
        {
            if sessions.status == status {
                return;
            }
            sessions.status = status;
            self.touch();
        }
    }
}
