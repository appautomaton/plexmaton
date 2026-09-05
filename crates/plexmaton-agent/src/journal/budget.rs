use std::collections::BTreeMap;

use plexmaton_core::{HeadName, TokenUsage};

use super::{JournalProjectionError, RecoveryProjection, SessionJournal};
use crate::{
    InputUsageAnchor, ModelRequest, RequestAttemptOwner, RequestAttemptTerminalState,
    RequestEnvironment,
};

/// Authoritative context and its applicable measurement, projected together before estimation.
#[derive(Debug)]
pub struct BudgetBasis {
    pub request: ModelRequest,
    pub anchor: Option<InputUsageAnchor>,
    pub recovery: Option<RecoveryProjection>,
}

impl SessionJournal {
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
        let request = projection.into_request();
        let spans: Vec<_> = request
            .atoms
            .iter()
            .map(|atom| {
                let mut positions = atom.source_entries().iter().map(|id| {
                    *positions.get(id).unwrap_or_else(|| {
                        unreachable!("projected atoms belong to their selected path")
                    })
                });
                let first = positions
                    .next()
                    .unwrap_or_else(|| unreachable!("atoms have source entries"));
                positions.fold((first, first), |(min, max), next| {
                    (min.min(next), max.max(next))
                })
            })
            .collect();
        let mut anchor: Option<InputUsageAnchor> = None;
        for attempt in self.request_attempts() {
            let fact = attempt.authorization();
            if !matches!(fact.owner(), RequestAttemptOwner::AgentStep { .. })
                || fact.environment() != environment
            {
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
            let count = spans.partition_point(|&(_, end)| end <= boundary);
            // A boundary inside an atom cannot describe the encoded prefix of that whole atom.
            if spans
                .get(count)
                .is_some_and(|&(start, _)| start <= boundary)
            {
                continue;
            }
            if anchor
                .as_ref()
                .is_none_or(|prior| count >= prior.atom_count)
            {
                anchor = Some(InputUsageAnchor {
                    attempt_id: fact.attempt_id().clone(),
                    atom_count: count,
                    input_tokens: counts.input,
                });
            }
        }
        Ok(BudgetBasis {
            request,
            anchor,
            recovery,
        })
    }
}

#[cfg(test)]
mod tests;
