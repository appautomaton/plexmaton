//! Bounded AGENTS.md discovery at the conversation-open boundary, never during journal replay.

use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
};

use plexmaton_file_tools::{BoundedReadError, FileCancellation, PathError, WorkspaceRoot};
use plexmaton_provider::{ConfigError, MAX_WORKSPACE_INSTRUCTION_BYTES, ResolvedModel};
use serde::Serialize;
use thiserror::Error;

const FILENAME: &str = "AGENTS.md";
const MAX_PROJECT_DIRECTORIES: usize = 128;
const PREAMBLE: &str = include_str!("agent_instructions/prompt.md");

#[derive(Debug, Error)]
pub(super) enum InstructionError {
    #[error("AGENTS.md loading was cancelled")]
    Cancelled,
    #[error("cannot open instruction root {path:?}: {source}")]
    Root { path: PathBuf, source: PathError },
    #[error("cannot resolve instruction directory {path:?}: {kind:?}")]
    Directory { path: PathBuf, kind: io::ErrorKind },
    #[error("the working directory is outside the instruction project root")]
    OutsideProject,
    #[error("the instruction project path exceeds the {MAX_PROJECT_DIRECTORIES}-directory bound")]
    TooDeep,
    #[error("instruction source paths must be UTF-8")]
    InvalidPath,
    #[error("cannot read instructions at {path:?}: {source}")]
    Read {
        path: PathBuf,
        source: BoundedReadError,
    },
    #[error("instructions at {path:?} must be UTF-8 text without NUL bytes")]
    InvalidText { path: PathBuf },
    #[error("AGENTS.md context exceeds the {MAX_WORKSPACE_INSTRUCTION_BYTES}-byte bound")]
    TooLarge,
    #[error("cannot configure workspace instructions: {0}")]
    Configuration(#[from] ConfigError),
}

/// Composes current instruction files with a model without changing its configured system prompt.
pub(super) fn resolve_model(
    model: &ResolvedModel,
    home: &Path,
    project_root: &Path,
    cwd: &Path,
    cancellation: &FileCancellation,
) -> Result<ResolvedModel, InstructionError> {
    let text = load(home, project_root, cwd, cancellation)?;
    Ok(model.with_workspace_instructions(text)?)
}

fn load(
    home: &Path,
    project_root: &Path,
    cwd: &Path,
    cancellation: &FileCancellation,
) -> Result<String, InstructionError> {
    check_cancelled(cancellation)?;
    let project = pin(project_root)?;
    let cwd = fs::canonicalize(cwd).map_err(|error| InstructionError::Directory {
        path: cwd.to_path_buf(),
        kind: error.kind(),
    })?;
    let relative = cwd
        .strip_prefix(project.as_path())
        .map_err(|_| InstructionError::OutsideProject)?;
    let directories: Vec<_> = relative.ancestors().collect();
    if directories.len() > MAX_PROJECT_DIRECTORIES {
        return Err(InstructionError::TooDeep);
    }
    // Cwd is trusted startup state, but must still name a directory inside the pinned checkout.
    if !relative.as_os_str().is_empty() {
        project
            .open_directory(path_text(relative)?)
            .map_err(|source| InstructionError::Root {
                path: cwd.clone(),
                source,
            })?;
    }
    let mut snapshot = Snapshot::new(&cwd)?;
    match fs::symlink_metadata(home) {
        Ok(_) => snapshot.read(
            &pin(home)?,
            Path::new(FILENAME),
            Scope::Conversation,
            cancellation,
        )?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(InstructionError::Directory {
                path: home.to_path_buf(),
                kind: error.kind(),
            });
        }
    }
    for directory in directories.into_iter().rev() {
        check_cancelled(cancellation)?;
        let scope_path = if directory.as_os_str().is_empty() {
            project.as_path().to_path_buf()
        } else {
            project.as_path().join(directory)
        };
        let scope = Scope::Directory(path_text(&scope_path)?.to_owned());
        snapshot.read(&project, &directory.join(FILENAME), scope, cancellation)?;
    }
    check_cancelled(cancellation)?;
    snapshot.text.push_str("]}");
    Ok(snapshot.text)
}

fn pin(path: &Path) -> Result<WorkspaceRoot, InstructionError> {
    WorkspaceRoot::open(path).map_err(|source| InstructionError::Root {
        path: path.to_path_buf(),
        source,
    })
}

fn path_text(path: &Path) -> Result<&str, InstructionError> {
    path.to_str().ok_or(InstructionError::InvalidPath)
}

fn check_cancelled(cancellation: &FileCancellation) -> Result<(), InstructionError> {
    if cancellation.is_cancelled() {
        Err(InstructionError::Cancelled)
    } else {
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(tag = "kind", content = "path", rename_all = "snake_case")]
enum Scope {
    Conversation,
    Directory(String),
}

#[derive(Serialize)]
struct InstructionFile {
    source: String,
    scope: Scope,
    instructions: String,
}

struct Snapshot {
    text: String,
    paths: BTreeSet<PathBuf>,
    files: usize,
}

impl Snapshot {
    fn new(cwd: &Path) -> Result<Self, InstructionError> {
        let text = format!(
            "{PREAMBLE}\n{{\"working_directory\":{},\"files\":[",
            serde_json::json!(path_text(cwd)?)
        );
        if text.len() + 2 > MAX_WORKSPACE_INSTRUCTION_BYTES {
            return Err(InstructionError::TooLarge);
        }
        Ok(Self {
            text,
            paths: BTreeSet::new(),
            files: 0,
        })
    }

    fn read(
        &mut self,
        root: &WorkspaceRoot,
        relative: &Path,
        scope: Scope,
        cancellation: &FileCancellation,
    ) -> Result<(), InstructionError> {
        let path = root.as_path().join(relative);
        if !self.paths.insert(path.clone()) {
            return Ok(());
        }
        let read = match root.read_prefix(
            path_text(relative)?,
            MAX_WORKSPACE_INSTRUCTION_BYTES,
            cancellation,
        ) {
            Ok(read) => read,
            Err(BoundedReadError::Path(PathError::NotFound)) => return Ok(()),
            Err(BoundedReadError::Cancelled) => return Err(InstructionError::Cancelled),
            Err(source) => return Err(InstructionError::Read { path, source }),
        };
        if !read.complete {
            return Err(InstructionError::TooLarge);
        }
        let instructions = String::from_utf8(read.bytes)
            .map_err(|_| InstructionError::InvalidText { path: path.clone() })?;
        if instructions.contains('\0') {
            return Err(InstructionError::InvalidText { path });
        }
        if instructions.trim().is_empty() {
            return Ok(());
        }
        let file = InstructionFile {
            source: path_text(&path)?.to_owned(),
            scope,
            instructions,
        };
        let encoded = serde_json::to_string(&file)
            .expect("fixed string fields and scope variants are JSON serializable");
        let separator = usize::from(self.files != 0);
        // Reserve the closing array/object too; JSON escaping must not evade the publication cap.
        if self.text.len() + separator + encoded.len() + 2 > MAX_WORKSPACE_INSTRUCTION_BYTES {
            return Err(InstructionError::TooLarge);
        }
        if self.files != 0 {
            self.text.push(',');
        }
        self.text.push_str(&encoded);
        self.files += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
