//! The ordered queue of things a background agent needs from the user.

use plexmaton_core::{AgentId, AttentionId, AttentionKind};

use super::ordered::OrderedById;
use crate::intent::Direction;

/// One queued background request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttentionView {
    pub id: AttentionId,
    pub agent_id: AgentId,
    pub kind: AttentionKind,
    pub summary: String,
    /// Whether the user has been to this request.
    ///
    /// Deliberately not "resolved". Acknowledging is the user saying they have seen it; resolving is
    /// the agent being unblocked, which needs an approval a Phase 00 runtime cannot grant. An
    /// acknowledged request therefore stays queued and stays visible — it is still outstanding.
    pub acknowledged: bool,
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
    /// Which request the user is on. An index rather than an identity: entries keep their arrival
    /// position for the queue's whole life, so nothing can renumber under the cursor.
    cursor: usize,
}

impl AttentionQueue {
    /// Records a request, coalescing onto an existing one with the same identity.
    ///
    /// A repeated request arrives unacknowledged, because an agent that asks again is asking again.
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

    /// Number the user has not been to yet, which is what reads as action required.
    pub(super) fn pending(&self) -> usize {
        self.items.iter().filter(|item| !item.acknowledged).count()
    }

    /// Where the queue's own cursor is, clamped to what is queued.
    pub(super) fn cursor(&self) -> usize {
        self.cursor.min(self.items.len().saturating_sub(1))
    }

    /// Moves the cursor one request, clamped at both ends like every other list here.
    pub(super) fn move_cursor(&mut self, direction: Direction) -> bool {
        let last = self.items.len().saturating_sub(1);
        let next = match direction {
            Direction::Forward => self.cursor().saturating_add(1).min(last),
            Direction::Backward => self.cursor().saturating_sub(1),
        };
        let moved = next != self.cursor();
        self.cursor = next;
        moved
    }

    /// Marks the request under the cursor as seen and reports whose it was.
    ///
    /// Returns `None` on an empty queue rather than inventing a target: an intent with nothing to
    /// act on is a no-op the reducer decides, not a defect the router could have prevented.
    pub(super) fn acknowledge(&mut self) -> Option<AgentId> {
        let key = self.items.key_at(self.cursor())?.clone();
        let item = self.items.get_mut(&key)?;
        item.acknowledged = true;
        Some(item.agent_id.clone())
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{AgentId, AttentionId, AttentionKind};

    use super::{AttentionQueue, AttentionView};
    use crate::intent::Direction;

    fn request(id: &str, summary: &str) -> AttentionView {
        AttentionView {
            id: AttentionId::new(id).unwrap_or_else(|error| panic!("fixture: {error}")),
            agent_id: AgentId::new("agent-b").unwrap_or_else(|error| panic!("fixture: {error}")),
            kind: AttentionKind::Approval,
            summary: summary.to_owned(),
            acknowledged: false,
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

    /// ATT-3: being told is not the same as being answered.
    #[test]
    fn acknowledging_marks_one_request_and_a_repeat_unmarks_it() {
        let mut queue = AttentionQueue::default();
        queue.request(request("ask-1", "first"));
        queue.request(request("ask-2", "other"));
        assert_eq!(queue.pending(), 2);

        assert_eq!(
            queue.acknowledge().map(|id| id.to_string()),
            Some("agent-b".to_owned())
        );
        assert_eq!(queue.pending(), 1);
        assert_eq!(
            queue.len(),
            2,
            "an acknowledged request is still outstanding"
        );

        queue.request(request("ask-1", "revised"));
        assert_eq!(
            queue.pending(),
            2,
            "an agent that asks again is asking again"
        );
    }

    #[test]
    fn the_cursor_clamps_at_both_ends_and_survives_an_empty_queue() {
        let mut queue = AttentionQueue::default();
        assert_eq!(queue.cursor(), 0);
        assert!(!queue.move_cursor(Direction::Forward));
        assert_eq!(queue.acknowledge(), None);

        queue.request(request("ask-1", "first"));
        queue.request(request("ask-2", "other"));
        assert!(queue.move_cursor(Direction::Forward));
        assert_eq!(queue.cursor(), 1);
        assert!(!queue.move_cursor(Direction::Forward), "clamped at the end");
        assert!(queue.move_cursor(Direction::Backward));
        assert!(
            !queue.move_cursor(Direction::Backward),
            "clamped at the start"
        );
    }
}
