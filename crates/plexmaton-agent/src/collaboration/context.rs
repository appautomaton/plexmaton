use std::collections::BTreeMap;
use std::sync::Arc;

use super::{CollaborationError, CollaborationItemRef, ResolvedTurnAdmission};
use crate::{ContextAtomValue, ConversationJournal, ModelRequest};

/// Independent retained-admission bound for one runner's transient materialization.
pub const MAX_RESOLVED_TURNS: usize = 256;
/// Aggregate semantic source bytes; fixed container overhead is bounded by item counts.
pub const MAX_RESOLVED_CONTEXT_BYTES: usize = 16 * 1024 * 1024;

/// Explicit projection states; a reference is never mistaken for resolved model content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CollaborationContext {
    Reference(CollaborationItemRef),
    Resolved(Arc<ResolvedTurnAdmission>),
}

impl CollaborationContext {
    /// Returns the same source identity before and after materialization.
    pub fn reference(&self) -> &CollaborationItemRef {
        match self {
            Self::Reference(reference) => reference,
            Self::Resolved(resolved) => resolved.reference(),
        }
    }
}

/// Bounded disposable materialization; canonical data remains in the collaboration log.
#[derive(Default)]
pub struct ResolvedContext {
    turns: BTreeMap<CollaborationItemRef, Arc<ResolvedTurnAdmission>>,
    bytes: usize,
}

impl ResolvedContext {
    /// Cache only immutable canonical sources. Session binding is checked on every resolution.
    pub fn insert(
        &mut self,
        resolved: Arc<ResolvedTurnAdmission>,
    ) -> Result<(), CollaborationError> {
        if let Some(old) = self.turns.get(resolved.reference()) {
            return if old == &resolved {
                Ok(())
            } else {
                Err(CollaborationError::ItemIdentityConflict)
            };
        }
        if self.turns.len() >= MAX_RESOLVED_TURNS
            || self.bytes + resolved.retained_bytes() > MAX_RESOLVED_CONTEXT_BYTES
        {
            return Err(CollaborationError::ContextCapacity);
        }
        self.bytes += resolved.retained_bytes();
        self.turns.insert(resolved.reference().clone(), resolved);
        Ok(())
    }

    /// Validates all references before mutating the transient request, never the session journal.
    pub fn resolve(
        &self,
        request: &mut ModelRequest,
        journal: &ConversationJournal,
    ) -> Result<(), CollaborationError> {
        if &request.session_id != journal.conversation_id() {
            return Err(CollaborationError::ForeignReference);
        }
        let mut replacements = Vec::new();
        for (index, atom) in request.atoms.iter().enumerate() {
            if let ContextAtomValue::Collaboration(context) = atom.value() {
                let resolved = self
                    .turns
                    .get(context.reference())
                    .ok_or(CollaborationError::UnresolvedContext)?;
                let [source] = atom.source_entries() else {
                    return Err(CollaborationError::InvalidReference);
                };
                journal.validate_collaboration_source(source, resolved)?;
                replacements.push((index, Arc::clone(resolved)));
            }
        }
        for (index, resolved) in replacements {
            request.atoms[index].resolve_collaboration(resolved)?;
        }
        Ok(())
    }
}
