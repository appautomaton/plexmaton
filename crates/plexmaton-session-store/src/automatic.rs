//! A file is materialized only when the first user turn reaches the journal writer.
use crate::{ConversationDirectory, JournalFile, StoreError};
use plexmaton_agent::{ConversationMetadata, JournalEntryPayload, JournalRecord, UnixMillis};
use plexmaton_core::ConversationId;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Automatic startup retains only its one bootstrap announcement until user input exists.
/// After that boundary, all appends use the ordinary file adapter without buffering.
pub struct AutomaticJournal {
    root: PathBuf,
    path: PathBuf,
    metadata: ConversationMetadata,
    state: State,
}

enum State {
    Empty,
    Announced(Box<JournalRecord>),
    File(Box<JournalFile>),
    Failed,
}

impl AutomaticJournal {
    /// Allocates an identity and chronology without accessing or creating a directory/file.
    pub fn new(root: impl AsRef<Path>, created_at: UnixMillis) -> Self {
        let id = ConversationId::new(format!("session-{}", Uuid::now_v7()))
            .unwrap_or_else(|_| unreachable!("a generated UUIDv7 is a valid identity"));
        let root = root.as_ref().to_path_buf();
        let path = root.join("sessions").join(format!("{id}.jsonl"));
        Self {
            root,
            path,
            metadata: ConversationMetadata::new(id, created_at),
            state: State::Empty,
        }
    }

    /// The eventual filename, not evidence that the file already exists.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Identical metadata is handed to the reducer and written into the eventual header.
    pub fn metadata(&self) -> &ConversationMetadata {
        &self.metadata
    }

    /// Retains one announcement; the first user or collaboration turn writes it and the turn before returning.
    /// Other pre-turn mutations are rejected, so this never grows a second in-memory transcript.
    pub fn append(&mut self, record: JournalRecord) -> Result<(), StoreError> {
        if let State::File(file) = &mut self.state {
            return file
                .append(record)
                .map_err(|failure| failure.into_parts().0);
        }
        match std::mem::replace(&mut self.state, State::Failed) {
            State::Empty if is_bootstrap(&record) => {
                self.state = State::Announced(Box::new(record))
            }
            State::Announced(bootstrap) if is_first_turn(&record) => {
                let mut file = ConversationDirectory::under(&self.root)?.create(
                    self.metadata.conversation_id().clone(),
                    self.metadata.created_at_unix_ms(),
                )?;
                for record in [*bootstrap, record] {
                    file.append(record)
                        .map_err(|failure| failure.into_parts().0)?;
                }
                self.state = State::File(Box::new(file));
            }
            State::Failed => return Err(StoreError::WriterPoisoned),
            _ => return Err(StoreError::InvalidAutomaticBootstrap),
        }
        Ok(())
    }
}

fn is_bootstrap(record: &JournalRecord) -> bool {
    matches!(record, JournalRecord::AppendEntry { entry, .. } if matches!(entry.payload, JournalEntryPayload::AgentCreated { .. }))
}
fn is_first_turn(record: &JournalRecord) -> bool {
    matches!(record, JournalRecord::AppendEntry { entry, .. } if matches!(entry.payload, JournalEntryPayload::TurnStarted { .. } | JournalEntryPayload::CollaborationTurnStarted { .. }))
}
