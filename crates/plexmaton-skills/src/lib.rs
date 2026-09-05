//! Bounded Agent Skills discovery and exact reads through pinned filesystem roots.

mod catalog;
mod discovery;
mod loading;
mod parser;
mod types;

pub use catalog::{SkillCatalog, SkillError};
pub use types::{
    LoadedSkill, MAX_SKILL_DESCRIPTION_CHARACTERS, MAX_SKILL_NAME_CHARACTERS, SkillDiagnostic,
    SkillDiagnosticKind, SkillEntry, SkillInvocation, SkillInvocationPolicy, SkillMetadataError,
    SkillMetadataField, SkillName, SkillNameError, SkillOrigin,
};

/// Maximum retained frontmatter bytes for one candidate.
pub const MAX_FRONTMATTER_BYTES: usize = 16 * 1024;
/// Maximum exact Markdown body or resource bytes returned by one load.
pub const MAX_SKILL_CONTENT_BYTES: usize = 256 * 1024;
/// Maximum direct directory entries examined across all configured roots.
pub const MAX_SKILL_CANDIDATES: usize = 256;
/// Maximum aggregate catalog bytes retained across winning summaries.
pub const MAX_SKILL_CATALOG_BYTES: usize = 64 * 1024;
/// Maximum UTF-8 bytes retained for one canonical source locator.
pub const MAX_SKILL_LOCATION_BYTES: usize = 4096;
