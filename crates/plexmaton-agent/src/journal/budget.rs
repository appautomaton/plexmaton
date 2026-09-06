use std::collections::BTreeMap;

use plexmaton_core::{HeadName, TokenUsage};

use super::{ConversationJournal, JournalProjectionError, RecoveryProjection};
use crate::{
    ContextEpoch, InputUsageAnchor, ModelRequest, RequestAttemptOwner, RequestAttemptTerminalState,
    RequestEnvironment,
};

/// Authoritative context and its applicable measurement, projected together before estimation.
#[derive(Debug)]
pub struct BudgetBasis {
    pub request: ModelRequest,
    pub anchor: Option<InputUsageAnchor>,
    pub recovery: Option<RecoveryProjection>,
    pub context_epoch: ContextEpoch,
    pub base_atom_count: usize,
}

impl ConversationJournal {
    /// Projects once; selects a measured input prefix without replaying every historical request.
    pub fn budget_basis(
        &self,
        head: &HeadName,
        environment: &RequestEnvironment,
    ) -> Result<BudgetBasis, JournalProjectionError> {
        let path = self.path(head)?;
        let positions: BTreeMap<_, _> = path
            .iter()
            .enumerate()
            .map(|(index, entry)| (&entry.id, index))
            .collect();
        let projection = self.project(head)?;
        let recovery = projection.recovery().cloned();
        let context_epoch = projection.context_epoch().clone();
        let base_atom_count = projection.base_atom_count();
        let request = projection.into_request();
        let mut anchor: Option<InputUsageAnchor> = None;
        for attempt in self.request_attempts() {
            let fact = attempt.authorization();
            if !matches!(fact.owner(), RequestAttemptOwner::AgentStep { .. })
                || fact.environment() != environment
            {
                continue;
            }
            if self.context_epoch_at(fact.semantic_boundary())? != context_epoch {
                continue;
            }
            let Some(&boundary) = positions.get(fact.semantic_boundary()) else {
                continue;
            };
            let Some(RequestAttemptTerminalState::Dispatched {
                usage: TokenUsage::Complete(counts) | TokenUsage::Partial(counts),
                ..
            }) = attempt.terminal().map(|terminal| terminal.terminal())
            else {
                continue;
            };
            let covered = |atom: &crate::ContextAtom| {
                atom.source_entries().iter().all(|id| {
                    positions
                        .get(id)
                        .is_some_and(|position| *position <= boundary)
                })
            };
            let splits_atom = request.atoms.iter().any(|atom| {
                let before = atom
                    .source_entries()
                    .iter()
                    .filter_map(|id| positions.get(id));
                let any_covered = before.clone().any(|position| *position <= boundary);
                let any_later = before.into_iter().any(|position| *position > boundary);
                any_covered && any_later
            });
            if splits_atom {
                continue;
            }
            let count = request
                .atoms
                .iter()
                .take_while(|atom| covered(atom))
                .count();
            if request.atoms[count..].iter().any(covered) {
                continue;
            }
            if anchor
                .as_ref()
                .is_none_or(|prior| count >= prior.atom_count)
            {
                anchor = Some(InputUsageAnchor {
                    attempt_id: fact.attempt_id().clone(),
                    context_epoch: context_epoch.clone(),
                    atom_count: count,
                    input_tokens: counts.input,
                });
            }
        }
        Ok(BudgetBasis {
            request,
            anchor,
            recovery,
            context_epoch,
            base_atom_count,
        })
    }
}

#[cfg(test)]
mod tests;
