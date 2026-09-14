//! Canonical control, mail and context queries through the sole writer.

use super::*;

impl CollaborationWriter {
    /// Reconstructs one canonical child binding without transferring file ownership.
    pub(crate) async fn delegated_control(
        &self,
        delegation: DelegationId,
    ) -> Result<DelegatedConversationControl, CollaborationWriterError> {
        let (reply, result) = oneshot::channel();
        let sender = self
            .sender
            .as_ref()
            .ok_or(CollaborationWriterError::Closed)?;
        match sender.try_send(Command::DelegatedControl { delegation, reply }) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                return Err(CollaborationWriterError::Busy);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return Err(CollaborationWriterError::Closed);
            }
        }
        result
            .await
            .map_err(|_| CollaborationWriterError::WorkerFailed)?
    }

    /// Returns the current canonical task, endpoints, revision and controller for one delegation.
    pub(crate) async fn delegation_view(
        &self,
        delegation: DelegationId,
    ) -> Result<DelegationView, CollaborationWriterError> {
        let (reply, result) = oneshot::channel();
        let sender = self
            .sender
            .as_ref()
            .ok_or(CollaborationWriterError::Closed)?;
        match sender.try_send(Command::DelegationView { delegation, reply }) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                return Err(CollaborationWriterError::Busy);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return Err(CollaborationWriterError::Closed);
            }
        }
        result
            .await
            .map_err(|_| CollaborationWriterError::WorkerFailed)?
    }

    /// Reconstructs every canonical child capability in creation order for root resume.
    pub(crate) async fn delegated_controls(
        &self,
    ) -> Result<Vec<DelegatedConversationControl>, CollaborationWriterError> {
        let (reply, result) = oneshot::channel();
        let sender = self
            .sender
            .as_ref()
            .ok_or(CollaborationWriterError::Closed)?;
        match sender.try_send(Command::DelegatedControls { reply }) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                return Err(CollaborationWriterError::Busy);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return Err(CollaborationWriterError::Closed);
            }
        }
        result
            .await
            .map_err(|_| CollaborationWriterError::WorkerFailed)?
    }

    /// Returns an owned snapshot from the current canonical file owner (CMP-1).
    pub(crate) async fn project_mail(
        &self,
        endpoint: MailEndpoint,
    ) -> Result<CollaborationMailProjection, CollaborationWriterError> {
        let (reply, result) = oneshot::channel();
        let sender = self
            .sender
            .as_ref()
            .ok_or(CollaborationWriterError::Closed)?;
        match sender.try_send(Command::ProjectMail { endpoint, reply }) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                return Err(CollaborationWriterError::Busy);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return Err(CollaborationWriterError::Closed);
            }
        }
        result
            .await
            .map_err(|_| CollaborationWriterError::WorkerFailed)?
    }

    /// Reads mail and its selected session admissions from one canonical writer observation.
    pub(crate) async fn project_session_mail(
        &self,
        endpoint: MailEndpoint,
        references: Vec<CollaborationItemRef>,
    ) -> Result<SessionMailSourceSnapshot, CollaborationWriterError> {
        let (reply, result) = oneshot::channel();
        let sender = self
            .sender
            .as_ref()
            .ok_or(CollaborationWriterError::Closed)?;
        match sender.try_send(Command::ProjectSessionMail {
            endpoint,
            references,
            reply,
        }) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                return Err(CollaborationWriterError::Busy);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return Err(CollaborationWriterError::Closed);
            }
        }
        result
            .await
            .map_err(|_| CollaborationWriterError::WorkerFailed)?
    }

    /// Every acknowledged record in the log, in append order.
    ///
    /// A reader that decides what the log *means on screen* needs the records themselves, not a
    /// view derived from them: a delegation view keeps only the current task, so a reader built on
    /// one cannot show that the task was ever changed.
    pub(crate) async fn records(
        &self,
    ) -> Result<Vec<CollaborationRecord>, CollaborationWriterError> {
        let (reply, result) = oneshot::channel();
        let sender = self
            .sender
            .as_ref()
            .ok_or(CollaborationWriterError::Closed)?;
        match sender.try_send(Command::Records { reply }) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                return Err(CollaborationWriterError::Busy);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return Err(CollaborationWriterError::Closed);
            }
        }
        result
            .await
            .map_err(|_| CollaborationWriterError::WorkerFailed)?
    }

    /// Materializes exact session references through the current canonical file owner.
    pub(crate) async fn resolve_context(
        &self,
        references: Vec<CollaborationItemRef>,
    ) -> Result<Vec<Arc<ResolvedTurnAdmission>>, CollaborationWriterError> {
        let (reply, result) = oneshot::channel();
        let sender = self
            .sender
            .as_ref()
            .ok_or(CollaborationWriterError::Closed)?;
        match sender.try_send(Command::ResolveContext { references, reply }) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                return Err(CollaborationWriterError::Busy);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return Err(CollaborationWriterError::Closed);
            }
        }
        result
            .await
            .map_err(|_| CollaborationWriterError::WorkerFailed)?
    }
}
