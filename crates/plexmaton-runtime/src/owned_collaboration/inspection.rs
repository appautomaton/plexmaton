//! Disposable child-session inspection through a dedicated bounded lane.

use super::*;

impl OwnedCollaboration {
    pub async fn link_child_collaboration_item(
        &mut self,
        conversation: &ConversationId,
        reference: plexmaton_agent::collaboration::CollaborationItemRef,
    ) -> Result<(), OwnedRunnerError> {
        let slot = self
            .runners
            .get_mut(conversation)
            .ok_or(OwnedRunnerError::Closed)?;
        slot.runner.link_collaboration_item(reference).await
    }

    pub async fn child_session_source(
        &mut self,
        conversation: &ConversationId,
    ) -> Result<Option<crate::CollaborationSessionSource>, OwnedRunnerError> {
        let Some(slot) = self.runners.get_mut(conversation) else {
            return Ok(None);
        };
        if slot.finished {
            return Ok(None);
        }
        slot.runner.session_source().await.map(Some)
    }
}
