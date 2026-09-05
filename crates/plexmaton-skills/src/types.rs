use serde::Serialize;
use thiserror::Error;
use unicode_normalization::UnicodeNormalization as _;

/// Maximum normalized skill-name length in Unicode scalar values.
pub const MAX_SKILL_NAME_CHARACTERS: usize = 64;
/// Maximum skill-description length in Unicode scalar values.
pub const MAX_SKILL_DESCRIPTION_CHARACTERS: usize = 1024;

/// A normalized, validated Agent Skills name.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SkillName(String);

impl SkillName {
    /// Normalizes and validates one name using the Agent Skills character grammar.
    pub fn new(value: &str) -> Result<Self, SkillNameError> {
        let normalized: String = value.trim().nfkc().collect();
        if normalized.is_empty() {
            return Err(SkillNameError::Empty);
        }
        if normalized.chars().count() > MAX_SKILL_NAME_CHARACTERS {
            return Err(SkillNameError::TooLong);
        }
        if normalized != normalized.to_lowercase() {
            return Err(SkillNameError::NotLowercase);
        }
        if normalized.starts_with('-') || normalized.ends_with('-') || normalized.contains("--") {
            return Err(SkillNameError::InvalidHyphen);
        }
        if !normalized
            .chars()
            .all(|character| character == '-' || character.is_alphanumeric())
        {
            return Err(SkillNameError::InvalidCharacter);
        }
        Ok(Self(normalized))
    }

    /// Returns the normalized skill name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SkillName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Why an Agent Skills name is invalid after NFKC normalization and trimming.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillNameError {
    #[error("skill names must not be empty")]
    Empty,
    #[error("skill names exceed the 64-character bound")]
    TooLong,
    #[error("skill names must be lowercase")]
    NotLowercase,
    #[error("skill names may contain only Unicode alphanumeric characters and hyphens")]
    InvalidCharacter,
    #[error("skill names must not start, end, or repeat a hyphen")]
    InvalidHyphen,
}

/// The configured root which owns a discovered skill.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillOrigin {
    ProjectPlexmaton,
    ProjectAgents,
    User,
}

/// The boundary requesting a skill body or one of its resources.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillInvocation {
    Model,
    User,
}

/// Independent model and user invocation controls normalized from frontmatter.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct SkillInvocationPolicy {
    pub model: bool,
    pub user: bool,
}

impl Default for SkillInvocationPolicy {
    fn default() -> Self {
        Self {
            model: true,
            user: true,
        }
    }
}

impl SkillInvocationPolicy {
    pub(crate) fn permits(self, invocation: SkillInvocation) -> bool {
        match invocation {
            SkillInvocation::Model => self.model,
            SkillInvocation::User => self.user,
        }
    }
}

/// One bounded catalog summary and the origin required to load it later.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SkillEntry {
    pub origin: SkillOrigin,
    pub name: SkillName,
    pub description: String,
    pub invocation: SkillInvocationPolicy,
    /// Canonical display identity of the winning `SKILL.md`; never used for file opening.
    pub location: String,
    #[serde(skip)]
    pub(crate) root_index: usize,
    #[serde(skip)]
    pub(crate) bundle: String,
}

/// One non-fatal discovery problem omitted from the usable catalog.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SkillDiagnostic {
    pub origin: SkillOrigin,
    pub path: String,
    pub kind: SkillDiagnosticKind,
}

/// Stable categories for discovery diagnostics.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SkillDiagnosticKind {
    RootUnavailable,
    Unreadable,
    InvalidMetadata { error: SkillMetadataError },
    Shadowed { winner: SkillOrigin },
}

/// Stable frontmatter validation failures.
#[derive(Clone, Debug, Eq, Error, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SkillMetadataError {
    #[error("SKILL.md must start with a YAML frontmatter delimiter")]
    MissingFrontmatter,
    #[error("skill frontmatter exceeds the 16 KiB bound")]
    FrontmatterTooLarge,
    #[error("skill frontmatter is not valid UTF-8")]
    InvalidUtf8,
    #[error("skill frontmatter is not valid YAML")]
    InvalidYaml,
    #[error("skill frontmatter must be a mapping")]
    NotMapping,
    #[error("skill frontmatter is missing {field:?}")]
    MissingField { field: SkillMetadataField },
    #[error("skill frontmatter field {field:?} has the wrong type")]
    InvalidFieldType { field: SkillMetadataField },
    #[error("skill name is invalid: {error}")]
    InvalidName { error: SkillNameError },
    #[error("the normalized skill name must match its normalized directory name")]
    NameDirectoryMismatch,
    #[error("skill description must not be empty")]
    EmptyDescription,
    #[error("skill description exceeds the 1,024-character bound")]
    DescriptionTooLong,
    #[error("the canonical skill location exceeds the 4 KiB bound")]
    LocationTooLong,
    #[error("the canonical skill location is not valid UTF-8")]
    InvalidLocationUtf8,
}

/// Frontmatter fields whose presence and scalar types have behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillMetadataField {
    Name,
    Description,
    DisableModelInvocation,
    UserInvocable,
}

/// Exact loaded skill content suitable for serialization into model or journal state.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LoadedSkill {
    pub origin: SkillOrigin,
    pub name: SkillName,
    /// Canonical source identity of the winning `SKILL.md`; reads remain descriptor-relative.
    pub location: String,
    pub resource: Option<String>,
    pub text: String,
    pub digest: String,
}
