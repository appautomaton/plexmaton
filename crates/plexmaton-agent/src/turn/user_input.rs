use plexmaton_core::{
    AgentId, AgentStatus, ConversationEvent, TranscriptItemId, TranscriptRole, TurnId,
};

use super::{Agent, DeliveryBoundary};
use crate::{
    JournalEntryPayload, Reaction, SkillActivation, UndeliveredInput, UndeliveredReason, UnixMillis,
};

impl Agent {
    pub(super) fn submit(&mut self, text: String, reaction: &mut Reaction) {
        self.submit_input(text, None, reaction);
    }

    pub(super) fn submit_with_skill(
        &mut self,
        text: String,
        skill: SkillActivation,
        reaction: &mut Reaction,
    ) {
        self.submit_input(text, Some(skill), reaction);
    }

    fn submit_input(
        &mut self,
        text: String,
        skill: Option<SkillActivation>,
        reaction: &mut Reaction,
    ) {
        if self.is_running() {
            self.queue(
                DeliveryBoundary::NextTurn,
                text,
                skill,
                reaction.observed_at(),
                reaction,
            );
            return;
        }
        self.open_turn(text, skill, reaction.observed_at(), reaction);
    }

    pub(super) fn steer(&mut self, text: String, reaction: &mut Reaction) {
        self.steer_input(text, None, reaction);
    }

    pub(super) fn steer_with_skill(
        &mut self,
        text: String,
        skill: SkillActivation,
        reaction: &mut Reaction,
    ) {
        self.steer_input(text, Some(skill), reaction);
    }

    fn steer_input(
        &mut self,
        text: String,
        skill: Option<SkillActivation>,
        reaction: &mut Reaction,
    ) {
        if !self.is_running() {
            let undelivered = UndeliveredInput::with_skill(
                text,
                skill.map(|skill| skill.name().to_owned()),
                UndeliveredReason::NoActiveTurn,
            );
            reaction.undelivered.push(undelivered);
            return;
        }
        self.queue(
            DeliveryBoundary::NextStep,
            text,
            skill,
            reaction.observed_at(),
            reaction,
        );
    }

    fn queue(
        &mut self,
        boundary: DeliveryBoundary,
        text: String,
        skill: Option<SkillActivation>,
        accepted_at: UnixMillis,
        reaction: &mut Reaction,
    ) {
        if let Some(undelivered) = self.input.queue(boundary, text, skill, accepted_at) {
            reaction.undelivered.push(undelivered);
        }
    }

    pub(super) fn open_turn(
        &mut self,
        text: String,
        skill: Option<SkillActivation>,
        accepted_at: UnixMillis,
        reaction: &mut Reaction,
    ) {
        let turn_id = self.record.next_turn_id();
        self.record_turn_start(turn_id.clone(), text, skill, accepted_at, reaction);
        self.open_step(turn_id, 1, reaction);
    }

    fn record_turn_start(
        &mut self,
        turn_id: TurnId,
        text: String,
        skill: Option<SkillActivation>,
        accepted_at: UnixMillis,
        reaction: &mut Reaction,
    ) {
        let item = self.record.next_item_id();
        let agent_id = self.record.agent_id().clone();
        self.record.commit(
            JournalEntryPayload::TurnStarted {
                agent_id: agent_id.clone(),
                item_id: item.clone(),
                turn_id: turn_id.clone(),
                text: text.clone(),
                accepted_at,
                opened_at: reaction.observed_at(),
            },
            reaction,
        );
        self.emit_user(agent_id.clone(), item, text, reaction);
        if let Some(activation) = skill {
            self.record.commit(
                JournalEntryPayload::SkillActivated {
                    agent_id: agent_id.clone(),
                    turn_id,
                    activation,
                },
                reaction,
            );
        }
        self.record.emit(
            reaction,
            ConversationEvent::AgentStatusChanged {
                agent_id,
                status: AgentStatus::Running,
            },
        );
    }

    pub(super) fn record_steering(
        &mut self,
        turn_id: TurnId,
        text: String,
        skill: Option<SkillActivation>,
        accepted_at: UnixMillis,
        reaction: &mut Reaction,
    ) {
        let item = self.record.next_item_id();
        let agent_id = self.record.agent_id().clone();
        self.record.commit(
            JournalEntryPayload::SteeringAccepted {
                agent_id: agent_id.clone(),
                item_id: item.clone(),
                turn_id: turn_id.clone(),
                text: text.clone(),
                accepted_at,
            },
            reaction,
        );
        self.emit_user(agent_id, item, text, reaction);
        if let Some(activation) = skill {
            self.record.commit(
                JournalEntryPayload::SkillActivated {
                    agent_id: self.record.agent_id().clone(),
                    turn_id,
                    activation,
                },
                reaction,
            );
        }
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
            ConversationEvent::TranscriptItemStarted {
                agent_id: agent_id.clone(),
                item_id: item_id.clone(),
                role: TranscriptRole::User,
            },
        );
        self.record.emit(
            reaction,
            ConversationEvent::TranscriptDelta {
                agent_id: agent_id.clone(),
                item_id: item_id.clone(),
                item_revision: 1,
                text,
            },
        );
        self.record.emit(
            reaction,
            ConversationEvent::TranscriptItemFinalized {
                agent_id,
                item_id,
                item_revision: 2,
            },
        );
    }
}
