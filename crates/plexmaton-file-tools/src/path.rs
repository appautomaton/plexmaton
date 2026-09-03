//! Relative-path validation and descriptor-relative file opening.

use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io,
    path::{Component, Path, PathBuf},
};

#[cfg(unix)]
use std::sync::Arc;

use thiserror::Error;

pub(crate) const MAX_PATH_CHARACTERS: usize = 4096;
pub(crate) const MAX_PATH_BYTES: usize = 4096;

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
    #[error("an entry already exists at the path")]
    AlreadyExists,
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

/// A validated leaf whose parent directory remains pinned for one mutation.
pub(crate) struct MutationPath {
    parent: File,
    leaf: OsString,
    display: String,
}

#[cfg(unix)]
impl MutationPath {
    pub(crate) fn parent(&self) -> &File {
        &self.parent
    }

    pub(crate) fn leaf(&self) -> &OsStr {
        &self.leaf
    }

    pub(crate) fn display(&self) -> &str {
        &self.display
    }

    pub(crate) fn open_existing(&self) -> Result<File, PathError> {
        let file = open_leaf(&self.parent, &self.leaf)?;
        if !file
            .metadata()
            .map_err(|error| PathError::Io(error.kind()))?
            .is_file()
        {
            return Err(PathError::NotFile);
        }
        Ok(file)
    }

