//! One step's streaming half: the message it assembles and the calls it asks for.
//!
//! A step is one request to the model. Everything it produces arrives as deltas, so the message on
//! screen and the message the record keeps are built here from the same text — the deltas are what
//! the reader watches, and the assembled string is what the model is shown next.

use plexmaton_core::{SessionEvent, TranscriptItemId, TranscriptRole, TurnId};

use crate::interface::Reaction;
use crate::journal::JournalEntryPayload;
use crate::model::ProviderReplay;
use crate::record::Record;
use crate::tools::ToolCall;

/// What one step has assembled so far.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Step {
    answer: StreamedText,
    reasoning: StreamedText,
    replay: Vec<ProviderReplay>,
    output_order: Vec<StepOutput>,
    warnings: Vec<String>,
    /// Calls this step has asked for, held until it ends so they dispatch as one batch.
    calls: Vec<ToolCall>,
    /// Which step of the turn this is, counting from one.
    index: u16,
    usage_reported: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StepOutput {
    Answer,
    Reasoning,
    Replay(usize),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StreamedText {
    role: TranscriptRole,
    item: TranscriptItemId,
    opened: bool,
    revision: u64,
    text: String,
}

impl StreamedText {
    const fn new(role: TranscriptRole, item: TranscriptItemId) -> Self {
        Self {
            role,
            item,
            opened: false,
            revision: 0,
            text: String::new(),
        }
    }

    fn append(&mut self, record: &mut Record, reaction: &mut Reaction, delta: String) -> bool {
        let opened_now = !std::mem::replace(&mut self.opened, true);
        if opened_now {
            record.emit(
                reaction,
                SessionEvent::TranscriptItemStarted {
                    agent_id: record.agent_id().clone(),
                    item_id: self.item.clone(),
                    role: self.role,
                },
            );
        }
        self.text.push_str(&delta);
        self.revision = self.revision.saturating_add(1);
        record.emit(
            reaction,
            SessionEvent::TranscriptDelta {
                agent_id: record.agent_id().clone(),
                item_id: self.item.clone(),
                item_revision: self.revision,
                text: delta,
            },
        );
        opened_now
    }

    fn close(self, record: &mut Record, reaction: &mut Reaction) {
        if !self.opened {
            return;
        }
        let agent_id = record.agent_id().clone();
        record.commit(
            JournalEntryPayload::Message {
                agent_id: agent_id.clone(),
                item_id: self.item.clone(),
                role: self.role,
                text: self.text,
            },
            reaction,
        );
        record.emit(
            reaction,
            SessionEvent::TranscriptItemFinalized {
                agent_id,
                item_id: self.item,
                item_revision: self.revision.saturating_add(1),
            },
        );
    }
}

impl Step {
    /// Opens the `index`th step of a turn.
    pub(crate) fn new(turn_id: TurnId, index: u16) -> Self {
        Self {
            answer: StreamedText::new(
                TranscriptRole::Assistant,
                Record::stream_item_id(&turn_id, index, TranscriptRole::Assistant),
            ),
            reasoning: StreamedText::new(
                TranscriptRole::Reasoning,
                Record::stream_item_id(&turn_id, index, TranscriptRole::Reasoning),
            ),
            replay: Vec::new(),
            output_order: Vec::new(),
            warnings: Vec::new(),
            calls: Vec::new(),
            index,
            usage_reported: false,
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
        if self.answer.append(record, reaction, delta) {
            self.output_order.push(StepOutput::Answer);
        }
    }

    /// Appends provider-returned plaintext reasoning to its own transcript item (PRV-3).
    pub(crate) fn append_reasoning(
        &mut self,
        record: &mut Record,
        reaction: &mut Reaction,
        delta: String,
    ) {
        if self.reasoning.append(record, reaction, delta) {
            self.output_order.push(StepOutput::Reasoning);
        }
    }

    /// Retains one already-bounded opaque item for exact provider replay (PRV-3).
    pub(crate) fn retain_replay(&mut self, replay: ProviderReplay) {
        self.output_order
            .push(StepOutput::Replay(self.replay.len()));
        self.replay.push(replay);
    }

    /// Holds a call until the step ends, because a step's calls dispatch as one batch.
    pub(crate) fn collect(&mut self, call: ToolCall) {
        self.calls.push(call);
    }

    pub(crate) fn contains_call(&self, call_id: &plexmaton_core::ToolCallId) -> bool {
        self.calls.iter().any(|call| &call.call_id == call_id)
    }

    pub(crate) fn defer_warning(&mut self, message: &str) {
        self.warnings.push(message.to_owned());
    }

    /// Accepts exactly one usage report for this provider step (LIVE-4).
    pub(crate) fn mark_usage_reported(&mut self) -> bool {
        !std::mem::replace(&mut self.usage_reported, true)
    }

    /// Ends the step: finalizes the message, records what it said, hands back what it asked for.
    ///
    /// Reasoning and replay are recorded even when the assistant answer is empty. Only an empty
    /// assistant item is omitted: it would paint a blank row and add an empty message to the next
    /// request without preserving any model output.
    pub(crate) fn close(
        self,
        record: &mut Record,
        reaction: &mut Reaction,
    ) -> (Vec<ToolCall>, Vec<String>) {
        let mut answer = Some(self.answer);
        let mut reasoning = Some(self.reasoning);
        let mut replay: Vec<_> = self.replay.into_iter().map(Some).collect();
        for output in self.output_order {
            match output {
                StepOutput::Answer => answer
                    .take()
                    .unwrap_or_else(|| unreachable!("answer opens once"))
                    .close(record, reaction),
                StepOutput::Reasoning => reasoning
                    .take()
                    .unwrap_or_else(|| unreachable!("reasoning opens once"))
                    .close(record, reaction),
                StepOutput::Replay(index) => {
                    let item = replay[index]
                        .take()
                        .unwrap_or_else(|| unreachable!("replay output is consumed once"));
                    record.commit(JournalEntryPayload::ProviderReplay(item), reaction);
                }
            }
        }
        (self.calls, self.warnings)
    }
}
