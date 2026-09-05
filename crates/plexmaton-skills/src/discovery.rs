use std::{collections::HashMap, io, path::Path};

use plexmaton_file_tools::{
    BoundedReadError, DirectoryListError, FileCancellation, PathError, WorkspaceRoot,
};

use crate::{
    MAX_FRONTMATTER_BYTES, MAX_SKILL_CANDIDATES, MAX_SKILL_CATALOG_BYTES, MAX_SKILL_LOCATION_BYTES,
    SkillCatalog, SkillDiagnostic, SkillDiagnosticKind, SkillEntry, SkillError, SkillMetadataError,
    SkillName, SkillNameError, SkillOrigin, catalog::SkillRoot, parser::parse_skill,
};

const FRONTMATTER_READ_BYTES: usize = MAX_FRONTMATTER_BYTES + 16;
// Runtime framing and the enclosing JSON array stay outside individual serialized entries.
const CATALOG_PUBLICATION_RESERVE_BYTES: usize = 1024;

fn open_authority_root(path: &Path) -> Result<Option<WorkspaceRoot>, SkillError> {
    match WorkspaceRoot::open(path) {
        Ok(root) => Ok(Some(root)),
        Err(PathError::InvalidRoot) if !path.exists() => Ok(None),
        Err(source) => Err(SkillError::AuthorityRoot {
            path: path.to_path_buf(),
            source,
        }),
    }
}

struct ActiveRoot {
    origin: SkillOrigin,
    index: usize,
    root: WorkspaceRoot,
}

struct DiscoveryState {
    winners: HashMap<SkillName, usize>,
    candidates_seen: usize,
    catalog_bytes: usize,
}

impl Default for DiscoveryState {
    fn default() -> Self {
        Self {
            winners: HashMap::new(),
            candidates_seen: 0,
            catalog_bytes: CATALOG_PUBLICATION_RESERVE_BYTES + 2,
        }
    }
}

impl SkillCatalog {
    /// Discovers direct `<name>/SKILL.md` bundles in fixed project then user precedence.
    pub fn discover(
        user_home: &Path,
        project_root: &Path,
        cancellation: &FileCancellation,
    ) -> Result<Self, SkillError> {
        if cancellation.is_cancelled() {
            return Err(SkillError::Cancelled);
        }
        let mut catalog = Self {
            entries: Vec::new(),
            diagnostics: Vec::new(),
            roots: Vec::new(),
        };
        let mut state = DiscoveryState::default();
        if let Some(project) = open_authority_root(project_root)? {
            catalog.discover_root(
                &project,
                ".plexmaton/skills",
                SkillOrigin::ProjectPlexmaton,
                cancellation,
                &mut state,
            )?;
            catalog.discover_root(
                &project,
                ".agents/skills",
                SkillOrigin::ProjectAgents,
                cancellation,
                &mut state,
            )?;
        }
        if let Some(user) = open_authority_root(user_home)? {
            catalog.discover_root(&user, "skills", SkillOrigin::User, cancellation, &mut state)?;
        }
        catalog
            .entries
            .sort_by(|left, right| left.name.cmp(&right.name));
        Ok(catalog)
    }

    fn discover_root(
        &mut self,
        authority: &WorkspaceRoot,
        relative: &str,
        origin: SkillOrigin,
        cancellation: &FileCancellation,
        state: &mut DiscoveryState,
    ) -> Result<(), SkillError> {
        if cancellation.is_cancelled() {
            return Err(SkillError::Cancelled);
        }
        let path = authority.as_path().join(relative);
        let root = match authority.open_directory(relative) {
            Ok(root) => root,
            Err(PathError::NotFound) => return Ok(()),
            Err(_) => {
                self.diagnostic(origin, &path, SkillDiagnosticKind::RootUnavailable);
                return Ok(());
            }
        };
        let root_index = self.roots.len();
        self.roots.push(SkillRoot {
            origin,
            root: root.clone(),
        });
        let active = ActiveRoot {
            origin,
            index: root_index,
            root,
        };
        let remaining = MAX_SKILL_CANDIDATES.saturating_sub(state.candidates_seen);
        let candidates = match active.root.list_names(remaining, cancellation) {
            Ok(candidates) => candidates,
            Err(DirectoryListError::Cancelled) => return Err(SkillError::Cancelled),
            Err(DirectoryListError::LimitExceeded { .. }) => {
                return Err(SkillError::CandidateLimit {
                    limit: MAX_SKILL_CANDIDATES,
                });
            }
            Err(DirectoryListError::Io(_)) => {
                self.diagnostic(origin, &path, SkillDiagnosticKind::RootUnavailable);
                return Ok(());
            }
        };
        state.candidates_seen = state.candidates_seen.saturating_add(candidates.len());
        for candidate in candidates {
            self.discover_candidate(&active, candidate, cancellation, state)?;
        }
        Ok(())
    }

