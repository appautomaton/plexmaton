use std::fmt;

/// A collaboration refusal leaves the canonical reduction unchanged.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CollaborationError {
    InvalidLimits,
    InvalidReference,
    ForeignReference,
    InvalidTurnBoundary,
    DuplicateTurnAdmission,
    StaleTurnAdmission,
    NoPendingItems,
    TurnSourceCapacity,
    ContextCapacity,
    UnresolvedContext,
    UnsupportedContext,
    PersistenceRequired,
    EmptyText,
    TextTooLarge,
    IdentityTooLarge,
    TooManyArtifacts,
    DuplicateArtifact,
    UnknownEndpoint,
    EndpointIdentityConflict,
    SameSession,
    UnknownDelegation,
    DuplicateDelegation,
    WorkerAlreadyAssigned,
    DelegationCycle,
    WrongAuthor,
    StaleRevision,
    UserAuthority,
    ItemIdentityConflict,
    MailIdentityConflict,
    UnexpectedSequence,
    ItemCapacity,
    DelegationCapacity,
    MailCapacity,
}

impl fmt::Display for CollaborationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Error variants contain no task or mail text; diagnostics must not become another inbox.
        write!(formatter, "collaboration admission refused: {self:?}")
    }
}

impl std::error::Error for CollaborationError {}
