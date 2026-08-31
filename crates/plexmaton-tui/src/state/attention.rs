//! The ordered queue of things a background agent needs from the user.

use plexmaton_core::{AgentId, AttentionId, AttentionKind};

use super::ordered::OrderedById;

/// One queued background request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttentionView {
    pub id: AttentionId,
    pub agent_id: AgentId,
    pub kind: AttentionKind,
    pub summary: String,
}

/// Requests waiting for the user, in the order they arrived.
///
/// Queueing is the whole mechanism: an agent that needs something joins this and nothing else
/// happens. It does not move focus, change the selected transcript, or open a surface, which is
/// what keeps a background worker from interrupting the conversation the user is having.
///
/// Repeated requests under one identity replace the entry in place rather than adding a second, so
/// an agent that asks twice produces one queue item instead of a notification storm.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct AttentionQueue {
    items: OrderedById<AttentionId, AttentionView>,
}

impl AttentionQueue {
    /// Records a request, coalescing onto an existing one with the same identity.
    pub(super) fn request(&mut self, item: AttentionView) {
        self.items.upsert(item.id.clone(), item);
    }

    /// Iterates queued requests in arrival order.
    pub(super) fn iter(&self) -> impl Iterator<Item = &AttentionView> {
        self.items.iter()
    }

    /// Number of requests awaiting the user.
    pub(super) fn len(&self) -> usize {
        self.items.len()
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{AgentId, AttentionId, AttentionKind};

    use super::{AttentionQueue, AttentionView};

    fn request(id: &str, summary: &str) -> AttentionView {
        AttentionView {
            id: AttentionId::new(id).unwrap_or_else(|error| panic!("fixture: {error}")),
            agent_id: AgentId::new("agent-b").unwrap_or_else(|error| panic!("fixture: {error}")),
            kind: AttentionKind::Approval,
            summary: summary.to_owned(),
        }
    }

    #[test]
    fn an_agent_asking_twice_produces_one_queue_item() {
        let mut queue = AttentionQueue::default();
        queue.request(request("ask-1", "first"));
        queue.request(request("ask-2", "other"));
        queue.request(request("ask-1", "revised"));

        let summaries: Vec<_> = queue.iter().map(|item| item.summary.as_str()).collect();
        assert_eq!(
            summaries,
            ["revised", "other"],
            "a repeated request replaces its entry and keeps its place in the queue"
        );
        assert_eq!(queue.len(), 2);
    }
}
