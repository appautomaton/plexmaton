//! Bounded user input waiting for the turn or step boundary it names.

use std::collections::VecDeque;

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
struct QueuedInput {
    boundary: DeliveryBoundary,
    text: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct InputQueue {
    pending: VecDeque<QueuedInput>,
    bytes: usize,
}

impl InputQueue {
    /// Takes ownership when there is room, or returns the exact input immediately.
    pub(super) fn queue(
        &mut self,
        boundary: DeliveryBoundary,
        text: String,
    ) -> Option<UndeliveredInput> {
        let Some(next_bytes) = self.bytes.checked_add(text.len()) else {
            return Some(UndeliveredInput::new(text, UndeliveredReason::QueueFull));
        };
        if self.pending.len() >= MAX_QUEUED_INPUTS || next_bytes > MAX_QUEUED_BYTES {
            return Some(UndeliveredInput::new(text, UndeliveredReason::QueueFull));
        }
        self.pending.push_back(QueuedInput { boundary, text });
        self.bytes = next_bytes;
        None
    }

    /// Claims inputs for one boundary in arrival order and leaves the other route untouched.
    pub(super) fn claim(&mut self, boundary: DeliveryBoundary) -> Vec<String> {
        let mut claimed = Vec::new();
        let mut waiting = VecDeque::with_capacity(self.pending.len());
        while let Some(input) = self.pending.pop_front() {
            if input.boundary == boundary {
                claimed.push(input.text);
            } else {
                waiting.push_back(input);
            }
        }
        self.pending = waiting;
        self.refresh_bytes();
        claimed
    }

    /// Claims the oldest input for one boundary, leaving later turns queued separately.
    pub(super) fn claim_one(&mut self, boundary: DeliveryBoundary) -> Option<String> {
        let index = self
            .pending
            .iter()
            .position(|input| input.boundary == boundary)?;
        let claimed = self.pending.remove(index).map(|input| input.text);
        self.refresh_bytes();
        claimed
    }

    /// Returns inputs for one boundary with their payloads intact.
    pub(super) fn reject(
        &mut self,
        boundary: DeliveryBoundary,
        reason: UndeliveredReason,
    ) -> Vec<UndeliveredInput> {
        self.claim(boundary)
            .into_iter()
            .map(|text| UndeliveredInput::new(text, reason))
            .collect()
    }

    /// Returns every pending input in arrival order.
    pub(super) fn reject_all(&mut self, reason: UndeliveredReason) -> Vec<UndeliveredInput> {
        self.bytes = 0;
        self.pending
            .drain(..)
            .map(|input| UndeliveredInput::new(input.text, reason))
            .collect()
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
    use crate::UndeliveredReason;

    /// LOOP-6 includes the resource boundary: overflow returns ownership instead of growing the
    /// queue or dropping the payload.
    #[test]
    fn the_input_queue_is_bounded_and_returns_overflow() {
        let mut queue = InputQueue::default();
        for index in 0..MAX_QUEUED_INPUTS {
            assert_eq!(
                queue.queue(DeliveryBoundary::NextTurn, format!("message {index}")),
                None
            );
        }

        let overflow = queue
            .queue(DeliveryBoundary::NextStep, "keep me".to_owned())
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
        queue.queue(DeliveryBoundary::NextTurn, "turn one".to_owned());
        queue.queue(DeliveryBoundary::NextStep, "step one".to_owned());
        queue.queue(DeliveryBoundary::NextTurn, "turn two".to_owned());
        queue.queue(DeliveryBoundary::NextStep, "step two".to_owned());

        assert_eq!(
            queue.claim(DeliveryBoundary::NextStep),
            ["step one", "step two"]
        );
        assert_eq!(
            queue.claim(DeliveryBoundary::NextTurn),
            ["turn one", "turn two"]
        );
        assert_eq!(queue.bytes, 0);
    }

    #[test]
    fn the_input_queue_bounds_payload_bytes_without_truncating_overflow() {
        let mut queue = InputQueue::default();
        let text = "x".repeat(MAX_QUEUED_BYTES.saturating_add(1));

        let overflow = queue
            .queue(DeliveryBoundary::NextTurn, text.clone())
            .unwrap_or_else(|| panic!("the byte budget accepted an oversized payload"));

        assert_eq!(overflow.text, text, "the rejected payload stays exact");
        assert_eq!(overflow.reason, UndeliveredReason::QueueFull);
        assert_eq!(queue.pending(DeliveryBoundary::NextTurn).count(), 0);
    }
}
