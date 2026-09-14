//! Read-only mail views derived from acknowledged collaboration records.

use super::{
    CollaborationError, CollaborationEvent, CollaborationItemRef, CollaborationLedger,
    MailEndpoint, MailEnvelope,
};

/// Relationship between the inspected Conversation and one accepted mail item.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MailDirection {
    Incoming,
    Sent,
}

/// One canonical mail item with its full attribution and stable source identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectedMail {
    reference: CollaborationItemRef,
    direction: MailDirection,
    envelope: MailEnvelope,
}

impl ProjectedMail {
    #[must_use]
    pub const fn reference(&self) -> &CollaborationItemRef {
        &self.reference
    }

    #[must_use]
    pub const fn direction(&self) -> MailDirection {
        self.direction
    }

    #[must_use]
    pub const fn envelope(&self) -> &MailEnvelope {
        &self.envelope
    }
}

/// Complete bounded Incoming/Sent snapshot in canonical first-appearance order (CMP-1).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollaborationMailProjection {
    endpoint: MailEndpoint,
    items: Vec<ProjectedMail>,
}

impl CollaborationMailProjection {
    #[must_use]
    pub const fn endpoint(&self) -> &MailEndpoint {
        &self.endpoint
    }

    #[must_use]
    pub fn items(&self) -> &[ProjectedMail] {
        &self.items
    }
}

impl CollaborationLedger {
    /// Projects both directions without copying mail into a session transcript (CMP-1).
    pub fn project_mail(
        &self,
        endpoint: &MailEndpoint,
    ) -> Result<CollaborationMailProjection, CollaborationError> {
        endpoint.validate()?;
        if !self.has_endpoint(endpoint) {
            return Err(CollaborationError::UnknownEndpoint);
        }
        let mut items = Vec::new();
        for record in self.records() {
            let CollaborationEvent::MailAccepted { mail } = &record.event else {
                continue;
            };
            let direction = if &mail.to == endpoint {
                MailDirection::Incoming
            } else if &mail.from == endpoint {
                MailDirection::Sent
            } else {
                continue;
            };
            items.push(ProjectedMail {
                reference: self.item_reference(&record.id)?,
                direction,
                envelope: mail.clone(),
            });
        }
        Ok(CollaborationMailProjection {
            endpoint: endpoint.clone(),
            items,
        })
    }
}
