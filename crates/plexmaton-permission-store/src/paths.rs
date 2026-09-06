use std::{
    fs::File,
    io,
    os::unix::fs::{DirBuilderExt as _, MetadataExt as _},
    path::{Path, PathBuf},
};

use rustix::fs::{AtFlags, Mode, OFlags};

use crate::{PermissionStoreError as Error, ProjectIdentity};

pub(crate) const LOG: &str = "permissions.jsonl";
pub(crate) const LOCK: &str = "permissions.lock";
pub(crate) const INITIALIZED: &[u8] = b"plexmaton-project-permissions-1\n";

pub(crate) struct StorePaths {
    pub project: ProjectIdentity,
    project_path: PathBuf,
    project_directory: File,
    home_path: PathBuf,
    home: File,
    projects: File,
    pub directory: File,
    pub lock: File,
}

impl StorePaths {
    pub fn open(home: &Path, project: &Path) -> Result<Self, Error> {
        let project_path =
            std::fs::canonicalize(project).map_err(|e| Error::io("resolve project", e))?;
        let project_directory = open_directory(&project_path)?;
        let project = ProjectIdentity::from_directory(&project_path, &project_directory)?;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(home)
            .map_err(|e| Error::io("create personal root", e))?;
        let home_path =
            std::fs::canonicalize(home).map_err(|e| Error::io("resolve personal root", e))?;
        let home = open_directory(&home_path)?;
        validate(&home, true, false)?;
        let projects = private_directory(&home, "projects")?;
        let directory = private_directory(&projects, &project.key())?;
        let lock = open_file(&directory, LOCK, OFlags::CREATE)?;
        let this = Self {
            project,
            project_path,
            project_directory,
            home_path,
            home,
            projects,
            directory,
            lock,
        };
        this.revalidate()?;
        Ok(this)
    }

    pub fn revalidate(&self) -> Result<(), Error> {
        let current_project = open_directory(&self.project_path)?;
        if ProjectIdentity::from_directory(&self.project_path, &current_project)? != self.project
            || !same_file(&self.project_directory, &current_project)?
        {
            return Err(Error::IdentityChanged);
        }
        let home = open_directory(&self.home_path)?;
        validate(&home, true, false)?;
        if !same_file(&home, &self.home)? {
            return Err(Error::IdentityChanged);
        }
        same_child(&self.home, "projects", &self.projects)?;
        same_child(&self.projects, &self.project.key(), &self.directory)?;
        same_child(&self.directory, LOCK, &self.lock)?;
        validate(&self.projects, true, true)?;
        validate(&self.directory, true, true)?;
        validate(&self.lock, false, true)
    }
}

fn open_directory(path: &Path) -> Result<File, Error> {
    rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map(File::from)
    .map_err(|e| Error::io("open pinned directory", e))
}

fn private_directory(parent: &File, name: &str) -> Result<File, Error> {
    match rustix::fs::mkdirat(parent, name, Mode::from_raw_mode(0o700)) {
        Ok(()) => parent
            .sync_all()
            .map_err(|e| Error::io("sync new directory", e))?,
        Err(rustix::io::Errno::EXIST) => {}
        Err(error) => return Err(Error::io("create permission directory", error)),
    }
    let file = rustix::fs::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map(File::from)
    .map_err(|e| Error::io("open permission directory", e))?;
    validate(&file, true, true)?;
    Ok(file)
}

pub(crate) fn open_file(parent: &File, name: &str, flags: OFlags) -> Result<File, Error> {
    // NONBLOCK ensures a hostile FIFO cannot block before its type is rejected.
    let file = rustix::fs::openat(
        parent,
        name,
        OFlags::RDWR | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK | flags,
        Mode::from_raw_mode(0o600),
    )
    .map(File::from)
    .map_err(|e| Error::io("open permission file", e))?;
    validate(&file, false, true)?;
    Ok(file)
}

pub(crate) fn is_missing(error: &Error) -> bool {
    matches!(
        error,
        Error::Io {
            kind: io::ErrorKind::NotFound,
            ..
        }
    )
}

fn validate(file: &File, directory: bool, private: bool) -> Result<(), Error> {
    let meta = file
        .metadata()
        .map_err(|e| Error::io("inspect permission path", e))?;
    let forbidden = if private { 0o077 } else { 0o022 };
    if meta.uid() != rustix::process::geteuid().as_raw()
        || meta.mode() & forbidden != 0
        || if directory {
            !meta.is_dir()
        } else {
            !meta.is_file() || meta.nlink() != 1
        }
    {
        return Err(Error::UnsafePath);
    }
    Ok(())
}

pub(crate) fn same_file(a: &File, b: &File) -> Result<bool, Error> {
    let a = a
        .metadata()
        .map_err(|e| Error::io("inspect pinned file", e))?;
    let b = b
        .metadata()
        .map_err(|e| Error::io("inspect current file", e))?;
    Ok(a.dev() == b.dev() && a.ino() == b.ino())
}

pub(crate) fn same_child(parent: &File, name: &str, file: &File) -> Result<(), Error> {
    let current = rustix::fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|e| Error::io("revalidate permission path", e))?;
    let pinned = rustix::fs::fstat(file).map_err(|e| Error::io("inspect pinned file", e))?;
    if current.st_dev != pinned.st_dev || current.st_ino != pinned.st_ino {
        return Err(Error::IdentityChanged);
    }
    Ok(())
}
