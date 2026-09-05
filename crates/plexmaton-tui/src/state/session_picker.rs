//! Read-only session discovery supplied by the composition root; no storage access here.
use super::{CommandPalette, ViewState};
use crate::SurfaceId;
use plexmaton_core::SessionId;

pub(crate) const VISIBLE_SESSIONS: usize = 6;
pub const MAX_SESSION_CHOICES: usize = 200;

/// A bounded display projection, never a journal or provider replay payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionChoice {
    pub id: SessionId,
    pub title: String,
}

/// Explicit discovery/open lifecycle; failures leave the current conversation untouched.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionPickerStatus {
    Loading,
    Ready,
    Opening,
    ListFailed,
    OpenFailed,
    Busy,
    DraftPresent,
}

impl SessionPickerStatus {
    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::Loading => "Loading saved conversations…",
            Self::Ready => "No saved conversation matches",
            Self::Opening => "Restoring conversation…",
            Self::ListFailed => "Could not read saved conversations. Esc to return.",
            Self::OpenFailed => "Cannot open: session locked, damaged, or missing.",
            Self::Busy => "Stop the current run before switching conversations.",
            Self::DraftPresent => "Send or clear your draft before switching conversations.",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SessionPicker {
    pub entries: Vec<SessionChoice>,
    pub status: SessionPickerStatus,
    pub limited: bool,
}

impl SessionPicker {
    pub fn matches(&self, query: &str) -> Vec<&SessionChoice> {
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
    pub fn is_session_picker(&self) -> bool {
        self.sessions.is_some()
    }

    pub(crate) fn choice_window(&self, height: u16) -> std::ops::Range<usize> {
        let reserved = if self.is_session_picker() { 5 } else { 4 };
        let limit = if self.is_session_picker() {
            VISIBLE_SESSIONS
        } else {
            super::Command::ALL.len()
        };
        let count = usize::from(height.saturating_sub(reserved)).clamp(1, limit);
        let start = self.chosen_index().saturating_sub(count - 1);
        start..start + count
    }

    pub(crate) fn session_rows_visible(&self, height: u16) -> bool {
        height >= 6
            || self
                .sessions
                .as_ref()
                .is_some_and(|s| s.status == SessionPickerStatus::Ready)
    }

    pub(crate) fn chosen_session(&self) -> Option<SessionId> {
        let sessions = self.sessions.as_ref()?;
        if !matches!(
            sessions.status,
            SessionPickerStatus::Ready | SessionPickerStatus::OpenFailed
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
    pub(crate) fn open_session_picker(&mut self) {
        let focus = self
            .command_palette
            .as_ref()
            .map_or(SurfaceId::Composer, CommandPalette::return_focus);
        let mut palette = CommandPalette::opened_from(focus);
        palette.sessions = Some(SessionPicker {
            entries: Vec::new(),
            status: SessionPickerStatus::Loading,
            limited: false,
        });
        self.command_palette = Some(palette);
        self.focus.prefer(SurfaceId::CommandPalette);
        self.touch();
    }

    pub(crate) fn set_session_choices(&mut self, mut entries: Vec<SessionChoice>, limited: bool) {
        let Some(palette) = self.command_palette.as_mut() else {
            return;
        };
        let Some(sessions) = palette.sessions.as_mut() else {
            return;
        };
        sessions.limited = limited || entries.len() > MAX_SESSION_CHOICES;
        entries.truncate(MAX_SESSION_CHOICES);
        sessions.entries = entries;
        sessions.status = SessionPickerStatus::Ready;
        palette.reclamp();
        self.touch();
    }

    pub(crate) fn session_picker_status(&mut self, status: SessionPickerStatus) {
        if let Some(sessions) = self
            .command_palette
            .as_mut()
            .and_then(|p| p.sessions.as_mut())
        {
            if sessions.status == status {
                return;
            }
            sessions.status = status;
            self.touch();
        }
    }
}
