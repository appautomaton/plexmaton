//! One step's streaming half: the message it assembles and the calls it asks for.
//!
//! A step is one request to the model. Everything it produces arrives as deltas, so the message on
//! screen and the message the record keeps are built here from the same text — the deltas are what
//! the reader watches, and the assembled string is what the model is shown next.

use plexmaton_core::{SessionEvent, TranscriptItemId, TranscriptRole};

use crate::interface::Reaction;
use crate::model::RequestItem;
use crate::record::Record;
use crate::tools::ToolCall;

/// What one step has assembled so far.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Step {
    /// The transcript item, once a delta has opened it.
    item: Option<TranscriptItemId>,
    /// Revisions issued for that item so far.
    revision: u64,
    /// Text assembled from the deltas, which is what the record keeps.
    text: String,
    /// Calls this step has asked for, held until it ends so they dispatch as one batch.
    calls: Vec<ToolCall>,
    /// Which step of the turn this is, counting from one.
    index: u16,
}

impl Step {
    /// Opens the `index`th step of a turn.
    pub(crate) fn new(index: u16) -> Self {
        Self {
            item: None,
            revision: 0,
            text: String::new(),
            calls: Vec::new(),
            index,
        }
    }

    /// Which step of the turn this is.
    pub(crate) fn index(&self) -> u16 {
        self.index
    }

    /// Appends one delta, opening the assistant message if this is the first.
    ///
    /// Opening lazily is what lets a step that says nothing leave nothing behind. The revision
    /// counts every delta, so a projection can detect a lost or repeated one without comparing
    /// text.
    pub(crate) fn append(&mut self, record: &mut Record, reaction: &mut Reaction, delta: String) {
        let item_id = match &self.item {
            Some(open) => open.clone(),
            None => {
                let opened = record.next_item_id();
                self.item = Some(opened.clone());
                let started = SessionEvent::TranscriptItemStarted {
                    agent_id: record.agent_id().clone(),
                    item_id: opened.clone(),
                    role: TranscriptRole::Assistant,
                };
                record.emit(reaction, started);
                opened
            }
        };
        self.text.push_str(&delta);
        self.revision = self.revision.saturating_add(1);
        let item_revision = self.revision;
        record.emit(
            reaction,
            SessionEvent::TranscriptDelta {
                agent_id: record.agent_id().clone(),
                item_id,
                item_revision,
                text: delta,
            },
        );
    }

    /// Holds a call until the step ends, because a step's calls dispatch as one batch.
    pub(crate) fn collect(&mut self, call: ToolCall) {
        self.calls.push(call);
    }

    /// Ends the step: finalizes the message, records what it said, hands back what it asked for.
    ///
    /// A step that produced no text finalizes nothing and records nothing. An empty assistant
    /// message is a blank row on screen and an empty turn in the next request, which is worse than
    /// the absence it would be recording.
    pub(crate) fn close(self, record: &mut Record, reaction: &mut Reaction) -> Vec<ToolCall> {
        if let Some(item_id) = self.item {
            record.emit(
                reaction,
                SessionEvent::TranscriptItemFinalized {
                    agent_id: record.agent_id().clone(),
                    item_id,
                    item_revision: self.revision.saturating_add(1),
                },
            );
        }
        if !self.text.is_empty() {
            record.push(RequestItem::Assistant { text: self.text });
        }
        self.calls
    }
}
