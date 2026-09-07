//! Canonical cross-session admission and attributed delegation state (COL-1–COL-3).
//!
//! This reducer performs no I/O or effects. A runtime authenticates incoming actors; a storage
//! adapter acknowledges the exact prepared record before publishing its derived views.

mod error;
mod ledger;
mod types;

#[cfg(test)]
mod tests;

pub use error::CollaborationError;
pub use ledger::{CollaborationLedger, Preparation};
pub use types::{
    ArtifactReference, CollaborationEvent, CollaborationLimits, CollaborationRecord,
    CollaborationSequence, CollaborationText, DelegationAuthor, DelegationRevision, DelegationView,
    ItemReceipt, MAX_COLLABORATION_ID_BYTES, MAX_COLLABORATION_ITEMS, MAX_COLLABORATION_TEXT_BYTES,
    MAX_DELEGATIONS, MAX_MAIL_ARTIFACTS, MAX_RETAINED_MAIL_BYTES, MailEndpoint, MailEnvelope,
};
