//! Read-only conversation discovery supplied by the composition root; no storage access here.
use super::{Drawer, TextInput, ViewState, drawer::Shown};
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
            Self::ListFailed => "Could not read saved conversations. New conversation still works.",
            Self::OpenFailed => "Cannot open: check configuration or conversation file.",
            Self::Busy => "Stop the current run before switching conversations.",
            Self::DraftPresent => "Send or clear your draft before switching conversations.",
        }
    }
}

/// What the user asked the Conversations page to open.
///
/// Only the composition root can do it: a new conversation needs the replacement owner and a saved
/// one its loader (SPK-2, SPK-3), so the request leaves the workspace as a value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConversationRequest {
    /// An empty conversation; storage is created on its first submitted message.
    New,
    /// A saved conversation, by identity.
    Saved(ConversationId),
}

/// One row of the page: the standing first row, or a saved conversation the query admits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConversationRow<'a> {
    New,
    Saved(&'a ConversationChoice),
}

impl<'a> ConversationRow<'a> {
    pub(crate) const fn label(self) -> &'a str {
        match self {
            Self::New => "New conversation",
            Self::Saved(entry) => entry.title.as_str(),
        }
    }

    /// The row's note: what starting fresh means, or which file a saved row is.
    pub(crate) fn note(self) -> &'a str {
        match self {
            Self::New => "Starts empty and is saved on its first message",
            Self::Saved(entry) => entry.id.as_str(),
        }
    }

    fn request(self) -> ConversationRequest {
        match self {
            Self::New => ConversationRequest::New,
            Self::Saved(entry) => ConversationRequest::Saved(entry.id.clone()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConversationPicker {
    pub entries: Vec<ConversationChoice>,
    pub status: ConversationPickerStatus,
    pub limited: bool,
    /// The page's own search, so returning to the list finds the list's filter untouched.
    pub(crate) query: TextInput,
    pub(crate) chosen: usize,
}

impl ConversationPicker {
    pub(crate) const fn loading() -> Self {
        Self {
            entries: Vec::new(),
            status: ConversationPickerStatus::Loading,
            limited: false,
            query: TextInput::new(),
            chosen: 0,
        }
    }

    pub(crate) fn opening(&self) -> bool {
        self.status == ConversationPickerStatus::Opening
    }

    /// New conversation first, then every saved conversation the query admits.
    ///
    /// The first row stands whatever the listing did: a history that cannot be read is no reason
    /// to refuse a fresh start.
    pub(crate) fn rows(&self) -> Vec<ConversationRow<'_>> {
        let query = self.query.text().trim().to_lowercase();
        let mut rows = Vec::with_capacity(self.entries.len() + 1);
        if "new conversation".contains(&query) {
            rows.push(ConversationRow::New);
        }
        rows.extend(
            self.entries
                .iter()
                .filter(|entry| {
                    entry.title.to_lowercase().contains(&query)
                        || entry.id.as_str().to_lowercase().contains(&query)
                })
                .map(ConversationRow::Saved),
        );
        rows
    }
}

impl Drawer {
    pub fn is_conversation_picker(&self) -> bool {
        self.conversations().is_some()
    }

    pub(crate) fn choice_window(&self, height: u16) -> std::ops::Range<usize> {
        let reserved = 4
            + 2 * self.choice_gap(height)
            + u16::from(self.conversation_note_visible(height))
            + 2 * crate::surface::ContentInsets::for_surface(SurfaceId::Drawer, height).vertical;
        let limit = if self.is_conversation_picker() {
            VISIBLE_CONVERSATIONS
        } else {
            super::Page::ALL.len()
        };
        let count = usize::from(height.saturating_sub(reserved)).clamp(1, limit);
        let start = self.chosen_index().saturating_sub(count - 1);
        start..start + count
    }

    /// DRW-2: optional spacing yields before the selected row, status or footer.
    pub(crate) fn choice_gap(&self, height: u16) -> u16 {
        u16::from(
            self.content_height(height) >= 5 + u16::from(self.conversation_note_visible(height)),
        )
    }

    /// The row under the marker explains itself on the page's note row.
    pub(crate) fn conversation_note_visible(&self, _height: u16) -> bool {
        self.is_conversation_picker()
    }

    fn content_height(&self, height: u16) -> u16 {
        height.saturating_sub(
            2 + 2 * crate::surface::ContentInsets::for_surface(SurfaceId::Drawer, height).vertical,
        )
    }

    /// The row under the marker, once the page can act on it (`conversation_request_at`).
    pub(crate) fn chosen_request(&self) -> Option<ConversationRequest> {
        self.conversation_request_at(self.conversations()?.chosen)
    }

    /// A row's request, once the page can act on it: a saved row needs the listing, and nothing
    /// is chosen while an open is in flight.
    pub(crate) fn conversation_request_at(&self, index: usize) -> Option<ConversationRequest> {
        let picker = self.conversations()?;
        let row = *picker.rows().get(index)?;
        let admitted = match row {
            ConversationRow::New => matches!(
                picker.status,
                ConversationPickerStatus::Ready
                    | ConversationPickerStatus::OpenFailed
                    | ConversationPickerStatus::ListFailed
            ),
            ConversationRow::Saved(_) => matches!(
                picker.status,
                ConversationPickerStatus::Ready | ConversationPickerStatus::OpenFailed
            ),
        };
        admitted.then(|| row.request())
    }
}

impl ViewState {
    /// Shows the Conversations page, loading. A page already on screen keeps its rows and status:
    /// the composition root calls this before it lists and before it opens, and the second call
    /// must not blank what the first delivered.
    pub(crate) fn open_conversation_picker(&mut self) {
        if self
            .drawer
            .as_ref()
            .is_some_and(Drawer::is_conversation_picker)
        {
            return;
        }
        self.show_page(Shown::Conversations(ConversationPicker::loading()));
    }

    pub(crate) fn set_conversation_choices(
        &mut self,
        mut entries: Vec<ConversationChoice>,
        limited: bool,
    ) {
        let Some(drawer) = self.drawer.as_mut() else {
            return;
        };
        let Some(picker) = drawer.conversations_mut() else {
            return;
        };
        picker.limited = limited || entries.len() > MAX_CONVERSATION_CHOICES;
        entries.truncate(MAX_CONVERSATION_CHOICES);
        picker.entries = entries;
        picker.status = ConversationPickerStatus::Ready;
        drawer.reclamp();
        self.touch();
    }

    pub(crate) fn conversation_picker_status(&mut self, status: ConversationPickerStatus) {
        if let Some(picker) = self.drawer.as_mut().and_then(Drawer::conversations_mut) {
            if picker.status == status {
                return;
            }
            picker.status = status;
            self.touch();
        }
    }
}
