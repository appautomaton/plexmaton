use std::path::PathBuf;

use plexmaton_file_tools::{BoundedReadError, PathError, WorkspaceRoot};
use thiserror::Error;

use crate::{
    SkillDiagnostic, SkillEntry, SkillInvocation, SkillMetadataError, SkillName, SkillNameError,
    SkillOrigin,
};

pub(crate) struct SkillRoot {
    pub(crate) origin: SkillOrigin,
    pub(crate) root: WorkspaceRoot,
}

/// A deterministic summary catalog whose winning roots remain descriptor-pinned for later reads.
pub struct SkillCatalog {
    pub(crate) entries: Vec<SkillEntry>,
    pub(crate) diagnostics: Vec<SkillDiagnostic>,
    pub(crate) roots: Vec<SkillRoot>,
}

impl std::fmt::Debug for SkillCatalog {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SkillCatalog")
            .field("entries", &self.entries)
            .field("diagnostics", &self.diagnostics)
            .finish_non_exhaustive()
    }
}

impl SkillCatalog {
    /// Returns sorted usable summaries with their winning origins and invocation policy.
    #[must_use]
    pub fn entries(&self) -> &[SkillEntry] {
        &self.entries
    }

    /// Returns deterministic non-fatal discovery findings.
    #[must_use]
    pub fn diagnostics(&self) -> &[SkillDiagnostic] {
        &self.diagnostics
    }
}

/// Typed discovery or exact-load failure.
#[derive(Debug, Eq, Error, PartialEq)]
pub enum SkillError {
    #[error("skill operation was cancelled")]
    Cancelled,
    #[error("skill discovery exceeded the {limit}-candidate bound")]
    CandidateLimit { limit: usize },
    #[error("skill catalog exceeded the {limit}-byte bound")]
    CatalogLimit { limit: usize },
    #[error("skill authority root {path:?} could not be pinned: {source}")]
    AuthorityRoot { path: PathBuf, source: PathError },
    #[error("invalid skill name: {0}")]
    InvalidName(SkillNameError),
    #[error("unknown skill {name}")]
    UnknownSkill { name: SkillName },
    #[error("skill {name} does not permit {invocation:?} invocation")]
    InvocationDenied {
        name: SkillName,
        invocation: SkillInvocation,
    },
    #[error("skill resources must be non-empty relative paths beneath the selected bundle")]
    InvalidResource,
    #[error("skill content exceeds the {limit}-byte bound")]
    ContentTooLarge { limit: usize },
    #[error("skill content is not valid UTF-8")]
    InvalidUtf8,
    #[error("skill {name} metadata changed: {error}")]
    MetadataChanged {
        name: SkillName,
        error: SkillMetadataError,
    },
    #[error("skill {name} no longer identifies the selected source")]
    SourceChanged { name: SkillName },
    #[error("could not read {origin:?} skill path {path}: {source}")]
    Read {
        origin: SkillOrigin,
        path: String,
        source: BoundedReadError,
    },
    #[error("skill catalog internal locator invariant failed")]
    CatalogInvariant,
}
