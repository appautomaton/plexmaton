//! Bounded opaque handles to versions observed through an open file descriptor.

use std::{collections::VecDeque, fs::File};

const MAX_OBSERVATIONS: usize = 1024;

/// Opaque session-local identity returned to the model after a successful read (WFS-3).
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ObservationId(u64);

impl ObservationId {
    pub(crate) const UNRECORDED: Self = Self(0);

    #[must_use]
    pub fn as_token(self) -> String {
        format!("obs-{:016x}", self.0)
    }
}

/// File and exact descriptor metadata one successful read observed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservedFile {
    path: String,
    version: FileVersion,
}

impl ObservedFile {
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn version(&self) -> &FileVersion {
        &self.version
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileVersion {
    len: u64,
    modified_nanos: Option<u128>,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    changed_seconds: i64,
    #[cfg(unix)]
    changed_nanos: i64,
}

impl FileVersion {
    pub(crate) fn read(file: &File) -> std::io::Result<Self> {
        let metadata = file.metadata()?;
        let modified_nanos = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos());
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok(Self {
                len: metadata.len(),
                modified_nanos,
                device: metadata.dev(),
                inode: metadata.ino(),
                changed_seconds: metadata.ctime(),
                changed_nanos: metadata.ctime_nsec(),
            })
        }
        #[cfg(not(unix))]
        {
            Ok(Self {
                len: metadata.len(),
                modified_nanos,
            })
        }
    }
}

#[derive(Default)]
pub(crate) struct ObservationStore {
    next: u64,
    entries: VecDeque<(ObservationId, ObservedFile)>,
}

impl ObservationStore {
    pub(crate) fn record(&mut self, path: String, version: FileVersion) -> ObservationId {
        self.next = match self.next.checked_add(1) {
            Some(next) => next,
            None => {
                self.entries.clear();
                1
            }
        };
        let id = ObservationId(self.next);
        if self.entries.len() == MAX_OBSERVATIONS {
            self.entries.pop_front();
        }
        self.entries.push_back((id, ObservedFile { path, version }));
        id
    }

    pub(crate) fn get(&self, id: ObservationId) -> Option<&ObservedFile> {
        self.entries
            .iter()
            .find_map(|(candidate, observed)| (*candidate == id).then_some(observed))
    }
}
