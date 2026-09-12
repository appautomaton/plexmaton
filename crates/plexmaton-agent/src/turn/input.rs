//! Bounded user input waiting for the turn or step boundary it names.

use std::collections::VecDeque;

use crate::interface::{UndeliveredInput, UndeliveredReason};
use crate::{SkillActivation, UnixMillis};

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
    pub(super) skill: Option<SkillActivation>,
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
        skill: Option<SkillActivation>,
        accepted_at: UnixMillis,
    ) -> Option<UndeliveredInput> {
        let attachment_bytes = skill
            .as_ref()
            .map_or(Some(0), SkillActivation::retained_bytes);
        let Some(next_bytes) = self
            .bytes
            .checked_add(text.len())
            .and_then(|bytes| bytes.checked_add(attachment_bytes?))
        else {
            return Some(queue_full(text, skill.as_ref()));
        };
        if self.pending.len() >= MAX_QUEUED_INPUTS || next_bytes > MAX_QUEUED_BYTES {
            return Some(queue_full(text, skill.as_ref()));
        }
        let Some(next_order) = self.next_order.checked_add(1) else {
            return Some(queue_full(text, skill.as_ref()));
        };
        self.pending.push_back(QueuedInput {
            order: self.next_order,
            boundary,
            text,
            skill,
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

    /// Removes the most recently accepted message, wherever it was waiting to be sent.
    ///
    /// By arrival order, not by queue: the user is undoing one `Enter`, and which queue this agent
    /// routed that message to is not something they chose.
    pub(super) fn withdraw_last(&mut self) -> Option<QueuedInput> {
        let index = self
            .pending
            .iter()
            .enumerate()
            .max_by_key(|(_, input)| input.order)
            .map(|(index, _)| index)?;
        let withdrawn = self.pending.remove(index);
        self.refresh_bytes();
        withdrawn
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
        self.bytes = self.pending.iter().fold(0, |bytes, input| {
            bytes.saturating_add(input.text.len()).saturating_add(
                input
                    .skill
                    .as_ref()
                    .map_or(Some(0), SkillActivation::retained_bytes)
                    .unwrap_or(usize::MAX),
            )
        });
    }
}

fn queue_full(text: String, skill: Option<&SkillActivation>) -> UndeliveredInput {
    UndeliveredInput::with_skill(
        text,
        skill.map(|skill| skill.name().to_owned()),
        UndeliveredReason::QueueFull,
    )
}

#[cfg(test)]
mod tests {
    use super::{DeliveryBoundary, InputQueue, MAX_QUEUED_BYTES, MAX_QUEUED_INPUTS};
    use crate::{SkillActivation, SkillSource, UndeliveredReason, UnixMillis};

    fn skill(instructions: String) -> SkillActivation {
        SkillActivation::new(
            "review".to_owned(),
            SkillSource::ProjectShared,
            "/workspace/.agents/skills/review/SKILL.md".to_owned(),
            "a".repeat(64),
            instructions,
        )
        .expect("skill fixture")
    }

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
                    None,
                    UnixMillis::EPOCH,
                ),
                None
            );
        }

        let overflow = queue
            .queue(
                DeliveryBoundary::NextStep,
                "keep me".to_owned(),
                None,
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
            None,
            UnixMillis::new(1),
        );
        queue.queue(
            DeliveryBoundary::NextStep,
            "step one".to_owned(),
            None,
            UnixMillis::new(2),
        );
        queue.queue(
            DeliveryBoundary::NextTurn,
            "turn two".to_owned(),
            None,
            UnixMillis::new(3),
        );
        queue.queue(
            DeliveryBoundary::NextStep,
            "step two".to_owned(),
            None,
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
            .queue(
                DeliveryBoundary::NextTurn,
                text.clone(),
                None,
                UnixMillis::EPOCH,
            )
            .unwrap_or_else(|| panic!("the byte budget accepted an oversized payload"));

        assert_eq!(overflow.text, text, "the rejected payload stays exact");
        assert_eq!(overflow.reason, UndeliveredReason::QueueFull);
        assert_eq!(queue.pending(DeliveryBoundary::NextTurn).count(), 0);
    }

    /// SKL-5: queue occupancy includes retained skill context, and a claimed input keeps it exact.
    #[test]
    fn skl_5_queue_bounds_include_and_preserve_skill_attachments() {
        let mut queue = InputQueue::default();
        let exact = skill("follow these instructions".to_owned());
        assert_eq!(
            queue.queue(
                DeliveryBoundary::NextStep,
                "$review check this".to_owned(),
                Some(exact.clone()),
                UnixMillis::new(9),
            ),
            None
        );
        let claimed = queue
            .claim_one(DeliveryBoundary::NextStep)
            .expect("queued skill");
        assert_eq!(claimed.skill.as_ref(), Some(&exact));
        assert_eq!(claimed.text, "$review check this");

        let too_large_for_queue = skill("x".repeat(crate::MAX_SKILL_INSTRUCTION_BYTES));
        let original = "$review keep exact".to_owned();
        let overflow = queue
            .queue(
                DeliveryBoundary::NextTurn,
                original.clone(),
                Some(too_large_for_queue),
                UnixMillis::EPOCH,
            )
            .expect("attachment metadata exceeds the queue byte bound");
        assert_eq!(overflow.text, original);
        assert_eq!(overflow.skill.as_deref(), Some("review"));
        assert_eq!(overflow.reason, UndeliveredReason::QueueFull);
        assert_eq!(queue.pending(DeliveryBoundary::NextTurn).count(), 0);
    }
}
