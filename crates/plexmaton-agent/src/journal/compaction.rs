use plexmaton_core::{AgentId, ConversationEntryId, HeadName};

use super::{ConversationJournal, JournalEntryPayload, JournalError};
use crate::{
    CompactionAttemptFinished, CompactionCheckpoint, CompactionOutcome, CompactionSource,
    ContextAtom, ContextEpoch, RequestAttemptAuthorized, RequestAttemptOwner,
    required_user_context,
};

impl ConversationJournal {
    /// Full collected result for one compaction attempt, indexed into its canonical record.
    #[must_use]
    pub fn compaction_attempt(
        &self,
        attempt_id: &crate::RequestAttemptId,
    ) -> Option<&CompactionAttemptFinished> {
        let index = *self.compaction_attempt_records.get(attempt_id)?;
        match self.records.get(index) {
            Some(crate::JournalRecord::CompactionAttemptFinished { fact, .. }) => Some(fact),
            _ => unreachable!("compaction attempt index points at its canonical record"),
        }
    }

    pub(super) fn apply_compaction_attempt_finished(&mut self, fact: &CompactionAttemptFinished) {
        let attempt_id = fact.attempt_id().clone();
        let owner = self
            .request_attempts
            .get(&attempt_id)
            .unwrap_or_else(|| unreachable!("validated compaction attempt remains indexed"))
            .authorization()
            .owner()
            .clone();
        self.request_attempts
            .get_mut(&attempt_id)
            .unwrap_or_else(|| unreachable!("validated compaction attempt remains indexed"))
            .finish(fact.terminal().clone());
        self.active_request_owners.remove(&owner);
        self.compaction_attempt_records
            .insert(attempt_id, self.records.len());
    }

    /// Freezes the selected branch boundary and its nearest ancestral checkpoint (CPL-1, CPL-5).
    pub fn compaction_source(&self, head: &HeadName) -> Result<CompactionSource, JournalError> {
        let boundary = self
            .head_target(head)?
            .cloned()
            .ok_or_else(|| JournalError::EmptyCompactionSource(head.clone()))?;
        let epoch = epoch_from_path(&self.path(head)?);
        Ok(CompactionSource::new(
            head.clone(),
            self.head_revision(head)?,
            boundary,
            epoch,
        ))
    }

    pub(super) fn validate_compaction_attempt_finished(
        &self,
        fact: &CompactionAttemptFinished,
    ) -> Result<(), JournalError> {
        self.validate_request_terminal(fact.terminal())?;
        fact.validate()
            .map_err(|error| JournalError::InvalidCompactionAttempt {
                attempt_id: fact.attempt_id().clone(),
                error,
            })?;
        let attempt = self
            .request_attempt(fact.attempt_id())
            .unwrap_or_else(|| unreachable!("request terminal validation found its authorization"));
        if !matches!(
            attempt.authorization().owner(),
            RequestAttemptOwner::Compaction { .. }
        ) {
            return Err(JournalError::RequestAttemptIsNotCompaction(
                fact.attempt_id().clone(),
            ));
        }
        Ok(())
    }

