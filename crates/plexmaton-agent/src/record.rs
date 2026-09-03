//! The session record: what the model is shown, and the one counter numbering what the screen is.
//!
//! Both views come from here so there is nothing to reconcile. The request is assembled from the
//! items; the events are numbered as they are emitted. A second place that numbered events would
//! make the projection reject the first one it saw out of order.

use plexmaton_core::{
    AgentId, ApprovalId, AttentionId, EventSequence, SessionEvent, SessionEventEnvelope,
    ToolCallId, TranscriptItemId, TurnId,
};

use crate::interface::Reaction;
use crate::model::{ModelRequest, RequestItem};

/// One agent's conversation and its event sequence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Record {
    agent_id: AgentId,
    items: Vec<RequestItem>,
    next_sequence: u64,
    next_item: u64,
    next_turn: u64,
    next_approval: u64,
}

impl Record {
    /// Starts an empty record whose first event will be numbered one, as producers conventionally
    /// begin and the projection expects.
    pub(crate) fn new(agent_id: AgentId) -> Self {
        Self {
            agent_id,
            items: Vec::new(),
            next_sequence: 1,
            next_item: 0,
            next_turn: 0,
            next_approval: 0,
        }
    }

    pub(crate) fn agent_id(&self) -> &AgentId {
        &self.agent_id
    }

    pub(crate) fn items(&self) -> &[RequestItem] {
        &self.items
    }

    pub(crate) fn push(&mut self, item: RequestItem) {
        self.items.push(item);
    }

    /// What a call was called, read back so a projection sees one name per call for its whole life.
    pub(crate) fn label_of(&self, call_id: &ToolCallId) -> String {
        self.items
            .iter()
            .find_map(|item| match item {
                RequestItem::ToolCall(call) if &call.call_id == call_id => Some(call.name.clone()),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// The conversation as the model would be shown it right now.
    pub(crate) fn request(&self) -> ModelRequest {
        ModelRequest {
            items: self.items.clone(),
        }
    }

    /// Numbers one event and adds it to what this input produced.
    pub(crate) fn emit(&mut self, reaction: &mut Reaction, event: SessionEvent) {
        let sequence = EventSequence::new(self.next_sequence);
        self.next_sequence = self.next_sequence.saturating_add(1);
        reaction
            .events
            .push(SessionEventEnvelope { sequence, event });
    }

    /// A transcript identity nothing else will use.
    pub(crate) fn next_item_id(&mut self) -> TranscriptItemId {
        self.next_item = self.next_item.saturating_add(1);
        let ordinal = self.next_item;
        TranscriptItemId::new(format!("{}-{ordinal}", self.agent_id))
            .unwrap_or_else(|error| unreachable!("a formatted identity is never empty: {error}"))
    }

    /// A turn identity nothing else in this agent will use.
    pub(crate) fn next_turn_id(&mut self) -> TurnId {
        self.next_turn = self.next_turn.saturating_add(1);
        TurnId::new(format!("{}-turn-{}", self.agent_id, self.next_turn))
            .unwrap_or_else(|error| unreachable!("a formatted identity is never empty: {error}"))
    }

    /// Paired approval and Attention identities for one pending call.
    pub(crate) fn next_approval_ids(&mut self) -> (ApprovalId, AttentionId) {
        self.next_approval = self.next_approval.saturating_add(1);
        let ordinal = self.next_approval;
        let approval = ApprovalId::new(format!("{}-approval-{ordinal}", self.agent_id))
            .unwrap_or_else(|error| unreachable!("a formatted identity is never empty: {error}"));
        let attention = AttentionId::new(format!("{}-attention-{ordinal}", self.agent_id))
            .unwrap_or_else(|error| unreachable!("a formatted identity is never empty: {error}"));
        (approval, attention)
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{AgentId, SessionEvent};

    use super::Record;
    use crate::interface::Reaction;
    use crate::model::RequestItem;

    fn record() -> Record {
        Record::new(AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}")))
    }

    #[test]
    fn one_record_numbers_one_stream_from_one() {
        let mut record = record();
        let mut reaction = Reaction::default();

        for _ in 0..3 {
            let item_id = record.next_item_id();
            record.emit(
                &mut reaction,
                SessionEvent::RuntimeWarning {
                    agent_id: record.agent_id().clone(),
                    item_id,
                    message: "noticed".to_owned(),
                },
            );
        }

        let sequences: Vec<_> = reaction
            .events
            .iter()
            .map(|envelope| envelope.sequence.get())
            .collect();
        assert_eq!(sequences, [1, 2, 3]);
    }

    #[test]
    fn every_transcript_identity_is_new_and_names_its_agent() {
        let mut record = record();

        let first = record.next_item_id();
        let second = record.next_item_id();

        assert_ne!(first, second);
        assert!(first.as_str().starts_with("agent-a"));
    }

    #[test]
    fn turn_and_approval_identities_are_stable_and_distinct() {
        let mut record = record();

        assert_ne!(record.next_turn_id(), record.next_turn_id());
        let first = record.next_approval_ids();
        let second = record.next_approval_ids();
        assert_ne!(first.0, second.0);
        assert_ne!(first.1, second.1);
    }

    #[test]
    fn the_request_is_the_record_and_nothing_else() {
        let mut record = record();
        record.push(RequestItem::User {
            text: "hello".to_owned(),
        });

        assert_eq!(record.request().items, record.items());
    }
}
