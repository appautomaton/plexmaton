use plexmaton_core::ConversationEntryId;

use super::{ContextAtom, ContextAtomValue};

impl ContextAtom {
    /// Retains only a canonical reference in the session projection (CIN-2).
    pub fn collaboration(
        source: ConversationEntryId,
        reference: crate::collaboration::CollaborationItemRef,
    ) -> Self {
        Self {
            source_entries: vec![source].into_boxed_slice(),
            value: ContextAtomValue::Collaboration(
                crate::collaboration::CollaborationContext::Reference(reference),
            ),
        }
    }

    /// Replaces a transient reference projection with immutable source content, never a user atom.
    pub fn resolve_collaboration(
        &mut self,
        resolved: std::sync::Arc<crate::collaboration::ResolvedTurnAdmission>,
    ) -> Result<(), crate::collaboration::CollaborationError> {
        let ContextAtomValue::Collaboration(context) = &self.value else {
            return Err(crate::collaboration::CollaborationError::InvalidReference);
        };
        if context.reference() != resolved.reference() {
            return Err(crate::collaboration::CollaborationError::ForeignReference);
        }
        self.value = ContextAtomValue::Collaboration(
            crate::collaboration::CollaborationContext::Resolved(resolved),
        );
        Ok(())
    }
}
