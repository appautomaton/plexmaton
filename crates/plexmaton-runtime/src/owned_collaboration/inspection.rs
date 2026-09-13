//! Disposable child-session inspection through a dedicated bounded lane.

use super::*;

impl OwnedCollaboration {
    pub(crate) async fn child_session_source(
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
