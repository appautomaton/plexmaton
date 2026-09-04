use plexmaton_core::{
    AgentId, AgentStatus, SessionEvent, TranscriptItemId, TranscriptRole, TurnId,
};

use super::{Agent, DeliveryBoundary};
use crate::timing::UsageAccumulator;
use crate::{JournalEntryPayload, Reaction, UndeliveredInput, UndeliveredReason, UnixMillis};

impl Agent {
    pub(super) fn submit(&mut self, text: String, reaction: &mut Reaction) {
        if self.is_running() {
            self.queue(
                DeliveryBoundary::NextTurn,
                text,
                reaction.observed_at(),
                reaction,
            );
            return;
        }
        self.open_turn(text, reaction.observed_at(), reaction);
    }

    pub(super) fn steer(&mut self, text: String, reaction: &mut Reaction) {
        if !self.is_running() {
            reaction
                .undelivered
                .push(UndeliveredInput::new(text, UndeliveredReason::NoActiveTurn));
            return;
        }
        self.queue(
            DeliveryBoundary::NextStep,
            text,
            reaction.observed_at(),
            reaction,
        );
    }

    fn queue(
        &mut self,
        boundary: DeliveryBoundary,
        text: String,
        accepted_at: UnixMillis,
        reaction: &mut Reaction,
    ) {
        if let Some(undelivered) = self.input.queue(boundary, text, accepted_at) {
            reaction.undelivered.push(undelivered);
        }
    }

    pub(super) fn open_turn(
        &mut self,
        text: String,
        accepted_at: UnixMillis,
        reaction: &mut Reaction,
    ) {
        let turn_id = self.record.next_turn_id();
        self.record_turn_start(turn_id.clone(), text, accepted_at, reaction);
        self.open_step(turn_id, 1, UsageAccumulator::default(), reaction);
    }

    fn record_turn_start(
        &mut self,
        turn_id: TurnId,
        text: String,
        accepted_at: UnixMillis,
        reaction: &mut Reaction,
    ) {
        let item = self.record.next_item_id();
        let agent_id = self.record.agent_id().clone();
        self.record.commit(
            JournalEntryPayload::TurnStarted {
                agent_id: agent_id.clone(),
                item_id: item.clone(),
                turn_id,
                text: text.clone(),
                accepted_at,
                opened_at: reaction.observed_at(),
            },
            reaction,
        );
        self.emit_user(agent_id.clone(), item, text, reaction);
        self.record.emit(
            reaction,
            SessionEvent::AgentStatusChanged {
                agent_id,
                status: AgentStatus::Running,
            },
        );
    }

    pub(super) fn record_steering(
        &mut self,
        turn_id: TurnId,
        text: String,
        accepted_at: UnixMillis,
        reaction: &mut Reaction,
    ) {
        let item = self.record.next_item_id();
        let agent_id = self.record.agent_id().clone();
        self.record.commit(
            JournalEntryPayload::SteeringAccepted {
                agent_id: agent_id.clone(),
                item_id: item.clone(),
                turn_id,
                text: text.clone(),
                accepted_at,
            },
            reaction,
        );
        self.emit_user(agent_id, item, text, reaction);
    }

    fn emit_user(
        &mut self,
        agent_id: AgentId,
        item_id: TranscriptItemId,
        text: String,
        reaction: &mut Reaction,
    ) {
        self.record.emit(
            reaction,
            SessionEvent::TranscriptItemStarted {
                agent_id: agent_id.clone(),
                item_id: item_id.clone(),
                role: TranscriptRole::User,
            },
        );
        self.record.emit(
            reaction,
            SessionEvent::TranscriptDelta {
                agent_id: agent_id.clone(),
                item_id: item_id.clone(),
                item_revision: 1,
                text,
            },
        );
        self.record.emit(
            reaction,
            SessionEvent::TranscriptItemFinalized {
                agent_id,
                item_id,
                item_revision: 2,
            },
        );
    }
}
