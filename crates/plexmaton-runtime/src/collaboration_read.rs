//! Session-aware collaboration projections over canonical mail and selected journal facts.

use std::collections::BTreeMap;

use plexmaton_agent::JournalError;
use plexmaton_agent::collaboration::{
    CollaborationError, CollaborationItemRef, MailDirection, MailEndpoint, ProjectedMail,
};
use plexmaton_core::ConversationId;
use thiserror::Error;

use crate::{
    CollaborationSessionSource, CollaborationWriterError, OwnedCollaboration, OwnedRunnerError,
};

/// What the inspected recipient session proves about one canonical mail item.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionMailInclusion {
    /// Incoming mail is accepted but absent from every selected-branch turn admission.
    Queued,
    /// The selected branch durably started the exact admission that included this mail.
    Included { admission: CollaborationItemRef },
    /// The inspected session sent this mail; the recipient session was not joined.
    OtherRecipient,
}

/// One canonical mail item plus only the inclusion fact its selected recipient session proves.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionProjectedMail {
    mail: ProjectedMail,
    inclusion: SessionMailInclusion,
}

impl SessionProjectedMail {
    #[must_use]
    pub const fn mail(&self) -> &ProjectedMail {
        &self.mail
    }

    #[must_use]
    pub const fn inclusion(&self) -> &SessionMailInclusion {
        &self.inclusion
    }
}

/// Complete canonical mail snapshot joined with one selected session branch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollaborationSessionMailProjection {
    endpoint: MailEndpoint,
    items: Vec<SessionProjectedMail>,
}

impl CollaborationSessionMailProjection {
    #[must_use]
    pub const fn endpoint(&self) -> &MailEndpoint {
        &self.endpoint
    }

    #[must_use]
    pub fn items(&self) -> &[SessionProjectedMail] {
        &self.items
    }
}

/// Refusal rather than a guessed queued/included state.
#[derive(Debug, Error)]
pub enum CollaborationReadError {
    #[error("selected session source does not belong to this collaboration owner")]
    SourceMismatch,
    #[error("selected child runner is unavailable")]
    RunnerUnavailable,
    #[error("selected child runner cannot provide its session snapshot: {0}")]
    Runner(#[from] OwnedRunnerError),
    #[error("selected session branch cannot be projected: {0:?}")]
    Journal(JournalError),
    #[error("canonical collaboration source is invalid: {0}")]
    Collaboration(#[from] CollaborationError),
    #[error("collaboration writer cannot provide one canonical observation: {0}")]
    Writer(#[from] CollaborationWriterError),
}

impl OwnedCollaboration {
    /// Reads CMP-2 from one exact live child through its bounded inspection lane.
    pub async fn child_session_mail_snapshot(
        &mut self,
        conversation: &ConversationId,
    ) -> Result<CollaborationSessionMailProjection, CollaborationReadError> {
        let source = self
            .child_session_source(conversation)
            .await?
            .ok_or(CollaborationReadError::RunnerUnavailable)?;
        self.session_mail_snapshot(source).await
    }

    /// Joins CMP-1 mail with CIN-2 selected-branch inclusion without inferring consumption.
    pub async fn session_mail_snapshot(
        &self,
        source: CollaborationSessionSource,
    ) -> Result<CollaborationSessionMailProjection, CollaborationReadError> {
        if !self
            .ingress
            .as_ref()
            .is_some_and(|ingress| ingress.authenticates_session_source(&source))
        {
            return Err(CollaborationReadError::SourceMismatch);
        }
        let (endpoint, journal, head) = source.into_parts();
        let origins = journal
            .collaboration_inclusions(&head)
            .map_err(CollaborationReadError::Journal)?;
        let references = origins
            .iter()
            .map(|origin| origin.reference().clone())
            .collect();
        let source = self
            .writer
            .project_session_mail(endpoint.clone(), references)
            .await?;
        if source.turns.len() != origins.len() {
            return Err(CollaborationError::InvalidReference.into());
        }
        let mut included = BTreeMap::new();
        for (origin, turn) in origins.iter().zip(&source.turns) {
            if origin.reference() != turn.reference() {
                return Err(CollaborationError::InvalidReference.into());
            }
            journal.validate_collaboration_source(origin.entry(), turn)?;
            for item in turn.items() {
                if included
                    .insert(item.reference.clone(), turn.reference().clone())
                    .is_some()
                {
                    return Err(CollaborationError::InvalidReference.into());
                }
            }
        }
        let items = source
            .mail
            .items()
            .iter()
            .cloned()
            .map(|mail| {
                let inclusion = match mail.direction() {
                    MailDirection::Incoming => included
                        .get(mail.reference())
                        .cloned()
                        .map_or(SessionMailInclusion::Queued, |admission| {
                            SessionMailInclusion::Included { admission }
                        }),
                    MailDirection::Sent => SessionMailInclusion::OtherRecipient,
                };
                SessionProjectedMail { mail, inclusion }
            })
            .collect();
        Ok(CollaborationSessionMailProjection { endpoint, items })
    }
}

#[cfg(test)]
mod tests;
