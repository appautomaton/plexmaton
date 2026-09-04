//! Bounded user input waiting for the turn or step boundary it names.

use std::collections::VecDeque;

use crate::UnixMillis;
use crate::interface::{UndeliveredInput, UndeliveredReason};

/// A burst of input is bounded independently of transcript history. The value is deliberately
/// generous for interactive typing and small enough that a producer cannot turn a stalled agent
/// into an unbounded in-memory queue.
const MAX_QUEUED_INPUTS: usize = 64;
const MAX_QUEUED_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DeliveryBoundary {
    NextTurn,
    NextStep,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct QueuedInput {
    pub(super) order: u64,
    pub(super) boundary: DeliveryBoundary,
    pub(super) text: String,
    pub(super) accepted_at: UnixMillis,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct InputQueue {
    pending: VecDeque<QueuedInput>,
    bytes: usize,
    next_order: u64,
}

impl InputQueue {
    /// Takes ownership when there is room, or returns the exact input immediately.
    pub(super) fn queue(
        &mut self,
        boundary: DeliveryBoundary,
        text: String,
        accepted_at: UnixMillis,
    ) -> Option<UndeliveredInput> {
        let Some(next_bytes) = self.bytes.checked_add(text.len()) else {
            return Some(UndeliveredInput::new(text, UndeliveredReason::QueueFull));
        };
        if self.pending.len() >= MAX_QUEUED_INPUTS || next_bytes > MAX_QUEUED_BYTES {
            return Some(UndeliveredInput::new(text, UndeliveredReason::QueueFull));
        }
        let Some(next_order) = self.next_order.checked_add(1) else {
            return Some(UndeliveredInput::new(text, UndeliveredReason::QueueFull));
        };
        self.pending.push_back(QueuedInput {
            order: self.next_order,
            boundary,
            text,
            accepted_at,
        });
        self.next_order = next_order;
        self.bytes = next_bytes;
        None
    }

    /// Claims inputs for one boundary in arrival order and leaves the other route untouched.
    pub(super) fn claim(&mut self, boundary: DeliveryBoundary) -> Vec<QueuedInput> {
        let mut claimed = Vec::new();
        let mut waiting = VecDeque::with_capacity(self.pending.len());
        while let Some(input) = self.pending.pop_front() {
            if input.boundary == boundary {
                claimed.push(input);
            } else {
                waiting.push_back(input);
            }
        }
        self.pending = waiting;
        self.refresh_bytes();
        claimed
    }

    /// Claims the oldest input for one boundary, leaving later turns queued separately.
    pub(super) fn claim_one(&mut self, boundary: DeliveryBoundary) -> Option<QueuedInput> {
        let index = self
            .pending
            .iter()
            .position(|input| input.boundary == boundary)?;
        let claimed = self.pending.remove(index);
        self.refresh_bytes();
        claimed
    }

    /// Removes every pending input in arrival order.
    pub(super) fn drain_all(&mut self) -> Vec<QueuedInput> {
        self.bytes = 0;
        self.pending.drain(..).collect()
    }

    pub(super) fn pending(&self, boundary: DeliveryBoundary) -> impl Iterator<Item = &str> {
        self.pending
            .iter()
            .filter_map(move |input| (input.boundary == boundary).then_some(input.text.as_str()))
    }

    fn refresh_bytes(&mut self) {
        self.bytes = self.pending.iter().map(|input| input.text.len()).sum();
    }
}

#[cfg(test)]
mod tests {
    use super::{DeliveryBoundary, InputQueue, MAX_QUEUED_BYTES, MAX_QUEUED_INPUTS};
    use crate::{UndeliveredReason, UnixMillis};

    /// LOOP-6 includes the resource boundary: overflow returns ownership instead of growing the
    /// queue or dropping the payload.
    #[test]
    fn the_input_queue_is_bounded_and_returns_overflow() {
        let mut queue = InputQueue::default();
        for index in 0..MAX_QUEUED_INPUTS {
            assert_eq!(
                queue.queue(
                    DeliveryBoundary::NextTurn,
                    format!("message {index}"),
                    UnixMillis::EPOCH,
                ),
                None
            );
        }

        let overflow = queue
            .queue(
                DeliveryBoundary::NextStep,
                "keep me".to_owned(),
                UnixMillis::EPOCH,
            )
            .unwrap_or_else(|| panic!("the bounded queue accepted one too many inputs"));

        assert_eq!(overflow.text, "keep me");
        assert_eq!(overflow.reason, UndeliveredReason::QueueFull);
        assert_eq!(
            queue.pending(DeliveryBoundary::NextTurn).count(),
            MAX_QUEUED_INPUTS
        );
        assert_eq!(queue.pending(DeliveryBoundary::NextStep).count(), 0);
    }

    #[test]
    fn each_boundary_claims_only_its_inputs_in_arrival_order() {
        let mut queue = InputQueue::default();
        queue.queue(
            DeliveryBoundary::NextTurn,
            "turn one".to_owned(),
            UnixMillis::new(1),
        );
        queue.queue(
            DeliveryBoundary::NextStep,
            "step one".to_owned(),
            UnixMillis::new(2),
        );
        queue.queue(
            DeliveryBoundary::NextTurn,
            "turn two".to_owned(),
            UnixMillis::new(3),
        );
        queue.queue(
            DeliveryBoundary::NextStep,
            "step two".to_owned(),
            UnixMillis::new(4),
        );

        assert_eq!(
            queue
                .claim(DeliveryBoundary::NextStep)
                .into_iter()
                .map(|input| input.text)
                .collect::<Vec<_>>(),
            ["step one", "step two"]
        );
        assert_eq!(
            queue
                .claim(DeliveryBoundary::NextTurn)
                .into_iter()
                .map(|input| input.text)
                .collect::<Vec<_>>(),
            ["turn one", "turn two"]
        );
        assert_eq!(queue.bytes, 0);
    }

    #[test]
    fn the_input_queue_bounds_payload_bytes_without_truncating_overflow() {
        let mut queue = InputQueue::default();
        let text = "x".repeat(MAX_QUEUED_BYTES.saturating_add(1));

        let overflow = queue
            .queue(DeliveryBoundary::NextTurn, text.clone(), UnixMillis::EPOCH)
            .unwrap_or_else(|| panic!("the byte budget accepted an oversized payload"));

        assert_eq!(overflow.text, text, "the rejected payload stays exact");
        assert_eq!(overflow.reason, UndeliveredReason::QueueFull);
        assert_eq!(queue.pending(DeliveryBoundary::NextTurn).count(), 0);
    }
}
