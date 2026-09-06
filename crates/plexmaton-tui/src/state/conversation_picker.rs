//! Read-only conversation discovery supplied by the composition root; no storage access here.
//!
//! The rows live in the composer menu behind `/resume` (SPK-1); this file owns what they are,
//! what leaves the workspace when one is chosen, and what a refused switch says (SPK-2).
use super::{
    ConversationNote, ViewState,
    composer_menu::{Command, Listing},
};
use plexmaton_core::ConversationId;

pub const MAX_CONVERSATION_CHOICES: usize = 200;

/// A bounded display projection, never a journal or provider replay payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationChoice {
    pub id: ConversationId,
    pub title: String,
}

/// Where `/resume`'s listing stands: its one status row, read under the rows or instead of them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversationPickerStatus {
    Loading,
    Ready,
    Opening,
    ListFailed,
    /// The chosen row did not open; the rows can be chosen again.
    Refused(SwitchRefusal),
}

impl ConversationPickerStatus {
    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::Loading => "Loading saved conversations…",
            Self::Ready => "No saved conversation matches",
            Self::Opening => "Opening conversation…",
            Self::ListFailed => "Could not read saved conversations.",
            Self::Refused(refusal) => refusal.message(),
        }
    }

    pub(crate) const fn is_failure(self) -> bool {
        match self {
            Self::ListFailed => true,
            Self::Refused(refusal) => refusal.is_failure(),
            Self::Loading | Self::Ready | Self::Opening => false,
        }
    }
}

/// Why a conversation did not open. The answer lands where the request still is: the listing's
/// status row for a chosen row, and a note on the conversation for `/new`, whose draft is gone
/// (SPK-2).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SwitchRefusal {
    Busy,
    DraftPresent,
    RequestInFlight,
    OpenFailed,
}

impl SwitchRefusal {
    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::Busy => "Stop the current run before switching conversations.",
            Self::DraftPresent => "Send or clear your draft before switching conversations.",
            Self::RequestInFlight => "Wait for the conversation request already running.",
            Self::OpenFailed => "Cannot open: check configuration or conversation file.",
        }
    }

    pub(crate) const fn is_failure(self) -> bool {
        matches!(self, Self::OpenFailed)
    }
}

/// What the user asked about conversations.
///
/// Only the composition root can do any of it: listing needs the directory, a new conversation
/// needs the replacement owner and a saved one its loader (SPK-2, SPK-3), so the request leaves
/// the workspace as a value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConversationRequest {
    /// The saved conversations, for `/resume`'s rows.
    List,
    /// An empty conversation; storage is created on its first submitted message.
    New,
    /// A saved conversation, by identity.
    Saved(ConversationId),
}

/// The composition root's standing permission to list or switch (SPK-3), and `/resume`'s rows
/// while it lists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ConversationPicker {
    /// `/resume`'s rows and where they stand; the draft is their query.
    Listing {
        entries: Vec<ConversationChoice>,
        status: ConversationPickerStatus,
        limited: bool,
    },
    /// `/new` in flight: nothing to show, and a place for its result to land.
    Switching,
}

impl ConversationPicker {
    pub(crate) const fn loading() -> Self {
        Self::Listing {
            entries: Vec::new(),
            status: ConversationPickerStatus::Loading,
            limited: false,
        }
    }

    pub(crate) const fn status(&self) -> ConversationPickerStatus {
        match self {
            Self::Listing { status, .. } => *status,
            Self::Switching => ConversationPickerStatus::Opening,
        }
    }

    /// Every saved conversation the query admits, newest first as listed.
    pub(crate) fn matching(&self, query: &str) -> Vec<&ConversationChoice> {
        let Self::Listing { entries, .. } = self else {
            return Vec::new();
        };
        let query = query.trim().to_lowercase();
        entries
            .iter()
            .filter(|entry| {
                entry.title.to_lowercase().contains(&query)
                    || entry.id.as_str().to_lowercase().contains(&query)
            })
            .collect()
    }

    /// Whether a saved row can be opened now: the listing is in, and no open is in flight.
    pub(crate) const fn admits_open(&self) -> bool {
        matches!(
            self,
            Self::Listing {
                status: ConversationPickerStatus::Ready | ConversationPickerStatus::Refused(_),
                ..
            }
        )
    }

    pub(crate) fn choice(&self, id: &ConversationId) -> Option<&ConversationChoice> {
        self.matching("").into_iter().find(|entry| &entry.id == id)
    }

