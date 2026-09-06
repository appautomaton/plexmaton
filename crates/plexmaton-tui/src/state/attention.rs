//! The ordered queue of things a background agent needs from the user.

use plexmaton_core::{AgentId, AttentionId, AttentionKind, AttentionRequest};

use super::ordered::OrderedById;
use crate::intent::Direction;

/// One queued background request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttentionView {
    pub id: AttentionId,
    pub agent_id: AgentId,
    /// The typed request. Approval identity and capabilities stay attached to the queue item instead
    /// of being copied into a presentation-owned waiter.
    pub request: AttentionRequest,
    /// Whether the user has been to this request.
    ///
    /// Deliberately not "resolved". Acknowledging is the user saying they have seen it; resolving is
    /// the agent being unblocked, which needs an approval a Phase 00 runtime cannot grant. An
    /// acknowledged request therefore stays queued and stays visible — it is still outstanding.
    pub acknowledged: bool,
}

impl AttentionView {
    /// Presentation category derived from the semantic request.
    #[must_use]
    pub const fn kind(&self) -> AttentionKind {
        self.request.kind()
    }

    /// Bounded queue text derived from the semantic request.
    #[must_use]
    pub fn summary(&self) -> &str {
        self.request.summary()
    }
}

/// Stable target returned when the user visits one request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AttentionTarget {
    pub(super) id: AttentionId,
    pub(super) agent_id: AgentId,
    pub(super) kind: AttentionKind,
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
    ///
    /// Reports whether the queue now reads differently. Re-sending a request the user has not been
    /// to yet says nothing new, and re-sending one they had seen un-acknowledges it — which is a
    /// change, and the one that makes ATT-3 visible.
    pub(super) fn request(&mut self, item: AttentionView) -> bool {
        self.items.upsert(item.id.clone(), item)
    }

    /// Iterates queued requests in arrival order.
    pub(super) fn iter(&self) -> impl Iterator<Item = &AttentionView> {
        self.items.iter()
    }

    /// Returns one request by stable identity.
    pub(super) fn get(&self, id: &AttentionId) -> Option<&AttentionView> {
        self.items.get(id)
    }

    pub(super) fn update_offer(
        &mut self,
        id: &AttentionId,
        offer: Option<plexmaton_core::RememberPermissionOffer>,
    ) {
        if let Some(item) = self.items.get_mut(id)
            && let AttentionRequest::Approval { remember, .. } = &mut item.request
        {
            *remember = offer;
        }
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
    pub(super) fn move_cursor(&mut self, visible: &[AttentionId], direction: Direction) -> bool {
        let current = self.items.key_at(self.cursor());
        let index = visible
            .iter()
            .position(|id| Some(id) == current)
            .unwrap_or(0);
        let next = match direction {
            Direction::Forward => index.saturating_add(1).min(visible.len().saturating_sub(1)),
            Direction::Backward => index.saturating_sub(1),
        };
        visible.get(next).is_some_and(|id| self.select(id))
    }

    pub(super) fn select(&mut self, id: &AttentionId) -> bool {
        let Some(index) = self.items.iter().position(|item| &item.id == id) else {
            return false;
        };
        let changed = self.cursor != index;
        self.cursor = index;
        changed
    }

    /// Marks the request under the cursor as seen and reports whose it was.
    ///
    /// Returns `None` on an empty queue rather than inventing a target: an intent with nothing to
    /// act on is a no-op the reducer decides, not a defect the router could have prevented.
    pub(super) fn acknowledge(&mut self) -> Option<AttentionTarget> {
        let key = self.items.key_at(self.cursor())?.clone();
        let item = self.items.get_mut(&key)?;
        item.acknowledged = true;
        Some(AttentionTarget {
            id: item.id.clone(),
            agent_id: item.agent_id.clone(),
            kind: item.kind(),
        })
    }

    /// Removes a resolved request and keeps the cursor on the same logical neighbor.
    pub(super) fn resolve(&mut self, id: &AttentionId) -> bool {
        let Some((removed, _item)) = self.items.remove(id) else {
            return false;
        };
        if removed < self.cursor {
            self.cursor = self.cursor.saturating_sub(1);
        }
        self.cursor = self.cursor.min(self.items.len().saturating_sub(1));
        true
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{AgentId, ApprovalId, AttentionId, AttentionRequest, ToolCallId};

    use super::{AttentionQueue, AttentionView};
    use crate::intent::Direction;

    fn request(id: &str, summary: &str) -> AttentionView {
        AttentionView {
            id: AttentionId::new(id).unwrap_or_else(|error| panic!("fixture: {error}")),
            agent_id: AgentId::new("agent-b").unwrap_or_else(|error| panic!("fixture: {error}")),
            request: AttentionRequest::Approval {
                reason: plexmaton_core::ApprovalReason::PermissionRequired,
                remember: None,
                approval_id: ApprovalId::new(format!("approval-{id}"))
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                call_id: ToolCallId::new(format!("call-{id}"))
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                tool: "edit".to_owned(),
                capabilities: Vec::new(),
                detail: summary.to_owned(),
            },
            acknowledged: false,
        }
    }

    #[test]
    fn an_agent_asking_twice_produces_one_queue_item() {
        let mut queue = AttentionQueue::default();
        queue.request(request("ask-1", "first"));
        queue.request(request("ask-2", "other"));
        queue.request(request("ask-1", "revised"));

        let summaries: Vec<_> = queue.iter().map(AttentionView::summary).collect();
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
            queue
                .acknowledge()
                .map(|target| target.agent_id.to_string()),
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
        assert!(!queue.move_cursor(
            &queue.iter().map(|item| item.id.clone()).collect::<Vec<_>>(),
            Direction::Forward
        ));
        assert_eq!(queue.acknowledge(), None);

        queue.request(request("ask-1", "first"));
        queue.request(request("ask-2", "other"));
        assert!(queue.move_cursor(
            &queue.iter().map(|item| item.id.clone()).collect::<Vec<_>>(),
            Direction::Forward
        ));
        assert_eq!(queue.cursor(), 1);
        assert!(
            !queue.move_cursor(
                &queue.iter().map(|item| item.id.clone()).collect::<Vec<_>>(),
                Direction::Forward
            ),
            "clamped at the end"
        );
        assert!(queue.move_cursor(
            &queue.iter().map(|item| item.id.clone()).collect::<Vec<_>>(),
            Direction::Backward
        ));
        assert!(
            !queue.move_cursor(
                &queue.iter().map(|item| item.id.clone()).collect::<Vec<_>>(),
                Direction::Backward
            ),
            "clamped at the start"
        );
    }

    #[test]
    fn resolving_removes_only_the_named_request_and_repairs_the_cursor() {
        let mut queue = AttentionQueue::default();
        queue.request(request("ask-1", "first"));
        queue.request(request("ask-2", "second"));
        queue.request(request("ask-3", "third"));
        queue.move_cursor(
            &queue.iter().map(|item| item.id.clone()).collect::<Vec<_>>(),
            Direction::Forward,
        );
        queue.move_cursor(
            &queue.iter().map(|item| item.id.clone()).collect::<Vec<_>>(),
            Direction::Forward,
        );

        let second = AttentionId::new("ask-2").unwrap_or_else(|error| panic!("fixture: {error}"));
        assert!(queue.resolve(&second));
        assert_eq!(queue.cursor(), 1);
        assert_eq!(
            queue.iter().map(AttentionView::summary).collect::<Vec<_>>(),
            ["first", "third"]
        );
        assert!(!queue.resolve(&second));
    }
}