    fn discover_candidate(
        &mut self,
        root: &ActiveRoot,
        candidate: std::ffi::OsString,
        cancellation: &FileCancellation,
        state: &mut DiscoveryState,
    ) -> Result<(), SkillError> {
        if cancellation.is_cancelled() {
            return Err(SkillError::Cancelled);
        }
        let bundle = match candidate.into_string() {
            Ok(bundle) => bundle,
            Err(_) => {
                self.diagnostic(
                    root.origin,
                    root.root.as_path(),
                    SkillDiagnosticKind::InvalidMetadata {
                        error: SkillMetadataError::InvalidName {
                            error: SkillNameError::InvalidCharacter,
                        },
                    },
                );
                return Ok(());
            }
        };
        let candidate_path = root.root.as_path().join(&bundle);
        match root.root.open_directory(&bundle) {
            Ok(_) => {}
            Err(PathError::Io(io::ErrorKind::NotADirectory)) => return Ok(()),
            Err(_) => {
                self.diagnostic(
                    root.origin,
                    &candidate_path,
                    SkillDiagnosticKind::Unreadable,
                );
                return Ok(());
            }
        }
        let relative = Path::new(&bundle).join("SKILL.md");
        let read = match root.root.read_prefix(
            &relative.to_string_lossy(),
            FRONTMATTER_READ_BYTES,
            cancellation,
        ) {
            Ok(read) => read,
            Err(BoundedReadError::Path(PathError::NotFound)) => return Ok(()),
            Err(BoundedReadError::Cancelled) => return Err(SkillError::Cancelled),
            Err(_) => {
                self.diagnostic(
                    root.origin,
                    &candidate_path,
                    SkillDiagnosticKind::Unreadable,
                );
                return Ok(());
            }
        };
        let parsed = match parse_skill(&read.bytes, read.complete, &bundle) {
            Ok(parsed) => parsed,
            Err(error) => {
                self.diagnostic(
                    root.origin,
                    &relative,
                    SkillDiagnosticKind::InvalidMetadata { error },
                );
                return Ok(());
            }
        };
        if let Some(winner_index) = state.winners.get(&parsed.name).copied() {
            let winner = self
                .entries
                .get(winner_index)
                .map(|entry| entry.origin)
                .ok_or(SkillError::CatalogInvariant)?;
            self.diagnostic(
                root.origin,
                &relative,
                SkillDiagnosticKind::Shadowed { winner },
            );
            return Ok(());
        }
        let location_path = root.root.as_path().join(&bundle).join("SKILL.md");
        let Some(location) = location_path.to_str().map(str::to_owned) else {
            self.diagnostic(
                root.origin,
                &relative,
                SkillDiagnosticKind::InvalidMetadata {
                    error: SkillMetadataError::InvalidLocationUtf8,
                },
            );
            return Ok(());
        };
        if location.len() > MAX_SKILL_LOCATION_BYTES {
            self.diagnostic(
                root.origin,
                &relative,
                SkillDiagnosticKind::InvalidMetadata {
                    error: SkillMetadataError::LocationTooLong,
                },
            );
            return Ok(());
        }
        let entry = SkillEntry {
            origin: root.origin,
            name: parsed.name,
            description: parsed.description,
            invocation: parsed.invocation,
            location,
            root_index: root.index,
            bundle,
        };
        let entry_bytes = serde_json::to_vec(&entry)
            .map_err(|_| SkillError::CatalogInvariant)?
            .len()
            .saturating_add(1);
        state.catalog_bytes = state.catalog_bytes.saturating_add(entry_bytes);
        if state.catalog_bytes > MAX_SKILL_CATALOG_BYTES {
            return Err(SkillError::CatalogLimit {
                limit: MAX_SKILL_CATALOG_BYTES,
            });
        }
        let index = self.entries.len();
        state.winners.insert(entry.name.clone(), index);
        self.entries.push(entry);
        Ok(())
    }

    fn diagnostic(&mut self, origin: SkillOrigin, path: &Path, kind: SkillDiagnosticKind) {
        self.diagnostics.push(SkillDiagnostic {
            origin,
            path: path.to_string_lossy().into_owned(),
            kind,
        });
    }
}