    /// Whether the newest 200 stood in for a longer directory (SPK-1).
    pub(crate) const fn limited(&self) -> bool {
        matches!(self, Self::Listing { limited: true, .. })
    }
}

impl ViewState {
    /// Makes room for the listing the composition root is about to supply. A listing already on
    /// screen keeps its rows and status: the root calls this before it lists and before it opens,
    /// and the second call must not blank what the first delivered.
    pub(crate) fn open_conversation_picker(&mut self) {
        if self.composer_menu.conversations.is_some() {
            return;
        }
        self.composer_menu.conversations = Some(ConversationPicker::loading());
        self.sync_composer_menu();
        self.touch();
    }

    /// A switch is in flight: the listing's rows wait behind `Opening`, and `/new` gets a place
    /// for its result to land.
    pub(crate) fn begin_conversation_switch(&mut self) {
        match self.composer_menu.conversations.as_mut() {
            Some(ConversationPicker::Listing { status, .. }) => {
                *status = ConversationPickerStatus::Opening;
            }
            Some(ConversationPicker::Switching) => {}
            None => self.composer_menu.conversations = Some(ConversationPicker::Switching),
        }
        self.touch();
    }

    pub(crate) fn set_conversation_choices(
        &mut self,
        mut choices: Vec<ConversationChoice>,
        partial: bool,
    ) {
        let Some(ConversationPicker::Listing {
            entries,
            status,
            limited,
        }) = self.composer_menu.conversations.as_mut()
        else {
            return;
        };
        *limited = partial || choices.len() > MAX_CONVERSATION_CHOICES;
        choices.truncate(MAX_CONVERSATION_CHOICES);
        *entries = choices;
        *status = ConversationPickerStatus::Ready;
        self.sync_composer_menu();
        self.touch();
    }

    pub(crate) fn conversation_picker_status(&mut self, next: ConversationPickerStatus) {
        if let Some(ConversationPicker::Listing { status, .. }) =
            self.composer_menu.conversations.as_mut()
            && *status != next
        {
            *status = next;
            self.touch();
        }
    }

    /// Whether the composition root's permission to list or switch still stands (SPK-3).
    pub(crate) fn conversation_picker_open(&self) -> bool {
        self.composer_menu.conversations.is_some()
    }

    /// The conversation is open: the `/resume` draft that asked for it is consumed and the menu
    /// closes. Any other draft is the user's and stays.
    pub(crate) fn close_conversation_picker(&mut self) {
        self.composer_menu.conversations = None;
        if self.exact_command() == Some(Command::Resume) {
            self.take_command_draft();
        } else {
            self.sync_composer_menu();
        }
        self.touch();
    }

    /// The switch did not happen. A listing that asked says so in its status row and offers its
    /// rows again; `/new`, which has nothing left to wait for, is answered with a note (SPK-2).
    pub(crate) fn report_switch_refusal(&mut self, refusal: SwitchRefusal) {
        match self.composer_menu.conversations.as_mut() {
            // Another request was refused while this listing's own open is still in flight.
            Some(ConversationPicker::Listing {
                status: ConversationPickerStatus::Opening,
                ..
            }) if refusal == SwitchRefusal::RequestInFlight => {}
            Some(ConversationPicker::Listing { status, .. }) => {
                *status = ConversationPickerStatus::Refused(refusal);
                self.touch();
                return;
            }
            Some(ConversationPicker::Switching) => self.composer_menu.conversations = None,
            None => {}
        }
        if let Some(primary) = self.primary_agent().map(|agent| agent.id.clone()) {
            self.report_note(&primary, ConversationNote::SwitchRefused(refusal));
        }
        self.touch();
    }

    /// Drops a listing whose rows have no home: the draft stopped asking for them, and nothing is
    /// opening. The composition root reads the withdrawal and cancels its loader (SPK-3).
    pub(super) fn drop_unlisted_conversations(&mut self) -> bool {
        let listed = self.menu_listing() == Some(Listing::Conversations);
        let idle_listing = matches!(
            &self.composer_menu.conversations,
            Some(ConversationPicker::Listing { status, .. })
                if *status != ConversationPickerStatus::Opening
        );
        if listed || !idle_listing {
            return false;
        }
        self.composer_menu.conversations = None;
        true
    }

    /// The request a chosen row leaves as, once the listing can act on it.
    pub(crate) fn conversation_request(&self, id: &ConversationId) -> Option<ConversationRequest> {
        let picker = self.composer_menu.conversations.as_ref()?;
        (picker.admits_open() && picker.choice(id).is_some())
            .then(|| ConversationRequest::Saved(id.clone()))
    }
}
