//! Catalog-issued, bounded permission facts. These are not reconstructed from UI detail.
use serde::{Deserialize, Serialize};

/// The native mutation definitions intentionally admitted to the file-change preset.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FileChangeOperation {
    Create,
    Edit,
}

/// A validated relative path and its exact native operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeFileChange {
    operation: FileChangeOperation,
    path: String,
}

impl NativeFileChange {
    /// Creates facts only for a native create operation on an already admitted relative path.
    #[must_use]
    pub fn create(path: String) -> Option<Self> {
        Self::new(FileChangeOperation::Create, path)
    }
    /// Creates facts only for a native edit operation on an already admitted relative path.
    #[must_use]
    pub fn edit(path: String) -> Option<Self> {
        Self::new(FileChangeOperation::Edit, path)
    }

    fn new(operation: FileChangeOperation, path: String) -> Option<Self> {
        if path.is_empty()
            || path.len() > 4096
            || path.contains('\0')
            || path.starts_with('/')
            || path.split('/').any(|part| matches!(part, "" | "." | ".."))
        {
            return None;
        }
        Some(Self { operation, path })
    }

    pub(super) const fn operation(&self) -> FileChangeOperation {
        self.operation
    }

    /// Broad file-change grants exclude agent controls and Git metadata at any nesting depth.
    #[must_use]
    pub fn is_project_file(&self) -> bool {
        !self.path.split('/').any(|part| {
            matches!(
                part.to_ascii_lowercase().as_str(),
                ".git" | ".plexmaton" | ".agents" | ".codex" | "agents.md"
            )
        })
    }
}

/// Exact command source plus trusted execution context, independent of approval display text.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CommandPermission {
    source: String,
    context: [u8; 32],
}

impl<'de> Deserialize<'de> for CommandPermission {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            source: String,
            context: [u8; 32],
        }
        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.source, wire.context)
            .ok_or_else(|| serde::de::Error::custom("invalid bounded command permission"))
    }
}

impl CommandPermission {
    /// `context` fingerprints the physical working directory, shell and captured environment.
    #[must_use]
    pub fn new(source: String, context: [u8; 32]) -> Option<Self> {
        if source.is_empty() || source.len() > 24 * 1024 || source.contains('\0') {
            return None;
        }
        Some(Self { source, context })
    }

    /// Exact shell text the user can review before granting reuse.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Opaque execution-context identity; it never exposes captured environment values.
    #[must_use]
    pub const fn context(&self) -> &[u8; 32] {
        &self.context
    }
}

/// Permission-relevant facts frozen by a trusted catalog at admission.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum PermissionSubject {
    /// This definition has no reusable semantic scope; capability fallback still applies.
    #[default]
    Opaque,
    /// Known native create/edit, never inferred from a `FileWrite` capability.
    NativeFileChange(NativeFileChange),
    /// Command source and execution context captured by its executor.
    Command {
        /// Exact source and execution context, retained even when parsing cannot offer reuse.
        command: CommandPermission,
        /// Bounded facts from the command adapter, never parsed by the semantic evaluator.
        syntax: super::CommandSyntax,
    },
}
