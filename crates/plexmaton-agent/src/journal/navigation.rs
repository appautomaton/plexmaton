use plexmaton_core::{AgentId, ConversationEntryId, HeadName, TurnId};

use super::{
    ConversationEntry, ConversationJournal, JournalEntryPayload, JournalError, JournalRecord,
};
use crate::TreeNavigationRefusal;

/// Canonical rewind boundary and historical user input resolved from one stable entry identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedRewindTarget {
    /// Entry at which the new head ends, or `None` for the empty root.
    pub(crate) boundary: Option<ConversationEntryId>,
    /// User entry whose exact draft is materialized only after an actual navigation is accepted.
    pub(crate) draft_entry_id: Option<ConversationEntryId>,
}

/// Historical explicit skill selection paired with its original text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RewindDraft {
    pub(crate) text: String,
    pub(crate) skill_name: Option<String>,
}

impl ConversationJournal {
    /// Resolves a user or completed assistant entry to the stable canonical boundary it denotes.
    ///
    /// This is also the eligibility check tree projection uses; callers must not infer boundaries
    /// from rendered roles or parent links alone (TRE-7). The result retains only a stable reference
    /// to a user draft. Its text and skill are loaded by `materialize_rewind_draft` only when a
    /// navigation actually selects that entry, keeping all-head snapshots from cloning user content.
    pub(crate) fn resolve_rewind_target(
        &self,
        agent_id: &AgentId,
        entry_id: &ConversationEntryId,
    ) -> Result<ResolvedRewindTarget, TreeNavigationRefusal> {
        let entry = self
            .entries
            .get(entry_id)
            .ok_or_else(|| TreeNavigationRefusal::MissingTarget(entry_id.clone()))?;
        match &entry.payload {
            JournalEntryPayload::TurnStarted {
                agent_id: owner, ..
            } => {
                self.ensure_target_agent(agent_id, owner)?;
                self.validate_stable_target(entry.parent_id.as_ref())
                    .map_err(TreeNavigationRefusal::Journal)?;
                Ok(ResolvedRewindTarget {
                    boundary: entry.parent_id.clone(),
                    draft_entry_id: Some(entry.id.clone()),
                })
            }
            JournalEntryPayload::SteeringAccepted { .. } => {
                Err(TreeNavigationRefusal::SteeringTarget(entry_id.clone()))
            }
            JournalEntryPayload::AssistantOutput {
                agent_id: owner,
                step_id,
                ..
            } => {
                self.ensure_target_agent(agent_id, owner)?;
                let turn_id = step_id.turn_id();
                let start = self.turn_starts.get(turn_id).ok_or_else(|| {
                    TreeNavigationRefusal::AssistantTurnIncomplete(turn_id.clone())
                })?;
                self.ensure_target_agent(agent_id, &start.agent_id)?;
                if !self.is_ancestor(&start.entry_id, entry_id) {
                    return Err(TreeNavigationRefusal::TargetOutsideTurn {
                        entry_id: entry_id.clone(),
                        turn_id: turn_id.clone(),
                    });
                }
                let finish = self.turn_finishes.get(turn_id).ok_or_else(|| {
                    TreeNavigationRefusal::AssistantTurnIncomplete(turn_id.clone())
                })?;
                if !self.is_ancestor(entry_id, &finish.fact.semantic_boundary) {
                    return Err(TreeNavigationRefusal::TargetOutsideTurn {
                        entry_id: entry_id.clone(),
                        turn_id: turn_id.clone(),
                    });
                }
                let final_assistant =
                    self.final_assistant_on_turn(&finish.fact.semantic_boundary, turn_id);
                if final_assistant != Some(entry_id) {
                    return Err(TreeNavigationRefusal::InteriorAssistantTarget(
                        entry_id.clone(),
                    ));
                }
                self.validate_stable_target(Some(&finish.fact.semantic_boundary))
                    .map_err(TreeNavigationRefusal::Journal)?;
                Ok(ResolvedRewindTarget {
                    boundary: Some(finish.fact.semantic_boundary.clone()),
                    draft_entry_id: None,
                })
            }
            _ => Err(TreeNavigationRefusal::UnsupportedTarget(entry_id.clone())),
        }
    }