    pub(super) fn validate_compaction_checkpoint(
        &self,
        head: &HeadName,
        agent_id: &AgentId,
        checkpoint: &CompactionCheckpoint,
    ) -> Result<(), JournalError> {
        let plan = checkpoint.plan();
        if &self.compaction_source(head)? != plan.source() {
            return Err(JournalError::CompactionSourceChanged);
        }
        if !self.path(head)?.iter().any(|entry| {
            matches!(
                &entry.payload,
                JournalEntryPayload::AgentCreated { agent_id: created, .. } if created == agent_id
            )
        }) {
            return Err(JournalError::MissingCompactionAgent(agent_id.clone()));
        }
        let finished = self
            .compaction_attempt(checkpoint.successful_attempt_id())
            .ok_or_else(|| {
                JournalError::MissingCompactionAttempt(checkpoint.successful_attempt_id().clone())
            })?;
        let authorization = self
            .request_attempt(checkpoint.successful_attempt_id())
            .unwrap_or_else(|| unreachable!("collected attempt retains its authorization"))
            .authorization();
        if !matches!(
            authorization.owner(),
            RequestAttemptOwner::Compaction { compaction_id } if compaction_id == plan.id()
        ) {
            return Err(JournalError::CompactionOwnerMismatch(
                checkpoint.successful_attempt_id().clone(),
            ));
        }
        if authorization.semantic_boundary() != plan.source().boundary() {
            return Err(JournalError::CompactionSourceChanged);
        }
        if authorization.environment() != plan.environment() {
            return Err(JournalError::CompactionEnvironmentMismatch(
                checkpoint.successful_attempt_id().clone(),
            ));
        }
        if !matches!(finished.outcome(), CompactionOutcome::Complete { .. }) {
            return Err(JournalError::CompactionAttemptFailed(
                checkpoint.successful_attempt_id().clone(),
            ));
        }
        if finished
            .outcome()
            .output()
            .and_then(crate::AssistantOutput::replay)
            .is_some_and(|replay| replay.compatible_with() != plan.environment().compatibility())
        {
            return Err(JournalError::CompactionEnvironmentMismatch(
                checkpoint.successful_attempt_id().clone(),
            ));
        }

        let projection = self.project(head).map_err(|error| match error {
            crate::JournalProjectionError::Journal(error) => error,
            _ => JournalError::InvalidCompactionCut,
        })?;
        if projection.recovery().is_some() {
            return Err(JournalError::InvalidCompactionCut);
        }
        let _replacement = self.checkpoint_replacement(
            &projection.request().atoms,
            checkpoint,
            plan.source().boundary().clone(),
        )?;
        Ok(())
    }

    pub(super) fn checkpoint_replacement(
        &self,
        atoms: &[ContextAtom],
        checkpoint: &CompactionCheckpoint,
        checkpoint_entry_id: ConversationEntryId,
    ) -> Result<Vec<ContextAtom>, JournalError> {
        let finished = self
            .compaction_attempt(checkpoint.successful_attempt_id())
            .ok_or_else(|| {
                JournalError::MissingCompactionAttempt(checkpoint.successful_attempt_id().clone())
            })?;
        let summary = finished.complete_summary_text().ok_or_else(|| {
            JournalError::CompactionAttemptFailed(checkpoint.successful_attempt_id().clone())
        })?;
        if summary.len() > checkpoint.plan().max_summary_bytes() {
            return Err(JournalError::CompactionSummaryExceedsPlan);
        }
        let cut = resolve_cut(atoms, checkpoint.plan().cut())?;
        let pinned_count = cut.pinned_user.as_ref().map_or(0, std::ops::Range::len);
        if cut.first_retained <= pinned_count {
            return Err(JournalError::CompactionMakesNoProgress);
        }
        let mut replacement = Vec::with_capacity(
            1_usize
                .saturating_add(pinned_count)
                .saturating_add(atoms.len().saturating_sub(cut.first_retained)),
        );
        replacement.push(ContextAtom::compaction_summary(
            checkpoint_entry_id,
            summary,
        ));
        if let Some(pinned) = cut.pinned_user {
            replacement.extend_from_slice(&atoms[pinned]);
        }
        replacement.extend_from_slice(&atoms[cut.first_retained..]);
        Ok(replacement)
    }

    pub(super) fn context_epoch_at(
        &self,
        boundary: &ConversationEntryId,
    ) -> Result<ContextEpoch, JournalError> {
        let mut cursor = Some(boundary);
        let mut path = Vec::new();
        while let Some(id) = cursor {
            let entry = self
                .entries
                .get(id)
                .ok_or_else(|| JournalError::MissingEntry(id.clone()))?;
            path.push(entry);
            cursor = entry.parent_id.as_ref();
        }
        path.reverse();
        Ok(epoch_from_path(&path))
    }

