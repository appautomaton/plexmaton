//! Canonical control, mail and context queries through the sole writer.

use super::*;

impl CollaborationWriter {
    /// Admits one read on the inspection lane, waiting for room instead of refusing it (SCH-2).
    ///
    /// A read carries no mutation to hand back, so waiting costs the caller nothing it owns.
    /// Refusing did cost: every query shared the single control slot, so any command in flight
    /// turned a read into a failure its caller reported as fatal, ending the session.
    async fn inspect<T>(
        &self,
        command: impl FnOnce(oneshot::Sender<Result<T, CollaborationWriterError>>) -> Command,
    ) -> Result<T, CollaborationWriterError> {
        let (reply, result) = oneshot::channel();
        let permit = self
            .inspection
            .as_ref()
            .ok_or(CollaborationWriterError::Closed)?
            .reserve()
            .await
            .map_err(|_| CollaborationWriterError::Closed)?;
        permit.send(command(reply));
        result
            .await
            .map_err(|_| CollaborationWriterError::WorkerFailed)?
    }

    /// Reconstructs one canonical child binding without transferring file ownership.
    pub(crate) async fn delegated_control(
        &self,
        delegation: DelegationId,
    ) -> Result<DelegatedConversationControl, CollaborationWriterError> {
        self.inspect(|reply| Command::DelegatedControl { delegation, reply })
            .await
    }

    /// Returns the current canonical task, endpoints, revision and controller for one delegation.
    pub(crate) async fn delegation_view(
        &self,
        delegation: DelegationId,
    ) -> Result<DelegationView, CollaborationWriterError> {
        self.inspect(|reply| Command::DelegationView { delegation, reply })
            .await
    }

    /// Reconstructs every canonical child capability in creation order for root resume.
    pub(crate) async fn delegated_controls(
        &self,
    ) -> Result<Vec<DelegatedConversationControl>, CollaborationWriterError> {
        self.inspect(|reply| Command::DelegatedControls { reply })
            .await
    }

    /// Returns an owned snapshot from the current canonical file owner (CMP-1).
    pub(crate) async fn project_mail(
        &self,
        endpoint: MailEndpoint,
    ) -> Result<CollaborationMailProjection, CollaborationWriterError> {
        self.inspect(|reply| Command::ProjectMail { endpoint, reply })
            .await
    }

    /// Reads mail and its selected session admissions from one canonical writer observation.
    pub(crate) async fn project_session_mail(
        &self,
        endpoint: MailEndpoint,
        references: Vec<CollaborationItemRef>,
    ) -> Result<SessionMailSourceSnapshot, CollaborationWriterError> {
        self.inspect(|reply| Command::ProjectSessionMail {
            endpoint,
            references,
            reply,
        })
        .await
    }

    /// Every acknowledged record in the log, in append order.
    ///
    /// A reader that decides what the log *means on screen* needs the records themselves, not a
    /// view derived from them: a delegation view keeps only the current task, so a reader built on
    /// one cannot show that the task was ever changed.
    pub(crate) async fn records(
        &self,
    ) -> Result<Vec<CollaborationRecord>, CollaborationWriterError> {
        self.inspect(|reply| Command::Records { reply }).await
    }

    /// Materializes exact session references through the current canonical file owner.
    pub(crate) async fn resolve_context(
        &self,
        references: Vec<CollaborationItemRef>,
    ) -> Result<Vec<Arc<ResolvedTurnAdmission>>, CollaborationWriterError> {
        self.inspect(|reply| Command::ResolveContext { references, reply })
            .await
    }
}
