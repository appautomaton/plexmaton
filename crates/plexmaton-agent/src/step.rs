//! One step's streaming half: the message it assembles and the calls it asks for.
//!
//! A step is one request to the model. Everything it produces arrives as deltas, so the message on
//! screen and the message the record keeps are built here from the same text — the deltas are what
//! the reader watches, and the assembled string is what the model is shown next.

use plexmaton_core::{SessionEvent, TranscriptItemId, TranscriptRole};

use crate::interface::Reaction;
use crate::model::{ProviderReplay, RequestItem};
use crate::record::Record;
use crate::tools::ToolCall;

/// What one step has assembled so far.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Step {
    answer: StreamedText,
    reasoning: StreamedText,
    replay: Vec<ProviderReplay>,
    /// Calls this step has asked for, held until it ends so they dispatch as one batch.
    calls: Vec<ToolCall>,
    /// Which step of the turn this is, counting from one.
    index: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StreamedText {
    role: TranscriptRole,
    item: Option<TranscriptItemId>,
    revision: u64,
    text: String,
}

impl StreamedText {
    const fn new(role: TranscriptRole) -> Self {
        Self {
            role,
            item: None,
            revision: 0,
            text: String::new(),
        }
    }

    fn append(&mut self, record: &mut Record, reaction: &mut Reaction, delta: String) {
        let item_id = match &self.item {
            Some(open) => open.clone(),
            None => {
                let opened = record.next_item_id();
                self.item = Some(opened.clone());
                record.emit(
                    reaction,
                    SessionEvent::TranscriptItemStarted {
                        agent_id: record.agent_id().clone(),
                        item_id: opened.clone(),
                        role: self.role,
                    },
                );
                opened
            }
        };
        self.text.push_str(&delta);
        self.revision = self.revision.saturating_add(1);
        record.emit(
            reaction,
            SessionEvent::TranscriptDelta {
                agent_id: record.agent_id().clone(),
                item_id,
                item_revision: self.revision,
                text: delta,
            },
        );
    }

    fn close(self, record: &mut Record, reaction: &mut Reaction) -> Option<String> {
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
        (!self.text.is_empty()).then_some(self.text)
    }
}

impl Step {
    /// Opens the `index`th step of a turn.
    pub(crate) fn new(index: u16) -> Self {
        Self {
            answer: StreamedText::new(TranscriptRole::Assistant),
            reasoning: StreamedText::new(TranscriptRole::Reasoning),
            replay: Vec::new(),
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
        self.answer.append(record, reaction, delta);
    }

    /// Appends provider-returned plaintext reasoning to its own transcript item (PRV-3).
    pub(crate) fn append_reasoning(
        &mut self,
        record: &mut Record,
        reaction: &mut Reaction,
        delta: String,
    ) {
        self.reasoning.append(record, reaction, delta);
    }

    /// Retains one already-bounded opaque item for exact provider replay (PRV-3).
    pub(crate) fn retain_replay(&mut self, replay: ProviderReplay) {
        self.replay.push(replay);
    }

    /// Holds a call until the step ends, because a step's calls dispatch as one batch.
    pub(crate) fn collect(&mut self, call: ToolCall) {
        self.calls.push(call);
    }

    /// Ends the step: finalizes the message, records what it said, hands back what it asked for.
    ///
    /// Reasoning and replay are recorded even when the assistant answer is empty. Only an empty
    /// assistant item is omitted: it would paint a blank row and add an empty message to the next
    /// request without preserving any model output.
    pub(crate) fn close(self, record: &mut Record, reaction: &mut Reaction) -> Vec<ToolCall> {
        if let Some(text) = self.reasoning.close(record, reaction) {
            record.push(RequestItem::Reasoning { text });
        }
        for replay in self.replay {
            record.push(RequestItem::ProviderReplay(replay));
        }
        if let Some(text) = self.answer.close(record, reaction) {
            record.push(RequestItem::Assistant { text });
        }
        self.calls
    }
}
