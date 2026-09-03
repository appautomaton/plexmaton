use std::io;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{ObservationId, PathError};

/// JSON Schema character ceiling for each mutation text field.
pub const MAX_MUTATION_ARGUMENT_CHARACTERS: usize = 48 * 1024;
/// Aggregate UTF-8 byte ceiling across one mutation's model-supplied text.
pub const MAX_MUTATION_ARGUMENT_BYTES: usize = 48 * 1024;
pub const MAX_MUTATION_EDITS: usize = 16;
pub const MAX_MUTATION_SOURCE_BYTES: usize = 8 * 1024 * 1024;
/// Maximum complete canonical patch retained for one successful edit.
///
/// MUT-6 bounds the changed text, path and edit count independently. Prefixing every changed line
/// can at most double the 48 KiB text budget; the remainder covers one 4 KiB path plus bounded
/// headers and no-final-newline markers for sixteen edits. The 8 MiB source limit is deliberately
/// absent because unchanged file bytes never enter the patch.
pub const MAX_EDIT_PRESENTATION_BYTES: usize =
    (MAX_MUTATION_ARGUMENT_BYTES * 2) + crate::path::MAX_PATH_BYTES + (4 * 1024);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EditArguments {
    pub(crate) path: String,
    pub(crate) observation: String,
    pub(crate) edits: Vec<ExactEdit>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExactEdit {
    pub(crate) old_text: String,
    pub(crate) new_text: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateArguments {
    pub(crate) path: String,
    pub(crate) content: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CanonicalEdit {
    pub(crate) path: String,
    pub(crate) observation: String,
    pub(crate) source_len: usize,
    pub(crate) splices: Vec<ByteSplice>,
}

impl CanonicalEdit {
    pub(crate) fn observation_id(&self) -> Result<ObservationId, MutationError> {
        ObservationId::from_token(&self.observation).ok_or(MutationError::StaleObservation)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ByteSplice {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) expected: String,
    pub(crate) replacement: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AppliedEdit {
    pub(crate) edits_applied: usize,
    pub(crate) patch: String,
}

/// Why a model-requested file mutation could not become or execute a canonical change.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum MutationError {
    #[error("mutation arguments are invalid or exceed their hard bounds")]
    InvalidArguments,
    #[error("the read observation is absent, evicted, or stale")]
    StaleObservation,
    #[error("the exact source text was not found inside the observed window")]
    SourceMismatch,
    #[error("the exact source text occurs more than once")]
    AmbiguousTarget,
    #[error("two edits overlap in the original file")]
    OverlappingEdits,
    #[error("the mutation source exceeds its byte bound")]
    SourceTooLarge,
    #[error("the resulting file exceeds its byte bound")]
    ResultTooLarge,
    #[error("the complete canonical edit patch exceeds its hard byte bound")]
    PresentationTooLarge,
    #[error("the target is not valid UTF-8 text")]
    InvalidUtf8,
    #[error("the target contains a NUL byte and is treated as binary")]
    Binary,
    #[error("the create target already exists")]
    CreateCollision,
    #[error("the file changed before its mutation could commit")]
    ChangedBeforeCommit,
    #[error("the owned staging entry changed before publication")]
    StagingChanged,
    #[error("the mutation was cancelled before commit")]
    Cancelled,
    #[error(transparent)]
    Path(#[from] PathError),
    #[error("file mutation I/O failed: {0:?}")]
    Io(io::ErrorKind),
}

impl MutationError {
    pub(crate) const fn kind(&self) -> &'static str {
        match self {
            Self::InvalidArguments => "invalid_arguments",
            Self::StaleObservation => "stale_observation",
            Self::SourceMismatch => "source_mismatch",
            Self::AmbiguousTarget => "ambiguous_target",
            Self::OverlappingEdits => "overlapping_edits",
            Self::SourceTooLarge => "source_too_large",
            Self::ResultTooLarge => "result_too_large",
            Self::PresentationTooLarge => "presentation_too_large",
            Self::InvalidUtf8 => "invalid_utf8",
            Self::Binary => "binary",
            Self::CreateCollision => "create_collision",
            Self::ChangedBeforeCommit => "changed_before_commit",
            Self::StagingChanged => "staging_changed",
            Self::Cancelled => "cancelled",
            Self::Path(_) => "path",
            Self::Io(_) => "io",
        }
    }
}
