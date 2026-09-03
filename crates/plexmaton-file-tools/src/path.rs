//! Relative-path validation and descriptor-relative file opening.

use std::{
    ffi::OsString,
    fs::File,
    io,
    path::{Component, Path, PathBuf},
};

#[cfg(unix)]
use std::sync::Arc;

use thiserror::Error;

const MAX_PATH_BYTES: usize = 4096;

/// Canonical directory from which every file-tool path is resolved (WFS-1).
#[derive(Clone, Debug)]
pub struct WorkspaceRoot {
    canonical: PathBuf,
    #[cfg(unix)]
    directory: Arc<File>,
}

/// Why a model path cannot name an object inside the pinned workspace.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum PathError {
    #[error("the workspace root is not an existing directory")]
    InvalidRoot,
    #[error("secure workspace file tools are not available on this platform")]
    UnsupportedPlatform,
    #[error("the path must be a non-empty relative workspace path")]
    NotRelative,
    #[error("the path exceeds the workspace-path byte bound")]
    TooLong,
    #[error("the path does not exist")]
    NotFound,
    #[error("symbolic links are not accepted by the file tools")]
    Symlink,
    #[error("the path does not name a regular file")]
    NotFile,
    #[error("the workspace path could not be opened: {0:?}")]
    Io(io::ErrorKind),
}

pub(crate) struct SearchTarget {
    pub(crate) display: String,
    pub(crate) directory: File,
    pub(crate) file: Option<File>,
}

#[cfg(unix)]
impl SearchTarget {
    pub(crate) fn open_child_file(&self, supplied: &str) -> Result<(String, File), PathError> {
        if self.file.is_some() {
            return Err(PathError::NotFile);
        }
        let relative = validate_relative(supplied, false)?;
        let file = open_beneath(&self.directory, &relative.components)?;
        if !file
            .metadata()
            .map_err(|error| PathError::Io(error.kind()))?
            .is_file()
        {
            return Err(PathError::NotFile);
        }
        let display = if self.display == "." {
            relative.display
        } else {
            Path::new(&self.display)
                .join(relative.display)
                .to_string_lossy()
                .into_owned()
        };
        Ok((display, file))
    }
}

impl WorkspaceRoot {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, PathError> {
        #[cfg(not(unix))]
        {
            let _root = root;
            return Err(PathError::UnsupportedPlatform);
        }
        #[cfg(unix)]
        let canonical = std::fs::canonicalize(root).map_err(|_| PathError::InvalidRoot)?;
        #[cfg(unix)]
        if !canonical.is_dir() {
            return Err(PathError::InvalidRoot);
        }
        #[cfg(unix)]
        let directory = Arc::new(pin_root(&canonical)?);
        #[cfg(unix)]
        Ok(Self {
            canonical,
            directory,
        })
    }

