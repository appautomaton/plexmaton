//! Successful ingress settlement and its canonical collaboration reference.

use plexmaton_agent::collaboration::{CollaborationItemRef, MailEndpoint};
use plexmaton_core::ToolCallId;

use super::{CollaborationIngressFailure, CollaborationIngressOutcome};

/// Tool-facing outcome paired with the canonical fact acknowledged by the owner.
///
/// The model receives only [`CollaborationIngressOutcome`]. The reference is retained separately so
/// the sender session can record a placement anchor without copying the collaboration payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollaborationIngressResult {
    outcome: CollaborationIngressOutcome,
    reference: CollaborationItemRef,
}

impl CollaborationIngressResult {
    pub(super) fn new(
        outcome: CollaborationIngressOutcome,
        reference: CollaborationItemRef,
    ) -> Self {
        Self { outcome, reference }
    }

    #[must_use]
    pub const fn outcome(&self) -> &CollaborationIngressOutcome {
        &self.outcome
    }

    #[must_use]
    pub const fn reference(&self) -> &CollaborationItemRef {
        &self.reference
    }

    pub(crate) fn into_parts(self) -> (CollaborationIngressOutcome, CollaborationItemRef) {
        (self.outcome, self.reference)
    }
}

/// Observable settlement retained by the root even when the originating tool wait disappeared.
#[derive(Debug)]
pub struct CollaborationIngressSettlement {
    pub(super) call_id: ToolCallId,
    pub(super) caller: Option<MailEndpoint>,
    pub(super) result: Result<CollaborationIngressResult, CollaborationIngressFailure>,
    pub(super) reply_delivered: bool,
}

impl CollaborationIngressSettlement {
    #[must_use]
    pub const fn call_id(&self) -> &ToolCallId {
        &self.call_id
    }

    #[must_use]
    pub const fn caller(&self) -> Option<&MailEndpoint> {
        self.caller.as_ref()
    }

    pub const fn result(&self) -> &Result<CollaborationIngressResult, CollaborationIngressFailure> {
        &self.result
    }

    #[must_use]
    pub const fn reply_delivered(&self) -> bool {
        self.reply_delivered
    }
}