    pub(crate) fn require_absent(&self) -> Result<(), PathError> {
        match rustix::fs::statat(
            &self.parent,
            &self.leaf,
            rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
        ) {
            Ok(_) => Err(PathError::AlreadyExists),
            Err(error) if error == rustix::io::Errno::NOENT => Ok(()),
            Err(error) => Err(map_rustix(error)),
        }
    }
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
    pub(crate) fn mutation_path(&self, supplied: &str) -> Result<MutationPath, PathError> {
        let relative = validate_relative(supplied, false)?;
        let (parent, leaf) =
            open_parent(&self.directory, &relative.components).or_else(|error| {
                self.reject_visible_symlink(&relative)?;
                Err(error)
            })?;
        Ok(MutationPath {
            parent,
            leaf,
            display: relative.display,
        })
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

pub(crate) fn normalize_file_path(supplied: &str) -> Result<String, PathError> {
    validate_relative(supplied, false).map(|path| path.display)
}

pub(crate) fn normalize_search_path(supplied: &str) -> Result<String, PathError> {
    validate_relative(supplied, true).map(|path| path.display)
}

#[cfg(test)]
fn validate_file_path(supplied: &str) -> Result<(), PathError> {
    normalize_file_path(supplied).map(|_| ())
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
    let (directory, file_name) = open_parent(root, components)?;
    open_leaf(&directory, &file_name)
}

#[cfg(unix)]
fn open_parent(root: &File, components: &[OsString]) -> Result<(File, OsString), PathError> {
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
    Ok((directory, file_name.clone()))
}

#[cfg(unix)]
fn open_leaf(parent: &File, file_name: &OsStr) -> Result<File, PathError> {
    use rustix::fs::{Mode, OFlags, openat};

    let file = openat(
        parent,
        file_name,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
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

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::Read as _,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::{PathError, WorkspaceRoot, validate_file_path};

    static NEXT: AtomicU64 = AtomicU64::new(1);

    fn fixture(name: &str) -> std::path::PathBuf {
        let suffix = NEXT.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "plexmaton-mutation-path-{name}-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir(&directory).unwrap_or_else(|error| panic!("create fixture: {error}"));
        directory
    }

    #[test]
    fn model_paths_enforce_the_decoded_utf8_byte_bound() {
        assert!(validate_file_path(&"é".repeat(2048)).is_ok());
        assert_eq!(
            validate_file_path(&"é".repeat(2049)),
            Err(PathError::TooLong)
        );
    }

    #[test]
    fn mutation_paths_pin_the_parent_and_keep_the_leaf_separate() {
        let directory = fixture("pinned");
        fs::create_dir(directory.join("parent"))
            .unwrap_or_else(|error| panic!("create parent: {error}"));
        fs::write(directory.join("parent/file"), b"original")
            .unwrap_or_else(|error| panic!("write original: {error}"));
        let root = WorkspaceRoot::open(&directory)
            .unwrap_or_else(|error| panic!("open workspace: {error}"));
        let target = root
            .mutation_path("parent/file")
            .unwrap_or_else(|error| panic!("resolve mutation path: {error}"));

        fs::rename(directory.join("parent"), directory.join("old-parent"))
            .unwrap_or_else(|error| panic!("rename parent: {error}"));
        fs::create_dir(directory.join("parent"))
            .unwrap_or_else(|error| panic!("create replacement parent: {error}"));
        fs::write(directory.join("parent/file"), b"replacement")
            .unwrap_or_else(|error| panic!("write replacement: {error}"));
        let mut bytes = Vec::new();
        target
            .open_existing()
            .unwrap_or_else(|error| panic!("open pinned file: {error}"))
            .read_to_end(&mut bytes)
            .unwrap_or_else(|error| panic!("read pinned file: {error}"));

        assert_eq!(target.display(), "parent/file");
        assert_eq!(target.leaf(), std::ffi::OsStr::new("file"));
        assert!(target.parent().metadata().is_ok());
        assert_eq!(bytes, b"original");
        fs::remove_dir_all(directory).unwrap_or_else(|error| panic!("remove fixture: {error}"));
    }

    #[test]
    fn mutation_paths_refuse_symlinked_parents_and_leaves() {
        let directory = fixture("symlinks");
        fs::create_dir(directory.join("real"))
            .unwrap_or_else(|error| panic!("create parent: {error}"));
        fs::write(directory.join("real/file"), b"inside")
            .unwrap_or_else(|error| panic!("write file: {error}"));
        std::os::unix::fs::symlink("real", directory.join("linked-parent"))
            .unwrap_or_else(|error| panic!("symlink parent: {error}"));
        std::os::unix::fs::symlink("file", directory.join("real/linked-file"))
            .unwrap_or_else(|error| panic!("symlink leaf: {error}"));
        let root = WorkspaceRoot::open(&directory)
            .unwrap_or_else(|error| panic!("open workspace: {error}"));

        assert!(matches!(
            root.mutation_path("linked-parent/file"),
            Err(PathError::Symlink)
        ));
        let linked_leaf = root
            .mutation_path("real/linked-file")
            .unwrap_or_else(|error| panic!("resolve linked leaf parent: {error}"));
        assert_eq!(linked_leaf.require_absent(), Err(PathError::AlreadyExists));
        assert!(matches!(
            linked_leaf.open_existing(),
            Err(PathError::Symlink)
        ));
        fs::remove_dir_all(directory).unwrap_or_else(|error| panic!("remove fixture: {error}"));
    }

    #[test]
    fn mutation_paths_allow_a_missing_leaf_but_not_a_missing_parent() {
        let directory = fixture("missing");
        fs::create_dir(directory.join("parent"))
            .unwrap_or_else(|error| panic!("create parent: {error}"));
        let root = WorkspaceRoot::open(&directory)
            .unwrap_or_else(|error| panic!("open workspace: {error}"));

        let missing_leaf = root
            .mutation_path("parent/new")
            .unwrap_or_else(|error| panic!("resolve missing leaf: {error}"));
        assert_eq!(missing_leaf.require_absent(), Ok(()));
        assert!(matches!(
            missing_leaf.open_existing(),
            Err(PathError::NotFound)
        ));
        let existing = root
            .mutation_path("parent")
            .unwrap_or_else(|error| panic!("resolve existing leaf: {error}"));
        assert_eq!(existing.require_absent(), Err(PathError::AlreadyExists));
        assert!(matches!(
            root.mutation_path("missing/new"),
            Err(PathError::NotFound)
        ));
        fs::remove_dir_all(directory).unwrap_or_else(|error| panic!("remove fixture: {error}"));
    }
}