    /// Stable display path for status and test evidence, never a resolution authority.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.canonical
    }

    pub(crate) fn open_file(&self, supplied: &str) -> Result<(String, File), PathError> {
        let relative = validate_relative(supplied, false)?;
        #[cfg(unix)]
        let file = open_beneath(&self.directory, &relative.components).or_else(|error| {
            self.reject_visible_symlink(&relative)?;
            Err(error)
        })?;
        #[cfg(not(unix))]
        let file = {
            self.reject_visible_symlink(&relative)?;
            open_beneath(&self.canonical, &relative.components)?
        };
        if !file
            .metadata()
            .map_err(|error| PathError::Io(error.kind()))?
            .is_file()
        {
            return Err(PathError::NotFile);
        }
        Ok((relative.display, file))
    }

    #[cfg(unix)]
    pub(crate) fn search_target(&self, supplied: &str) -> Result<SearchTarget, PathError> {
        let relative = validate_relative(supplied, true)?;
        let target = if relative.components.is_empty() {
            self.directory
                .try_clone()
                .map_err(|error| PathError::Io(error.kind()))?
        } else {
            open_beneath(&self.directory, &relative.components).or_else(|error| {
                self.reject_visible_symlink(&relative)?;
                Err(error)
            })?
        };
        let metadata = target
            .metadata()
            .map_err(|error| PathError::Io(error.kind()))?;
        if metadata.is_dir() {
            Ok(SearchTarget {
                display: relative.display,
                directory: target,
                file: None,
            })
        } else if metadata.is_file() {
            Ok(SearchTarget {
                display: relative.display,
                directory: self
                    .directory
                    .try_clone()
                    .map_err(|error| PathError::Io(error.kind()))?,
                file: Some(target),
            })
        } else {
            Err(PathError::NotFile)
        }
    }

    #[cfg(not(unix))]
    pub(crate) fn search_target(&self, _supplied: &str) -> Result<SearchTarget, PathError> {
        Err(PathError::UnsupportedPlatform)
    }

    fn reject_visible_symlink(&self, relative: &ValidatedPath) -> Result<(), PathError> {
        let mut current = self.canonical.clone();
        for component in &relative.components {
            current.push(component);
            let metadata = std::fs::symlink_metadata(&current).map_err(map_path_io)?;
            if metadata.file_type().is_symlink() {
                return Err(PathError::Symlink);
            }
        }
        Ok(())
    }
}

struct ValidatedPath {
    display: String,
    components: Vec<OsString>,
}

fn validate_relative(supplied: &str, allow_root: bool) -> Result<ValidatedPath, PathError> {
    if supplied.len() > MAX_PATH_BYTES {
        return Err(PathError::TooLong);
    }
    let path = Path::new(supplied);
    if path.is_absolute() {
        return Err(PathError::NotRelative);
    }
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => components.push(value.to_owned()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(PathError::NotRelative);
            }
        }
    }
    if components.is_empty() && !allow_root {
        return Err(PathError::NotRelative);
    }
    let display = if components.is_empty() {
        ".".to_owned()
    } else {
        components
            .iter()
            .collect::<PathBuf>()
            .to_string_lossy()
            .into_owned()
    };
    Ok(ValidatedPath {
        display,
        components,
    })
}

pub(crate) fn validate_file_path(supplied: &str) -> Result<(), PathError> {
    validate_relative(supplied, false).map(|_| ())
}

pub(crate) fn validate_search_path(supplied: &str) -> Result<(), PathError> {
    validate_relative(supplied, true).map(|_| ())
}

fn map_path_io(error: io::Error) -> PathError {
    match error.kind() {
        io::ErrorKind::NotFound => PathError::NotFound,
        kind => PathError::Io(kind),
    }
}

#[cfg(unix)]
fn pin_root(root: &Path) -> Result<File, PathError> {
    use rustix::fs::{Mode, OFlags, open};

    open(
        root,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map(File::from)
    .map_err(map_rustix)
}

#[cfg(unix)]
fn open_beneath(root: &File, components: &[OsString]) -> Result<File, PathError> {
    use rustix::fs::{Mode, OFlags, openat};

    let directory_flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW;
    let mut directory = root
        .try_clone()
        .map_err(|error| PathError::Io(error.kind()))?;
    let Some((file_name, parents)) = components.split_last() else {
        return Err(PathError::NotRelative);
    };
    for component in parents {
        directory = File::from(
            openat(&directory, component, directory_flags, Mode::empty()).map_err(map_rustix)?,
        );
    }
    let file = openat(
        &directory,
        file_name,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(map_rustix)?;
    Ok(File::from(file))
}

#[cfg(unix)]
fn map_rustix(error: rustix::io::Errno) -> PathError {
    if error == rustix::io::Errno::LOOP {
        PathError::Symlink
    } else if error == rustix::io::Errno::NOENT {
        PathError::NotFound
    } else {
        PathError::Io(io::Error::from_raw_os_error(error.raw_os_error()).kind())
    }
}

#[cfg(not(unix))]
fn open_beneath(_root: &Path, _components: &[OsString]) -> Result<File, PathError> {
    Err(PathError::UnsupportedPlatform)
}
