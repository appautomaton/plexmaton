//! Bounded opaque handles to versions observed through an open file descriptor.

use std::{collections::VecDeque, fs::File, ops::Range};

const MAX_OBSERVATIONS: usize = 1024;

/// Opaque session-local identity returned to the model after a successful read (WFS-3).
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ObservationId(u64);

impl ObservationId {
    pub(crate) const UNRECORDED: Self = Self(0);

    pub(crate) fn from_token(token: &str) -> Option<Self> {
        let digits = token.strip_prefix("obs-")?;
        if digits.len() != 16
            || !digits
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return None;
        }
        let value = u64::from_str_radix(digits, 16).ok()?;
        (value != Self::UNRECORDED.0).then_some(Self(value))
    }

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
    byte_range: Range<usize>,
}

impl ObservedFile {
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn version(&self) -> &FileVersion {
        &self.version
    }

    pub(crate) fn byte_range(&self) -> &Range<usize> {
        &self.byte_range
    }

    pub(crate) fn contains(&self, candidate: Range<usize>) -> bool {
        candidate.start <= candidate.end
            && self.byte_range.start <= candidate.start
            && candidate.end <= self.byte_range.end
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

    pub(crate) fn len(&self) -> u64 {
        self.len
    }
}

#[derive(Default)]
pub(crate) struct ObservationStore {
    next: u64,
    entries: VecDeque<(ObservationId, ObservedFile)>,
}

impl ObservationStore {
    pub(crate) fn record(
        &mut self,
        path: String,
        version: FileVersion,
        byte_range: Range<usize>,
    ) -> ObservationId {
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
        self.entries.push_back((
            id,
            ObservedFile {
                path,
                version,
                byte_range,
            },
        ));
        id
    }

    pub(crate) fn get(&self, id: ObservationId) -> Option<&ObservedFile> {
        self.entries
            .iter()
            .find_map(|(candidate, observed)| (*candidate == id).then_some(observed))
    }
}

#[cfg(test)]
mod tests {
    use std::{fs::File, ops::Range};

    use super::{FileVersion, ObservationId, ObservationStore};

    #[test]
    fn observation_tokens_have_one_strict_nonzero_spelling() {
        let expected = ObservationId(0x12ab);
        assert_eq!(
            ObservationId::from_token(&expected.as_token()),
            Some(expected)
        );
        for invalid in [
            "obs-0000000000000000",
            "obs-00000000000012AB",
            "obs-00000000000012ag",
            "obs-0000000000012ab",
            "obs-000000000000012ab",
            "OBS-00000000000012ab",
            "00000000000012ab",
        ] {
            assert_eq!(ObservationId::from_token(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn an_observation_authorizes_only_ranges_inside_its_read_window() {
        let executable =
            std::env::current_exe().unwrap_or_else(|error| panic!("locate fixture: {error}"));
        let file = File::open(executable).unwrap_or_else(|error| panic!("open fixture: {error}"));
        let version =
            FileVersion::read(&file).unwrap_or_else(|error| panic!("version fixture: {error}"));
        let mut store = ObservationStore::default();
        let id = store.record("Cargo.toml".to_owned(), version, 10..20);
        let observed = store
            .get(id)
            .unwrap_or_else(|| panic!("missing observation"));

        assert_eq!(observed.byte_range(), &(10..20));
        for contained in [10..10, 10..20, 12..18, 20..20] {
            assert!(observed.contains(contained.clone()), "{contained:?}");
        }
        for outside in [9..10, 9..11, 19..21, 20..21, Range { start: 15, end: 14 }] {
            assert!(!observed.contains(outside.clone()), "{outside:?}");
        }
    }
}