    pub(super) fn compaction_agent_id(&self, authorization: &RequestAttemptAuthorized) -> &AgentId {
        let entry = self
            .entries
            .get(authorization.semantic_boundary())
            .unwrap_or_else(|| unreachable!("validated authorization retains its boundary"));
        payload_agent_id(&entry.payload)
    }
}

struct ResolvedCut {
    first_retained: usize,
    pinned_user: Option<std::ops::Range<usize>>,
}

fn resolve_cut(
    atoms: &[ContextAtom],
    cut: &crate::CompactionCut,
) -> Result<ResolvedCut, JournalError> {
    let first_ids: Vec<_> = atoms
        .iter()
        .map(|atom| {
            atom.source_entries()
                .first()
                .unwrap_or_else(|| unreachable!("context atoms always retain a source"))
        })
        .collect();
    let first = first_ids
        .iter()
        .position(|id| *id == cut.first_covered())
        .ok_or(JournalError::InvalidCompactionCut)?;
    let last = first_ids
        .iter()
        .position(|id| *id == cut.last_covered())
        .ok_or(JournalError::InvalidCompactionCut)?;
    if first != 0 || last < first {
        return Err(JournalError::InvalidCompactionCut);
    }
    let first_retained = last + 1;
    match (cut.first_retained(), first_ids.get(first_retained)) {
        (Some(expected), Some(actual)) if expected == *actual => {}
        (None, None) => {}
        _ => return Err(JournalError::InvalidCompactionCut),
    }
    let covered_latest_user = required_user_context(atoms).filter(|range| range.start <= last);
    let pinned_user = match (cut.pinned_user(), covered_latest_user) {
        (Some(expected), Some(range)) if expected == first_ids[range.start] => {
            Some(range.start..range.end.min(first_retained))
        }
        (None, None) => None,
        _ => return Err(JournalError::InvalidCompactionCut),
    };
    Ok(ResolvedCut {
        first_retained,
        pinned_user,
    })
}

fn epoch_from_path(path: &[&super::ConversationEntry]) -> ContextEpoch {
    path.iter()
        .rev()
        .find_map(|entry| {
            matches!(
                entry.payload,
                JournalEntryPayload::CompactionCheckpoint { .. }
            )
            .then(|| ContextEpoch::Checkpoint(entry.id.clone()))
        })
        .unwrap_or(ContextEpoch::Original)
}

fn payload_agent_id(payload: &JournalEntryPayload) -> &AgentId {
    match payload {
        JournalEntryPayload::AgentCreated { agent_id, .. }
        | JournalEntryPayload::TurnStatusChanged { agent_id, .. }
        | JournalEntryPayload::TurnStarted { agent_id, .. }
        | JournalEntryPayload::CollaborationTurnStarted { agent_id, .. }
        | JournalEntryPayload::TurnRetried { agent_id, .. }
        | JournalEntryPayload::SteeringAccepted { agent_id, .. }
        | JournalEntryPayload::SkillActivated { agent_id, .. }
        | JournalEntryPayload::ToolPermissionDecided { agent_id, .. }
        | JournalEntryPayload::AssistantOutput { agent_id, .. }
        | JournalEntryPayload::CompactionCheckpoint { agent_id, .. }
        | JournalEntryPayload::ToolCallRequested { agent_id, .. }
        | JournalEntryPayload::ToolCallChanged { agent_id, .. }
        | JournalEntryPayload::AttentionRequested { agent_id, .. }
        | JournalEntryPayload::AttentionResolved { agent_id, .. }
        | JournalEntryPayload::ArtifactAnnounced { agent_id, .. }
        | JournalEntryPayload::RuntimeWarning { agent_id, .. }
        | JournalEntryPayload::RuntimeError { agent_id, .. }
        | JournalEntryPayload::TurnInterruptedByRecovery { agent_id, .. } => agent_id,
        JournalEntryPayload::MailDelivered { to, .. } => to,
    }
}
