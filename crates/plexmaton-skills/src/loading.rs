use std::path::{Component, Path, PathBuf};

use plexmaton_file_tools::{BoundedFileRead, BoundedReadError, FileCancellation};
use sha2::{Digest as _, Sha256};

use crate::{
    LoadedSkill, MAX_FRONTMATTER_BYTES, MAX_SKILL_CONTENT_BYTES, SkillCatalog, SkillEntry,
    SkillError, SkillInvocation, SkillName, SkillOrigin, catalog::SkillRoot, parser::parse_skill,
};

const FRONTMATTER_READ_BYTES: usize = MAX_FRONTMATTER_BYTES + 16;
const DIGEST_DOMAIN: &[u8] = b"plexmaton.skill-content.sha256.v1";

impl SkillCatalog {
    /// Reads the current winning body or one relative resource through its pinned root.
    pub fn read(
        &self,
        name: &str,
        resource: Option<&str>,
        invocation: SkillInvocation,
        cancellation: &FileCancellation,
    ) -> Result<LoadedSkill, SkillError> {
        if cancellation.is_cancelled() {
            return Err(SkillError::Cancelled);
        }
        let name = SkillName::new(name).map_err(SkillError::InvalidName)?;
        let entry = self
            .entries
            .binary_search_by(|candidate| candidate.name.cmp(&name))
            .ok()
            .and_then(|index| self.entries.get(index))
            .ok_or_else(|| SkillError::UnknownSkill { name: name.clone() })?;
        if !entry.invocation.permits(invocation) {
            return Err(SkillError::InvocationDenied { name, invocation });
        }
        let root = self
            .roots
            .get(entry.root_index)
            .ok_or(SkillError::CatalogInvariant)?;
        let (resource, bytes) = match resource {
            Some(resource) => {
                self.read_resource(root, entry, resource, invocation, cancellation)?
            }
            None => self.read_body(root, entry, invocation, cancellation)?,
        };
        let text = String::from_utf8(bytes).map_err(|_| SkillError::InvalidUtf8)?;
        let digest = content_digest(
            root.origin,
            &entry.name,
            &entry.location,
            resource.as_deref(),
            text.as_bytes(),
        );
        Ok(LoadedSkill {
            origin: root.origin,
            name: entry.name.clone(),
            location: entry.location.clone(),
            resource,
            text,
            digest,
        })
    }

    fn read_resource(
        &self,
        root: &SkillRoot,
        entry: &SkillEntry,
        resource: &str,
        invocation: SkillInvocation,
        cancellation: &FileCancellation,
    ) -> Result<(Option<String>, Vec<u8>), SkillError> {
        let current = self.read_metadata(root, entry, cancellation)?;
        ensure_current(entry, &current, invocation)?;
        let resource = normalize_resource(resource)?;
        let path = Path::new(&entry.bundle).join(&resource);
        let read = read_prefix(
            root,
            &path.to_string_lossy(),
            MAX_SKILL_CONTENT_BYTES,
            cancellation,
        )?;
        if !read.complete {
            return Err(SkillError::ContentTooLarge {
                limit: MAX_SKILL_CONTENT_BYTES,
            });
        }
        let current = self.read_metadata(root, entry, cancellation)?;
        ensure_current(entry, &current, invocation)?;
        Ok((Some(resource), read.bytes))
    }

    fn read_body(
        &self,
        root: &SkillRoot,
        entry: &SkillEntry,
        invocation: SkillInvocation,
        cancellation: &FileCancellation,
    ) -> Result<(Option<String>, Vec<u8>), SkillError> {
        let path = Path::new(&entry.bundle).join("SKILL.md");
        let max_file_bytes = MAX_SKILL_CONTENT_BYTES
            .saturating_add(MAX_FRONTMATTER_BYTES)
            .saturating_add(16);
        let read = read_prefix(root, &path.to_string_lossy(), max_file_bytes, cancellation)?;
        if !read.complete {
            return Err(SkillError::ContentTooLarge {
                limit: MAX_SKILL_CONTENT_BYTES,
            });
        }
        let parsed = parse_skill(&read.bytes, true, &entry.bundle).map_err(|error| {
            SkillError::MetadataChanged {
                name: entry.name.clone(),
                error,
            }
        })?;
        ensure_current(entry, &parsed, invocation)?;
        let body = read
            .bytes
            .get(parsed.body_offset..)
            .ok_or(SkillError::CatalogInvariant)?;
        if body.len() > MAX_SKILL_CONTENT_BYTES {
            return Err(SkillError::ContentTooLarge {
                limit: MAX_SKILL_CONTENT_BYTES,
            });
        }
        Ok((None, body.to_vec()))
    }

    fn read_metadata(
        &self,
        root: &SkillRoot,
        entry: &SkillEntry,
        cancellation: &FileCancellation,
    ) -> Result<crate::parser::ParsedSkill, SkillError> {
        let path = Path::new(&entry.bundle).join("SKILL.md");
        let read = read_prefix(
            root,
            &path.to_string_lossy(),
            FRONTMATTER_READ_BYTES,
            cancellation,
        )?;
        parse_skill(&read.bytes, read.complete, &entry.bundle).map_err(|error| {
            SkillError::MetadataChanged {
                name: entry.name.clone(),
                error,
            }
        })
    }
}

fn read_prefix(
    root: &SkillRoot,
    path: &str,
    max_bytes: usize,
    cancellation: &FileCancellation,
) -> Result<BoundedFileRead, SkillError> {
    root.root
        .read_prefix(path, max_bytes, cancellation)
        .map_err(|source| match source {
            BoundedReadError::Cancelled => SkillError::Cancelled,
            source => SkillError::Read {
                origin: root.origin,
                path: path.to_owned(),
                source,
            },
        })
}

fn ensure_current(
    entry: &SkillEntry,
    current: &crate::parser::ParsedSkill,
    invocation: SkillInvocation,
) -> Result<(), SkillError> {
    if current.name != entry.name {
        return Err(SkillError::SourceChanged {
            name: entry.name.clone(),
        });
    }
    if !current.invocation.permits(invocation) {
        return Err(SkillError::InvocationDenied {
            name: entry.name.clone(),
            invocation,
        });
    }
    Ok(())
}

fn normalize_resource(resource: &str) -> Result<String, SkillError> {
    if resource.is_empty() || resource.len() > 4096 {
        return Err(SkillError::InvalidResource);
    }
    let path = Path::new(resource);
    if path.is_absolute() {
        return Err(SkillError::InvalidResource);
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => normalized.push(value),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(SkillError::InvalidResource);
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err(SkillError::InvalidResource);
    }
    Ok(normalized.to_string_lossy().into_owned())
}

fn content_digest(
    origin: SkillOrigin,
    name: &SkillName,
    location: &str,
    resource: Option<&str>,
    bytes: &[u8],
) -> String {
    let mut digest = Sha256::new();
    digest.update(DIGEST_DOMAIN);
    let origin_tag = match origin {
        SkillOrigin::ProjectPlexmaton => 0,
        SkillOrigin::ProjectAgents => 1,
        SkillOrigin::User => 2,
    };
    digest_field(&mut digest, &[origin_tag]);
    digest_field(&mut digest, name.as_str().as_bytes());
    digest_field(&mut digest, location.as_bytes());
    match resource {
        Some(resource) => {
            digest_field(&mut digest, b"resource");
            digest_field(&mut digest, resource.as_bytes());
        }
        None => digest_field(&mut digest, b"main"),
    }
    digest_field(&mut digest, bytes);
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn digest_field(digest: &mut Sha256, value: &[u8]) {
    digest.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
    digest.update(value);
}
