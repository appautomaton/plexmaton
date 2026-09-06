//! Physical checkout discovery and the single bounded project-configuration reader.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read as _},
    os::unix::fs::OpenOptionsExt as _,
    path::{Path, PathBuf},
};

use crate::permission_config::PermissionDeclarations;
use plexmaton_agent::{PermissionChangeError, PermissionConfiguration, PermissionRuleSource};
use plexmaton_file_tools::{BoundedReadError, FileCancellation, PathError, WorkspaceRoot};
use plexmaton_provider::{ModelRegistry, ModelSelection, ResolvedModel};
use plexmaton_runtime::{NativePermissionCompiler, ProjectPermissionConfigurationSource};
use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use thiserror::Error;

const PROJECT_CONFIG_PATH: &str = ".plexmaton/config.toml";
const MAX_PROJECT_CONFIG_BYTES: usize = 64 * 1024;
const MAX_GITFILE_BYTES: u64 = 4096;

/// A project setting failed before it could safely select a user-defined model.
#[derive(Debug, Error)]
pub(crate) enum ProjectConfigError {
    #[error("the starting working directory could not be resolved: {0:?}")]
    InvalidWorkingDirectory(io::ErrorKind),
    #[error("the starting working directory is not a directory")]
    WorkingDirectoryNotDirectory,
    #[error("the project root could not be pinned: {0}")]
    InvalidProjectRoot(PathError),
    #[error("the project configuration could not be read: {0}")]
    Read(BoundedReadError),
    #[error("the project configuration exceeds the 64 KiB byte bound")]
    TooLarge,
    #[error("the project configuration is not UTF-8")]
    InvalidUtf8,
    #[error("the project configuration is invalid")]
    InvalidConfiguration,
    #[error("project model `{model}` does not exist under provider `{provider}`")]
    UnknownModel { provider: String, model: String },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectConfig {
    active_model: Option<ModelSelection>,
    #[serde(default)]
    permissions: PermissionDeclarations,
}

pub(crate) struct LoadedProjectConfig {
    value: Option<(ProjectConfig, [u8; 32])>,
}

pub(crate) struct ProjectPermissionReader {
    root: PathBuf,
    compiler: NativePermissionCompiler,
}

impl ProjectPermissionReader {
    pub(crate) fn new(root: PathBuf, compiler: NativePermissionCompiler) -> Self {
        Self { root, compiler }
    }
}

impl ProjectPermissionConfigurationSource for ProjectPermissionReader {
    fn load(
        &self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Option<PermissionConfiguration>, PermissionChangeError> {
        if cancelled() {
            return Err(PermissionChangeError::Unavailable);
        }
        let loaded = load(&self.root).map_err(|_| PermissionChangeError::Unavailable)?;
        if cancelled() {
            return Err(PermissionChangeError::Unavailable);
        }
        loaded.permissions(&self.compiler)
    }
}

impl LoadedProjectConfig {
    pub(crate) fn select_model(
        &self,
        registry: &ModelRegistry,
    ) -> Result<ResolvedModel, ProjectConfigError> {
        let Some(selection) = self
            .value
            .as_ref()
            .and_then(|(config, _)| config.active_model.as_ref())
        else {
            return Ok(registry.active_model().clone());
        };
        registry
            .model(selection.provider(), selection.model())
            .cloned()
            .ok_or_else(|| ProjectConfigError::UnknownModel {
                provider: selection.provider().to_owned(),
                model: selection.model().to_owned(),
            })
    }

    fn permissions(
        &self,
        compiler: &NativePermissionCompiler,
    ) -> Result<Option<PermissionConfiguration>, PermissionChangeError> {
        match &self.value {
            Some((config, fingerprint)) => {
                let fingerprint = *fingerprint;
                let rules = config.permissions.compile(compiler, |index| {
                    PermissionRuleSource::ProjectConfiguration { fingerprint, index }
                })?;
                PermissionConfiguration::new(fingerprint, rules).map(Some)
            }
            None => Ok(None),
        }
    }
}

/// Finds the nearest physical checkout containing a real Git marker.
///
/// A linked-worktree gitfile establishes its containing checkout as the root. Its `gitdir` target
/// is validated but never returned, so project configuration cannot come from the common repo.
pub(crate) fn discover_project_root(cwd: &Path) -> Result<PathBuf, ProjectConfigError> {
    let starting = fs::canonicalize(cwd)
        .map_err(|error| ProjectConfigError::InvalidWorkingDirectory(error.kind()))?;
    if !starting.is_dir() {
        return Err(ProjectConfigError::WorkingDirectoryNotDirectory);
    }
    Ok(starting
        .ancestors()
        .find(|candidate| valid_git_marker(candidate))
        .unwrap_or(&starting)
        .to_path_buf())
}

/// Reads the complete project file without permitting credentials or user-owned configuration tables.
pub(crate) fn load(project_root: &Path) -> Result<LoadedProjectConfig, ProjectConfigError> {
    let root = WorkspaceRoot::open(project_root).map_err(ProjectConfigError::InvalidProjectRoot)?;
    let cancellation = FileCancellation::new();
    let read = match root.read_prefix(PROJECT_CONFIG_PATH, MAX_PROJECT_CONFIG_BYTES, &cancellation)
    {
        Ok(read) => read,
        Err(BoundedReadError::Path(PathError::NotFound)) => {
            return Ok(LoadedProjectConfig { value: None });
        }
        Err(error) => return Err(ProjectConfigError::Read(error)),
    };
    if !read.complete {
        return Err(ProjectConfigError::TooLarge);
    }
    let source = std::str::from_utf8(&read.bytes).map_err(|_| ProjectConfigError::InvalidUtf8)?;
    let config = toml::from_str(source).map_err(|_| ProjectConfigError::InvalidConfiguration)?;
    Ok(LoadedProjectConfig {
        value: Some((config, Sha256::digest(&read.bytes).into())),
    })
}

fn valid_git_marker(checkout: &Path) -> bool {
    let marker = checkout.join(".git");
    let Ok(metadata) = fs::symlink_metadata(&marker) else {
        return false;
    };
    metadata.file_type().is_dir()
        || (metadata.file_type().is_file() && valid_gitfile(checkout, &marker, metadata.len()))
}

fn valid_gitfile(checkout: &Path, marker: &Path, length: u64) -> bool {
    if length > MAX_GITFILE_BYTES {
        return false;
    }
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32);
    let Ok(file) = options.open(marker) else {
        return false;
    };
    let Some(target) = read_gitdir(file) else {
        return false;
    };
    let target = Path::new(&target);
    let target = if target.is_absolute() {
        target.to_path_buf()
    } else {
        checkout.join(target)
    };
    fs::canonicalize(target).is_ok_and(|target| target.is_dir())
}

fn read_gitdir(file: File) -> Option<String> {
    let mut bytes = Vec::new();
    file.take(MAX_GITFILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > usize::try_from(MAX_GITFILE_BYTES).ok()? {
        return None;
    }
    let source = std::str::from_utf8(&bytes).ok()?.trim();
    let target = source.strip_prefix("gitdir:")?.trim();
    (!target.is_empty()
        && !target
            .chars()
            .any(|character| matches!(character, '\n' | '\r' | '\0')))
    .then(|| target.to_owned())
}

#[cfg(test)]
mod tests;