    /// Loads exact user text and historical skill metadata for an already resolved target.
    ///
    /// Call only after a concrete navigation request chose the row. Tree projection uses the
    /// lightweight resolved reference for eligibility and never materializes every historical
    /// user draft (TRE-2, TRE-5, TRE-7).
    pub(crate) fn materialize_rewind_draft(
        &self,
        resolved: &ResolvedRewindTarget,
    ) -> Option<RewindDraft> {
        let entry_id = resolved.draft_entry_id.as_ref()?;
        let entry = self
            .entries
            .get(entry_id)
            .unwrap_or_else(|| unreachable!("resolved user draft remains in the journal"));
        let JournalEntryPayload::TurnStarted {
            agent_id,
            turn_id,
            text,
            ..
        } = &entry.payload
        else {
            unreachable!("only a resolved user turn retains a draft entry")
        };
        let skill_name = self
            .skill_activation_after(entry, agent_id, turn_id)
            .map(|activation| activation.name().to_owned());
        Some(RewindDraft {
            text: text.clone(),
            skill_name,
        })
    }

    /// Picks a deterministic sequence-derived destination name without reusing retired names.
    pub(crate) fn fresh_rewind_head_name(&self) -> Result<HeadName, TreeNavigationRefusal> {
        let sequence = self.next_sequence.get();
        let mut suffix = 1_u64;
        loop {
            let candidate = if suffix == 1 {
                format!("rewind-{sequence}")
            } else {
                format!("rewind-{sequence}-{suffix}")
            };
            let name = HeadName::new(candidate)
                .unwrap_or_else(|error| unreachable!("generated rewind name is valid: {error}"));
            if !self.heads.contains_key(&name) && !self.retired_heads.contains(&name) {
                return Ok(name);
            }
            suffix = suffix
                .checked_add(1)
                .ok_or(TreeNavigationRefusal::HeadNameExhausted)?;
        }
    }

    /// Rejects a destination that still points into a turn, even though SelectHead's durable
    /// record validator only compares its revision (TRE-7).
    pub(crate) fn validate_navigation_head_target(
        &self,
        head: &HeadName,
    ) -> Result<(), TreeNavigationRefusal> {
        let target = self.head_target(head).map_err(|error| match error {
            JournalError::MissingHead(missing) => TreeNavigationRefusal::MissingHead(missing),
            other => TreeNavigationRefusal::Journal(other),
        })?;
        self.validate_stable_target(target)
            .map_err(|error| match error {
                JournalError::UnstableTurnTarget(turn_id) => {
                    TreeNavigationRefusal::UnstableDestination {
                        head: head.clone(),
                        turn_id,
                    }
                }
                other => TreeNavigationRefusal::Journal(other),
            })
    }

    fn ensure_target_agent(
        &self,
        expected: &AgentId,
        actual: &AgentId,
    ) -> Result<(), TreeNavigationRefusal> {
        if expected == actual {
            return Ok(());
        }
        Err(TreeNavigationRefusal::ForeignTargetAgent {
            expected: expected.clone(),
            actual: actual.clone(),
        })
    }

    fn skill_activation_after<'a>(
        &'a self,
        start: &ConversationEntry,
        agent_id: &AgentId,
        turn_id: &TurnId,
    ) -> Option<&'a crate::SkillActivation> {
        self.records.iter().find_map(|record| {
            let JournalRecord::AppendEntry { entry, .. } = record else {
                return None;
            };
            if entry.parent_id.as_ref() != Some(&start.id) {
                return None;
            }
            match &entry.payload {
                JournalEntryPayload::SkillActivated {
                    agent_id: owner,
                    turn_id: activated_turn,
                    activation,
                } if owner == agent_id && activated_turn == turn_id => Some(activation),
                _ => None,
            }
        })
    }

    fn is_ancestor(
        &self,
        ancestor: &ConversationEntryId,
        descendant: &ConversationEntryId,
    ) -> bool {
        let mut cursor = Some(descendant);
        while let Some(id) = cursor {
            if id == ancestor {
                return true;
            }
            cursor = self
                .entries
                .get(id)
                .and_then(|entry| entry.parent_id.as_ref());
        }
        false
    }

    fn final_assistant_on_turn(
        &self,
        boundary: &ConversationEntryId,
        turn_id: &TurnId,
    ) -> Option<&ConversationEntryId> {
        let mut cursor = Some(boundary);
        while let Some(id) = cursor {
            let entry = self.entries.get(id)?;
            if matches!(
                &entry.payload,
                JournalEntryPayload::AssistantOutput { step_id, .. }
                    if step_id.turn_id() == turn_id
            ) {
                return Some(&entry.id);
            }
            cursor = entry.parent_id.as_ref();
        }
        None
    }
}
