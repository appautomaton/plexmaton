//! Versioned opaque selectors derived from immutable canonical origins.

use std::fmt::Write as _;

use plexmaton_agent::collaboration::MAX_COLLABORATION_ID_BYTES;
use serde::Serialize;
use sha2::{Digest as _, Sha256};

use super::CollaborationToolArgumentError;

/// Opaque candidate handle that carries no authority until the owner resolves it.
#[derive(Clone, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct TargetSelector(String);

impl TargetSelector {
    /// Validates the exact versioned selector syntax accepted from a model.
    pub fn new(value: impl Into<String>) -> Result<Self, CollaborationToolArgumentError> {
        parse_selector(value.into(), "target-v1-").map(Self)
    }

    /// Returns the exact token the owner must resolve against its authenticated registry.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn issued_for(
        creation: &plexmaton_agent::collaboration::CollaborationItemRef,
    ) -> Self {
        let mut digest = Sha256::new();
        digest.update(b"plexmaton-target-selector-v1\0");
        hash_part(&mut digest, creation.collaboration.as_str());
        hash_part(&mut digest, creation.item.as_str());
        digest.update(creation.sequence.0.to_be_bytes());
        Self(encode("target-v1-", digest.finalize()))
    }
}

impl std::fmt::Debug for TargetSelector {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TargetSelector")
            .field("bytes", &self.0.len())
            .finish()
    }
}

impl std::fmt::Display for TargetSelector {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Opaque candidate artifact handle; the owner validates it in the sender's Conversation.
#[derive(Clone, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ArtifactSelector(String);

impl ArtifactSelector {
    /// Validates the exact versioned selector syntax accepted from a model.
    pub fn new(value: impl Into<String>) -> Result<Self, CollaborationToolArgumentError> {
        parse_selector(value.into(), "artifact-v1-").map(Self)
    }

    /// Returns the exact token the owner must resolve in the sender's Conversation.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn issued_for(origin: &plexmaton_agent::ArtifactAnnouncementOrigin) -> Self {
        let mut digest = Sha256::new();
        digest.update(b"plexmaton-artifact-selector-v1\0");
        hash_part(&mut digest, origin.conversation().as_str());
        hash_part(&mut digest, origin.agent().as_str());
        hash_part(&mut digest, origin.entry().as_str());
        digest.update(origin.sequence().get().to_be_bytes());
        hash_part(&mut digest, origin.artifact().as_str());
        Self(encode("artifact-v1-", digest.finalize()))
    }
}

impl std::fmt::Debug for ArtifactSelector {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ArtifactSelector")
            .field("bytes", &self.0.len())
            .finish()
    }
}

fn parse_selector(value: String, prefix: &str) -> Result<String, CollaborationToolArgumentError> {
    if value.trim().is_empty() || value.len() > MAX_COLLABORATION_ID_BYTES {
        return Err(CollaborationToolArgumentError::InvalidSelector);
    }
    let Some(digest) = value.strip_prefix(prefix) else {
        return Err(CollaborationToolArgumentError::InvalidSelector);
    };
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(CollaborationToolArgumentError::InvalidSelector);
    }
    Ok(value)
}

fn encode(prefix: &str, digest: impl AsRef<[u8]>) -> String {
    let digest = digest.as_ref();
    let mut selector = String::with_capacity(prefix.len() + digest.len() * 2);
    selector.push_str(prefix);
    for byte in digest {
        write!(&mut selector, "{byte:02x}")
            .unwrap_or_else(|_| unreachable!("writing to String cannot fail"));
    }
    selector
}

fn hash_part(digest: &mut Sha256, value: &str) {
    digest.update(
        u64::try_from(value.len())
            .unwrap_or_else(|_| unreachable!("bounded identity length fits u64"))
            .to_be_bytes(),
    );
    digest.update(value.as_bytes());
}
